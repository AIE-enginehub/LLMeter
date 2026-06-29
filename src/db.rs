use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use uuid::Uuid;

// ============================================================
// 数据模型（仅保留代理核心流程使用的结构体）
// ============================================================

/// 组织
#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize, serde::Deserialize)]
pub struct Organization {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub credit: rust_decimal::Decimal,
    pub overdraft_limit: rust_decimal::Decimal,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// API 密钥（颁发给组织的访问凭证）
#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize, serde::Deserialize)]
pub struct ApiKey {
    pub id: Uuid,
    pub org_id: Uuid,
    pub name: String,
    pub key_hash: String,
    pub key_prefix: String,
    pub is_active: bool,
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// 模型配置（每组织的模型转发规则）
#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize, serde::Deserialize)]
pub struct ModelConfig {
    pub id: Uuid,
    pub org_id: Uuid,
    pub name: String,
    pub protocol: String,
    pub model_patterns: String,
    pub base_url: String,
    pub real_api_key: String,
    pub priority: i32,
    pub is_active: bool,
    /// 压缩开关覆盖：None 继承全局，Some(true/false) 覆盖
    pub compression_enabled: Option<bool>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ============================================================
// 连接池与迁移
// ============================================================

/// 初始化 PostgreSQL 连接池
pub async fn init_pool(database_url: &str) -> PgPool {
    PgPoolOptions::new()
        .max_connections(50)
        .min_connections(5)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .idle_timeout(std::time::Duration::from_secs(300))
        .connect(database_url)
        .await
        .expect("无法连接数据库")
}

/// 执行 SQL 迁移（所有语句使用 IF NOT EXISTS，天然幂等）
pub async fn run_migrations(pool: &PgPool) {
    let sql1 = include_str!("../migrations/001_init.sql");
    let sql2 = include_str!("../migrations/002_credit_system.sql");
    let sql3 = include_str!("../migrations/003_overdraft.sql");
    let sql4 = include_str!("../migrations/004_long_context.sql");
    let sql5 = include_str!("../migrations/005_prompt_compression.sql");

    for sql in [sql1, sql2, sql3, sql4, sql5] {
        for statement in sql.split(';') {
            let meaningful: String = statement
                .lines()
                .filter(|line| !line.trim_start().starts_with("--"))
                .collect::<Vec<_>>()
                .join("\n");
            if meaningful.trim().is_empty() {
                continue;
            }
            if let Err(e) = sqlx::query(statement.trim()).execute(pool).await {
                tracing::error!("迁移执行失败: {e}");
            }
        }
    }
    tracing::info!("数据库迁移完成");
}

// ============================================================
// API Key 查询
// ============================================================

/// 根据 key hash 查找 API Key 及其所属组织（仅返回有效记录）
pub async fn find_api_key_by_hash(
    pool: &PgPool,
    hash: &str,
) -> Option<(ApiKey, Organization)> {
    let rec = sqlx::query(
        r#"
        SELECT
            k.id AS k_id, k.org_id, k.name AS k_name, k.key_hash, k.key_prefix,
            k.is_active AS k_active, k.last_used_at, k.created_at AS k_created,
            o.id AS o_id, o.name AS o_name, o.slug, o.credit, o.overdraft_limit,
            o.is_active AS o_active, o.created_at AS o_created, o.updated_at AS o_updated
        FROM api_keys k
        JOIN organizations o ON o.id = k.org_id
        WHERE k.key_hash = $1 AND k.is_active = true AND o.is_active = true
        "#,
    )
    .bind(hash)
    .fetch_optional(pool)
    .await
    .ok()??;

    let api_key = ApiKey {
        id: rec.get("k_id"),
        org_id: rec.get("org_id"),
        name: rec.get("k_name"),
        key_hash: rec.get("key_hash"),
        key_prefix: rec.get("key_prefix"),
        is_active: rec.get("k_active"),
        last_used_at: rec.get("last_used_at"),
        created_at: rec.get("k_created"),
    };

    let org = Organization {
        id: rec.get("o_id"),
        name: rec.get("o_name"),
        slug: rec.get("slug"),
        credit: rec.get("credit"),
        overdraft_limit: rec.get("overdraft_limit"),
        is_active: rec.get("o_active"),
        created_at: rec.get("o_created"),
        updated_at: rec.get("o_updated"),
    };

    Some((api_key, org))
}

/// 更新 API Key 最后使用时间
pub async fn touch_api_key(pool: &PgPool, key_id: Uuid) {
    let _ = sqlx::query("UPDATE api_keys SET last_used_at = now() WHERE id = $1")
        .bind(key_id)
        .execute(pool)
        .await;
}

// ============================================================
// 积分与设置查询
// ============================================================

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct CreditRates {
    pub input_rate: f64,
    pub output_rate: f64,
    pub cached_rate: f64,
    /// 长上下文阈值（输入 Token 数），为 None 或 0 时不启用
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub long_context_threshold: Option<u64>,
    /// 长上下文输入 Token 比例，输入 Token >= 阈值时使用此比例计算
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub long_context_input_rate: Option<f64>,
    /// 长上下文输出 Token 比例
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub long_context_output_rate: Option<f64>,
    /// 长上下文缓存 Token 比例
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub long_context_cached_rate: Option<f64>,
}

impl Default for CreditRates {
    fn default() -> Self {
        Self {
            input_rate: 1221.0,
            output_rate: 203.5,
            cached_rate: 12210.0,
            long_context_threshold: None,
            long_context_input_rate: None,
            long_context_output_rate: None,
            long_context_cached_rate: None,
        }
    }
}

/// 获取全局的积分扣除比例
pub async fn get_credit_rates(pool: &PgPool) -> CreditRates {
    let row = sqlx::query("SELECT value FROM global_settings WHERE key = 'credit_rates'")
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();

    if let Some(r) = row {
        if let Ok(val) = r.try_get::<serde_json::Value, _>("value") {
            if let Ok(rates) = serde_json::from_value(val) {
                return rates;
            }
        }
    }
    CreditRates::default()
}

/// 获取全局提示词压缩配置（global_settings key = 'compression'），缺失时返回禁用的默认值
pub async fn get_compression_config(pool: &PgPool) -> crate::compress::CompressionConfig {
    let row = sqlx::query("SELECT value FROM global_settings WHERE key = 'compression'")
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();

    if let Some(r) = row {
        if let Ok(val) = r.try_get::<serde_json::Value, _>("value") {
            if let Ok(cfg) = serde_json::from_value(val) {
                return cfg;
            }
        }
    }
    crate::compress::CompressionConfig::default()
}

/// 扣除组织积分并记录流水
pub async fn deduct_credit(
    pool: &PgPool,
    org_id: Uuid,
    amount: rust_decimal::Decimal,
    reference_id: &str,
) -> Result<(), sqlx::Error> {
    if amount.is_zero() {
        return Ok(());
    }

    let mut tx = pool.begin().await?;

    let row = sqlx::query(
        "UPDATE organizations SET credit = credit - $1, updated_at = now() WHERE id = $2 RETURNING credit"
    )
    .bind(amount)
    .bind(org_id)
    .fetch_one(&mut *tx)
    .await?;

    let balance_after: rust_decimal::Decimal = row.get("credit");

    sqlx::query(
        "INSERT INTO credit_logs (org_id, amount, balance_after, transaction_type, reference_id) VALUES ($1, $2, $3, 'consume', $4)"
    )
    .bind(org_id)
    .bind(-amount)
    .bind(balance_after)
    .bind(reference_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

/// 获取组织中指定协议的第一个可用模型配置（用于无模型名的透传请求，如 /v1/files, /v1/models）
pub async fn find_first_model_config(pool: &PgPool, org_id: Uuid, protocol: &str) -> Option<ModelConfig> {
    sqlx::query_as::<_, ModelConfig>(
        "SELECT id, org_id, name, protocol, model_patterns, base_url, \
                real_api_key, priority, is_active, compression_enabled, created_at, updated_at \
         FROM model_configs WHERE org_id = $1 AND is_active = true AND protocol = $2 \
         ORDER BY priority ASC LIMIT 1"
    )
    .bind(org_id)
    .bind(protocol)
    .fetch_optional(pool)
    .await
    .ok()?
}

/// 根据组织 ID 和模型名称匹配所有配置（按优先级降序），用于失败重试
pub async fn find_all_model_configs(
    pool: &PgPool,
    org_id: Uuid,
    model: &str,
) -> Vec<ModelConfig> {
    let configs = sqlx::query_as::<_, ModelConfig>(
        r#"
        SELECT id, org_id, name, protocol, model_patterns, base_url,
               real_api_key, priority, is_active, compression_enabled, created_at, updated_at
        FROM model_configs
        WHERE org_id = $1 AND is_active = true
        ORDER BY priority ASC
        "#,
    )
    .bind(org_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    configs.into_iter().filter(|cfg| {
        let patterns = cfg.model_patterns.trim();
        if patterns.is_empty() {
            model.starts_with(cfg.name.trim())
        } else {
            patterns
                .split(',')
                .map(|p| p.trim())
                .filter(|p| !p.is_empty())
                .any(|pattern| glob_match::glob_match(pattern, model))
        }
    }).collect()
}



