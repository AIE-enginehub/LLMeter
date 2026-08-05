-- 初始标准计费模型价格（每百万 Token）。
--
-- 价格来源与核对日期（2026-08-05）：
--   OpenAI: https://developers.openai.com/api/docs/pricing （Standard 档）
--   DeepSeek: https://api-docs.deepseek.com/zh-cn/quick_start/pricing/
--   Qwen: 管理员提供的百炼模型价格截图。
--
-- 国外模型按既定规则使用 USD、7.20 结算汇率和 1.68 综合销售系数；
-- 国内模型按 CNY 和 1.30 销售系数。价格版本从北京时间 2026-08-05 00:00 起生效。

-- provider 为空表示价格适用于任意模型转发配置，避免服务商显示名称影响价格匹配。
INSERT INTO model_pricings (provider, model_name, is_active)
VALUES
    ('', 'gpt-5.4', true),
    ('', 'gpt-5.4-mini', true),
    ('', 'gpt-5.6-sol', true),
    ('', 'gpt-5.6-terra', true),
    ('', 'gpt-5.6-luna', true),
    ('', 'qwen3.7-max-2026-06-08', true),
    ('', 'qwen3.8-max', true),
    ('', 'deepseek-v4-pro', true),
    ('', 'deepseek-v4-flash', true),
    ('', 'MiniMax-M3', true)
ON CONFLICT (provider, model_name) DO UPDATE
SET is_active = true,
    updated_at = now();

-- 已存在完全相同的生效版本时不重复插入；如果管理员此前配置过价格，则追加新版本而不覆盖历史。
WITH seed (
    model_name, currency, region_type,
    input_price, cached_input_price, cache_write_price, output_price,
    long_context_threshold, long_input_price, long_cached_price,
    long_cache_write_price, long_output_price,
    multiplier, exchange_rate, effective_at
) AS (
    VALUES
        -- GPT-5.4：超过 272K 输入后，整个请求使用长上下文价格。
        ('gpt-5.4',      'USD', 'international',  2.50, 0.25,  NULL,  15.00, 272000,  5.00, 0.50,  NULL,  22.50, 1.68, 7.20, '2026-08-05 00:00:00+08'::TIMESTAMPTZ),
        ('gpt-5.4-mini', 'USD', 'international',  0.75, 0.075, NULL,   4.50,   NULL,  NULL, NULL,  NULL,   NULL, 1.68, 7.20, '2026-08-05 00:00:00+08'::TIMESTAMPTZ),

        ('gpt-5.6-sol',   'USD', 'international', 5.00, 0.50, 6.25, 30.00, 272000, 10.00, 1.00, 12.50, 45.00, 1.68, 7.20, '2026-08-05 00:00:00+08'::TIMESTAMPTZ),
        ('gpt-5.6-terra', 'USD', 'international', 2.00, 0.20, 2.50, 12.00, 272000,  4.00, 0.40,  5.00, 18.00, 1.68, 7.20, '2026-08-05 00:00:00+08'::TIMESTAMPTZ),
        ('gpt-5.6-luna',  'USD', 'international', 0.20, 0.02, 0.25,  1.20, 272000,  0.40, 0.04,  0.50,  1.80, 1.68, 7.20, '2026-08-05 00:00:00+08'::TIMESTAMPTZ),

        -- Qwen 截图同时列出自动缓存命中和显式缓存命中；当前统一 usage 字段采用“输入（缓存命中）”价格。
        ('qwen3.7-max-2026-06-08', 'CNY', 'domestic', 12.00, 2.40, 15.00, 36.00, NULL, NULL, NULL, NULL, NULL, 1.30, 1.00, '2026-08-05 00:00:00+08'::TIMESTAMPTZ),
        ('qwen3.8-max',             'CNY', 'domestic', 12.00, 1.50, 15.00, 36.00, NULL, NULL, NULL, NULL, NULL, 1.30, 1.00, '2026-08-05 00:00:00+08'::TIMESTAMPTZ),

        -- DeepSeek 官方人民币价格；缓存由平台自动构建，没有独立缓存写入费用。
        ('deepseek-v4-pro',   'CNY', 'domestic', 3.00, 0.025, NULL, 6.00, NULL, NULL, NULL, NULL, NULL, 1.30, 1.00, '2026-08-05 00:00:00+08'::TIMESTAMPTZ),
        ('deepseek-v4-flash', 'CNY', 'domestic', 1.00, 0.020, NULL, 2.00, NULL, NULL, NULL, NULL, NULL, 1.30, 1.00, '2026-08-05 00:00:00+08'::TIMESTAMPTZ),

        -- MiniMax-M3 按业务约定免费：输入、缓存读取和输出均不计费。
        ('MiniMax-M3', 'CNY', 'domestic', 0.00, 0.00, NULL, 0.00, NULL, NULL, NULL, NULL, NULL, 1.30, 1.00, '2026-08-05 00:00:00+08'::TIMESTAMPTZ)
), resolved AS (
    SELECT p.id AS pricing_id, seed.*
    FROM seed
    JOIN model_pricings p ON p.provider = '' AND p.model_name = seed.model_name
)
INSERT INTO model_price_versions (
    pricing_id, version, currency, region_type,
    input_price, cached_input_price, cache_write_price, output_price,
    long_context_threshold, long_input_price, long_cached_price,
    long_cache_write_price, long_output_price,
    multiplier, exchange_rate, effective_at
)
SELECT
    resolved.pricing_id,
    COALESCE((SELECT MAX(existing_version.version)
              FROM model_price_versions existing_version
              WHERE existing_version.pricing_id = resolved.pricing_id), 0) + 1,
    resolved.currency, resolved.region_type,
    resolved.input_price, resolved.cached_input_price, resolved.cache_write_price, resolved.output_price,
    resolved.long_context_threshold, resolved.long_input_price, resolved.long_cached_price,
    resolved.long_cache_write_price, resolved.long_output_price,
    resolved.multiplier, resolved.exchange_rate, resolved.effective_at
FROM resolved
WHERE NOT EXISTS (
    SELECT 1
    FROM model_price_versions existing
    WHERE existing.pricing_id = resolved.pricing_id
      AND existing.currency = resolved.currency
      AND existing.region_type = resolved.region_type
      AND existing.input_price = resolved.input_price
      AND existing.cached_input_price IS NOT DISTINCT FROM resolved.cached_input_price
      AND existing.cache_write_price IS NOT DISTINCT FROM resolved.cache_write_price
      AND existing.output_price = resolved.output_price
      AND existing.long_context_threshold IS NOT DISTINCT FROM resolved.long_context_threshold
      AND existing.long_input_price IS NOT DISTINCT FROM resolved.long_input_price
      AND existing.long_cached_price IS NOT DISTINCT FROM resolved.long_cached_price
      AND existing.long_cache_write_price IS NOT DISTINCT FROM resolved.long_cache_write_price
      AND existing.long_output_price IS NOT DISTINCT FROM resolved.long_output_price
      AND existing.multiplier = resolved.multiplier
      AND existing.exchange_rate = resolved.exchange_rate
      AND existing.effective_at = resolved.effective_at
);
