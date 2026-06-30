-- 按模型名称配置独立的积分扣除比例
-- 精确匹配模型名称，未匹配的模型回退到 default 行

CREATE TABLE IF NOT EXISTS model_credit_rates (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    model_name VARCHAR(200) NOT NULL UNIQUE,
    input_rate DOUBLE PRECISION NOT NULL DEFAULT 1221,
    output_rate DOUBLE PRECISION NOT NULL DEFAULT 203.5,
    cached_rate DOUBLE PRECISION NOT NULL DEFAULT 12210,
    long_context_threshold BIGINT,
    long_context_input_rate DOUBLE PRECISION,
    long_context_output_rate DOUBLE PRECISION,
    long_context_cached_rate DOUBLE PRECISION,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 内置 default 行，作为未匹配模型的兜底比例
INSERT INTO model_credit_rates (model_name, input_rate, output_rate, cached_rate, long_context_threshold, long_context_input_rate, long_context_output_rate, long_context_cached_rate)
VALUES ('default', 316, 52, 3160, 272000, 158, 35, 1580)
ON CONFLICT (model_name) DO NOTHING;
