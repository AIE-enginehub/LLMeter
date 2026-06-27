-- ============================================================
-- 提示词压缩 / Prompt compression
-- 在转发上游前压缩请求体中的自然语言文本，减少送往外部 LLM 的输入 Token。
-- 所有语句均为幂等操作。
-- ============================================================

-- 1. 请求日志：记录压缩是否发生及估算节省（原始 request_body 仍完整保留以供审计）
ALTER TABLE request_logs ADD COLUMN IF NOT EXISTS compressed BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE request_logs ADD COLUMN IF NOT EXISTS compression_mode VARCHAR(20);
ALTER TABLE request_logs ADD COLUMN IF NOT EXISTS original_prompt_chars INT;
ALTER TABLE request_logs ADD COLUMN IF NOT EXISTS forwarded_prompt_chars INT;
ALTER TABLE request_logs ADD COLUMN IF NOT EXISTS est_tokens_saved INT;

-- 2. 模型配置：每模型压缩开关覆盖（NULL 继承全局，true/false 覆盖）
ALTER TABLE model_configs ADD COLUMN IF NOT EXISTS compression_enabled BOOLEAN;

-- 3. 全局压缩配置默认值（默认关闭，需管理员显式开启）
INSERT INTO global_settings (key, value) VALUES (
    'compression',
    '{"enabled":false,"mode":"prose","scope":{"system":true,"user":true,"assistant":false},"min_field_chars":80,"min_savings_pct":5,"max_body_bytes":8388608,"emit_response_header":true}'::jsonb
) ON CONFLICT (key) DO NOTHING;
