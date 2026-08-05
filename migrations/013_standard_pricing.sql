-- 双计费模式：保留现有协议比例计费，并新增基于官方单价的标准价格计费。

-- 迁移前已经存在的企业必须继续使用旧计费方式；新企业默认使用标准价格计费。
ALTER TABLE organizations ADD COLUMN IF NOT EXISTS billing_mode VARCHAR(30);
UPDATE organizations SET billing_mode = 'contract_ratio' WHERE billing_mode IS NULL;
ALTER TABLE organizations ALTER COLUMN billing_mode SET DEFAULT 'standard_pricing';
ALTER TABLE organizations ALTER COLUMN billing_mode SET NOT NULL;

ALTER TABLE organizations DROP CONSTRAINT IF EXISTS organizations_billing_mode_check;
ALTER TABLE organizations ADD CONSTRAINT organizations_billing_mode_check
    CHECK (billing_mode IN ('contract_ratio', 'standard_pricing'));

-- 标准价格目录。provider 为空表示该模型价格适用于所有服务商。
CREATE TABLE IF NOT EXISTS model_pricings (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    provider VARCHAR(100) NOT NULL DEFAULT '',
    model_name VARCHAR(200) NOT NULL,
    is_active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(provider, model_name)
);

-- 价格版本只新增、不覆盖，保证历史请求可以复核。
CREATE TABLE IF NOT EXISTS model_price_versions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    pricing_id UUID NOT NULL REFERENCES model_pricings(id) ON DELETE RESTRICT,
    version INT NOT NULL,
    currency VARCHAR(3) NOT NULL,
    region_type VARCHAR(20) NOT NULL,
    input_price NUMERIC(30, 12) NOT NULL,
    cached_input_price NUMERIC(30, 12),
    cache_write_price NUMERIC(30, 12),
    output_price NUMERIC(30, 12) NOT NULL,
    long_context_threshold BIGINT,
    long_input_price NUMERIC(30, 12),
    long_cached_price NUMERIC(30, 12),
    long_cache_write_price NUMERIC(30, 12),
    long_output_price NUMERIC(30, 12),
    multiplier NUMERIC(30, 12) NOT NULL,
    exchange_rate NUMERIC(30, 12) NOT NULL,
    effective_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(pricing_id, version),
    CHECK (currency IN ('CNY', 'USD')),
    CHECK (region_type IN ('domestic', 'international')),
    CHECK (input_price >= 0 AND output_price >= 0),
    CHECK (cached_input_price IS NULL OR cached_input_price >= 0),
    CHECK (cache_write_price IS NULL OR cache_write_price >= 0),
    CHECK (multiplier > 0 AND exchange_rate > 0)
);
CREATE INDEX IF NOT EXISTS idx_model_price_versions_lookup
    ON model_price_versions(pricing_id, effective_at DESC, version DESC);

-- 每笔请求保存当时使用的计费方式、价格版本和完整金额快照。
ALTER TABLE request_logs ADD COLUMN IF NOT EXISTS billing_mode VARCHAR(30) NOT NULL DEFAULT 'contract_ratio';
ALTER TABLE request_logs ADD COLUMN IF NOT EXISTS price_version_id UUID REFERENCES model_price_versions(id) ON DELETE RESTRICT;
ALTER TABLE request_logs ADD COLUMN IF NOT EXISTS official_cost NUMERIC(30, 12);
ALTER TABLE request_logs ADD COLUMN IF NOT EXISTS official_currency VARCHAR(3);
ALTER TABLE request_logs ADD COLUMN IF NOT EXISTS exchange_rate NUMERIC(30, 12);
ALTER TABLE request_logs ADD COLUMN IF NOT EXISTS official_cost_cny NUMERIC(30, 12);
ALTER TABLE request_logs ADD COLUMN IF NOT EXISTS price_multiplier NUMERIC(30, 12);

-- 新计费保留高精度；旧数据数值保持不变。
ALTER TABLE request_logs ALTER COLUMN credit_cost TYPE NUMERIC(30, 6);
ALTER TABLE request_logs ALTER COLUMN money_cost TYPE NUMERIC(30, 12);
ALTER TABLE organizations ALTER COLUMN credit TYPE NUMERIC(30, 6);
ALTER TABLE organizations ALTER COLUMN overdraft_limit TYPE NUMERIC(30, 6);
ALTER TABLE credit_logs ALTER COLUMN amount TYPE NUMERIC(30, 6);
ALTER TABLE credit_logs ALTER COLUMN balance_after TYPE NUMERIC(30, 6);
