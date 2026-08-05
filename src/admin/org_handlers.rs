use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::AuthAdmin;
use crate::state::AppState;
use super::{err, friendly_db_err, ErrorResponse, OrgRow, ProjectRow, ApiKeyRow};

// ============================================================
// 组织管理
// ============================================================

#[derive(Deserialize)]
pub(super) struct CreateOrgRequest {
    name: String,
    slug: String,
    #[serde(default = "default_billing_mode")]
    billing_mode: String,
}

fn default_billing_mode() -> String { "standard_pricing".to_string() }

#[derive(Deserialize)]
pub(super) struct UpdateOrgRequest {
    name: Option<String>,
    slug: Option<String>,
    is_active: Option<bool>,
    overdraft_limit: Option<rust_decimal::Decimal>,
    credit_price: Option<rust_decimal::Decimal>,
    billing_mode: Option<String>,
}

/// GET /api/orgs — 列出所有组织
pub(super) async fn list_orgs(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
) -> Result<Json<Vec<OrgRow>>, (StatusCode, Json<ErrorResponse>)> {
    let rows = sqlx::query_as::<_, OrgRow>(
        "SELECT o.id, o.name, o.slug, o.credit, o.overdraft_limit, o.credit_price, o.billing_mode, o.is_active, \
                COALESCE((SELECT ABS(SUM(amount)) FROM credit_logs WHERE org_id = o.id AND transaction_type = 'consume'), 0) AS total_consumed, \
                o.created_at, o.updated_at \
         FROM organizations o ORDER BY o.created_at DESC",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(rows))
}

/// POST /api/orgs — 创建组织（同时自动创建默认项目）
pub(super) async fn create_org(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Json(body): Json<CreateOrgRequest>,
) -> Result<(StatusCode, Json<OrgRow>), (StatusCode, Json<ErrorResponse>)> {
    let mut tx = state.pool.begin().await.map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let row = sqlx::query_as::<_, OrgRow>(
        "INSERT INTO organizations (name, slug, billing_mode) VALUES ($1, $2, $3) \
         RETURNING id, name, slug, credit, overdraft_limit, credit_price, billing_mode, is_active, 0::NUMERIC AS total_consumed, created_at, updated_at",
    )
    .bind(&body.name)
    .bind(&body.slug)
    .bind(validate_billing_mode(&body.billing_mode)?)
    .fetch_one(&mut *tx)
    .await
    .map_err(friendly_db_err)?;

    sqlx::query("INSERT INTO projects (org_id, name, description) VALUES ($1, 'Default', '自动创建的默认项目')")
        .bind(row.id)
        .execute(&mut *tx)
        .await
        .map_err(friendly_db_err)?;

    tx.commit().await.map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok((StatusCode::CREATED, Json(row)))
}

/// PUT /api/orgs/:id — 更新组织
pub(super) async fn update_org(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateOrgRequest>,
) -> Result<Json<OrgRow>, (StatusCode, Json<ErrorResponse>)> {
    let billing_mode = body.billing_mode.as_deref().map(validate_billing_mode).transpose()?;
    let row = sqlx::query_as::<_, OrgRow>(
        "UPDATE organizations SET \
            name = COALESCE($2, name), \
            slug = COALESCE($3, slug), \
            is_active = COALESCE($4, is_active), \
            overdraft_limit = COALESCE($5, overdraft_limit), \
            credit_price = COALESCE($6, credit_price), \
            billing_mode = COALESCE($7, billing_mode), \
            updated_at = now() \
         WHERE id = $1 \
         RETURNING id, name, slug, credit, overdraft_limit, credit_price, billing_mode, is_active, \
                   COALESCE((SELECT ABS(SUM(amount)) FROM credit_logs WHERE org_id = id AND transaction_type = 'consume'), 0) AS total_consumed, \
                   created_at, updated_at",
    )
    .bind(id)
    .bind(&body.name)
    .bind(&body.slug)
    .bind(body.is_active)
    .bind(body.overdraft_limit)
    .bind(body.credit_price)
    .bind(billing_mode)
    .fetch_optional(&state.pool)
    .await
    .map_err(friendly_db_err)?
    .ok_or_else(|| err(StatusCode::NOT_FOUND, "Organization not found"))?;

    Ok(Json(row))
}

fn validate_billing_mode(value: &str) -> Result<&str, (StatusCode, Json<ErrorResponse>)> {
    match value {
        "contract_ratio" | "standard_pricing" => Ok(value),
        _ => Err(err(StatusCode::BAD_REQUEST, "不支持的计费方式")),
    }
}

/// DELETE /api/orgs/:id — 删除组织
pub(super) async fn delete_org(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    let result = sqlx::query("DELETE FROM organizations WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(friendly_db_err)?;

    if result.rows_affected() == 0 {
        return Err(err(StatusCode::NOT_FOUND, "Organization not found"));
    }

    Ok(StatusCode::NO_CONTENT)
}

// ============================================================
// 项目管理
// ============================================================

#[derive(Deserialize)]
pub(super) struct CreateProjectRequest {
    name: String,
    #[serde(default)]
    description: String,
}

#[derive(Deserialize)]
pub(super) struct UpdateProjectRequest {
    name: Option<String>,
    description: Option<String>,
    is_active: Option<bool>,
}

/// GET /api/orgs/:org_id/projects — 列出组织下的所有项目
pub(super) async fn list_projects(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Vec<ProjectRow>>, (StatusCode, Json<ErrorResponse>)> {
    let rows = sqlx::query_as::<_, ProjectRow>(
        "SELECT id, org_id, name, description, is_active, created_at, updated_at \
         FROM projects WHERE org_id = $1 ORDER BY created_at ASC",
    )
    .bind(org_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(rows))
}

/// POST /api/orgs/:org_id/projects — 创建项目
pub(super) async fn create_project(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Path(org_id): Path<Uuid>,
    Json(body): Json<CreateProjectRequest>,
) -> Result<(StatusCode, Json<ProjectRow>), (StatusCode, Json<ErrorResponse>)> {
    let row = sqlx::query_as::<_, ProjectRow>(
        "INSERT INTO projects (org_id, name, description) VALUES ($1, $2, $3) \
         RETURNING id, org_id, name, description, is_active, created_at, updated_at",
    )
    .bind(org_id)
    .bind(&body.name)
    .bind(&body.description)
    .fetch_one(&state.pool)
    .await
    .map_err(friendly_db_err)?;

    Ok((StatusCode::CREATED, Json(row)))
}

/// PUT /api/projects/:id — 更新项目
pub(super) async fn update_project(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateProjectRequest>,
) -> Result<Json<ProjectRow>, (StatusCode, Json<ErrorResponse>)> {
    let row = sqlx::query_as::<_, ProjectRow>(
        "UPDATE projects SET \
            name = COALESCE($2, name), \
            description = COALESCE($3, description), \
            is_active = COALESCE($4, is_active), \
            updated_at = now() \
         WHERE id = $1 \
         RETURNING id, org_id, name, description, is_active, created_at, updated_at",
    )
    .bind(id)
    .bind(&body.name)
    .bind(&body.description)
    .bind(body.is_active)
    .fetch_optional(&state.pool)
    .await
    .map_err(friendly_db_err)?
    .ok_or_else(|| err(StatusCode::NOT_FOUND, "项目不存在"))?;

    Ok(Json(row))
}

/// DELETE /api/projects/:id — 删除项目
pub(super) async fn delete_project(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    let result = sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(friendly_db_err)?;

    if result.rows_affected() == 0 {
        return Err(err(StatusCode::NOT_FOUND, "项目不存在"));
    }

    Ok(StatusCode::NO_CONTENT)
}

// ============================================================
// API Key 管理
// ============================================================

#[derive(Deserialize)]
pub(super) struct CreateKeyRequest {
    name: String,
}

/// 创建 key 时返回完整 key 值（仅此一次）
#[derive(Serialize)]
pub(super) struct CreateKeyResponse {
    id: Uuid,
    name: String,
    key: String,
    key_prefix: String,
    created_at: DateTime<Utc>,
}

/// GET /api/projects/:project_id/keys — 列出项目下的所有 API Key
pub(super) async fn list_keys(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Vec<ApiKeyRow>>, (StatusCode, Json<ErrorResponse>)> {
    let rows = sqlx::query_as::<_, ApiKeyRow>(
        "SELECT id, org_id, project_id, name, key_prefix, is_active, last_used_at, created_at \
         FROM api_keys WHERE project_id = $1 ORDER BY created_at DESC",
    )
    .bind(project_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(rows))
}

/// POST /api/projects/:project_id/keys — 在项目下创建 API Key
/// Key 格式: gc-{slug}-{24 hex}，前缀包含组织 slug 便于识别归属
pub(super) async fn create_key(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Path(project_id): Path<Uuid>,
    Json(body): Json<CreateKeyRequest>,
) -> Result<(StatusCode, Json<CreateKeyResponse>), (StatusCode, Json<ErrorResponse>)> {
    #[derive(sqlx::FromRow)]
    struct ProjectOrg {
        org_id: Uuid,
        slug: String,
    }

    let po = sqlx::query_as::<_, ProjectOrg>(
        "SELECT p.org_id, o.slug FROM projects p JOIN organizations o ON o.id = p.org_id WHERE p.id = $1",
    )
    .bind(project_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .ok_or_else(|| err(StatusCode::NOT_FOUND, "项目不存在"))?;

    let raw_uuid = Uuid::new_v4().to_string().replace('-', "");
    let raw_key = format!("gc-{}-{}", po.slug, &raw_uuid[..24]);
    let key_prefix = format!("gc-{}-", po.slug);

    let mut hasher = Sha256::new();
    hasher.update(raw_key.as_bytes());
    let key_hash = format!("{:x}", hasher.finalize());

    #[derive(sqlx::FromRow)]
    struct InsertedKey {
        id: Uuid,
        created_at: DateTime<Utc>,
    }

    let row = sqlx::query_as::<_, InsertedKey>(
        "INSERT INTO api_keys (org_id, project_id, name, key_hash, key_prefix) VALUES ($1, $2, $3, $4, $5) \
         RETURNING id, created_at",
    )
    .bind(po.org_id)
    .bind(project_id)
    .bind(&body.name)
    .bind(&key_hash)
    .bind(&key_prefix)
    .fetch_one(&state.pool)
    .await
    .map_err(friendly_db_err)?;

    Ok((
        StatusCode::CREATED,
        Json(CreateKeyResponse {
            id: row.id,
            name: body.name,
            key: raw_key,
            key_prefix,
            created_at: row.created_at,
        }),
    ))
}

/// DELETE /api/keys/:id — 删除 API Key
pub(super) async fn delete_key(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    let result = sqlx::query("DELETE FROM api_keys WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(friendly_db_err)?;

    if result.rows_affected() == 0 {
        return Err(err(StatusCode::NOT_FOUND, "API key not found"));
    }

    Ok(StatusCode::NO_CONTENT)
}
