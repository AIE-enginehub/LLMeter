-- 为请求日志添加长上下文标记
ALTER TABLE request_logs ADD COLUMN IF NOT EXISTS is_long_context BOOLEAN NOT NULL DEFAULT false;
