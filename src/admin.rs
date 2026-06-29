use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::{delete, get, post, put},
    Json, Router,
};
use chrono::{DateTime, Utc};
use lettre::{
    message::{Mailbox, MultiPart, SinglePart, Attachment, header::ContentType},
    transport::smtp::authentication::Credentials,
    AsyncSmtpTransport, AsyncTransport, Tokio1Executor,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::{self, AuthAdmin};
use crate::state::AppState;

/// 双层 Option 反序列化：区分「字段缺省」(None) 与「显式 null」(Some(None))。
/// 用于 PUT 更新可空列时，让显式 null 能把列重置为 NULL，而非被 COALESCE 保留。
fn double_option<'de, D, T>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    Ok(Some(Option::deserialize(de)?))
}

// ============================================================
// 公共类型
// ============================================================

/// 统一错误响应
#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

/// 快捷构造错误响应
fn err(status: StatusCode, msg: impl Into<String>) -> (StatusCode, Json<ErrorResponse>) {
    (
        status,
        Json(ErrorResponse {
            error: msg.into(),
        }),
    )
}

/// 将数据库错误转为用户友好的提示
fn friendly_db_err(e: sqlx::Error) -> (StatusCode, Json<ErrorResponse>) {
    let msg = e.to_string();
    if msg.contains("unique constraint") {
        if msg.contains("organizations_slug_key") {
            return err(StatusCode::CONFLICT, "该标识 (slug) 已被其他组织使用");
        }
        if msg.contains("organizations_name_key") {
            return err(StatusCode::CONFLICT, "该组织名称已存在");
        }
        if msg.contains("api_keys") {
            return err(StatusCode::CONFLICT, "API Key 冲突，请重试");
        }
        if msg.contains("model_configs") {
            return err(StatusCode::CONFLICT, "模型配置名称已存在");
        }
        return err(StatusCode::CONFLICT, "数据重复，请检查输入");
    }
    if msg.contains("foreign key constraint") {
        return err(StatusCode::CONFLICT, "该记录被其他数据引用，无法操作");
    }
    if msg.contains("not-null constraint") {
        return err(StatusCode::BAD_REQUEST, "必填字段不能为空");
    }
    err(StatusCode::INTERNAL_SERVER_ERROR, "操作失败，请稍后重试")
}

// ============================================================
// 路由注册
// ============================================================

/// 构建管理后台所有 API 路由
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        // 认证（login 不需要 JWT）
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/me", get(me))
        .route("/api/auth/password", put(change_password))
        // 组织管理
        .route("/api/orgs", get(list_orgs).post(create_org))
        .route(
            "/api/orgs/{id}",
            put(update_org).delete(delete_org),
        )
        // 项目管理
        .route(
            "/api/orgs/{org_id}/projects",
            get(list_projects).post(create_project),
        )
        .route(
            "/api/projects/{id}",
            put(update_project).delete(delete_project),
        )
        // API Key 管理（归属项目）
        .route(
            "/api/projects/{project_id}/keys",
            get(list_keys).post(create_key),
        )
        .route("/api/keys/{id}", delete(delete_key))
        // 模型配置管理（保持在组织级别）
        .route(
            "/api/orgs/{org_id}/models",
            get(list_models).post(create_model),
        )
        .route(
            "/api/models/{id}",
            put(update_model).delete(delete_model),
        )
        // 日志与统计
        .route("/api/logs", get(list_logs))
        .route("/api/logs/{id}", get(get_log))
        .route("/api/stats", get(get_stats))
        // 积分系统
        .route(
            "/api/settings/credit_rates",
            get(get_credit_rates).put(update_credit_rates),
        )
        .route("/api/settings/mail", get(get_mail_settings).put(update_mail_settings))
        .route("/api/settings/compression", get(get_compression).put(update_compression))
        .route("/api/usage/export_report", post(export_usage_report))
        .route("/api/orgs/{id}/credit", post(recharge_credit))
        .route("/api/orgs/{id}/credit_logs", get(list_credit_logs))
}

// ============================================================
// 数据库行结构
// ============================================================

#[derive(Serialize, sqlx::FromRow)]
struct OrgRow {
    id: Uuid,
    name: String,
    slug: String,
    credit: rust_decimal::Decimal,
    overdraft_limit: rust_decimal::Decimal,
    credit_price: rust_decimal::Decimal,
    is_active: bool,
    total_consumed: Option<rust_decimal::Decimal>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// 项目（归属于组织）
#[derive(Serialize, sqlx::FromRow)]
struct ProjectRow {
    id: Uuid,
    org_id: Uuid,
    name: String,
    description: String,
    is_active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Serialize, sqlx::FromRow)]
struct ApiKeyRow {
    id: Uuid,
    org_id: Uuid,
    project_id: Option<Uuid>,
    name: String,
    key_prefix: String,
    is_active: bool,
    last_used_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

#[derive(Serialize, sqlx::FromRow)]
struct ModelConfigRow {
    id: Uuid,
    org_id: Uuid,
    name: String,
    protocol: String,
    model_patterns: String,
    base_url: String,
    real_api_key: String,
    priority: i32,
    is_active: bool,
    compression_enabled: Option<bool>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Serialize, sqlx::FromRow)]
struct AdminUserRow {
    id: Uuid,
    username: String,
    #[serde(skip)]
    password_hash: String,
    created_at: DateTime<Utc>,
}

#[derive(Serialize, sqlx::FromRow)]
struct LogRow {
    id: Uuid,
    org_id: Uuid,
    project_id: Option<Uuid>,
    api_key_id: Uuid,
    provider: String,
    model: Option<String>,
    path: String,
    method: String,
    is_stream: bool,
    request_body: Option<serde_json::Value>,
    response_body: Option<serde_json::Value>,
    response_status: Option<i32>,
    status: String,
    prompt_tokens: Option<i32>,
    completion_tokens: Option<i32>,
    cached_tokens: Option<i32>,
    total_tokens: Option<i32>,
    duration_ms: Option<i32>,
    error_message: Option<String>,
    credit_cost: Option<rust_decimal::Decimal>,
    money_cost: Option<rust_decimal::Decimal>,
    is_long_context: bool,
    compressed: bool,
    compression_mode: Option<String>,
    original_prompt_chars: Option<i32>,
    forwarded_prompt_chars: Option<i32>,
    est_tokens_saved: Option<i32>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// 日志列表返回（不含 body 大字段）
#[derive(Serialize, sqlx::FromRow)]
struct LogSummaryRow {
    id: Uuid,
    org_id: Uuid,
    project_id: Option<Uuid>,
    api_key_id: Uuid,
    provider: String,
    model: Option<String>,
    path: String,
    method: String,
    is_stream: bool,
    response_status: Option<i32>,
    status: String,
    prompt_tokens: Option<i32>,
    completion_tokens: Option<i32>,
    cached_tokens: Option<i32>,
    total_tokens: Option<i32>,
    duration_ms: Option<i32>,
    error_message: Option<String>,
    credit_cost: Option<rust_decimal::Decimal>,
    money_cost: Option<rust_decimal::Decimal>,
    is_long_context: bool,
    compressed: bool,
    est_tokens_saved: Option<i32>,
    created_at: DateTime<Utc>,
}

// ============================================================
// 认证相关 API
// ============================================================

#[derive(Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Serialize)]
struct LoginResponse {
    token: String,
    user: UserInfo,
}

#[derive(Serialize)]
struct UserInfo {
    id: Uuid,
    username: String,
}

/// POST /api/auth/login — 管理员登录
async fn login(
    State(state): State<Arc<AppState>>,
    Json(body): Json<LoginRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let user = sqlx::query_as::<_, AdminUserRow>(
        "SELECT id, username, password_hash, created_at FROM admin_users WHERE username = $1",
    )
    .bind(&body.username)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "Invalid username or password"))?;

    if !auth::verify_password(&body.password, &user.password_hash) {
        return Err(err(
            StatusCode::UNAUTHORIZED,
            "Invalid username or password",
        ));
    }

    let token = auth::generate_token(&state.jwt_secret, &user.id.to_string());

    let cookie = format!(
        "token={token}; HttpOnly; Path=/; Max-Age=86400; SameSite=Lax"
    );

    let response = LoginResponse {
        token: token.clone(),
        user: UserInfo {
            id: user.id,
            username: user.username,
        },
    };

    Ok((
        StatusCode::OK,
        [(header::SET_COOKIE, cookie)],
        Json(response),
    ))
}

/// POST /api/auth/logout — 登出，清除 cookie
async fn logout(
    _admin: AuthAdmin,
) -> impl IntoResponse {
    let cookie = "token=; HttpOnly; Path=/; Max-Age=0; SameSite=Lax";
    (StatusCode::OK, [(header::SET_COOKIE, cookie.to_string())], Json(serde_json::json!({"ok": true})))
}

/// GET /api/auth/me — 获取当前登录用户信息
async fn me(
    State(state): State<Arc<AppState>>,
    admin: AuthAdmin,
) -> Result<Json<UserInfo>, (StatusCode, Json<ErrorResponse>)> {
    let user_id: Uuid = admin
        .user_id
        .parse()
        .map_err(|_| err(StatusCode::UNAUTHORIZED, "Invalid user id in token"))?;

    let user = sqlx::query_as::<_, AdminUserRow>(
        "SELECT id, username, password_hash, created_at FROM admin_users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .ok_or_else(|| err(StatusCode::NOT_FOUND, "User not found"))?;

    Ok(Json(UserInfo {
        id: user.id,
        username: user.username,
    }))
}

#[derive(Deserialize)]
struct ChangePasswordRequest {
    old_password: String,
    new_password: String,
}

/// PUT /api/auth/password — 修改当前管理员密码
async fn change_password(
    State(state): State<Arc<AppState>>,
    admin: AuthAdmin,
    Json(body): Json<ChangePasswordRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    if body.new_password.len() < 6 {
        return Err(err(StatusCode::BAD_REQUEST, "新密码长度不能少于 6 位"));
    }

    let user_id: Uuid = admin
        .user_id
        .parse()
        .map_err(|_| err(StatusCode::UNAUTHORIZED, "Invalid user id in token"))?;

    let user = sqlx::query_as::<_, AdminUserRow>(
        "SELECT id, username, password_hash, created_at FROM admin_users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if !auth::verify_password(&body.old_password, &user.password_hash) {
        return Err(err(StatusCode::BAD_REQUEST, "原密码错误"));
    }

    let new_hash = auth::hash_password(&body.new_password);
    sqlx::query("UPDATE admin_users SET password_hash = $1 WHERE id = $2")
        .bind(&new_hash)
        .bind(user_id)
        .execute(&state.pool)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(serde_json::json!({"ok": true})))
}

// ============================================================
// 组织管理 API
// ============================================================

#[derive(Deserialize)]
struct CreateOrgRequest {
    name: String,
    slug: String,
}

#[derive(Deserialize)]
struct UpdateOrgRequest {
    name: Option<String>,
    slug: Option<String>,
    is_active: Option<bool>,
    overdraft_limit: Option<rust_decimal::Decimal>,
    credit_price: Option<rust_decimal::Decimal>,
}

/// GET /api/orgs — 列出所有组织
async fn list_orgs(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
) -> Result<Json<Vec<OrgRow>>, (StatusCode, Json<ErrorResponse>)> {
    let rows = sqlx::query_as::<_, OrgRow>(
        "SELECT o.id, o.name, o.slug, o.credit, o.overdraft_limit, o.credit_price, o.is_active, \
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
async fn create_org(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Json(body): Json<CreateOrgRequest>,
) -> Result<(StatusCode, Json<OrgRow>), (StatusCode, Json<ErrorResponse>)> {
    let mut tx = state.pool.begin().await.map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let row = sqlx::query_as::<_, OrgRow>(
        "INSERT INTO organizations (name, slug) VALUES ($1, $2) \
         RETURNING id, name, slug, credit, overdraft_limit, credit_price, is_active, 0::NUMERIC AS total_consumed, created_at, updated_at",
    )
    .bind(&body.name)
    .bind(&body.slug)
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
async fn update_org(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateOrgRequest>,
) -> Result<Json<OrgRow>, (StatusCode, Json<ErrorResponse>)> {
    let row = sqlx::query_as::<_, OrgRow>(
        "UPDATE organizations SET \
            name = COALESCE($2, name), \
            slug = COALESCE($3, slug), \
            is_active = COALESCE($4, is_active), \
            overdraft_limit = COALESCE($5, overdraft_limit), \
            credit_price = COALESCE($6, credit_price), \
            updated_at = now() \
         WHERE id = $1 \
         RETURNING id, name, slug, credit, overdraft_limit, credit_price, is_active, \
                   COALESCE((SELECT ABS(SUM(amount)) FROM credit_logs WHERE org_id = id AND transaction_type = 'consume'), 0) AS total_consumed, \
                   created_at, updated_at",
    )
    .bind(id)
    .bind(&body.name)
    .bind(&body.slug)
    .bind(body.is_active)
    .bind(body.overdraft_limit)
    .bind(body.credit_price)
    .fetch_optional(&state.pool)
    .await
    .map_err(friendly_db_err)?
    .ok_or_else(|| err(StatusCode::NOT_FOUND, "Organization not found"))?;

    Ok(Json(row))
}

/// DELETE /api/orgs/:id — 删除组织
async fn delete_org(
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
// 项目管理 API
// ============================================================

#[derive(Deserialize)]
struct CreateProjectRequest {
    name: String,
    #[serde(default)]
    description: String,
}

#[derive(Deserialize)]
struct UpdateProjectRequest {
    name: Option<String>,
    description: Option<String>,
    is_active: Option<bool>,
}

/// GET /api/orgs/:org_id/projects — 列出组织下的所有项目
async fn list_projects(
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
async fn create_project(
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
async fn update_project(
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
async fn delete_project(
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
// API Key 管理（归属项目）
// ============================================================

#[derive(Deserialize)]
struct CreateKeyRequest {
    name: String,
}

/// 创建 key 时返回完整 key 值（仅此一次）
#[derive(Serialize)]
struct CreateKeyResponse {
    id: Uuid,
    name: String,
    key: String,
    key_prefix: String,
    created_at: DateTime<Utc>,
}

/// GET /api/projects/:project_id/keys — 列出项目下的所有 API Key
async fn list_keys(
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
async fn create_key(
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
async fn delete_key(
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

// ============================================================
// 模型配置管理
// ============================================================

#[derive(Deserialize)]
struct CreateModelRequest {
    name: String,
    protocol: Option<String>,
    model_patterns: String,
    base_url: String,
    real_api_key: String,
    priority: Option<i32>,
    compression_enabled: Option<bool>,
}

#[derive(Deserialize)]
struct UpdateModelRequest {
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
async fn list_models(
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
async fn create_model(
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
async fn update_model(
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
async fn delete_model(
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

// ============================================================
// 日志查询 API
// ============================================================

#[derive(Deserialize)]
struct LogQuery {
    page: Option<i64>,
    #[serde(rename = "pageSize")]
    page_size: Option<i64>,
    org_id: Option<Uuid>,
    project_id: Option<Uuid>,
    api_key_id: Option<Uuid>,
    model: Option<String>,
    status: Option<String>,
    start_time: Option<String>,
    end_time: Option<String>,
}

#[derive(Serialize)]
struct PaginatedLogs {
    data: Vec<LogSummaryRow>,
    total: i64,
    page: i64,
    page_size: i64,
}

/// GET /api/logs — 分页查询日志列表
async fn list_logs(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Query(q): Query<LogQuery>,
) -> Result<Json<PaginatedLogs>, (StatusCode, Json<ErrorResponse>)> {
    let page = q.page.unwrap_or(1).max(1);
    let page_size = q.page_size.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * page_size;

    // 动态构建 WHERE 条件
    let mut conditions = Vec::new();
    let mut param_idx = 1u32;

    if q.org_id.is_some() {
        conditions.push(format!("org_id = ${param_idx}"));
        param_idx += 1;
    }
    if q.project_id.is_some() {
        conditions.push(format!("project_id = ${param_idx}"));
        param_idx += 1;
    }
    if q.api_key_id.is_some() {
        conditions.push(format!("api_key_id = ${param_idx}"));
        param_idx += 1;
    }
    if q.model.is_some() {
        conditions.push(format!("model ILIKE ${param_idx}"));
        param_idx += 1;
    }
    if q.status.is_some() {
        conditions.push(format!("status = ${param_idx}"));
        param_idx += 1;
    }
    let start_ts = q.start_time.as_ref().and_then(|s| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").ok()
        .or_else(|| chrono::NaiveDateTime::parse_from_str(&format!("{s} 00:00:00"), "%Y-%m-%d %H:%M:%S").ok()))
        .map(|dt| dt.and_utc());
    let end_ts = q.end_time.as_ref().and_then(|s| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").ok()
        .or_else(|| chrono::NaiveDateTime::parse_from_str(&format!("{s} 23:59:59"), "%Y-%m-%d %H:%M:%S").ok()))
        .map(|dt| dt.and_utc());
    if start_ts.is_some() {
        conditions.push(format!("created_at >= ${param_idx}"));
        param_idx += 1;
    }
    if end_ts.is_some() {
        conditions.push(format!("created_at <= ${param_idx}"));
        param_idx += 1;
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    // 查询总数
    let count_sql = format!("SELECT COUNT(*) as count FROM request_logs {where_clause}");
    let mut count_query = sqlx::query_scalar::<_, i64>(&count_sql);
    if let Some(ref v) = q.org_id { count_query = count_query.bind(v); }
    if let Some(ref v) = q.project_id { count_query = count_query.bind(v); }
    if let Some(ref v) = q.api_key_id { count_query = count_query.bind(v); }
    if let Some(ref v) = q.model { count_query = count_query.bind(format!("%{v}%")); }
    if let Some(ref v) = q.status { count_query = count_query.bind(v); }
    if let Some(ref v) = start_ts { count_query = count_query.bind(v); }
    if let Some(ref v) = end_ts { count_query = count_query.bind(v); }

    let total = count_query
        .fetch_one(&state.pool)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // 查询数据
    let data_sql = format!(
        "SELECT id, org_id, project_id, api_key_id, provider, model, path, method, is_stream, \
                response_status, status, prompt_tokens, completion_tokens, cached_tokens, \
                (COALESCE(prompt_tokens, 0) + COALESCE(completion_tokens, 0) + COALESCE(cached_tokens, 0)) AS total_tokens, \
                duration_ms, error_message, credit_cost, money_cost, is_long_context, compressed, est_tokens_saved, created_at \
         FROM request_logs {where_clause} \
         ORDER BY created_at DESC \
         LIMIT ${param_idx} OFFSET ${}",
        param_idx + 1
    );

    let mut data_query = sqlx::query_as::<_, LogSummaryRow>(&data_sql);
    if let Some(ref v) = q.org_id { data_query = data_query.bind(v); }
    if let Some(ref v) = q.project_id { data_query = data_query.bind(v); }
    if let Some(ref v) = q.api_key_id { data_query = data_query.bind(v); }
    if let Some(ref v) = q.model { data_query = data_query.bind(format!("%{v}%")); }
    if let Some(ref v) = q.status { data_query = data_query.bind(v); }
    if let Some(ref v) = start_ts { data_query = data_query.bind(v); }
    if let Some(ref v) = end_ts { data_query = data_query.bind(v); }
    data_query = data_query.bind(page_size).bind(offset);

    let data = data_query
        .fetch_all(&state.pool)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(PaginatedLogs {
        data,
        total,
        page,
        page_size,
    }))
}

/// GET /api/logs/:id — 日志详情
async fn get_log(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Path(id): Path<Uuid>,
) -> Result<Json<LogRow>, (StatusCode, Json<ErrorResponse>)> {
    let row = sqlx::query_as::<_, LogRow>(
        "SELECT id, org_id, project_id, api_key_id, provider, model, path, method, is_stream, \
                request_body, response_body, response_status, status, \
                prompt_tokens, completion_tokens, cached_tokens, total_tokens, \
                duration_ms, error_message, credit_cost, money_cost, is_long_context, \
                compressed, compression_mode, original_prompt_chars, forwarded_prompt_chars, est_tokens_saved, \
                created_at, updated_at \
         FROM request_logs WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .ok_or_else(|| err(StatusCode::NOT_FOUND, "Log not found"))?;

    Ok(Json(row))
}

// ============================================================
// 积分系统 API
// ============================================================

#[derive(Serialize, sqlx::FromRow)]
struct CreditLogSummary {
    id: Uuid,
    org_id: Uuid,
    amount: rust_decimal::Decimal,
    balance_after: rust_decimal::Decimal,
    transaction_type: String,
    reference_id: Option<String>,
    created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct RechargeRequest {
    amount: rust_decimal::Decimal,
    note: Option<String>,
}

/// GET /api/settings/credit_rates
async fn get_credit_rates(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
) -> Result<Json<crate::db::CreditRates>, (StatusCode, Json<ErrorResponse>)> {
    let rates = crate::db::get_credit_rates(&state.pool).await;
    Ok(Json(rates))
}

/// PUT /api/settings/credit_rates
async fn update_credit_rates(
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
async fn get_mail_settings(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
) -> Result<Json<crate::db::MailSettings>, (StatusCode, Json<ErrorResponse>)> {
    let settings = crate::db::get_mail_settings(&state.pool).await;
    Ok(Json(settings))
}

/// PUT /api/settings/mail
async fn update_mail_settings(
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
async fn get_compression(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
) -> Result<Json<crate::compress::CompressionConfig>, (StatusCode, Json<ErrorResponse>)> {
    let cfg = crate::db::get_compression_config(&state.pool).await;
    Ok(Json(cfg))
}

/// PUT /api/settings/compression
async fn update_compression(
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

#[derive(Deserialize)]
struct ExportUsageReportRequest {
    org_ids: Vec<Uuid>,
    month: Option<String>,
    start_time: Option<String>,
    end_time: Option<String>,
    recipient_email: String,
}

#[derive(Serialize)]
struct ExportUsageReportResponse {
    ok: bool,
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
    org_count: usize,
    total_requests: i64,
    total_credit_cost: f64,
    total_money_cost: f64,
}

/// 按组织+项目维度的汇总行
#[derive(sqlx::FromRow)]
struct UsageReportDetailRow {
    org_name: String,
    project_name: Option<String>,
    #[allow(dead_code)]
    credit_price: f64,
    request_count: i64,
    total_credit_cost: f64,
    total_money_cost: f64,
}

/// POST /api/usage/export_report
async fn export_usage_report(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Json(body): Json<ExportUsageReportRequest>,
) -> Result<Json<ExportUsageReportResponse>, (StatusCode, Json<ErrorResponse>)> {
    if body.org_ids.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "请至少选择一个企业"));
    }
    if body.recipient_email.trim().is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "收件邮箱不能为空"));
    }

    let (start_time, end_time) = parse_report_time_range(
        body.month.as_deref(),
        body.start_time.as_deref(),
        body.end_time.as_deref(),
    )?;

    let mail_settings = crate::db::get_mail_settings(&state.pool).await;
    validate_mail_settings(&mail_settings)?;

    let details = sqlx::query_as::<_, UsageReportDetailRow>(
        "SELECT o.name AS org_name, \
                p.name AS project_name, \
                COALESCE(o.credit_price::FLOAT8, 0) AS credit_price, \
                COUNT(*)::BIGINT AS request_count, \
                COALESCE(SUM(r.credit_cost)::FLOAT8, 0) AS total_credit_cost, \
                COALESCE(SUM(r.money_cost)::FLOAT8, 0) AS total_money_cost \
         FROM request_logs r \
         JOIN organizations o ON o.id = r.org_id \
         LEFT JOIN projects p ON p.id = r.project_id \
         WHERE r.org_id = ANY($1) AND r.created_at >= $2 AND r.created_at < $3 \
         GROUP BY o.name, p.name, o.credit_price \
         ORDER BY o.name, p.name",
    )
    .bind(&body.org_ids)
    .bind(start_time)
    .bind(end_time)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // 按组织分组
    let mut org_groups: Vec<(String, Vec<&UsageReportDetailRow>)> = Vec::new();
    for row in &details {
        if let Some(group) = org_groups.iter_mut().find(|(name, _)| name == &row.org_name) {
            group.1.push(row);
        } else {
            org_groups.push((row.org_name.clone(), vec![row]));
        }
    }

    let period_start = start_time.format("%Y-%m-%d").to_string();
    let period_end = (end_time - chrono::TimeDelta::seconds(1)).format("%Y-%m-%d").to_string();
    let generated_at = chrono::Utc::now().format("%Y-%m-%d %H:%M").to_string();
    let period_tag = format!("{}_{}", start_time.format("%Y%m%d"), (end_time - chrono::TimeDelta::seconds(1)).format("%Y%m%d"));

    // 为每个组织生成独立 PDF
    let mut pdf_attachments: Vec<PdfAttachment> = Vec::new();
    let mut total_requests: i64 = 0;
    let mut total_credit_cost: f64 = 0.0;
    let mut total_money_cost: f64 = 0.0;

    for (org_name, rows) in &org_groups {
        let bill_no = generate_bill_no();
        let projects: Vec<(Option<String>, i64, f64, f64)> = rows.iter()
            .map(|r| (r.project_name.clone(), r.request_count, r.total_credit_cost, r.total_money_cost))
            .collect();
        let org_requests: i64 = rows.iter().map(|r| r.request_count).sum();
        let org_credit: f64 = rows.iter().map(|r| r.total_credit_cost).sum();
        let org_money: f64 = rows.iter().map(|r| r.total_money_cost).sum();

        total_requests += org_requests;
        total_credit_cost += org_credit;
        total_money_cost += org_money;

        let pdf_data = generate_org_invoice_pdf(
            &bill_no, &period_start, &period_end, &generated_at,
            org_name, &projects, org_requests, org_credit, org_money,
        ).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;

        let filename = format!("LLMeter账单_{org_name}_{period_tag}.pdf");
        pdf_attachments.push(PdfAttachment { filename, data: pdf_data });
    }

    if pdf_attachments.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "所选时间段内无使用记录"));
    }

    send_usage_report_mail(
        &mail_settings,
        body.recipient_email.trim(),
        start_time,
        end_time,
        &pdf_attachments,
    )
    .await?;

    Ok(Json(ExportUsageReportResponse {
        ok: true,
        start_time,
        end_time,
        org_count: body.org_ids.len(),
        total_requests,
        total_credit_cost,
        total_money_cost,
    }))
}

fn parse_report_time_range(
    month: Option<&str>,
    start_time: Option<&str>,
    end_time: Option<&str>,
) -> Result<(DateTime<Utc>, DateTime<Utc>), (StatusCode, Json<ErrorResponse>)> {
    let has_custom = start_time.is_some() || end_time.is_some();
    if month.is_some() && has_custom {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "月份和自定义时间段只能二选一",
        ));
    }

    if let Some(month_str) = month {
        let month_start_date = chrono::NaiveDate::parse_from_str(
            &format!("{month_str}-01"),
            "%Y-%m-%d",
        )
        .map_err(|_| err(StatusCode::BAD_REQUEST, "月份格式错误，请使用 YYYY-MM"))?;
        let month_end_date = month_start_date
            .checked_add_months(chrono::Months::new(1))
            .ok_or_else(|| err(StatusCode::BAD_REQUEST, "月份无效"))?;
        let month_start = month_start_date
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| err(StatusCode::BAD_REQUEST, "月份无效"))?
            .and_utc();
        let month_end = month_end_date
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| err(StatusCode::BAD_REQUEST, "月份无效"))?
            .and_utc();
        return Ok((month_start, month_end));
    }

    let parse_start = |s: &str| {
        chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
            .ok()
            .or_else(|| {
                chrono::NaiveDateTime::parse_from_str(&format!("{s} 00:00:00"), "%Y-%m-%d %H:%M:%S")
                    .ok()
            })
            .map(|dt| dt.and_utc())
    };
    let parse_end_exclusive = |s: &str| {
        chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
            .ok()
            .map(|dt| dt.and_utc() + chrono::TimeDelta::seconds(1))
            .or_else(|| {
                chrono::NaiveDateTime::parse_from_str(&format!("{s} 23:59:59"), "%Y-%m-%d %H:%M:%S")
                    .ok()
                    .map(|dt| dt.and_utc() + chrono::TimeDelta::seconds(1))
            })
    };

    let start = start_time
        .and_then(parse_start)
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "请选择月份或完整的开始时间"))?;
    let end = end_time
        .and_then(parse_end_exclusive)
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "请选择完整的结束时间"))?;

    if end <= start {
        return Err(err(StatusCode::BAD_REQUEST, "结束时间必须大于开始时间"));
    }

    Ok((start, end))
}

fn validate_mail_settings(
    settings: &crate::db::MailSettings,
) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
    if settings.outbound.host.trim().is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "请先在系统设置中配置发件服务器地址"));
    }
    if settings.outbound.sender_email.trim().is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "请先在系统设置中配置发件邮箱"));
    }
    Ok(())
}

/// 内嵌的霞鹜新晰黑字体（静态 TTF 黑体，编译时打包，无运行时依赖）
static EMBEDDED_FONT: &[u8] = include_bytes!("../fonts/LXGWNeoXiHei.ttf");

fn generate_bill_no() -> String {
    let ts = chrono::Utc::now().format("%Y%m%d%H%M%S").to_string();
    let r: u32 = rand::random::<u32>() % 10000;
    format!("LLM-{ts}-{r:04}")
}

struct PdfAttachment {
    filename: String,
    data: Vec<u8>,
}

/// 对内嵌字体做子集化，只保留 `text` 中出现的字符，大幅减小 PDF 体积
fn subset_font(text: &str) -> Result<Vec<u8>, String> {
    use std::collections::BTreeSet;
    let reader = font_subset::FontReader::new(EMBEDDED_FONT)
        .map_err(|e| format!("字体读取失败: {e}"))?;
    let font = reader.read()
        .map_err(|e| format!("字体解析失败: {e}"))?;
    let chars: BTreeSet<char> = text.chars().collect();
    let subset = font.subset(&chars)
        .map_err(|e| format!("字体子集化失败: {e}"))?;
    Ok(subset.to_opentype())
}

/// 生成单个组织的账单 PDF（字体按需子集化，体积小、速度快）
fn generate_org_invoice_pdf(
    bill_no: &str,
    period_start: &str,
    period_end: &str,
    generated_at: &str,
    org_name: &str,
    projects: &[(Option<String>, i64, f64, f64)],
    total_requests: i64,
    total_credit: f64,
    total_money: f64,
) -> Result<Vec<u8>, String> {
    use printpdf::*;

    // ── 预先收集所有文本，用于字体子集化 ──
    let mut all_text = String::new();
    all_text.push_str("LLMeter 账单");
    all_text.push_str(&format!("账单编号：{bill_no}"));
    all_text.push_str(&format!("账单周期：{period_start} ～ {period_end}"));
    all_text.push_str(&format!("生成时间：{generated_at}"));
    all_text.push_str(&format!("组织：{org_name}"));
    all_text.push_str("总调用次数Credit 用量应付金额");
    all_text.push_str(&format!("{total_requests}{total_credit:.2}¥{total_money:.2}"));
    all_text.push_str("项目调用次数Credit金额 (¥)");
    for (name, reqs, credit, money) in projects {
        all_text.push_str(name.as_deref().unwrap_or("(未分配)"));
        all_text.push_str(&format!("{reqs}{credit:.2}{money:.2}"));
    }
    all_text.push_str("合计");
    all_text.push_str("此账单由 LLMeter 系统自动生成，如有疑问请联系管理员。");

    let font_data = subset_font(&all_text)?;

    let (doc, page1, layer1) = PdfDocument::new("LLMeter Invoice", Mm(210.0), Mm(297.0), "Layer 1");
    let font = doc.add_external_font(&mut std::io::Cursor::new(&font_data))
        .map_err(|e| format!("子集字体加载失败: {e}"))?;

    let layer = doc.get_page(page1).get_layer(layer1);

    let lx: f32 = 28.0;
    let rx: f32 = 182.0;
    let c2: f32 = 98.0;
    let c3: f32 = 132.0;
    let c4: f32 = 162.0;
    let mut y: f32 = 270.0;

    macro_rules! text {
        ($t:expr, $sz:expr, $x:expr, $yy:expr) => {
            layer.use_text($t, $sz, Mm($x), Mm($yy), &font);
        };
    }
    macro_rules! hline {
        ($yy:expr, $th:expr, $g:expr) => {
            layer.set_outline_color(Color::Greyscale(Greyscale::new($g, None)));
            layer.set_outline_thickness($th);
            layer.add_line(Line {
                points: vec![
                    (Point::new(Mm(lx), Mm($yy)), false),
                    (Point::new(Mm(rx), Mm($yy)), false),
                ],
                is_closed: false,
            });
        };
    }
    macro_rules! fill {
        ($g:expr) => {
            layer.set_fill_color(Color::Greyscale(Greyscale::new($g, None)));
        };
    }

    // ── 标题 ──
    fill!(0.0);
    text!("LLMeter 账单", 22.0, lx, y);
    y -= 16.0;

    // ── 账单信息 ──
    fill!(0.25);
    text!(&format!("账单编号：{bill_no}"), 9.0, lx, y);
    text!(&format!("账单周期：{period_start} ～ {period_end}"), 9.0, 108.0, y);
    y -= 5.5;
    text!(&format!("生成时间：{generated_at}"), 9.0, lx, y);
    fill!(0.0);
    text!(&format!("组织：{org_name}"), 9.0, 108.0, y);
    y -= 8.0;

    // ── 分割线 ──
    hline!(y, 0.6, 0.7);
    y -= 14.0;

    // ── 汇总区 ──
    fill!(0.3);
    text!("总调用次数", 8.0, lx, y + 6.0);
    text!("Credit 用量", 8.0, 78.0, y + 6.0);
    text!("应付金额", 8.0, 138.0, y + 6.0);
    fill!(0.0);
    text!(&total_requests.to_string(), 18.0, lx, y - 5.0);
    text!(&format!("{total_credit:.2}"), 18.0, 78.0, y - 5.0);
    text!(&format!("¥{total_money:.2}"), 18.0, 138.0, y - 5.0);
    y -= 22.0;

    // ── 分割线 ──
    hline!(y, 0.6, 0.7);
    y -= 12.0;

    // ── 表头 ──
    fill!(0.2);
    text!("项目", 9.0, lx, y);
    text!("调用次数", 9.0, c2, y);
    text!("Credit", 9.0, c3, y);
    text!("金额 (¥)", 9.0, c4, y);
    y -= 3.5;
    hline!(y, 0.4, 0.6);
    y -= 8.0;

    // ── 数据行 ──
    fill!(0.05);
    for (name, reqs, credit, money) in projects {
        let name = name.as_deref().unwrap_or("(未分配)");
        text!(name, 10.0, lx, y);
        text!(&reqs.to_string(), 10.0, c2, y);
        text!(&format!("{credit:.2}"), 10.0, c3, y);
        text!(&format!("{money:.2}"), 10.0, c4, y);
        y -= 7.5;
    }

    // ── 合计粗线 ──
    y += 3.0;
    hline!(y, 1.2, 0.0);
    y -= 9.0;

    // ── 合计行 ──
    fill!(0.0);
    text!("合计", 11.0, lx, y);
    text!(&total_requests.to_string(), 11.0, c2, y);
    text!(&format!("{total_credit:.2}"), 11.0, c3, y);
    text!(&format!("¥{total_money:.2}"), 11.0, c4, y);

    // ── 脚注 ──
    fill!(0.4);
    text!("此账单由 LLMeter 系统自动生成，如有疑问请联系管理员。", 7.0, lx, 30.0);

    doc.save_to_bytes().map_err(|e| format!("PDF 保存失败: {e}"))
}

async fn send_usage_report_mail(
    settings: &crate::db::MailSettings,
    recipient_email: &str,
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
    attachments: &[PdfAttachment],
) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
    let sender = if settings.outbound.sender_name.trim().is_empty() {
        settings.outbound.sender_email.clone()
    } else {
        format!(
            "{} <{}>",
            settings.outbound.sender_name.trim(),
            settings.outbound.sender_email.trim()
        )
    };

    let from_mailbox: Mailbox = sender
        .parse()
        .map_err(|_| err(StatusCode::BAD_REQUEST, "发件邮箱格式无效"))?;
    let to_mailbox: Mailbox = recipient_email
        .parse()
        .map_err(|_| err(StatusCode::BAD_REQUEST, "收件邮箱格式无效"))?;

    let subject = format!(
        "LLMeter 账单 {} - {}",
        start_time.format("%Y-%m-%d"),
        (end_time - chrono::TimeDelta::seconds(1)).format("%Y-%m-%d")
    );

    let att_count = attachments.len();
    let body_text = format!("请查收附件中的 LLMeter 使用账单（共 {att_count} 份）。");

    let mut mp = MultiPart::mixed()
        .singlepart(SinglePart::plain(body_text));

    for att in attachments {
        let ct: ContentType = "application/pdf".parse().unwrap();
        mp = mp.singlepart(
            Attachment::new(att.filename.clone())
                .body(att.data.clone(), ct)
        );
    }

    let email = lettre::Message::builder()
        .from(from_mailbox)
        .to(to_mailbox)
        .subject(subject)
        .multipart(mp)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut transport_builder = if settings.outbound.use_tls {
        AsyncSmtpTransport::<Tokio1Executor>::relay(settings.outbound.host.trim())
            .map_err(|_| err(StatusCode::BAD_REQUEST, "发件服务器地址无效"))?
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(settings.outbound.host.trim())
    };
    transport_builder = transport_builder.port(settings.outbound.port);

    if !settings.outbound.username.trim().is_empty() {
        transport_builder = transport_builder.credentials(Credentials::new(
            settings.outbound.username.clone(),
            settings.outbound.password.clone(),
        ));
    }

    let mailer = transport_builder.build();
    mailer
        .send(email)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("邮件发送失败: {e}")))?;

    Ok(())
}
/// POST /api/orgs/:id/credit
async fn recharge_credit(
    State(state): State<Arc<AppState>>,
    admin: AuthAdmin,
    Path(id): Path<Uuid>,
    Json(body): Json<RechargeRequest>,
) -> Result<Json<OrgRow>, (StatusCode, Json<ErrorResponse>)> {
    if body.amount.is_zero() {
        return Err(err(StatusCode::BAD_REQUEST, "Amount cannot be zero"));
    }

    let mut tx = state.pool.begin().await.map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let row = sqlx::query_as::<_, OrgRow>(
        "UPDATE organizations SET credit = credit + $1, updated_at = now() WHERE id = $2 \
         RETURNING id, name, slug, credit, overdraft_limit, credit_price, is_active, \
                   COALESCE((SELECT ABS(SUM(amount)) FROM credit_logs WHERE org_id = id AND transaction_type = 'consume'), 0) AS total_consumed, \
                   created_at, updated_at"
    )
    .bind(body.amount)
    .bind(id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .ok_or_else(|| err(StatusCode::NOT_FOUND, "Organization not found"))?;

    let ref_id = body.note.unwrap_or_else(|| format!("admin:{}", admin.user_id));

    sqlx::query(
        "INSERT INTO credit_logs (org_id, amount, balance_after, transaction_type, reference_id) \
         VALUES ($1, $2, $3, 'recharge', $4)"
    )
    .bind(id)
    .bind(body.amount)
    .bind(row.credit)
    .bind(ref_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    tx.commit().await.map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(row))
}

/// GET /api/orgs/:id/credit_logs?page=1&page_size=20&type=consume
async fn list_credit_logs(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Path(id): Path<Uuid>,
    Query(params): Query<CreditLogQuery>,
) -> Result<Json<CreditLogPage>, (StatusCode, Json<ErrorResponse>)> {
    let page = params.page.unwrap_or(1).max(1);
    let page_size = params.page_size.unwrap_or(20).min(100);
    let offset = (page - 1) * page_size;

    let (where_extra, type_filter) = match &params.r#type {
        Some(t) if !t.is_empty() => (" AND transaction_type = $2", Some(t.as_str())),
        _ => ("", None),
    };

    let count_sql = format!(
        "SELECT COUNT(*) FROM credit_logs WHERE org_id = $1{where_extra}"
    );
    let data_sql = format!(
        "SELECT id, org_id, amount, balance_after, transaction_type, reference_id, created_at \
         FROM credit_logs WHERE org_id = $1{where_extra} ORDER BY created_at DESC LIMIT {} OFFSET {}",
        page_size, offset
    );

    let total: i64 = if let Some(tf) = type_filter {
        sqlx::query_scalar(&count_sql).bind(id).bind(tf).fetch_one(&state.pool).await
    } else {
        sqlx::query_scalar(&count_sql).bind(id).fetch_one(&state.pool).await
    }.map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let rows: Vec<CreditLogSummary> = if let Some(tf) = type_filter {
        sqlx::query_as(&data_sql).bind(id).bind(tf).fetch_all(&state.pool).await
    } else {
        sqlx::query_as(&data_sql).bind(id).fetch_all(&state.pool).await
    }.map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(CreditLogPage {
        data: rows,
        total,
        page,
        page_size,
    }))
}

#[derive(Deserialize)]
struct CreditLogQuery {
    page: Option<i64>,
    page_size: Option<i64>,
    r#type: Option<String>,
}

#[derive(Serialize)]
struct CreditLogPage {
    data: Vec<CreditLogSummary>,
    total: i64,
    page: i64,
    page_size: i64,
}

#[derive(Deserialize)]
struct StatsQuery {
    days: Option<i32>,
    org_id: Option<Uuid>,
    project_id: Option<Uuid>,
    api_key_id: Option<Uuid>,
    model: Option<String>,
    start_time: Option<String>,
    end_time: Option<String>,
}

#[derive(Serialize)]
struct StatsResponse {
    overview: StatsOverview,
    by_org: Vec<OrgStats>,
    by_project: Vec<ProjectStats>,
    by_model: Vec<ModelStats>,
    daily_stats: Vec<DailyStats>,
}

#[derive(Serialize, sqlx::FromRow)]
struct StatsOverview {
    total_requests: i64,
    success_requests: i64,
    error_requests: i64,
    prompt_tokens: i64,
    completion_tokens: i64,
    cached_tokens: i64,
    total_tokens: i64,
    avg_duration_ms: f64,
    total_credit_cost: f64,
    compressed_requests: i64,
    est_tokens_saved: i64,
}

#[derive(Serialize, sqlx::FromRow)]
struct OrgStats {
    org_id: Uuid,
    org_name: String,
    request_count: i64,
    prompt_tokens: i64,
    completion_tokens: i64,
    cached_tokens: i64,
    total_tokens: i64,
    total_credit_cost: f64,
    error_count: i64,
}

#[derive(Serialize, sqlx::FromRow)]
struct ProjectStats {
    project_id: Option<Uuid>,
    project_name: String,
    org_name: String,
    request_count: i64,
    prompt_tokens: i64,
    completion_tokens: i64,
    cached_tokens: i64,
    total_tokens: i64,
    total_credit_cost: f64,
    error_count: i64,
}

#[derive(Serialize, sqlx::FromRow)]
struct ModelStats {
    model: String,
    request_count: i64,
    prompt_tokens: i64,
    completion_tokens: i64,
    cached_tokens: i64,
    total_tokens: i64,
    avg_duration_ms: f64,
    total_credit_cost: f64,
}

#[derive(Serialize, sqlx::FromRow)]
struct DailyStats {
    date: chrono::NaiveDate,
    request_count: i64,
    prompt_tokens: i64,
    completion_tokens: i64,
    cached_tokens: i64,
    total_tokens: i64,
    error_count: i64,
    credit_cost: f64,
}

/// GET /api/stats — 统计数据（支持组织/Key/模型筛选）
async fn get_stats(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Query(q): Query<StatsQuery>,
) -> Result<Json<StatsResponse>, (StatusCode, Json<ErrorResponse>)> {
    // 时间范围：优先使用 start_time/end_time，否则按 days 计算
    let parse_start = |s: &str| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").ok()
        .or_else(|| chrono::NaiveDateTime::parse_from_str(&format!("{s} 00:00:00"), "%Y-%m-%d %H:%M:%S").ok())
        .map(|dt| dt.and_utc());
    let parse_end = |s: &str| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").ok()
        .or_else(|| chrono::NaiveDateTime::parse_from_str(&format!("{s} 23:59:59"), "%Y-%m-%d %H:%M:%S").ok())
        .map(|dt| dt.and_utc());

    let since = q.start_time.as_deref().and_then(parse_start)
        .unwrap_or_else(|| Utc::now() - chrono::TimeDelta::days(q.days.unwrap_or(7).max(1) as i64));
    let until: Option<DateTime<Utc>> = q.end_time.as_deref().and_then(parse_end);

    // 动态构建 WHERE 条件和参数
    let mut extra_where = String::new();
    let mut param_idx: u32 = 2; // $1 = since
    if until.is_some() {
        extra_where += &format!(" AND created_at <= ${param_idx}");
        param_idx += 1;
    }
    if q.org_id.is_some() {
        extra_where += &format!(" AND org_id = ${param_idx}");
        param_idx += 1;
    }
    if q.project_id.is_some() {
        extra_where += &format!(" AND project_id = ${param_idx}");
        param_idx += 1;
    }
    if q.api_key_id.is_some() {
        extra_where += &format!(" AND api_key_id = ${param_idx}");
        param_idx += 1;
    }
    if q.model.is_some() {
        extra_where += &format!(" AND model ILIKE ${param_idx}");
    }

    let model_pattern = q.model.as_ref().map(|m| format!("%{m}%"));

    /// 将动态筛选参数绑定到已有查询上
    macro_rules! bind_filters {
        ($query:expr) => {{
            let mut q_inner = $query;
            if let Some(ref u) = until { q_inner = q_inner.bind(u); }
            if let Some(ref oid) = q.org_id { q_inner = q_inner.bind(oid); }
            if let Some(ref pid) = q.project_id { q_inner = q_inner.bind(pid); }
            if let Some(ref kid) = q.api_key_id { q_inner = q_inner.bind(kid); }
            if let Some(ref mp) = model_pattern { q_inner = q_inner.bind(mp); }
            q_inner
        }};
    }

    // 概览
    let overview_sql = format!(
        "SELECT \
            COUNT(*)::BIGINT AS total_requests, \
            COUNT(*) FILTER (WHERE status = 'success')::BIGINT AS success_requests, \
            COUNT(*) FILTER (WHERE status = 'error')::BIGINT AS error_requests, \
            COALESCE(SUM(prompt_tokens), 0)::BIGINT AS prompt_tokens, \
            COALESCE(SUM(completion_tokens), 0)::BIGINT AS completion_tokens, \
            COALESCE(SUM(cached_tokens), 0)::BIGINT AS cached_tokens, \
            (COALESCE(SUM(prompt_tokens), 0) + COALESCE(SUM(completion_tokens), 0) + COALESCE(SUM(cached_tokens), 0))::BIGINT AS total_tokens, \
            COALESCE(AVG(duration_ms)::FLOAT8, 0) AS avg_duration_ms, \
            COALESCE(SUM(credit_cost)::FLOAT8, 0) AS total_credit_cost, \
            COUNT(*) FILTER (WHERE compressed)::BIGINT AS compressed_requests, \
            COALESCE(SUM(est_tokens_saved), 0)::BIGINT AS est_tokens_saved \
         FROM request_logs WHERE created_at >= $1 {extra_where}"
    );
    let overview = bind_filters!(sqlx::query_as::<_, StatsOverview>(&overview_sql).bind(since))
        .fetch_one(&state.pool).await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // 按组织（使用 r. 前缀，因为有 JOIN）
    let org_extra = extra_where.replace("created_at", "r.created_at")
        .replace("org_id", "r.org_id")
        .replace("api_key_id", "r.api_key_id")
        .replace("model", "r.model");
    let org_sql = format!(
        "SELECT r.org_id, COALESCE(o.name, 'unknown') AS org_name, \
            COUNT(*)::BIGINT AS request_count, \
            COALESCE(SUM(r.prompt_tokens), 0)::BIGINT AS prompt_tokens, \
            COALESCE(SUM(r.completion_tokens), 0)::BIGINT AS completion_tokens, \
            COALESCE(SUM(r.cached_tokens), 0)::BIGINT AS cached_tokens, \
            (COALESCE(SUM(r.prompt_tokens), 0) + COALESCE(SUM(r.completion_tokens), 0) + COALESCE(SUM(r.cached_tokens), 0))::BIGINT AS total_tokens, \
            COALESCE(SUM(r.credit_cost)::FLOAT8, 0) AS total_credit_cost, \
            COUNT(*) FILTER (WHERE r.status = 'error')::BIGINT AS error_count \
         FROM request_logs r LEFT JOIN organizations o ON r.org_id = o.id \
         WHERE r.created_at >= $1 {org_extra} \
         GROUP BY r.org_id, o.name ORDER BY request_count DESC"
    );
    let by_org = bind_filters!(sqlx::query_as::<_, OrgStats>(&org_sql).bind(since))
        .fetch_all(&state.pool).await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // 按项目
    let proj_extra = extra_where.replace("created_at", "r.created_at")
        .replace("org_id", "r.org_id")
        .replace("project_id", "r.project_id")
        .replace("api_key_id", "r.api_key_id")
        .replace("model", "r.model");
    let proj_sql = format!(
        "SELECT r.project_id, COALESCE(p.name, 'unknown') AS project_name, COALESCE(o.name, 'unknown') AS org_name, \
            COUNT(*)::BIGINT AS request_count, \
            COALESCE(SUM(r.prompt_tokens), 0)::BIGINT AS prompt_tokens, \
            COALESCE(SUM(r.completion_tokens), 0)::BIGINT AS completion_tokens, \
            COALESCE(SUM(r.cached_tokens), 0)::BIGINT AS cached_tokens, \
            (COALESCE(SUM(r.prompt_tokens), 0) + COALESCE(SUM(r.completion_tokens), 0) + COALESCE(SUM(r.cached_tokens), 0))::BIGINT AS total_tokens, \
            COALESCE(SUM(r.credit_cost)::FLOAT8, 0) AS total_credit_cost, \
            COUNT(*) FILTER (WHERE r.status = 'error')::BIGINT AS error_count \
         FROM request_logs r \
         LEFT JOIN projects p ON r.project_id = p.id \
         LEFT JOIN organizations o ON r.org_id = o.id \
         WHERE r.created_at >= $1 {proj_extra} \
         GROUP BY r.project_id, p.name, o.name ORDER BY request_count DESC"
    );
    let by_project = bind_filters!(sqlx::query_as::<_, ProjectStats>(&proj_sql).bind(since))
        .fetch_all(&state.pool).await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // 按 model
    let model_sql = format!(
        "SELECT COALESCE(model, 'unknown') AS model, \
            COUNT(*)::BIGINT AS request_count, \
            COALESCE(SUM(prompt_tokens), 0)::BIGINT AS prompt_tokens, \
            COALESCE(SUM(completion_tokens), 0)::BIGINT AS completion_tokens, \
            COALESCE(SUM(cached_tokens), 0)::BIGINT AS cached_tokens, \
            (COALESCE(SUM(prompt_tokens), 0) + COALESCE(SUM(completion_tokens), 0) + COALESCE(SUM(cached_tokens), 0))::BIGINT AS total_tokens, \
            COALESCE(AVG(duration_ms)::FLOAT8, 0) AS avg_duration_ms, \
            COALESCE(SUM(credit_cost)::FLOAT8, 0) AS total_credit_cost \
         FROM request_logs WHERE created_at >= $1 {extra_where} \
         GROUP BY model ORDER BY request_count DESC"
    );
    let by_model = bind_filters!(sqlx::query_as::<_, ModelStats>(&model_sql).bind(since))
        .fetch_all(&state.pool).await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // 每日趋势
    let daily_sql = format!(
        "SELECT created_at::DATE AS date, \
            COUNT(*)::BIGINT AS request_count, \
            COALESCE(SUM(prompt_tokens), 0)::BIGINT AS prompt_tokens, \
            COALESCE(SUM(completion_tokens), 0)::BIGINT AS completion_tokens, \
            COALESCE(SUM(cached_tokens), 0)::BIGINT AS cached_tokens, \
            (COALESCE(SUM(prompt_tokens), 0) + COALESCE(SUM(completion_tokens), 0) + COALESCE(SUM(cached_tokens), 0))::BIGINT AS total_tokens, \
            COUNT(*) FILTER (WHERE status = 'error')::BIGINT AS error_count, \
            COALESCE(SUM(credit_cost)::FLOAT8, 0) AS credit_cost \
         FROM request_logs WHERE created_at >= $1 {extra_where} \
         GROUP BY date ORDER BY date"
    );
    let daily_stats = bind_filters!(sqlx::query_as::<_, DailyStats>(&daily_sql).bind(since))
        .fetch_all(&state.pool).await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(StatsResponse { overview, by_org, by_project, by_model, daily_stats }))
}
