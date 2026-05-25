use axum::{
    body::Body,
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use futures::stream::{self, StreamExt};
use rust_decimal::prelude::FromPrimitive;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

use crate::protocol::{self, Protocol, TokenUsage};
use crate::state::AppState;
use crate::static_files;

/// 从请求头中提取 API Key（支持 Bearer、x-api-key、x-goog-api-key）
fn extract_api_key(headers: &http::HeaderMap) -> Option<String> {
    if let Some(auth) = headers.get(http::header::AUTHORIZATION) {
        if let Ok(val) = auth.to_str() {
            if let Some(key) = val.strip_prefix("Bearer ") {
                return Some(key.trim().to_string());
            }
        }
    }
    for name in ["x-api-key", "x-goog-api-key"] {
        if let Some(key_header) = headers.get(name) {
            if let Ok(val) = key_header.to_str() {
                return Some(val.trim().to_string());
            }
        }
    }
    None
}

/// 计算 API Key 的 SHA256 哈希
fn hash_api_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
}

/// API 代理核心处理函数
pub async fn proxy_handler(
    State(state): State<Arc<AppState>>,
    req: Request,
) -> Result<Response, (StatusCode, String)> {
    let start = Instant::now();

    let (parts, body) = req.into_parts();
    let path = parts.uri.path().to_string();
    let query = parts.uri.query().unwrap_or("").to_string();
    let method = parts.method.clone();

    // 1. 提取 API Key，无 Key 时视为前端页面请求，返回静态文件
    let api_key = match extract_api_key(&parts.headers) {
        Some(key) => key,
        None => return Ok(static_files::serve_static(parts.uri).await.into_response()),
    };

    let key_hash = hash_api_key(&api_key);

    let key_info = crate::db::find_api_key_by_hash(&state.pool, &key_hash)
        .await
        .ok_or((StatusCode::UNAUTHORIZED, "Invalid or disabled API Key".to_string()))?;

    let (api_key_record, org) = key_info;

    // 异步更新 API Key 最后使用时间
    let pool_bg = state.pool.clone();
    let key_id = api_key_record.id;
    tokio::spawn(async move {
        crate::db::touch_api_key(&pool_bg, key_id).await;
    });

    // 2. 读取请求体
    let body_bytes = axum::body::to_bytes(body, 50 * 1024 * 1024)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Failed to read body: {e}")))?;

    let body_json: Option<Value> = serde_json::from_slice(&body_bytes).ok();
    let empty_json = Value::Null;
    let body_ref = body_json.as_ref().unwrap_or(&empty_json);

    // 3. 检测协议
    let detected_protocol = protocol::detect_protocol(&path, &parts.headers);

    // 4. 提取模型名（可能为 None，如 /v1/files, /v1/models 等辅助接口）
    let model_name = protocol::extract_model(detected_protocol, body_ref, &path);

    // 是否为透传请求（无模型名，只记日志不记用量/积分）
    let is_passthrough = model_name.is_none();

    // 仅非透传请求才检查积分余额（支持透支额度：credit + overdraft_limit <= 0 时拒绝）
    if !is_passthrough && org.credit + org.overdraft_limit <= rust_decimal::Decimal::ZERO {
        let provider = match detected_protocol {
            Protocol::OpenAI => "openai",
            Protocol::Anthropic => "anthropic",
            Protocol::Gemini => "gemini",
        };
        let log_id = Uuid::new_v4();
        let _ = sqlx::query(
            "INSERT INTO request_logs (id, org_id, api_key_id, provider, model, path, method, is_stream, request_body, status, error_message, response_status, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, false, $8, 'error', 'Insufficient quota', 402, now())"
        )
        .bind(log_id)
        .bind(org.id)
        .bind(api_key_record.id)
        .bind(provider)
        .bind(model_name.as_deref())
        .bind(&path)
        .bind(method.to_string())
        .bind(body_json.as_ref())
        .execute(&state.pool)
        .await;
        return Err((
            StatusCode::PAYMENT_REQUIRED,
            "Insufficient quota (额度不足)".to_string(),
        ));
    }

    // 5. 查找模型配置；无模型名时按检测到的协议取匹配的配置作为透传通道
    let protocol_str = match detected_protocol {
        Protocol::OpenAI => "openai",
        Protocol::Anthropic => "anthropic",
        Protocol::Gemini => "gemini",
    };
    let model_config = match &model_name {
        Some(name) => crate::db::find_model_config(&state.pool, org.id, name)
            .await
            .ok_or((StatusCode::NOT_FOUND, format!("No config found for model '{name}'")))?,
        None => crate::db::find_first_model_config(&state.pool, org.id, protocol_str)
            .await
            .ok_or((StatusCode::BAD_REQUEST, format!("No available {protocol_str} model config for this organization")))?,
    };

    // 配置中的 protocol 字段优先
    let actual_protocol = match model_config.protocol.as_str() {
        "anthropic" => Protocol::Anthropic,
        "gemini" => Protocol::Gemini,
        _ => detected_protocol,
    };

    // 6. 判断流式
    let is_stream = protocol::is_streaming(actual_protocol, body_ref, &path);

    // 为 OpenAI 流式请求自动注入 stream_options 以获取 usage 数据
    let forwarded_body = if is_stream && actual_protocol == Protocol::OpenAI {
        if let Some(mut json) = body_json.clone() {
            if let Some(obj) = json.as_object_mut() {
                obj.entry("stream_options").or_insert_with(|| {
                    serde_json::json!({"include_usage": true})
                });
            }
            Bytes::from(serde_json::to_vec(&json).unwrap_or_else(|_| body_bytes.to_vec()))
        } else {
            body_bytes.clone()
        }
    } else {
        body_bytes.clone()
    };

    // 7. 转换请求头（透传请求保留原始 content-type 等头信息）
    let forwarded_headers =
        protocol::transform_headers(actual_protocol, &parts.headers, &model_config.real_api_key);

    // 8. 构造目标 URL
    let base_url = model_config.base_url.trim_end_matches('/');
    let target_url = if is_passthrough {
        // 透传请求：提取 base_url 的 origin（scheme + host），直接拼接原始路径
        // 无论 base_url 是 "https://api.openai.com/v1" 还是 "https://api.openai.com"
        // 都统一取 "https://api.openai.com" + "/v1/files" = "https://api.openai.com/v1/files"
        let origin = base_url.find("://")
            .and_then(|i| base_url[i + 3..].find('/').map(|j| &base_url[..i + 3 + j]))
            .unwrap_or(base_url);
        if query.is_empty() { format!("{origin}{path}") } else { format!("{origin}{path}?{query}") }
    } else {
        let stripped_path = strip_protocol_prefix(&path, actual_protocol);
        if query.is_empty() { format!("{base_url}{stripped_path}") } else { format!("{base_url}{stripped_path}?{query}") }
    };
    tracing::info!("Proxy: {} {} -> {}", method, path, target_url);
    // 9. 转发请求
    let upstream_resp = state
        .http_client
        .request(method.clone(), &target_url)
        .headers(forwarded_headers)
        .body(forwarded_body)
        .send()
        .await
        .map_err(|e| {
            tracing::error!("Upstream request failed: {e}");
            (StatusCode::BAD_GATEWAY, format!("Upstream unreachable: {e}"))
        })?;

    let resp_status = upstream_resp.status();

    // 10. 准备创建请求日志的数据
    let log_org_id = org.id;
    let log_key_id = api_key_record.id;
    let log_provider = model_config.name.clone();
    let log_model = model_name.clone();
    let log_path = path.clone();
    let log_method = method.to_string();
    let log_body = body_json.clone();
    let log_id = Uuid::new_v4();

    // 10. 创建请求日志 (等待执行完成以避免与后续的 UPDATE 产生竞态条件)
    let _ = sqlx::query(
        "INSERT INTO request_logs (id, org_id, api_key_id, provider, model, path, method, is_stream, request_body, status) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'pending')"
    )
    .bind(log_id)
    .bind(log_org_id)
    .bind(log_key_id)
    .bind(&log_provider)
    .bind(log_model.as_deref())
    .bind(&log_path)
    .bind(&log_method)
    .bind(is_stream)
    .bind(log_body.as_ref())
    .execute(&state.pool)
    .await;

    // 11. 处理响应
    if is_stream {
        handle_streaming_response(state.pool.clone(), log_id, log_org_id, actual_protocol, upstream_resp, resp_status, start, is_passthrough).await
    } else {
        handle_normal_response(state.pool.clone(), log_id, log_org_id, actual_protocol, upstream_resp, resp_status, start, is_passthrough).await
    }
}

/// 去掉请求路径中的协议前缀，避免与 base_url 中的路径重复
/// 例：path="/v1/chat/completions" + OpenAI → "/chat/completions"
///     path="/v1beta/models/gemini:gen" + Gemini → "/models/gemini:gen"
///     path="/anthropic/v1/messages" + Anthropic → "/v1/messages"
fn strip_protocol_prefix(path: &str, protocol: Protocol) -> String {
    match protocol {
        Protocol::OpenAI => path
            .strip_prefix("/v1")
            .map(|s| s.to_string())
            .unwrap_or_else(|| path.to_string()),
        Protocol::Gemini => path
            .strip_prefix("/v1beta")
            .map(|s| s.to_string())
            .unwrap_or_else(|| path.to_string()),
        Protocol::Anthropic => path
            .strip_prefix("/anthropic")
            .map(|s| s.to_string())
            .unwrap_or_else(|| path.to_string()),
    }
}

/// 处理非流式响应
async fn handle_normal_response(
    pool: sqlx::PgPool,
    log_id: Uuid,
    org_id: Uuid,
    protocol: Protocol,
    upstream_resp: reqwest::Response,
    status: reqwest::StatusCode,
    start: Instant,
    is_passthrough: bool,
) -> Result<Response, (StatusCode, String)> {
    let resp_headers = upstream_resp.headers().clone();
    let resp_bytes = upstream_resp.bytes().await.map_err(|e| {
        (StatusCode::BAD_GATEWAY, format!("Failed to read upstream response: {e}"))
    })?;

    let resp_json: Option<Value> = match serde_json::from_slice(&resp_bytes) {
        Ok(json) => Some(json),
        Err(e) => {
            tracing::warn!("Failed to parse upstream response as JSON: {}. Raw body: {}", e, String::from_utf8_lossy(&resp_bytes));
            None
        }
    };
    
    let usage = if is_passthrough {
        TokenUsage::default()
    } else {
        resp_json.as_ref().map(|j| protocol::extract_token_usage(protocol, j)).unwrap_or_default()
    };

    let duration_ms = start.elapsed().as_millis() as i32;
    let status_code = status.as_u16();

    tokio::spawn(async move {
        let status_str = if (200..300).contains(&status_code) { "success" } else { "error" };
        let error_msg = if status_str == "error" {
            resp_json.as_ref().and_then(|j| j.get("error")).and_then(|e| e.get("message")).and_then(|m| m.as_str()).map(String::from)
        } else {
            None
        };

        let update_res = sqlx::query(
            "UPDATE request_logs SET response_status=$2, status=$3, prompt_tokens=$4, \
             completion_tokens=$5, cached_tokens=$6, total_tokens=$7, duration_ms=$8, \
             error_message=$9, response_body=$10, updated_at=now() WHERE id=$1"
        )
        .bind(log_id)
        .bind(status_code as i32)
        .bind(status_str)
        .bind(usage.prompt_tokens)
        .bind(usage.completion_tokens)
        .bind(usage.cached_tokens)
        .bind(usage.total_tokens)
        .bind(duration_ms)
        .bind(error_msg.clone())
        .bind(resp_json.clone())
        .execute(&pool)
        .await;

        if let Err(e) = &update_res {
            tracing::error!("Failed to update request_log: {}", e);
        }

        // 透传请求不扣积分
        if is_passthrough { return; }

        // 扣除积分
        if status_str == "success" {
            let rates = crate::db::get_credit_rates(&pool).await;
            let mut cost = 0.0;
            if let Some(p) = usage.prompt_tokens {
                if rates.input_rate > 0.0 {
                    cost += (p as f64) / rates.input_rate;
                }
            }
            if let Some(c) = usage.completion_tokens {
                if rates.output_rate > 0.0 {
                    cost += (c as f64) / rates.output_rate;
                }
            }
            if let Some(ca) = usage.cached_tokens {
                if rates.cached_rate > 0.0 {
                    cost += (ca as f64) / rates.cached_rate;
                }
            }
            // 长上下文倍率：输入 Token >= 阈值时，积分乘以倍率
            let mut is_long_context = false;
            if let (Some(threshold), Some(multiplier)) = (rates.long_context_threshold, rates.long_context_multiplier) {
                if threshold > 0 && multiplier > 0.0 {
                    if let Some(p) = usage.prompt_tokens {
                        if (p as u64) >= threshold {
                            cost *= multiplier;
                            is_long_context = true;
                        }
                    }
                }
            }
            if cost > 0.0 {
                if let Some(decimal_cost) = rust_decimal::Decimal::from_f64(cost) {
                    let _ = crate::db::deduct_credit(&pool, org_id, decimal_cost, &log_id.to_string()).await;
                    let _ = sqlx::query("UPDATE request_logs SET credit_cost = $1, is_long_context = $3 WHERE id = $2")
                        .bind(decimal_cost)
                        .bind(log_id)
                        .bind(is_long_context)
                        .execute(&pool)
                        .await;
                }
            } else if is_long_context {
                let _ = sqlx::query("UPDATE request_logs SET is_long_context = $2 WHERE id = $1")
                    .bind(log_id)
                    .bind(true)
                    .execute(&pool)
                    .await;
            }
        }
    });

    let mut builder = Response::builder().status(status_code);
    for (name, value) in resp_headers.iter() {
        let n = name.as_str().to_lowercase();
        if matches!(n.as_str(), "transfer-encoding" | "connection" | "keep-alive") {
            continue;
        }
        builder = builder.header(name.as_str(), value);
    }
    builder
        .body(Body::from(resp_bytes))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to build response: {e}")))
}

/// 处理流式 SSE 响应
async fn handle_streaming_response(
    pool: sqlx::PgPool,
    log_id: Uuid,
    org_id: Uuid,
    proto: Protocol,
    upstream_resp: reqwest::Response,
    status: reqwest::StatusCode,
    start: Instant,
    is_passthrough: bool,
) -> Result<Response, (StatusCode, String)> {
    let resp_headers = upstream_resp.headers().clone();
    let byte_stream = upstream_resp.bytes_stream();

    let pool_clone = pool.clone();
    let status_code = status.as_u16();
    let (usage_tx, usage_rx) = tokio::sync::oneshot::channel::<(TokenUsage, i32, String)>();

    // line_buf 用于缓冲跨 chunk 的不完整 SSE 行，确保 usage 提取不会因行被截断而丢失
    let body_stream = stream::unfold(
        (byte_stream, Some(usage_tx), TokenUsage::default(), String::new(), String::new()),
        move |(mut stream, tx, mut last_usage, mut accumulated_body, mut line_buf)| async move {
            match stream.next().await {
                Some(Ok(chunk)) => {
                    if let Ok(text) = std::str::from_utf8(&chunk) {
                        accumulated_body.push_str(text);
                        line_buf.push_str(text);
                        // 只处理以 \n 结尾的完整行，不完整的末尾保留到下个 chunk
                        if let Some(last_newline) = line_buf.rfind('\n') {
                            let complete = line_buf[..=last_newline].to_string();
                            line_buf = line_buf[last_newline + 1..].to_string();
                            if let Some(usage) = protocol::extract_streaming_usage(proto, &complete) {
                                last_usage = protocol::merge_token_usage(Some(last_usage), usage);
                            }
                        }
                    }
                    Some((Ok::<Bytes, std::io::Error>(chunk), (stream, tx, last_usage, accumulated_body, line_buf)))
                }
                Some(Err(e)) => {
                    let io_err = std::io::Error::new(std::io::ErrorKind::Other, e.to_string());
                    Some((Err(io_err), (stream, tx, last_usage, accumulated_body, line_buf)))
                }
                None => {
                    // 处理缓冲区中剩余的不完整行
                    if !line_buf.is_empty() {
                        if let Some(usage) = protocol::extract_streaming_usage(proto, &line_buf) {
                            last_usage = protocol::merge_token_usage(Some(last_usage), usage);
                        }
                    }
                    if let Some(tx) = tx {
                        let _ = tx.send((last_usage, start.elapsed().as_millis() as i32, accumulated_body));
                    }
                    None
                }
            }
        },
    );

    // 异步等待流结束并更新日志
    tokio::spawn(async move {
        let (usage, duration_ms, body_str) = match usage_rx.await {
            Ok(v) => v,
            Err(_) => {
                // 流被中断（客户端断开等），tx 被 drop 导致 rx 收到 Err
                tracing::warn!("Stream channel closed without sending usage for log {log_id}, marking as error");
                let duration_ms = start.elapsed().as_millis() as i32;
                let _ = sqlx::query(
                    "UPDATE request_logs SET response_status=$2, status='error', duration_ms=$3, \
                     error_message='Stream interrupted (client disconnected or upstream error)', updated_at=now() WHERE id=$1"
                )
                .bind(log_id)
                .bind(status_code as i32)
                .bind(duration_ms)
                .execute(&pool_clone)
                .await;
                return;
            }
        };

        let status_str = if (200..300).contains(&status_code) { "success" } else { "error" };
        let error_msg = if status_str == "error" {
            Some(body_str.chars().take(500).collect::<String>())
        } else {
            None
        };
        let resp_json = Value::String(body_str);

        let update_res = sqlx::query(
            "UPDATE request_logs SET response_status=$2, status=$3, prompt_tokens=$4, \
             completion_tokens=$5, cached_tokens=$6, total_tokens=$7, duration_ms=$8, \
             response_body=$9, error_message=$10, updated_at=now() WHERE id=$1"
        )
        .bind(log_id)
        .bind(status_code as i32)
        .bind(status_str)
        .bind(usage.prompt_tokens)
        .bind(usage.completion_tokens)
        .bind(usage.cached_tokens)
        .bind(usage.total_tokens)
        .bind(duration_ms)
        .bind(resp_json.clone())
        .bind(error_msg)
        .execute(&pool_clone)
        .await;

        if let Err(e) = &update_res {
            tracing::error!("Failed to update request_log (stream): {}", e);
        }

        // 透传请求不扣积分
        if is_passthrough { return; }

        // 扣除积分
        if status_str == "success" {
            let rates = crate::db::get_credit_rates(&pool_clone).await;
            let mut cost = 0.0;
            if let Some(p) = usage.prompt_tokens {
                if rates.input_rate > 0.0 {
                    cost += (p as f64) / rates.input_rate;
                }
            }
            if let Some(c) = usage.completion_tokens {
                if rates.output_rate > 0.0 {
                    cost += (c as f64) / rates.output_rate;
                }
            }
            if let Some(ca) = usage.cached_tokens {
                if rates.cached_rate > 0.0 {
                    cost += (ca as f64) / rates.cached_rate;
                }
            }
            // 长上下文倍率：输入 Token >= 阈值时，积分乘以倍率
            let mut is_long_context = false;
            if let (Some(threshold), Some(multiplier)) = (rates.long_context_threshold, rates.long_context_multiplier) {
                if threshold > 0 && multiplier > 0.0 {
                    if let Some(p) = usage.prompt_tokens {
                        if (p as u64) >= threshold {
                            cost *= multiplier;
                            is_long_context = true;
                        }
                    }
                }
            }
            if cost > 0.0 {
                if let Some(decimal_cost) = rust_decimal::Decimal::from_f64(cost) {
                    let _ = crate::db::deduct_credit(&pool_clone, org_id, decimal_cost, &log_id.to_string()).await;
                    let _ = sqlx::query("UPDATE request_logs SET credit_cost = $1, is_long_context = $3 WHERE id = $2")
                        .bind(decimal_cost)
                        .bind(log_id)
                        .bind(is_long_context)
                        .execute(&pool_clone)
                        .await;
                }
            } else if is_long_context {
                let _ = sqlx::query("UPDATE request_logs SET is_long_context = $2 WHERE id = $1")
                    .bind(log_id)
                    .bind(true)
                    .execute(&pool_clone)
                    .await;
            }
        }
    });

    let mut builder = Response::builder().status(status_code);
    for (name, value) in resp_headers.iter() {
        let n = name.as_str().to_lowercase();
        if matches!(n.as_str(), "transfer-encoding" | "connection" | "keep-alive") {
            continue;
        }
        builder = builder.header(name.as_str(), value);
    }
    builder
        .body(Body::from_stream(body_stream))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to build streaming response: {e}")))
}
