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
use super::{double_option, err, friendly_db_err, ErrorResponse, ModelConfigRow};

#[derive(Deserialize)]
pub(super) struct CreateModelRequest {
    name: String,
    protocol: Option<String>,
    model_patterns: String,
    base_url: String,
    real_api_key: String,
    priority: Option<i32>,
    compression_enabled: Option<bool>,
}

#[derive(Deserialize)]
pub(super) struct UpdateModelRequest {
    name: Option<String>,
    protocol: Option<String>,
    model_patterns: Option<String>,
    base_url: Option<String>,
    real_api_key: Option<String>,
    priority: Option<i32>,
    is_active: Option<bool>,
    /// 双层 Option：缺省=不变，显式 null=重置为继承全局，true/false=覆盖
    #[serde(default, deserialize_with = "double_option")]
    compression_enabled: Option<Option<bool>>,
}

/// GET /api/orgs/:org_id/models — 列出组织的模型配置
pub(super) async fn list_models(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Vec<ModelConfigRow>>, (StatusCode, Json<ErrorResponse>)> {
    let rows = sqlx::query_as::<_, ModelConfigRow>(
        "SELECT id, org_id, name, protocol, model_patterns, base_url, real_api_key, \
                priority, is_active, compression_enabled, created_at, updated_at \
         FROM model_configs WHERE org_id = $1 ORDER BY priority DESC, created_at DESC",
    )
    .bind(org_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(rows))
}

/// POST /api/orgs/:org_id/models — 创建模型配置
pub(super) async fn create_model(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Path(org_id): Path<Uuid>,
    Json(body): Json<CreateModelRequest>,
) -> Result<(StatusCode, Json<ModelConfigRow>), (StatusCode, Json<ErrorResponse>)> {
    let row = sqlx::query_as::<_, ModelConfigRow>(
        "INSERT INTO model_configs (org_id, name, protocol, model_patterns, base_url, real_api_key, priority, compression_enabled) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
         RETURNING id, org_id, name, protocol, model_patterns, base_url, real_api_key, \
                   priority, is_active, compression_enabled, created_at, updated_at",
    )
    .bind(org_id)
    .bind(&body.name)
    .bind(body.protocol.as_deref().unwrap_or("openai"))
    .bind(&body.model_patterns)
    .bind(&body.base_url)
    .bind(&body.real_api_key)
    .bind(body.priority.unwrap_or(0))
    .bind(body.compression_enabled)
    .fetch_one(&state.pool)
    .await
    .map_err(friendly_db_err)?;

    Ok((StatusCode::CREATED, Json(row)))
}

/// PUT /api/models/:id — 更新模型配置
pub(super) async fn update_model(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateModelRequest>,
) -> Result<Json<ModelConfigRow>, (StatusCode, Json<ErrorResponse>)> {
    let row = sqlx::query_as::<_, ModelConfigRow>(
        "UPDATE model_configs SET \
            name = COALESCE($2, name), \
            protocol = COALESCE($3, protocol), \
            model_patterns = COALESCE($4, model_patterns), \
            base_url = COALESCE($5, base_url), \
            real_api_key = COALESCE($6, real_api_key), \
            priority = COALESCE($7, priority), \
            is_active = COALESCE($8, is_active), \
            compression_enabled = CASE WHEN $10 THEN $9 ELSE compression_enabled END, \
            updated_at = now() \
         WHERE id = $1 \
         RETURNING id, org_id, name, protocol, model_patterns, base_url, real_api_key, \
                   priority, is_active, compression_enabled, created_at, updated_at",
    )
    .bind(id)
    .bind(&body.name)
    .bind(&body.protocol)
    .bind(&body.model_patterns)
    .bind(&body.base_url)
    .bind(&body.real_api_key)
    .bind(body.priority)
    .bind(body.is_active)
    .bind(body.compression_enabled.flatten()) // $9: 内层值（Some(None)→NULL）
    .bind(body.compression_enabled.is_some()) // $10: 是否要写入该列
    .fetch_optional(&state.pool)
    .await
    .map_err(friendly_db_err)?
    .ok_or_else(|| err(StatusCode::NOT_FOUND, "Model config not found"))?;

    Ok(Json(row))
}

/// DELETE /api/models/:id — 删除模型配置
pub(super) async fn delete_model(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    let result = sqlx::query("DELETE FROM model_configs WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(friendly_db_err)?;

    if result.rows_affected() == 0 {
        return Err(err(StatusCode::NOT_FOUND, "Model config not found"));
    }

    Ok(StatusCode::NO_CONTENT)
}
