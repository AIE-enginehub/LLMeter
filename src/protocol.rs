use http::HeaderMap;
use serde_json::Value;

/// 支持的 AI API 协议类型
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Protocol {
    OpenAI,
    Anthropic,
    Gemini,
}

/// Token 用量统计
#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub prompt_tokens: Option<i32>,
    pub completion_tokens: Option<i32>,
    pub cached_tokens: Option<i32>,
    pub total_tokens: Option<i32>,
}

/// 需要移除的 hop-by-hop 头列表
const HOP_BY_HOP_HEADERS: &[&str] = &[
    "connection",
    "keep-alive",
    "host",
    "expect",
    "transfer-encoding",
    "te",
    "trailer",
    "upgrade",
    "proxy-authorization",
    "proxy-authenticate",
    "accept-encoding", // 移除客户端的压缩请求，确保我们能读取到明文 JSON
];

/// 根据请求路径和 headers 检测 AI 协议类型
pub fn detect_protocol(path: &str, headers: &HeaderMap) -> Protocol {
    if path.starts_with("/v1beta/") {
        return Protocol::Gemini;
    }

    if path.starts_with("/anthropic/") {
        return Protocol::Anthropic;
    }

    // /v1/messages + anthropic-version header → Anthropic
    if path.starts_with("/v1/messages") && headers.contains_key("anthropic-version") {
        return Protocol::Anthropic;
    }

    Protocol::OpenAI
}

/// 从请求体或路径中提取模型名称
pub fn extract_model(protocol: Protocol, body: &Value, path: &str) -> Option<String> {
    match protocol {
        Protocol::OpenAI | Protocol::Anthropic => {
            body.get("model").and_then(|v| v.as_str()).map(String::from)
        }
        Protocol::Gemini => extract_gemini_model(path),
    }
}

/// 从 Gemini 路径中提取模型名
/// 例: `/v1beta/models/gemini-2.5-flash:generateContent` → `gemini-2.5-flash`
fn extract_gemini_model(path: &str) -> Option<String> {
    let path = path.split('?').next().unwrap_or(path);
    let segment = path
        .split('/')
        .find(|seg| seg.contains(':') && !seg.is_empty())?;
    let model = segment.split(':').next()?;
    if model.is_empty() {
        None
    } else {
        Some(model.to_string())
    }
}

/// 判断请求是否为流式传输
pub fn is_streaming(protocol: Protocol, body: &Value, path: &str) -> bool {
    match protocol {
        Protocol::OpenAI | Protocol::Anthropic => {
            body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false)
        }
        Protocol::Gemini => path.contains("streamGenerateContent"),
    }
}

/// 转换请求头：移除 hop-by-hop 头并设置目标协议的认证头
pub fn transform_headers(
    protocol: Protocol,
    headers: &HeaderMap,
    real_api_key: &str,
) -> HeaderMap {
    let mut new_headers = HeaderMap::new();

    for (name, value) in headers.iter() {
        let name_lower = name.as_str().to_lowercase();
        if HOP_BY_HOP_HEADERS.contains(&name_lower.as_str()) {
            continue;
        }
        // 移除认证头（后续统一重设）和 content-length（body 可能被修改）
        if matches!(
            name_lower.as_str(),
            "authorization" | "x-api-key" | "x-goog-api-key" | "content-length"
        ) {
            continue;
        }
        if let Ok(n) = http::header::HeaderName::from_bytes(name.as_str().as_bytes()) {
            new_headers.insert(n, value.clone());
        }
    }

    match protocol {
        Protocol::OpenAI => {
            let bearer = format!("Bearer {real_api_key}");
            if let Ok(v) = http::header::HeaderValue::from_str(&bearer) {
                new_headers.insert(http::header::AUTHORIZATION, v);
            }
        }
        Protocol::Anthropic => {
            if let Ok(v) = http::header::HeaderValue::from_str(real_api_key) {
                new_headers.insert("x-api-key", v);
            }
            // 保留原始 anthropic-version
            if let Some(ver) = headers.get("anthropic-version") {
                new_headers.insert("anthropic-version", ver.clone());
            }
        }
        Protocol::Gemini => {
            if let Ok(v) = http::header::HeaderValue::from_str(real_api_key) {
                new_headers.insert("x-goog-api-key", v);
            }
        }
    }

    new_headers
}

/// 从非流式响应体中提取 token 用量
pub fn extract_token_usage(protocol: Protocol, body: &Value) -> TokenUsage {
    match protocol {
        Protocol::OpenAI => {
            let usage = &body["usage"];
            // 兼容 Chat Completions API (prompt_tokens) 和 Responses API (input_tokens)
            let prompt = usage["prompt_tokens"].as_i64()
                .or_else(|| usage["input_tokens"].as_i64())
                .map(|v| v as i32);
            let completion = usage["completion_tokens"].as_i64()
                .or_else(|| usage["output_tokens"].as_i64())
                .map(|v| v as i32);
            let cached = usage["prompt_tokens_details"]["cached_tokens"].as_i64()
                .or_else(|| usage["input_tokens_details"]["cached_tokens"].as_i64())
                .map(|v| v as i32);
            let total = usage["total_tokens"].as_i64().map(|v| v as i32);
            TokenUsage { prompt_tokens: prompt, completion_tokens: completion, cached_tokens: cached, total_tokens: total }
        }
        Protocol::Anthropic => {
            let usage = &body["usage"];
            let input = usage["input_tokens"].as_i64().map(|v| v as i32);
            let output = usage["output_tokens"].as_i64().map(|v| v as i32);
            TokenUsage {
                prompt_tokens: input,
                completion_tokens: output,
                cached_tokens: usage["cache_read_input_tokens"]
                    .as_i64()
                    .map(|v| v as i32),
                total_tokens: match (input, output) {
                    (Some(i), Some(o)) => Some(i + o),
                    _ => None,
                },
            }
        }
        Protocol::Gemini => {
            let meta = body.get("usageMetadata");
            if let Some(meta) = meta {
                let prompt = meta.get("promptTokenCount").and_then(|v| v.as_i64()).map(|v| v as i32);
                let candidates = meta.get("candidatesTokenCount").and_then(|v| v.as_i64()).map(|v| v as i32);
                let cached = meta.get("cachedContentTokenCount").and_then(|v| v.as_i64()).map(|v| v as i32);
                let total = meta.get("totalTokenCount").and_then(|v| v.as_i64()).map(|v| v as i32);
                TokenUsage {
                    prompt_tokens: prompt,
                    completion_tokens: candidates,
                    cached_tokens: cached,
                    total_tokens: total.or_else(|| match (prompt, candidates) {
                        (Some(p), Some(c)) => Some(p + c),
                        _ => None,
                    }),
                }
            } else {
                TokenUsage::default()
            }
        }
    }
}

/// 从 SSE 流式 chunk 中提取 token 用量
/// 解析 `data: {...}` 行，尝试提取 usage 信息
pub fn extract_streaming_usage(protocol: Protocol, chunk: &str) -> Option<TokenUsage> {
    let mut result: Option<TokenUsage> = None;

    for line in chunk.lines() {
        let line = line.trim();
        if !line.starts_with("data: ") {
            continue;
        }
        let json_str = &line[6..];
        if json_str == "[DONE]" {
            continue;
        }
        let parsed = match serde_json::from_str::<Value>(json_str) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if protocol == Protocol::Anthropic {
            // Anthropic SSE 中 usage 分散在多个事件：
            // - message_start.message.usage 包含 input_tokens
            // - message_delta.usage 包含 output_tokens（及可能的 cache 信息）
            let usage = extract_anthropic_streaming_event(&parsed);
            if usage.prompt_tokens.is_some() || usage.completion_tokens.is_some() {
                result = Some(merge_token_usage(result, usage));
            }
        } else {
            // Chat Completions API: usage 在顶层 parsed["usage"]
            let mut usage = extract_token_usage(protocol, &parsed);
            // Responses API: usage 嵌套在 parsed["response"]["usage"]（如 response.completed 事件）
            if usage.prompt_tokens.is_none() && usage.completion_tokens.is_none() {
                if let Some(response_obj) = parsed.get("response") {
                    usage = extract_token_usage(protocol, response_obj);
                }
            }
            if usage.prompt_tokens.is_some() || usage.completion_tokens.is_some() {
                result = Some(merge_token_usage(result, usage));
            }
        }
    }

    result
}

/// 从 Anthropic 流式事件中提取 token 用量
fn extract_anthropic_streaming_event(event: &Value) -> TokenUsage {
    let event_type = event["type"].as_str().unwrap_or("");

    match event_type {
        // message_start: usage 嵌套在 message.usage 中
        "message_start" => {
            let usage = &event["message"]["usage"];
            let input = usage["input_tokens"].as_i64().map(|v| v as i32);
            let output = usage["output_tokens"].as_i64().map(|v| v as i32);
            TokenUsage {
                prompt_tokens: input,
                completion_tokens: output,
                cached_tokens: usage["cache_read_input_tokens"]
                    .as_i64()
                    .map(|v| v as i32),
                total_tokens: match (input, output) {
                    (Some(i), Some(o)) => Some(i + o),
                    _ => None,
                },
            }
        }
        // message_delta: usage 在顶层
        "message_delta" => {
            let usage = &event["usage"];
            let input = usage["input_tokens"].as_i64().map(|v| v as i32);
            let output = usage["output_tokens"].as_i64().map(|v| v as i32);
            TokenUsage {
                prompt_tokens: input,
                completion_tokens: output,
                cached_tokens: usage["cache_read_input_tokens"]
                    .as_i64()
                    .map(|v| v as i32),
                total_tokens: match (input, output) {
                    (Some(i), Some(o)) => Some(i + o),
                    _ => None,
                },
            }
        }
        _ => TokenUsage::default(),
    }
}

/// 合并两个 TokenUsage，新值覆盖旧值（None 保留旧值）
pub fn merge_token_usage(base: Option<TokenUsage>, update: TokenUsage) -> TokenUsage {
    let base = base.unwrap_or_default();
    let prompt = update.prompt_tokens.or(base.prompt_tokens);
    let completion = update.completion_tokens.or(base.completion_tokens);
    let cached = update.cached_tokens.or(base.cached_tokens);
    TokenUsage {
        prompt_tokens: prompt,
        completion_tokens: completion,
        cached_tokens: cached,
        total_tokens: match (prompt, completion) {
            (Some(p), Some(c)) => Some(p + c),
            _ => base.total_tokens,
        },
    }
}
