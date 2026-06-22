-- 为组织表添加积分单价字段（每积分对应的金额，默认 0 表示未设置）
ALTER TABLE organizations ADD COLUMN IF NOT EXISTS credit_price NUMERIC(18, 6) NOT NULL DEFAULT 0;

-- 为请求日志添加金额消耗字段（= credit_cost * 组织积分单价）
ALTER TABLE request_logs ADD COLUMN IF NOT EXISTS money_cost NUMERIC(18, 6)
