-- ============================================================
-- 邮件收发配置迁移
-- ============================================================

INSERT INTO global_settings (key, value)
VALUES (
    'mail_settings',
    '{
      "outbound": {
        "host": "",
        "port": 587,
        "username": "",
        "password": "",
        "sender_email": "",
        "sender_name": "",
        "use_tls": true
      }
    }'::jsonb
)
ON CONFLICT (key) DO NOTHING;
