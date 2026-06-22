-- ============================================================
-- 数据迁移：为现有组织创建默认项目，回填 api_keys 和 request_logs
-- ============================================================

-- 1. 为没有默认项目的组织创建默认项目
INSERT INTO projects (org_id, name, description)
SELECT id, 'Default', '自动创建的默认项目'
FROM organizations
WHERE id NOT IN (SELECT org_id FROM projects WHERE name = 'Default')
ON CONFLICT DO NOTHING;

-- 2. 将未分配项目的 api_keys 归入所属组织的默认项目
UPDATE api_keys
SET project_id = (
    SELECT p.id FROM projects p
    WHERE p.org_id = api_keys.org_id AND p.name = 'Default'
    LIMIT 1
)
WHERE project_id IS NULL;

-- 3. 将未分配项目的 request_logs 归入所属组织的默认项目
UPDATE request_logs
SET project_id = (
    SELECT p.id FROM projects p
    WHERE p.org_id = request_logs.org_id AND p.name = 'Default'
    LIMIT 1
)
WHERE project_id IS NULL
