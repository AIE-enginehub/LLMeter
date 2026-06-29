//! 预测式提示词压缩 / Predictive prompt compression.
//!
//! 在转发上游前压缩请求体中的自然语言文本，减少送往外部 LLM 的输入 Token，
//! 同时保持对客户端的 API 契约不变。核心词法规则移植自 PRECC
//! (`precc-cc-core/src/compress.rs`，token-saver 模式，MIT-0)，并将其针对
//! CLAUDE.md 设计的空白规则替换为「代码安全」版本——用户提示词常含代码，
//! 绝不能破坏缩进 / 代码块。
//!
//! Compresses natural-language text in the request body before forwarding upstream,
//! reducing input tokens sent to the external LLM while keeping the client-facing API
//! contract unchanged. Lexical rules are ported from PRECC; whitespace handling is a
//! code-safe variant (prompts routinely embed code, so indentation must be preserved).

use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

use crate::protocol::Protocol;

// ============================================================
// 配置 / Configuration
// ============================================================

/// 压缩后端模式。当前仅 `Prose`（确定性词法压缩）；预留 `Statistical` 等有损后端。
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CompressionMode {
    Off,
    Prose,
}

/// 压缩作用域：仅压缩哪些角色的文本。
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct Scope {
    #[serde(default = "d_true")]
    pub system: bool,
    #[serde(default = "d_true")]
    pub user: bool,
    #[serde(default)]
    pub assistant: bool,
}

impl Default for Scope {
    fn default() -> Self {
        Self { system: true, user: true, assistant: false }
    }
}

/// 全局压缩配置（持久化于 global_settings，key = 'compression'）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompressionConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "d_mode")]
    pub mode: CompressionMode,
    #[serde(default)]
    pub scope: Scope,
    /// 小于该字符数的字段跳过（避免对极短文本做净负收益压缩）
    #[serde(default = "d_min_field_chars")]
    pub min_field_chars: usize,
    /// 单字段估算节省比例低于该百分比时保留原文
    #[serde(default = "d_min_savings_pct")]
    pub min_savings_pct: usize,
    /// 请求体超过该字节数时整体跳过压缩
    #[serde(default = "d_max_body_bytes")]
    pub max_body_bytes: usize,
    /// 是否在响应中附带 X-LLMeter-Compression 信息头
    #[serde(default = "d_true")]
    pub emit_response_header: bool,
}

fn d_true() -> bool { true }
fn d_mode() -> CompressionMode { CompressionMode::Prose }
fn d_min_field_chars() -> usize { 80 }
fn d_min_savings_pct() -> usize { 5 }
fn d_max_body_bytes() -> usize { 8 * 1024 * 1024 }

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: CompressionMode::Prose,
            scope: Scope::default(),
            min_field_chars: d_min_field_chars(),
            min_savings_pct: d_min_savings_pct(),
            max_body_bytes: d_max_body_bytes(),
            emit_response_header: d_true(),
        }
    }
}

/// 单次请求的压缩统计。
#[derive(Debug, Clone, Default)]
pub struct CompressionStats {
    pub fields_compressed: usize,
    pub chars_before: usize,
    pub chars_after: usize,
    pub est_tokens_saved: usize,
}

impl CompressionStats {
    pub fn did_compress(&self) -> bool {
        self.fields_compressed > 0
    }

    /// 供 X-LLMeter-Compression 响应头使用的摘要。
    pub fn header_value(&self, mode: CompressionMode) -> String {
        let mode_str = match mode {
            CompressionMode::Prose => "prose",
            CompressionMode::Off => "off",
        };
        format!(
            "mode={mode_str}; fields={}; est_saved={}",
            self.fields_compressed, self.est_tokens_saved
        )
    }
}

// ============================================================
// 词法压缩规则（移植自 PRECC，去掉破坏缩进的空白规则）
// ============================================================

struct Rule {
    re: Regex,
    replacement: &'static str,
}

macro_rules! rule {
    ($pat:expr, $rep:expr) => {
        Rule { re: Regex::new($pat).unwrap(), replacement: $rep }
    };
}

/// 词法规则：去除填充词、改写冗长短语。均为 `\b` 锚定、与空白无关，移植自 PRECC。
/// 注意：PRECC 原版末尾的四条空白折叠规则（`\n +`、` +\n`、`  +`、`\n{3,}`）在此
/// **不**包含——它们会剥离行首缩进，破坏提示词中的代码。代码安全的空白处理见
/// [`process_prose_line`] 与 [`collapse_blank_lines`]。
static RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![
        // 填充词去除 / Filler removal
        rule!(r"(?i)\bplease\b", ""),
        rule!(r"(?i)\bkindly\b", ""),
        rule!(r"(?i)\bjust\b", ""),
        rule!(r"(?i)\bsimply\b", ""),
        rule!(r"(?i)\bbasically\b", ""),
        rule!(r"(?i)\bactually\b", ""),
        rule!(r"(?i)\bIn order to\b", "To"),
        rule!(r"(?i)\bdue to the fact that\b", "because"),
        rule!(r"(?i)\bat this point in time\b", "now"),
        rule!(r"(?i)\bin the event that\b", "if"),
        rule!(r"(?i)\bfor the purpose of\b", "to"),
        rule!(r"(?i)\bwith regard to\b", "re:"),
        rule!(r"(?i)\bin terms of\b", "re:"),
        rule!(r"(?i)\bIt is important to note that\b", "Note:"),
        rule!(r"(?i)\bIt should be noted that\b", "Note:"),
        rule!(r"(?i)\bAs mentioned (?:earlier|previously|above)\b", ""),
        rule!(r"(?i)\bAs you (?:may |might )?know\b", ""),
        // 动作短语 / Action patterns
        rule!(r"(?i)\bBefore doing anything else\b", "First"),
        // 常见短语 / Common phrases
        rule!(r"(?i)\byou should\b", ""),
        rule!(r"(?i)\bmake sure (?:to |that )?", "ensure "),
        rule!(r"(?i)\bkeep in mind (?:that )?", "note: "),
        rule!(r"(?i)\bfor example\b", "e.g."),
        rule!(r"(?i)\bsuch as\b", "e.g."),
        rule!(r"(?i)\betc\.?\b", "..."),
        rule!(r"(?i)\band so on\b", "..."),
        rule!(r"(?i)\band others?\b", "..."),
        rule!(r"(?i)\bincluding but not limited to\b", "incl."),
        rule!(r"(?i)\bin other words\b", "i.e."),
        rule!(r"(?i)\bthat is to say\b", "i.e."),
    ]
});

/// 折叠行内多个空格（≥2）的正则——仅作用于反引号外的散文片段。
static MULTISPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"  +").unwrap());
/// 折叠 3 个及以上连续换行为 2 个（最多保留一个空行）。
static BLANKLINES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n{3,}").unwrap());

/// 估算 Token 数（1 token ≈ 4 字节）。仅用于日志中的「估算」节省值；权威用量来自上游。
pub fn estimate_tokens(text: &str) -> usize {
    text.len() / 4
}

/// 代码安全的散文压缩。
///
/// 逐行处理：围栏代码块（``` / ~~~）、缩进代码行（≥4 空格或制表符开头）整行原样保留；
/// 散文行保留行首缩进、保护反引号内联片段，仅对其余部分应用词法规则并折叠行内多空格；
/// 最后折叠多余空行。绝不剥离行首缩进。
pub fn compress_prose(input: &str) -> String {
    let ends_nl = input.ends_with('\n');
    let mut out: Vec<String> = Vec::new();
    let mut in_fence = false;

    for line in input.split('\n') {
        let trimmed_start = line.trim_start();

        // 围栏代码块边界：原样保留并切换状态
        if trimmed_start.starts_with("```") || trimmed_start.starts_with("~~~") {
            out.push(line.to_string());
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            out.push(line.to_string());
            continue;
        }

        // 行首缩进 / 缩进代码（≥4 空格或含制表符）→ 整行保留
        let indent_len = line.len() - trimmed_start.len();
        let leading = &line[..indent_len];
        if leading.contains('\t') || indent_len >= 4 {
            out.push(line.to_string());
            continue;
        }

        // 散文行：保留行首缩进，处理其余部分
        let body = process_prose_line(trimmed_start);
        let mut combined = String::with_capacity(leading.len() + body.len());
        combined.push_str(leading);
        combined.push_str(body.trim_end());
        out.push(combined);
    }

    let joined = out.join("\n");
    let collapsed = BLANKLINES.replace_all(&joined, "\n\n").to_string();
    if ends_nl && !collapsed.ends_with('\n') {
        format!("{collapsed}\n")
    } else {
        collapsed
    }
}

/// 处理单个散文行（已去除行首缩进）：保护反引号内联代码，仅对其外部应用词法规则。
fn process_prose_line(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for (i, seg) in s.split('`').enumerate() {
        if i > 0 {
            result.push('`');
        }
        if i % 2 == 0 {
            // 反引号外：散文，应用规则 + 折叠多空格
            let mut t = seg.to_string();
            for r in RULES.iter() {
                t = r.re.replace_all(&t, r.replacement).to_string();
            }
            t = MULTISPACE.replace_all(&t, " ").to_string();
            result.push_str(&t);
        } else {
            // 反引号内：原样保留
            result.push_str(seg);
        }
    }
    result
}

// ============================================================
// 协议感知字段遍历 / Protocol-aware field walker
// ============================================================

/// 压缩请求体中符合作用域的自然语言文本字段，返回新 body 与统计。
///
/// 只替换字符串叶子值，绝不增删/重排 JSON 键或数组元素——结构可证明保持不变。
pub fn compress_request(
    protocol: Protocol,
    mut body: Value,
    cfg: &CompressionConfig,
) -> (Value, CompressionStats) {
    let mut stats = CompressionStats::default();
    match protocol {
        Protocol::OpenAI => compress_openai(&mut body, cfg, &mut stats),
        Protocol::Anthropic => compress_anthropic(&mut body, cfg, &mut stats),
        Protocol::Gemini => compress_gemini(&mut body, cfg, &mut stats),
    }
    (body, stats)
}

/// OpenAI 角色 → 作用域映射。tool/function 永不压缩。
fn role_in_scope_openai(role: &str, cfg: &CompressionConfig) -> bool {
    match role {
        "system" | "developer" => cfg.scope.system,
        "user" => cfg.scope.user,
        "assistant" => cfg.scope.assistant,
        _ => false,
    }
}

fn compress_openai(body: &mut Value, cfg: &CompressionConfig, stats: &mut CompressionStats) {
    // Chat Completions: messages[]
    if let Some(msgs) = body.get_mut("messages").and_then(|v| v.as_array_mut()) {
        for m in msgs.iter_mut() {
            let role = m.get("role").and_then(|r| r.as_str()).unwrap_or("").to_string();
            if !role_in_scope_openai(&role, cfg) {
                continue;
            }
            if let Some(content) = m.get_mut("content") {
                compress_content(content, &["text", "input_text"], cfg, stats);
            }
        }
    }

    // Responses API: instructions（指令，系统级）
    if cfg.scope.system {
        if let Some(instr) = body.get_mut("instructions") {
            compress_leaf(instr, cfg, stats);
        }
    }
    // Responses API: input（字符串或消息数组）
    if let Some(input) = body.get_mut("input") {
        match input {
            Value::String(_) => {
                if cfg.scope.user {
                    compress_leaf(input, cfg, stats);
                }
            }
            Value::Array(items) => {
                for it in items.iter_mut() {
                    let role = it.get("role").and_then(|r| r.as_str()).unwrap_or("user").to_string();
                    if !role_in_scope_openai(&role, cfg) {
                        continue;
                    }
                    if let Some(content) = it.get_mut("content") {
                        compress_content(content, &["text", "input_text"], cfg, stats);
                    }
                }
            }
            _ => {}
        }
    }
}

fn compress_anthropic(body: &mut Value, cfg: &CompressionConfig, stats: &mut CompressionStats) {
    // 顶层 system：字符串或 text block 数组
    if cfg.scope.system {
        if let Some(sys) = body.get_mut("system") {
            match sys {
                Value::String(_) => compress_leaf(sys, cfg, stats),
                Value::Array(blocks) => {
                    for b in blocks.iter_mut() {
                        if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                            if let Some(t) = b.get_mut("text") {
                                compress_leaf(t, cfg, stats);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    if let Some(msgs) = body.get_mut("messages").and_then(|v| v.as_array_mut()) {
        for m in msgs.iter_mut() {
            let role = m.get("role").and_then(|r| r.as_str()).unwrap_or("");
            let allow = match role {
                "user" => cfg.scope.user,
                "assistant" => cfg.scope.assistant,
                _ => false,
            };
            if !allow {
                continue;
            }
            if let Some(content) = m.get_mut("content") {
                // Anthropic 仅 {type:"text"} 块可压缩；tool_result/image/document/tool_use 跳过
                compress_content(content, &["text"], cfg, stats);
            }
        }
    }
}

fn compress_gemini(body: &mut Value, cfg: &CompressionConfig, stats: &mut CompressionStats) {
    if cfg.scope.system {
        if let Some(parts) = body
            .get_mut("systemInstruction")
            .and_then(|v| v.get_mut("parts"))
            .and_then(|v| v.as_array_mut())
        {
            for p in parts.iter_mut() {
                if let Some(t) = p.get_mut("text") {
                    compress_leaf(t, cfg, stats);
                }
            }
        }
    }

    if let Some(contents) = body.get_mut("contents").and_then(|v| v.as_array_mut()) {
        for c in contents.iter_mut() {
            let role = c.get("role").and_then(|r| r.as_str()).unwrap_or("user");
            let allow = match role {
                "model" => cfg.scope.assistant,
                _ => cfg.scope.user,
            };
            if !allow {
                continue;
            }
            if let Some(parts) = c.get_mut("parts").and_then(|v| v.as_array_mut()) {
                for p in parts.iter_mut() {
                    // 仅含 text 字符串的 part（跳过 inlineData/fileData/functionCall/functionResponse）
                    if p.get("text").map(|t| t.is_string()).unwrap_or(false) {
                        if let Some(t) = p.get_mut("text") {
                            compress_leaf(t, cfg, stats);
                        }
                    }
                }
            }
        }
    }
}

/// content 字段可能是字符串或 part 数组；只压缩允许的文本 part。
fn compress_content(
    content: &mut Value,
    text_types: &[&str],
    cfg: &CompressionConfig,
    stats: &mut CompressionStats,
) {
    match content {
        Value::String(_) => compress_leaf(content, cfg, stats),
        Value::Array(parts) => {
            for p in parts.iter_mut() {
                let ptype = p.get("type").and_then(|t| t.as_str()).unwrap_or("");
                if text_types.contains(&ptype) {
                    if let Some(t) = p.get_mut("text") {
                        compress_leaf(t, cfg, stats);
                    }
                }
            }
        }
        _ => {}
    }
}

/// 压缩一个字符串叶子，带逐字段安全护栏：过短跳过、收益不足/会变长/变空则保留原文。
fn compress_leaf(v: &mut Value, cfg: &CompressionConfig, stats: &mut CompressionStats) {
    let orig = match v {
        Value::String(s) => s.clone(),
        _ => return,
    };
    if let Some(new) = try_compress(&orig, cfg, stats) {
        *v = Value::String(new);
    }
}

/// 返回 `Some(compressed)` 仅当通过所有护栏；否则 `None`（保留原文）。
fn try_compress(orig: &str, cfg: &CompressionConfig, stats: &mut CompressionStats) -> Option<String> {
    if orig.chars().count() < cfg.min_field_chars {
        return None;
    }
    let compressed = compress_prose(orig);
    // 绝不变长 / 不变空
    if compressed.len() >= orig.len() {
        return None;
    }
    if compressed.trim().is_empty() && !orig.trim().is_empty() {
        return None;
    }
    let est_orig = estimate_tokens(orig);
    let est_new = estimate_tokens(&compressed);
    let saved = est_orig.saturating_sub(est_new);
    if est_orig == 0 || saved * 100 / est_orig < cfg.min_savings_pct {
        return None;
    }
    stats.fields_compressed += 1;
    stats.chars_before += orig.len();
    stats.chars_after += compressed.len();
    stats.est_tokens_saved += saved;
    Some(compressed)
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cfg() -> CompressionConfig {
        // 测试用：启用且阈值放宽，便于断言
        CompressionConfig {
            enabled: true,
            min_field_chars: 1,
            min_savings_pct: 0,
            ..Default::default()
        }
    }

    // ── compress_prose：词法 ──────────────────────────────────
    #[test]
    fn prose_removes_filler() {
        let out = compress_prose("Please just make sure to run the tests");
        assert!(!out.to_lowercase().contains("please"));
        assert!(!out.to_lowercase().contains("just"));
        assert!(out.contains("ensure"));
        assert!(out.contains("run the tests"));
    }

    #[test]
    fn prose_rewrites_phrases() {
        let out = compress_prose("In order to fix it, due to the fact that it fails.");
        assert!(out.contains("To"));
        assert!(out.contains("because"));
    }

    // ── compress_prose：代码安全 ──────────────────────────────
    #[test]
    fn prose_preserves_fenced_code_and_indent() {
        let input = "Please run this:\n```python\ndef f():\n    just_value = 1\n```\nactually done";
        let out = compress_prose(input);
        // 围栏内的缩进与 "just_value" 原样保留
        assert!(out.contains("    just_value = 1"));
        assert!(out.contains("```python"));
        // 围栏外的 filler 被移除
        assert!(!out.to_lowercase().contains("please run"));
        assert!(!out.contains("actually done"));
    }

    #[test]
    fn prose_preserves_indented_code_lines() {
        let input = "Note this:\n        deeply_indented(just=1)";
        let out = compress_prose(input);
        assert!(out.contains("        deeply_indented(just=1)"));
    }

    #[test]
    fn prose_preserves_inline_backticks() {
        let out = compress_prose("Use the `please_keep_this` helper to just run");
        assert!(out.contains("`please_keep_this`"));
        // 反引号外的 "just" 仍被移除
        assert!(!out.contains("to just run"));
    }

    #[test]
    fn prose_never_strips_leading_indent_under_four() {
        let out = compress_prose("  hello world");
        assert!(out.starts_with("  hello"));
    }

    // ── 协议遍历：结构保持 ────────────────────────────────────
    #[test]
    fn openai_compresses_user_and_system_only() {
        let body = json!({
            "model": "gpt-4o",
            "messages": [
                {"role": "system", "content": "Please just be concise in order to help."},
                {"role": "user", "content": "Kindly make sure to actually run all of the test suites now."},
                {"role": "assistant", "content": "Please keep this assistant text intact for now."}
            ],
            "tools": [{"type": "function", "function": {"name": "please_keep"}}]
        });
        let (out, stats) = compress_request(Protocol::OpenAI, body, &cfg());
        assert!(stats.fields_compressed >= 2);
        // assistant 默认跳过
        assert_eq!(out["messages"][2]["content"], json!("Please keep this assistant text intact for now."));
        // tools 原样
        assert_eq!(out["tools"][0]["function"]["name"], json!("please_keep"));
        // 结构不变：仍是 3 条消息
        assert_eq!(out["messages"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn openai_multimodal_parts_preserved() {
        let body = json!({
            "messages": [{"role": "user", "content": [
                {"type": "text", "text": "Please just make sure to describe this image in detail."},
                {"type": "image_url", "image_url": {"url": "http://x/y.png"}}
            ]}]
        });
        let (out, stats) = compress_request(Protocol::OpenAI, body, &cfg());
        assert_eq!(stats.fields_compressed, 1);
        assert_eq!(out["messages"][0]["content"][1]["image_url"]["url"], json!("http://x/y.png"));
        assert!(!out["messages"][0]["content"][0]["text"].as_str().unwrap().to_lowercase().contains("please"));
    }

    #[test]
    fn anthropic_system_and_user_text_blocks() {
        let body = json!({
            "system": "Please just answer in order to be helpful and concise overall.",
            "messages": [{"role": "user", "content": [
                {"type": "text", "text": "Kindly make sure to actually summarize the document now."},
                {"type": "tool_result", "content": "please keep raw"}
            ]}]
        });
        let (out, _) = compress_request(Protocol::Anthropic, body, &cfg());
        assert!(!out["system"].as_str().unwrap().to_lowercase().contains("please just"));
        assert_eq!(out["messages"][0]["content"][1]["content"], json!("please keep raw"));
    }

    #[test]
    fn gemini_text_parts_by_role() {
        let body = json!({
            "systemInstruction": {"parts": [{"text": "Please just be helpful in order to assist users."}]},
            "contents": [
                {"role": "user", "parts": [{"text": "Kindly make sure to actually answer the question now please."}]},
                {"role": "model", "parts": [{"text": "Please keep this model turn intact for sure now."}]}
            ]
        });
        let (out, stats) = compress_request(Protocol::Gemini, body, &cfg());
        assert!(stats.fields_compressed >= 2);
        assert_eq!(out["contents"][1]["parts"][0]["text"], json!("Please keep this model turn intact for sure now."));
    }

    // ── 护栏 ──────────────────────────────────────────────────
    #[test]
    fn guard_skips_short_fields() {
        let mut c = cfg();
        c.min_field_chars = 80;
        let body = json!({"messages": [{"role": "user", "content": "Please run."}]});
        let (out, stats) = compress_request(Protocol::OpenAI, body, &c);
        assert_eq!(stats.fields_compressed, 0);
        assert_eq!(out["messages"][0]["content"], json!("Please run."));
    }

    #[test]
    fn guard_min_savings_keeps_original() {
        let mut c = cfg();
        c.min_savings_pct = 90; // 不可能达到
        let body = json!({"messages": [{"role": "user", "content": "Please just make sure to run all of the integration tests now."}]});
        let (_out, stats) = compress_request(Protocol::OpenAI, body, &c);
        assert_eq!(stats.fields_compressed, 0);
    }

    #[test]
    fn disabled_default_config() {
        let c = CompressionConfig::default();
        assert!(!c.enabled);
        assert_eq!(c.mode, CompressionMode::Prose);
        assert_eq!(c.min_field_chars, 80);
    }
}
