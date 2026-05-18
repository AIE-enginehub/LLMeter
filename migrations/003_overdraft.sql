-- 为组织表添加透支额度字段（默认 0 表示不允许透支）
ALTER TABLE organizations ADD COLUMN IF NOT EXISTS overdraft_limit NUMERIC(18, 6) NOT NULL DEFAULT 0;
