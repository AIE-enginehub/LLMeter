-- 隐藏不再提供的标准模型价格。
-- 使用软停用而非物理删除，避免破坏引用旧价格版本的历史请求日志。
UPDATE model_pricings
SET is_active = false,
    updated_at = now()
WHERE provider = ''
  AND model_name IN ('gpt-5.4-nano', 'gpt-5.4-pro', 'gpt-5.6');
