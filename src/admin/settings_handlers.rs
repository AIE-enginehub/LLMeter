use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

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
    Json(body): Json<crate::db::MailSettings>,
) -> Result<Json<crate::db::MailSettings>, (StatusCode, Json<ErrorResponse>)> {
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
    cached_rate: f64,
    #[serde(default)]
    long_context_threshold: Option<i64>,
    #[serde(default)]
    long_context_input_rate: Option<f64>,
    #[serde(default)]
    long_context_output_rate: Option<f64>,
    #[serde(default)]
    long_context_cached_rate: Option<f64>,
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
            "INSERT INTO model_credit_rates (model_name, input_rate, output_rate, cached_rate, \
                    long_context_threshold, long_context_input_rate, long_context_output_rate, long_context_cached_rate) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
             RETURNING {}", crate::db::MODEL_RATE_COLS
        )
    )
    .bind(body.model_name.trim())
    .bind(body.input_rate)
    .bind(body.output_rate)
    .bind(body.cached_rate)
    .bind(body.long_context_threshold)
    .bind(body.long_context_input_rate)
    .bind(body.long_context_output_rate)
    .bind(body.long_context_cached_rate)
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
                model_name = $2, input_rate = $3, output_rate = $4, cached_rate = $5, \
                long_context_threshold = $6, long_context_input_rate = $7, long_context_output_rate = $8, long_context_cached_rate = $9, \
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
    .bind(body.long_context_threshold)
    .bind(body.long_context_input_rate)
    .bind(body.long_context_output_rate)
    .bind(body.long_context_cached_rate)
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
