-- ============================================================
-- 积分系统迁移
-- ============================================================

-- 1. 为组织表添加 credit 字段
ALTER TABLE organizations ADD COLUMN IF NOT EXISTS credit NUMERIC(18, 6) NOT NULL DEFAULT 0;

-- 2. 全局设置表
CREATE TABLE IF NOT EXISTS global_settings (
    key VARCHAR(50) PRIMARY KEY,
    value JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 插入默认的 Token -> Credit 兑换比例
-- 每 1221 输入 Token 扣除 1 Credit -> input_rate = 1221
-- 每 203.5 输出 Token 扣除 1 Credit -> output_rate = 203.5
-- 每 12210 缓存 Token 扣除 1 Credit -> cached_rate = 12210
INSERT INTO global_settings (key, value) 
VALUES ('credit_rates', '{"input_rate": 1221, "output_rate": 203.5, "cached_rate": 12210}')
ON CONFLICT (key) DO NOTHING;

-- 3. 积分流水表
CREATE TABLE IF NOT EXISTS credit_logs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    amount NUMERIC(18, 6) NOT NULL, -- 正数为增加，负数为扣除
    balance_after NUMERIC(18, 6) NOT NULL,
    transaction_type VARCHAR(20) NOT NULL, -- 'recharge' 或 'consume'
    reference_id VARCHAR(255), -- 关联的 request_log_id 或 备注
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_credit_logs_org ON credit_logs(org_id);
CREATE INDEX IF NOT EXISTS idx_credit_logs_created ON credit_logs(created_at DESC);

-- 4. 为 request_logs 表添加 credit_cost 字段
ALTER TABLE request_logs ADD COLUMN IF NOT EXISTS credit_cost NUMERIC(18, 6);
