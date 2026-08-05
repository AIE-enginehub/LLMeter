-- Gemini Developer API 标准计费价格（Paid Tier，每百万 Token）。
-- 来源：https://ai.google.dev/gemini-api/docs/pricing
-- 核对日期：2026-08-05。
--
-- Gemini 属于国外模型，按既定规则使用 USD、7.20 结算汇率和 1.68 综合销售系数。
-- Context caching 的按小时存储费用无法由当前请求 usage Token 计算，因此这里只录入缓存读取 Token 单价。

INSERT INTO model_pricings (provider, model_name, is_active)
VALUES
    ('', 'gemini-2.5-pro', true),
    ('', 'gemini-2.5-flash', true),
    ('', 'gemini-3-flash-preview', true),
    ('', 'gemini-3.1-pro-preview', true),
    ('', 'gemini-3.5-flash', true),
    ('', 'gemini-3.6-flash', true)
ON CONFLICT (provider, model_name) DO UPDATE
SET is_active = true,
    updated_at = now();

-- 已存在完全相同的生效版本时不重复插入；已有价格配置则追加不可变版本，不覆盖历史。
WITH seed (
    model_name,
    input_price, cached_input_price, output_price,
    long_context_threshold, long_input_price, long_cached_price, long_output_price
) AS (
    VALUES
        -- 超过 200K 输入后，整个请求使用长上下文价格。
        ('gemini-2.5-pro',          1.25, 0.125, 10.00, 200000, 2.50, 0.25, 15.00),
        ('gemini-2.5-flash',        0.30, 0.030,  2.50,   NULL, NULL, NULL,  NULL),
        ('gemini-3-flash-preview',  0.50, 0.050,  3.00,   NULL, NULL, NULL,  NULL),
        ('gemini-3.1-pro-preview',  2.00, 0.200, 12.00, 200000, 4.00, 0.40, 18.00),
        ('gemini-3.5-flash',        1.50, 0.150,  9.00,   NULL, NULL, NULL,  NULL),
        ('gemini-3.6-flash',        1.50, 0.150,  7.50,   NULL, NULL, NULL,  NULL)
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
    'USD', 'international',
    resolved.input_price, resolved.cached_input_price, NULL, resolved.output_price,
    resolved.long_context_threshold, resolved.long_input_price, resolved.long_cached_price,
    NULL, resolved.long_output_price,
    1.68, 7.20, '2026-08-05 00:00:00+08'::TIMESTAMPTZ
FROM resolved
WHERE NOT EXISTS (
    SELECT 1
    FROM model_price_versions existing
    WHERE existing.pricing_id = resolved.pricing_id
      AND existing.currency = 'USD'
      AND existing.region_type = 'international'
      AND existing.input_price = resolved.input_price
      AND existing.cached_input_price IS NOT DISTINCT FROM resolved.cached_input_price
      AND existing.cache_write_price IS NULL
      AND existing.output_price = resolved.output_price
      AND existing.long_context_threshold IS NOT DISTINCT FROM resolved.long_context_threshold
      AND existing.long_input_price IS NOT DISTINCT FROM resolved.long_input_price
      AND existing.long_cached_price IS NOT DISTINCT FROM resolved.long_cached_price
      AND existing.long_cache_write_price IS NULL
      AND existing.long_output_price IS NOT DISTINCT FROM resolved.long_output_price
      AND existing.multiplier = 1.68
      AND existing.exchange_rate = 7.20
      AND existing.effective_at = '2026-08-05 00:00:00+08'::TIMESTAMPTZ
);
