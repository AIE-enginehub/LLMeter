-- 并非所有模型都有缓存读写费用；比例为空时不计入积分扣除。

ALTER TABLE model_credit_rates
    ALTER COLUMN cached_rate DROP NOT NULL,
    ALTER COLUMN cached_rate DROP DEFAULT,
    ALTER COLUMN cache_write_rate DROP NOT NULL,
    ALTER COLUMN cache_write_rate DROP DEFAULT;
