use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::{delete, get, post, put},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::{self, AuthAdmin};
use crate::state::AppState;

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
        // API Key 管理
        .route(
            "/api/orgs/{org_id}/keys",
            get(list_keys).post(create_key),
        )
        .route("/api/keys/{id}", delete(delete_key))
        // 模型配置管理
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
        .route("/api/settings/credit_rates", get(get_credit_rates).put(update_credit_rates))
        .route("/api/settings/compression", get(get_compression).put(update_compression))
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
    is_active: bool,
    total_consumed: Option<rust_decimal::Decimal>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Serialize, sqlx::FromRow)]
struct ApiKeyRow {
    id: Uuid,
    org_id: Uuid,
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
}

/// GET /api/orgs — 列出所有组织
async fn list_orgs(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
) -> Result<Json<Vec<OrgRow>>, (StatusCode, Json<ErrorResponse>)> {
    let rows = sqlx::query_as::<_, OrgRow>(
        "SELECT o.id, o.name, o.slug, o.credit, o.overdraft_limit, o.is_active, \
                COALESCE((SELECT ABS(SUM(amount)) FROM credit_logs WHERE org_id = o.id AND transaction_type = 'consume'), 0) AS total_consumed, \
                o.created_at, o.updated_at \
         FROM organizations o ORDER BY o.created_at DESC",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(rows))
}

/// POST /api/orgs — 创建组织
async fn create_org(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Json(body): Json<CreateOrgRequest>,
) -> Result<(StatusCode, Json<OrgRow>), (StatusCode, Json<ErrorResponse>)> {
    let row = sqlx::query_as::<_, OrgRow>(
        "INSERT INTO organizations (name, slug) VALUES ($1, $2) \
         RETURNING id, name, slug, credit, overdraft_limit, is_active, 0::NUMERIC AS total_consumed, created_at, updated_at",
    )
    .bind(&body.name)
    .bind(&body.slug)
    .fetch_one(&state.pool)
    .await
    .map_err(friendly_db_err)?;

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
            updated_at = now() \
         WHERE id = $1 \
         RETURNING id, name, slug, credit, overdraft_limit, is_active, \
                   COALESCE((SELECT ABS(SUM(amount)) FROM credit_logs WHERE org_id = id AND transaction_type = 'consume'), 0) AS total_consumed, \
                   created_at, updated_at",
    )
    .bind(id)
    .bind(&body.name)
    .bind(&body.slug)
    .bind(body.is_active)
    .bind(body.overdraft_limit)
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
// API Key 管理
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

/// GET /api/orgs/:org_id/keys — 列出组织的所有 API Key
async fn list_keys(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Vec<ApiKeyRow>>, (StatusCode, Json<ErrorResponse>)> {
    let rows = sqlx::query_as::<_, ApiKeyRow>(
        "SELECT id, org_id, name, key_prefix, is_active, last_used_at, created_at \
         FROM api_keys WHERE org_id = $1 ORDER BY created_at DESC",
    )
    .bind(org_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(rows))
}

/// POST /api/orgs/:org_id/keys — 创建 API Key
/// Key 格式: gc-{slug}-{24 hex}，前缀包含组织 slug 便于识别归属
async fn create_key(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Path(org_id): Path<Uuid>,
    Json(body): Json<CreateKeyRequest>,
) -> Result<(StatusCode, Json<CreateKeyResponse>), (StatusCode, Json<ErrorResponse>)> {
    let slug: String = sqlx::query_scalar("SELECT slug FROM organizations WHERE id = $1")
        .bind(org_id)
        .fetch_one(&state.pool)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Organization not found"))?;

    let raw_uuid = Uuid::new_v4().to_string().replace('-', "");
    let raw_key = format!("gc-{}-{}", slug, &raw_uuid[..24]);

    let key_prefix = format!("gc-{}-", slug);

    let mut hasher = Sha256::new();
    hasher.update(raw_key.as_bytes());
    let key_hash = format!("{:x}", hasher.finalize());

    #[derive(sqlx::FromRow)]
    struct InsertedKey {
        id: Uuid,
        created_at: DateTime<Utc>,
    }

    let row = sqlx::query_as::<_, InsertedKey>(
        "INSERT INTO api_keys (org_id, name, key_hash, key_prefix) VALUES ($1, $2, $3, $4) \
         RETURNING id, created_at",
    )
    .bind(org_id)
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
    compression_enabled: Option<bool>,
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
            compression_enabled = COALESCE($9, compression_enabled), \
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
    .bind(body.compression_enabled)
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
    if let Some(ref v) = q.org_id {
        count_query = count_query.bind(v);
    }
    if let Some(ref v) = q.api_key_id {
        count_query = count_query.bind(v);
    }
    if let Some(ref v) = q.model {
        count_query = count_query.bind(format!("%{v}%"));
    }
    if let Some(ref v) = q.status {
        count_query = count_query.bind(v);
    }
    if let Some(ref v) = start_ts {
        count_query = count_query.bind(v);
    }
    if let Some(ref v) = end_ts {
        count_query = count_query.bind(v);
    }

    let total = count_query
        .fetch_one(&state.pool)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // 查询数据
    let data_sql = format!(
        "SELECT id, org_id, api_key_id, provider, model, path, method, is_stream, \
                response_status, status, prompt_tokens, completion_tokens, cached_tokens, \
                (COALESCE(prompt_tokens, 0) + COALESCE(completion_tokens, 0) + COALESCE(cached_tokens, 0)) AS total_tokens, \
                duration_ms, error_message, credit_cost, is_long_context, compressed, est_tokens_saved, created_at \
         FROM request_logs {where_clause} \
         ORDER BY created_at DESC \
         LIMIT ${param_idx} OFFSET ${}",
        param_idx + 1
    );

    let mut data_query = sqlx::query_as::<_, LogSummaryRow>(&data_sql);
    if let Some(ref v) = q.org_id {
        data_query = data_query.bind(v);
    }
    if let Some(ref v) = q.api_key_id {
        data_query = data_query.bind(v);
    }
    if let Some(ref v) = q.model {
        data_query = data_query.bind(format!("%{v}%"));
    }
    if let Some(ref v) = q.status {
        data_query = data_query.bind(v);
    }
    if let Some(ref v) = start_ts {
        data_query = data_query.bind(v);
    }
    if let Some(ref v) = end_ts {
        data_query = data_query.bind(v);
    }
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
        "SELECT id, org_id, api_key_id, provider, model, path, method, is_stream, \
                request_body, response_body, response_status, status, \
                prompt_tokens, completion_tokens, cached_tokens, total_tokens, \
                duration_ms, error_message, credit_cost, is_long_context, \
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
         RETURNING id, name, slug, credit, overdraft_limit, is_active, \
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
    api_key_id: Option<Uuid>,
    model: Option<String>,
    start_time: Option<String>,
    end_time: Option<String>,
}

#[derive(Serialize)]
struct StatsResponse {
    overview: StatsOverview,
    by_org: Vec<OrgStats>,
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

    Ok(Json(StatsResponse { overview, by_org, by_model, daily_stats }))
}
