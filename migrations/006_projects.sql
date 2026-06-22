-- ============================================================
-- 项目（Project）层级迁移
-- 在 Organization 和 API Key 之间新增 Project 层
-- 现有数据通过"默认项目"无损承接
-- ============================================================

-- 1. 创建项目表
CREATE TABLE IF NOT EXISTS projects (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name VARCHAR(100) NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    is_active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_projects_org ON projects(org_id);

-- 2. 为 api_keys 添加 project_id 列（允许 NULL 以兼容旧数据）
ALTER TABLE api_keys ADD COLUMN IF NOT EXISTS project_id UUID REFERENCES projects(id) ON DELETE CASCADE;

CREATE INDEX IF NOT EXISTS idx_api_keys_project ON api_keys(project_id);

-- 3. 为 request_logs 添加 project_id 列（允许 NULL 以兼容旧日志）
ALTER TABLE request_logs ADD COLUMN IF NOT EXISTS project_id UUID;

CREATE INDEX IF NOT EXISTS idx_request_logs_project ON request_logs(project_id)
