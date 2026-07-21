-- 支持独立记录和计费缓存写入 Token。
-- 历史模型配置默认继承输入 Token 比例，升级后可按模型单独调整。

ALTER TABLE request_logs
    ADD COLUMN IF NOT EXISTS cache_write_tokens INT;

ALTER TABLE model_credit_rates
    ADD COLUMN IF NOT EXISTS cache_write_rate DOUBLE PRECISION;

UPDATE model_credit_rates
SET cache_write_rate = input_rate
WHERE cache_write_rate IS NULL;

ALTER TABLE model_credit_rates
    ALTER COLUMN cache_write_rate SET NOT NULL,
    ALTER COLUMN cache_write_rate SET DEFAULT 1221;

ALTER TABLE model_credit_rates
    ADD COLUMN IF NOT EXISTS long_context_cache_write_rate DOUBLE PRECISION;

UPDATE model_credit_rates
SET long_context_cache_write_rate = long_context_input_rate
WHERE long_context_cache_write_rate IS NULL
  AND long_context_input_rate IS NOT NULL;
