use axum::{
    body::Body,
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use futures::stream::{self, StreamExt};
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

    // 4. 提取模型名
    let model_name = protocol::extract_model(detected_protocol, body_ref, &path);

    // 5. 查找模型配置
    let model_config = match &model_name {
        Some(name) => crate::db::find_model_config(&state.pool, org.id, name)
            .await
            .ok_or((StatusCode::NOT_FOUND, format!("No config found for model '{name}'")))?,
        None => {
            return Err((
                StatusCode::BAD_REQUEST,
                "Cannot extract model name from request".to_string(),
            ));
        }
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

    // 7. 转换请求头
    let forwarded_headers =
        protocol::transform_headers(actual_protocol, &parts.headers, &model_config.real_api_key);

    // 8. 构造目标 URL：去掉请求路径中的协议前缀以避免重复
    let base_url = model_config.base_url.trim_end_matches('/');
    let stripped_path = strip_protocol_prefix(&path, actual_protocol);
    let target_url = if query.is_empty() {
        format!("{base_url}{stripped_path}")
    } else {
        format!("{base_url}{stripped_path}?{query}")
    };
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

    // 10. 异步创建请求日志
    let log_pool = state.pool.clone();
    let log_org_id = org.id;
    let log_key_id = api_key_record.id;
    let log_provider = model_config.name.clone();
    let log_model = model_name.clone();
    let log_path = path.clone();
    let log_method = method.to_string();
    let log_body = body_json.clone();
    let log_id = Uuid::new_v4();

    tokio::spawn(async move {
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
        .execute(&log_pool)
        .await;
    });

    // 11. 处理响应
    if is_stream {
        handle_streaming_response(state.pool.clone(), log_id, actual_protocol, upstream_resp, resp_status, start).await
    } else {
        handle_normal_response(state.pool.clone(), log_id, actual_protocol, upstream_resp, resp_status, start).await
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
    protocol: Protocol,
    upstream_resp: reqwest::Response,
    status: reqwest::StatusCode,
    start: Instant,
) -> Result<Response, (StatusCode, String)> {
    let resp_headers = upstream_resp.headers().clone();
    let resp_bytes = upstream_resp.bytes().await.map_err(|e| {
        (StatusCode::BAD_GATEWAY, format!("Failed to read upstream response: {e}"))
    })?;

    let resp_json: Option<Value> = serde_json::from_slice(&resp_bytes).ok();
    let usage = resp_json
        .as_ref()
        .map(|j| protocol::extract_token_usage(protocol, j))
        .unwrap_or_default();

    let duration_ms = start.elapsed().as_millis() as i32;
    let status_code = status.as_u16();

    tokio::spawn(async move {
        let status_str = if (200..300).contains(&status_code) { "success" } else { "error" };
        let error_msg = if status_str == "error" {
            resp_json.as_ref().and_then(|j| j["error"]["message"].as_str()).map(String::from)
        } else {
            None
        };

        let _ = sqlx::query(
            "UPDATE request_logs SET response_status=$2, status=$3, prompt_tokens=$4, \
             completion_tokens=$5, cached_tokens=$6, total_tokens=$7, duration_ms=$8, \
             error_message=$9, updated_at=now() WHERE id=$1"
        )
        .bind(log_id)
        .bind(status_code as i32)
        .bind(status_str)
        .bind(usage.prompt_tokens)
        .bind(usage.completion_tokens)
        .bind(usage.cached_tokens)
        .bind(usage.total_tokens)
        .bind(duration_ms)
        .bind(error_msg.as_deref())
        .execute(&pool)
        .await;
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
    proto: Protocol,
    upstream_resp: reqwest::Response,
    status: reqwest::StatusCode,
    start: Instant,
) -> Result<Response, (StatusCode, String)> {
    let resp_headers = upstream_resp.headers().clone();
    let byte_stream = upstream_resp.bytes_stream();

    let pool_clone = pool.clone();
    let status_code = status.as_u16();
    let (usage_tx, usage_rx) = tokio::sync::oneshot::channel::<(TokenUsage, i32)>();

    let body_stream = stream::unfold(
        (byte_stream, Some(usage_tx), TokenUsage::default()),
        move |(mut stream, tx, mut last_usage)| async move {
            match stream.next().await {
                Some(Ok(chunk)) => {
                    if let Ok(text) = std::str::from_utf8(&chunk) {
                        if let Some(usage) = protocol::extract_streaming_usage(proto, text) {
                            last_usage = usage;
                        }
                    }
                    Some((Ok::<Bytes, std::io::Error>(chunk), (stream, tx, last_usage)))
                }
                Some(Err(e)) => {
                    let io_err = std::io::Error::new(std::io::ErrorKind::Other, e.to_string());
                    Some((Err(io_err), (stream, tx, last_usage)))
                }
                None => {
                    if let Some(tx) = tx {
                        let _ = tx.send((last_usage, start.elapsed().as_millis() as i32));
                    }
                    None
                }
            }
        },
    );

    // 异步等待流结束并更新日志
    tokio::spawn(async move {
        if let Ok((usage, duration_ms)) = usage_rx.await {
            let status_str = if (200..300).contains(&status_code) { "success" } else { "error" };
            let _ = sqlx::query(
                "UPDATE request_logs SET response_status=$2, status=$3, prompt_tokens=$4, \
                 completion_tokens=$5, cached_tokens=$6, total_tokens=$7, duration_ms=$8, \
                 updated_at=now() WHERE id=$1"
            )
            .bind(log_id)
            .bind(status_code as i32)
            .bind(status_str)
            .bind(usage.prompt_tokens)
            .bind(usage.completion_tokens)
            .bind(usage.cached_tokens)
            .bind(usage.total_tokens)
            .bind(duration_ms)
            .execute(&pool_clone)
            .await;
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
