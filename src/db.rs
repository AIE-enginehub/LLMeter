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
    pub credit_price: rust_decimal::Decimal,
    pub billing_mode: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// API 密钥（颁发给组织的访问凭证，归属于项目）
#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize, serde::Deserialize)]
pub struct ApiKey {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_id: Option<Uuid>,
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
    // 创建迁移记录表（追踪已执行的迁移，确保每个文件只执行一次）
    let _ = sqlx::query(
        "CREATE TABLE IF NOT EXISTS _migrations (
            name VARCHAR(255) PRIMARY KEY,
            applied_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )"
    ).execute(pool).await;

    let migrations: &[(&str, &str)] = &[
        ("001_init",                include_str!("../migrations/001_init.sql")),
        ("002_credit_system",       include_str!("../migrations/002_credit_system.sql")),
        ("003_overdraft",           include_str!("../migrations/003_overdraft.sql")),
        ("004_long_context",        include_str!("../migrations/004_long_context.sql")),
        ("005_mail_settings",       include_str!("../migrations/005_mail_settings.sql")),
        ("006_projects",            include_str!("../migrations/006_projects.sql")),
        ("007_project_data_migration", include_str!("../migrations/007_project_data_migration.sql")),
        ("008_credit_price",        include_str!("../migrations/008_credit_price.sql")),
        ("009_prompt_compression",  include_str!("../migrations/009_prompt_compression.sql")),
        ("010_model_credit_rates",  include_str!("../migrations/010_model_credit_rates.sql")),
        ("011_cache_write_tokens",  include_str!("../migrations/011_cache_write_tokens.sql")),
        ("012_optional_cache_rates", include_str!("../migrations/012_optional_cache_rates.sql")),
        ("013_standard_pricing",     include_str!("../migrations/013_standard_pricing.sql")),
        ("014_seed_model_pricings",  include_str!("../migrations/014_seed_model_pricings.sql")),
        ("015_remove_unwanted_model_pricings", include_str!("../migrations/015_remove_unwanted_model_pricings.sql")),
        ("016_seed_gemini_model_pricings", include_str!("../migrations/016_seed_gemini_model_pricings.sql")),
    ];

    for (name, sql) in migrations {
        let applied: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM _migrations WHERE name = $1)")
            .bind(name)
            .fetch_one(pool)
            .await
            .unwrap_or(false);

        if applied {
            continue;
        }

        tracing::info!("执行迁移: {name}");
        let mut has_error = false;
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
                tracing::error!("迁移 {name} 执行失败: {e}");
                has_error = true;
                break;
            }
        }

        if !has_error {
            let _ = sqlx::query("INSERT INTO _migrations (name) VALUES ($1) ON CONFLICT DO NOTHING")
                .bind(name)
                .execute(pool)
                .await;
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
            k.id AS k_id, k.org_id, k.project_id, k.name AS k_name, k.key_hash, k.key_prefix,
            k.is_active AS k_active, k.last_used_at, k.created_at AS k_created,
            o.id AS o_id, o.name AS o_name, o.slug, o.credit, o.overdraft_limit, o.credit_price, o.billing_mode,
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
        project_id: rec.get("project_id"),
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
        credit_price: rec.get("credit_price"),
        billing_mode: rec.get("billing_mode"),
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
    /// 缓存写入 Token 比例；旧全局配置缺失时使用默认比例
    #[serde(default = "default_cache_write_rate")]
    pub cache_write_rate: f64,
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
    /// 长上下文缓存写入 Token 比例
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub long_context_cache_write_rate: Option<f64>,
}

fn default_cache_write_rate() -> f64 {
    1221.0
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
pub struct SmtpSettings {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub sender_email: String,
    pub sender_name: String,
    #[serde(default = "default_true")]
    pub use_tls: bool,
}

impl Default for SmtpSettings {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: 587,
            username: String::new(),
            password: String::new(),
            sender_email: String::new(),
            sender_name: String::new(),
            use_tls: true,
        }
    }
}

pub const DEFAULT_SYSTEM_CONTACT_EMAIL: &str = "contact@enginehub.cn";

fn default_system_contact_email() -> String {
    DEFAULT_SYSTEM_CONTACT_EMAIL.to_string()
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
pub struct MailSettings {
    #[serde(default = "default_system_contact_email")]
    pub system_contact_email: String,
    #[serde(default)]
    pub outbound: SmtpSettings,
}

impl Default for MailSettings {
    fn default() -> Self {
        Self {
            system_contact_email: default_system_contact_email(),
            outbound: SmtpSettings::default(),
        }
    }
}

fn default_true() -> bool {
    true
}

impl Default for CreditRates {
    fn default() -> Self {
        Self {
            input_rate: 1221.0,
            output_rate: 203.5,
            cached_rate: 12210.0,
            cache_write_rate: default_cache_write_rate(),
            long_context_threshold: None,
            long_context_input_rate: None,
            long_context_output_rate: None,
            long_context_cached_rate: None,
            long_context_cache_write_rate: None,
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

/// 获取邮件收发配置（存储在 global_settings.mail_settings）
pub async fn get_mail_settings(pool: &PgPool) -> MailSettings {
    let row = sqlx::query("SELECT value FROM global_settings WHERE key = 'mail_settings'")
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();

    if let Some(r) = row {
        if let Ok(val) = r.try_get::<serde_json::Value, _>("value") {
            if let Ok(settings) = serde_json::from_value(val) {
                return settings;
            }
        }
    }
    MailSettings::default()
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

// ============================================================
// 模型积分扣除比例
// ============================================================

/// 按模型名称配置的独立积分扣除比例（精确匹配模型名称）
#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize, serde::Deserialize)]
pub struct ModelCreditRate {
    pub id: Uuid,
    pub model_name: String,
    pub input_rate: f64,
    pub output_rate: f64,
    pub cached_rate: Option<f64>,
    pub cache_write_rate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub long_context_threshold: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub long_context_input_rate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub long_context_output_rate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub long_context_cached_rate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub long_context_cache_write_rate: Option<f64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub const MODEL_RATE_COLS: &str = "id, model_name, input_rate, output_rate, cached_rate, cache_write_rate, \
    long_context_threshold, long_context_input_rate, long_context_output_rate, long_context_cached_rate, long_context_cache_write_rate, \
    created_at, updated_at";

/// 按模型名称精确匹配积分扣除比例，未匹配时回退到 default 行
pub async fn find_model_credit_rate(pool: &PgPool, model: &str) -> Option<ModelCreditRate> {
    let row = sqlx::query_as::<_, ModelCreditRate>(
        &format!("SELECT {MODEL_RATE_COLS} FROM model_credit_rates WHERE model_name = $1")
    )
    .bind(model)
    .fetch_optional(pool)
    .await
    .ok()?;

    if row.is_some() {
        return row;
    }

    sqlx::query_as::<_, ModelCreditRate>(
        &format!("SELECT {MODEL_RATE_COLS} FROM model_credit_rates WHERE model_name = 'default'")
    )
    .fetch_optional(pool)
    .await
    .ok()?
}

/// 列出所有模型积分扣除比例配置
pub async fn list_model_credit_rates(pool: &PgPool) -> Vec<ModelCreditRate> {
    sqlx::query_as::<_, ModelCreditRate>(
        &format!("SELECT {MODEL_RATE_COLS} FROM model_credit_rates ORDER BY model_name ASC")
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

// ============================================================
// 标准价格计费
// ============================================================

/// 请求开始时选定的不可变模型价格版本。
#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct StandardModelPrice {
    pub pricing_id: Uuid,
    pub price_version_id: Uuid,
    pub provider: String,
    pub model_name: String,
    pub version: i32,
    pub currency: String,
    pub region_type: String,
    pub input_price: rust_decimal::Decimal,
    pub cached_input_price: Option<rust_decimal::Decimal>,
    pub cache_write_price: Option<rust_decimal::Decimal>,
    pub output_price: rust_decimal::Decimal,
    pub long_context_threshold: Option<i64>,
    pub long_input_price: Option<rust_decimal::Decimal>,
    pub long_cached_price: Option<rust_decimal::Decimal>,
    pub long_cache_write_price: Option<rust_decimal::Decimal>,
    pub long_output_price: Option<rust_decimal::Decimal>,
    pub multiplier: rust_decimal::Decimal,
    pub exchange_rate: rust_decimal::Decimal,
    pub effective_at: DateTime<Utc>,
}

pub const STANDARD_MODEL_PRICE_COLS: &str =
    "p.id AS pricing_id, v.id AS price_version_id, p.provider, p.model_name, v.version, \
     v.currency, v.region_type, v.input_price, v.cached_input_price, v.cache_write_price, \
     v.output_price, v.long_context_threshold, v.long_input_price, v.long_cached_price, \
     v.long_cache_write_price, v.long_output_price, v.multiplier, v.exchange_rate, v.effective_at";

/// 优先匹配具体服务商，其次匹配服务商为空的通用价格；只选择请求开始时已生效的版本。
pub async fn find_standard_model_price(
    pool: &PgPool,
    provider: &str,
    model: &str,
    at: DateTime<Utc>,
) -> Option<StandardModelPrice> {
    sqlx::query_as::<_, StandardModelPrice>(&format!(
        "SELECT {STANDARD_MODEL_PRICE_COLS} \
         FROM model_pricings p \
         JOIN LATERAL ( \
             SELECT * FROM model_price_versions pv \
             WHERE pv.pricing_id = p.id AND pv.effective_at <= $3 \
             ORDER BY pv.effective_at DESC, pv.version DESC LIMIT 1 \
         ) v ON true \
         WHERE p.is_active = true AND p.model_name = $2 AND (p.provider = $1 OR p.provider = '') \
         ORDER BY CASE WHEN p.provider = $1 THEN 0 ELSE 1 END LIMIT 1"
    ))
    .bind(provider)
    .bind(model)
    .bind(at)
    .fetch_optional(pool)
    .await
    .ok()?
}

#[derive(Debug, Clone)]
pub struct StandardChargeSnapshot {
    pub billing_mode: &'static str,
    pub price_version_id: Uuid,
    pub official_cost: rust_decimal::Decimal,
    pub official_currency: String,
    pub exchange_rate: rust_decimal::Decimal,
    pub official_cost_cny: rust_decimal::Decimal,
    pub price_multiplier: rust_decimal::Decimal,
    pub money_cost: rust_decimal::Decimal,
    pub credit_cost: rust_decimal::Decimal,
    pub is_long_context: bool,
}

/// 原子写入费用快照、扣减余额并记录积分流水。
/// 通过锁定请求日志并检查 credit_cost，保证同一请求不会重复扣款。
pub async fn apply_standard_charge(
    pool: &PgPool,
    org_id: Uuid,
    log_id: Uuid,
    charge: &StandardChargeSnapshot,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let existing: Option<rust_decimal::Decimal> = sqlx::query_scalar(
        "SELECT credit_cost FROM request_logs WHERE id = $1 FOR UPDATE",
    )
    .bind(log_id)
    .fetch_one(&mut *tx)
    .await?;
    if existing.is_some() {
        tx.rollback().await?;
        return Ok(false);
    }

    let balance_after: rust_decimal::Decimal = sqlx::query_scalar(
        "UPDATE organizations SET credit = credit - $1, updated_at = now() \
         WHERE id = $2 RETURNING credit",
    )
    .bind(charge.credit_cost)
    .bind(org_id)
    .fetch_one(&mut *tx)
    .await?;

    if !charge.credit_cost.is_zero() {
        sqlx::query(
            "INSERT INTO credit_logs (org_id, amount, balance_after, transaction_type, reference_id) \
             VALUES ($1, $2, $3, 'consume', $4)",
        )
        .bind(org_id)
        .bind(-charge.credit_cost)
        .bind(balance_after)
        .bind(log_id.to_string())
        .execute(&mut *tx)
        .await?;
    }

    sqlx::query(
        "UPDATE request_logs SET billing_mode = $2, price_version_id = $3, official_cost = $4, \
             official_currency = $5, exchange_rate = $6, official_cost_cny = $7, \
             price_multiplier = $8, money_cost = $9, credit_cost = $10, is_long_context = $11 \
         WHERE id = $1",
    )
    .bind(log_id)
    .bind(charge.billing_mode)
    .bind(charge.price_version_id)
    .bind(charge.official_cost)
    .bind(&charge.official_currency)
    .bind(charge.exchange_rate)
    .bind(charge.official_cost_cny)
    .bind(charge.price_multiplier)
    .bind(charge.money_cost)
    .bind(charge.credit_cost)
    .bind(charge.is_long_context)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(true)
}
