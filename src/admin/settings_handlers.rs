use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

use crate::auth::AuthAdmin;
use crate::state::AppState;
use super::{err, friendly_db_err, ErrorResponse};

// ============================================================
// 全局设置 API
// ============================================================

/// GET /api/settings/credit_rates
pub(super) async fn get_credit_rates(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
) -> Result<Json<crate::db::CreditRates>, (StatusCode, Json<ErrorResponse>)> {
    let rates = crate::db::get_credit_rates(&state.pool).await;
    Ok(Json(rates))
}

/// PUT /api/settings/credit_rates
pub(super) async fn update_credit_rates(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Json(body): Json<crate::db::CreditRates>,
) -> Result<Json<crate::db::CreditRates>, (StatusCode, Json<ErrorResponse>)> {
    let val = serde_json::to_value(&body).unwrap();
    sqlx::query(
        "INSERT INTO global_settings (key, value) VALUES ('credit_rates', $1) \
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_at = now()"
    )
    .bind(val)
    .execute(&state.pool)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(body))
}

/// GET /api/settings/mail
pub(super) async fn get_mail_settings(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
) -> Result<Json<crate::db::MailSettings>, (StatusCode, Json<ErrorResponse>)> {
    let settings = crate::db::get_mail_settings(&state.pool).await;
    Ok(Json(settings))
}

/// PUT /api/settings/mail
pub(super) async fn update_mail_settings(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Json(mut body): Json<crate::db::MailSettings>,
) -> Result<Json<crate::db::MailSettings>, (StatusCode, Json<ErrorResponse>)> {
    body.system_contact_email = body.system_contact_email.trim().to_string();
    if body.system_contact_email.is_empty()
        || body.system_contact_email.parse::<lettre::Address>().is_err()
    {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "系统联系人邮箱格式无效",
        ));
    }

    let val = serde_json::to_value(&body).unwrap();
    sqlx::query(
        "INSERT INTO global_settings (key, value) VALUES ('mail_settings', $1) \
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_at = now()",
    )
    .bind(val)
    .execute(&state.pool)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(body))
}

/// GET /api/settings/compression
pub(super) async fn get_compression(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
) -> Result<Json<crate::compress::CompressionConfig>, (StatusCode, Json<ErrorResponse>)> {
    let cfg = crate::db::get_compression_config(&state.pool).await;
    Ok(Json(cfg))
}

/// PUT /api/settings/compression
pub(super) async fn update_compression(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Json(body): Json<crate::compress::CompressionConfig>,
) -> Result<Json<crate::compress::CompressionConfig>, (StatusCode, Json<ErrorResponse>)> {
    let val = serde_json::to_value(&body).unwrap();
    sqlx::query(
        "INSERT INTO global_settings (key, value) VALUES ('compression', $1) \
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_at = now()"
    )
    .bind(val)
    .execute(&state.pool)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(body))
}

// ============================================================
// 模型积分扣除比例 CRUD
// ============================================================

#[derive(Deserialize)]
pub(super) struct CreateModelCreditRate {
    model_name: String,
    input_rate: f64,
    output_rate: f64,
    cached_rate: Option<f64>,
    #[serde(default)]
    cache_write_rate: Option<f64>,
    #[serde(default)]
    long_context_threshold: Option<i64>,
    #[serde(default)]
    long_context_input_rate: Option<f64>,
    #[serde(default)]
    long_context_output_rate: Option<f64>,
    #[serde(default)]
    long_context_cached_rate: Option<f64>,
    #[serde(default)]
    long_context_cache_write_rate: Option<f64>,
}

/// GET /api/settings/model_credit_rates — 列出所有模型积分扣除比例
pub(super) async fn list_model_credit_rates(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
) -> Result<Json<Vec<crate::db::ModelCreditRate>>, (StatusCode, Json<ErrorResponse>)> {
    let rows = crate::db::list_model_credit_rates(&state.pool).await;
    Ok(Json(rows))
}

/// POST /api/settings/model_credit_rates — 新建模型积分扣除比例
pub(super) async fn create_model_credit_rate(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Json(body): Json<CreateModelCreditRate>,
) -> Result<Json<crate::db::ModelCreditRate>, (StatusCode, Json<ErrorResponse>)> {
    let row = sqlx::query_as::<_, crate::db::ModelCreditRate>(
        &format!(
            "INSERT INTO model_credit_rates (model_name, input_rate, output_rate, cached_rate, cache_write_rate, \
                    long_context_threshold, long_context_input_rate, long_context_output_rate, long_context_cached_rate, long_context_cache_write_rate) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
             RETURNING {}", crate::db::MODEL_RATE_COLS
        )
    )
    .bind(body.model_name.trim())
    .bind(body.input_rate)
    .bind(body.output_rate)
    .bind(body.cached_rate)
    .bind(body.cache_write_rate)
    .bind(body.long_context_threshold)
    .bind(body.long_context_input_rate)
    .bind(body.long_context_output_rate)
    .bind(body.long_context_cached_rate)
    .bind(body.long_context_cache_write_rate)
    .fetch_one(&state.pool)
    .await
    .map_err(friendly_db_err)?;
    Ok(Json(row))
}

/// PUT /api/settings/model_credit_rates/:id — 更新模型积分扣除比例
pub(super) async fn update_model_credit_rate(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Path(id): Path<Uuid>,
    Json(body): Json<CreateModelCreditRate>,
) -> Result<Json<crate::db::ModelCreditRate>, (StatusCode, Json<ErrorResponse>)> {
    let row = sqlx::query_as::<_, crate::db::ModelCreditRate>(
        &format!(
            "UPDATE model_credit_rates SET \
                model_name = $2, input_rate = $3, output_rate = $4, cached_rate = $5, cache_write_rate = $6, \
                long_context_threshold = $7, long_context_input_rate = $8, long_context_output_rate = $9, long_context_cached_rate = $10, long_context_cache_write_rate = $11, \
                updated_at = now() \
             WHERE id = $1 \
             RETURNING {}", crate::db::MODEL_RATE_COLS
        )
    )
    .bind(id)
    .bind(body.model_name.trim())
    .bind(body.input_rate)
    .bind(body.output_rate)
    .bind(body.cached_rate)
    .bind(body.cache_write_rate)
    .bind(body.long_context_threshold)
    .bind(body.long_context_input_rate)
    .bind(body.long_context_output_rate)
    .bind(body.long_context_cached_rate)
    .bind(body.long_context_cache_write_rate)
    .fetch_optional(&state.pool)
    .await
    .map_err(friendly_db_err)?
    .ok_or_else(|| err(StatusCode::NOT_FOUND, "Model credit rate not found"))?;
    Ok(Json(row))
}

/// DELETE /api/settings/model_credit_rates/:id — 删除模型积分扣除比例（default 不可删除）
pub(super) async fn delete_model_credit_rate(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    let deleted = sqlx::query("DELETE FROM model_credit_rates WHERE id = $1 AND model_name != 'default'")
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(friendly_db_err)?;
    if deleted.rows_affected() == 0 {
        return Err(err(StatusCode::FORBIDDEN, "默认比例不可删除 / Cannot delete default rate"));
    }
    Ok(StatusCode::NO_CONTENT)
}

// ============================================================
// 标准价格计费 CRUD（更新时创建不可变的新价格版本）
// ============================================================

#[derive(Deserialize)]
pub(super) struct SaveModelPricingRequest {
    #[serde(default)]
    provider: String,
    model_name: String,
    currency: String,
    region_type: String,
    input_price: Decimal,
    cached_input_price: Option<Decimal>,
    cache_write_price: Option<Decimal>,
    output_price: Decimal,
    long_context_threshold: Option<i64>,
    long_input_price: Option<Decimal>,
    long_cached_price: Option<Decimal>,
    long_cache_write_price: Option<Decimal>,
    long_output_price: Option<Decimal>,
    multiplier: Decimal,
    exchange_rate: Decimal,
    effective_at: Option<DateTime<Utc>>,
}

fn validate_model_pricing(body: &mut SaveModelPricingRequest) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
    body.provider = body.provider.trim().to_string();
    body.model_name = body.model_name.trim().to_string();
    body.currency = body.currency.trim().to_ascii_uppercase();
    body.region_type = body.region_type.trim().to_ascii_lowercase();
    if body.model_name.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "模型名称不能为空"));
    }
    if !matches!(body.currency.as_str(), "CNY" | "USD") {
        return Err(err(StatusCode::BAD_REQUEST, "币种只能是 CNY 或 USD"));
    }
    if !matches!(body.region_type.as_str(), "domestic" | "international") {
        return Err(err(StatusCode::BAD_REQUEST, "模型类型只能是国内或国外"));
    }
    if (body.currency == "CNY") != (body.region_type == "domestic") {
        return Err(err(StatusCode::BAD_REQUEST, "国内模型必须使用 CNY，国外模型必须使用 USD"));
    }
    let optional_prices = [
        body.cached_input_price,
        body.cache_write_price,
        body.long_input_price,
        body.long_cached_price,
        body.long_cache_write_price,
        body.long_output_price,
    ];
    if body.input_price.is_sign_negative()
        || body.output_price.is_sign_negative()
        || optional_prices.into_iter().flatten().any(|v| v.is_sign_negative())
        || body.multiplier <= Decimal::ZERO
        || body.exchange_rate <= Decimal::ZERO
    {
        return Err(err(StatusCode::BAD_REQUEST, "价格不得为负数，销售系数和汇率必须大于 0"));
    }
    if body.long_context_threshold.is_some_and(|v| v <= 0) {
        return Err(err(StatusCode::BAD_REQUEST, "长上下文阈值必须大于 0"));
    }
    let has_long_prices = body.long_input_price.is_some()
        || body.long_cached_price.is_some()
        || body.long_cache_write_price.is_some()
        || body.long_output_price.is_some();
    if body.long_context_threshold.is_some() && body.long_input_price.is_none() {
        return Err(err(StatusCode::BAD_REQUEST, "配置长上下文时必须填写长上下文输入价格"));
    }
    if body.long_context_threshold.is_none() && has_long_prices {
        return Err(err(StatusCode::BAD_REQUEST, "填写长上下文价格前请先配置长上下文阈值"));
    }
    if body.currency == "CNY" {
        body.exchange_rate = Decimal::ONE;
    }
    Ok(())
}

async fn fetch_model_pricing(
    pool: &sqlx::PgPool,
    id: Uuid,
) -> Result<crate::db::StandardModelPrice, (StatusCode, Json<ErrorResponse>)> {
    sqlx::query_as::<_, crate::db::StandardModelPrice>(&format!(
        "SELECT {} FROM model_pricings p \
         JOIN LATERAL (SELECT * FROM model_price_versions pv WHERE pv.pricing_id = p.id \
             ORDER BY pv.version DESC LIMIT 1) v ON true WHERE p.id = $1",
        crate::db::STANDARD_MODEL_PRICE_COLS
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .ok_or_else(|| err(StatusCode::NOT_FOUND, "模型价格不存在"))
}

pub(super) async fn list_model_pricings(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
) -> Result<Json<Vec<crate::db::StandardModelPrice>>, (StatusCode, Json<ErrorResponse>)> {
    let rows = sqlx::query_as::<_, crate::db::StandardModelPrice>(&format!(
        "SELECT {} FROM model_pricings p \
         JOIN LATERAL (SELECT * FROM model_price_versions pv WHERE pv.pricing_id = p.id \
             ORDER BY pv.version DESC LIMIT 1) v ON true \
         WHERE p.is_active = true ORDER BY p.model_name, p.provider",
        crate::db::STANDARD_MODEL_PRICE_COLS
    ))
    .fetch_all(&state.pool)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(rows))
}

pub(super) async fn create_model_pricing(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Json(mut body): Json<SaveModelPricingRequest>,
) -> Result<(StatusCode, Json<crate::db::StandardModelPrice>), (StatusCode, Json<ErrorResponse>)> {
    validate_model_pricing(&mut body)?;
    let effective_at = body.effective_at.unwrap_or_else(Utc::now);
    let mut tx = state.pool.begin().await.map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let pricing_id: Uuid = sqlx::query_scalar(
        "INSERT INTO model_pricings (provider, model_name, is_active) VALUES ($1, $2, true) \
         ON CONFLICT (provider, model_name) DO UPDATE SET is_active = true, updated_at = now() \
         RETURNING id",
    )
    .bind(&body.provider)
    .bind(&body.model_name)
    .fetch_one(&mut *tx)
    .await
    .map_err(friendly_db_err)?;
    let version: i32 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(version), 0) + 1 FROM model_price_versions WHERE pricing_id = $1",
    )
    .bind(pricing_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(friendly_db_err)?;
    insert_price_version(&mut tx, pricing_id, version, &body, effective_at).await?;
    tx.commit().await.map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok((StatusCode::CREATED, Json(fetch_model_pricing(&state.pool, pricing_id).await?)))
}

pub(super) async fn update_model_pricing(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Path(id): Path<Uuid>,
    Json(mut body): Json<SaveModelPricingRequest>,
) -> Result<Json<crate::db::StandardModelPrice>, (StatusCode, Json<ErrorResponse>)> {
    validate_model_pricing(&mut body)?;
    let effective_at = body.effective_at.unwrap_or_else(Utc::now);
    let mut tx = state.pool.begin().await.map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let exists = sqlx::query("UPDATE model_pricings SET provider = $2, model_name = $3, is_active = true, updated_at = now() WHERE id = $1")
        .bind(id).bind(&body.provider).bind(&body.model_name)
        .execute(&mut *tx).await.map_err(friendly_db_err)?;
    if exists.rows_affected() == 0 {
        return Err(err(StatusCode::NOT_FOUND, "模型价格不存在"));
    }
    sqlx::query("SELECT id FROM model_pricings WHERE id = $1 FOR UPDATE")
        .bind(id).fetch_one(&mut *tx).await.map_err(friendly_db_err)?;
    let version: i32 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(version), 0) + 1 FROM model_price_versions WHERE pricing_id = $1",
    )
    .bind(id).fetch_one(&mut *tx).await.map_err(friendly_db_err)?;
    insert_price_version(&mut tx, id, version, &body, effective_at).await?;
    tx.commit().await.map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(fetch_model_pricing(&state.pool, id).await?))
}

async fn insert_price_version(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    pricing_id: Uuid,
    version: i32,
    body: &SaveModelPricingRequest,
    effective_at: DateTime<Utc>,
) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
    sqlx::query(
        "INSERT INTO model_price_versions (pricing_id, version, currency, region_type, input_price, \
             cached_input_price, cache_write_price, output_price, long_context_threshold, long_input_price, \
             long_cached_price, long_cache_write_price, long_output_price, multiplier, exchange_rate, effective_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)",
    )
    .bind(pricing_id).bind(version).bind(&body.currency).bind(&body.region_type)
    .bind(body.input_price).bind(body.cached_input_price).bind(body.cache_write_price)
    .bind(body.output_price).bind(body.long_context_threshold).bind(body.long_input_price)
    .bind(body.long_cached_price).bind(body.long_cache_write_price).bind(body.long_output_price)
    .bind(body.multiplier).bind(body.exchange_rate).bind(effective_at)
    .execute(&mut **tx).await.map_err(friendly_db_err)?;
    Ok(())
}

pub(super) async fn delete_model_pricing(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    let result = sqlx::query("UPDATE model_pricings SET is_active = false, updated_at = now() WHERE id = $1")
        .bind(id).execute(&state.pool).await.map_err(friendly_db_err)?;
    if result.rows_affected() == 0 {
        return Err(err(StatusCode::NOT_FOUND, "模型价格不存在"));
    }
    Ok(StatusCode::NO_CONTENT)
}
