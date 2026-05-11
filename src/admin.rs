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
    is_active: bool,
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
}

/// GET /api/orgs — 列出所有组织
async fn list_orgs(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
) -> Result<Json<Vec<OrgRow>>, (StatusCode, Json<ErrorResponse>)> {
    let rows = sqlx::query_as::<_, OrgRow>(
        "SELECT id, name, slug, credit, is_active, created_at, updated_at FROM organizations ORDER BY created_at DESC",
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
         RETURNING id, name, slug, credit, is_active, created_at, updated_at",
    )
    .bind(&body.name)
    .bind(&body.slug)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| err(StatusCode::BAD_REQUEST, e.to_string()))?;

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
            updated_at = now() \
         WHERE id = $1 \
         RETURNING id, name, slug, credit, is_active, created_at, updated_at",
    )
    .bind(id)
    .bind(&body.name)
    .bind(&body.slug)
    .bind(body.is_active)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
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
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

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
/// Key 格式: gc-{uuid去掉横线的前32字符}，存储 sha256 hash，key_prefix 保存前 10 字符
async fn create_key(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Path(org_id): Path<Uuid>,
    Json(body): Json<CreateKeyRequest>,
) -> Result<(StatusCode, Json<CreateKeyResponse>), (StatusCode, Json<ErrorResponse>)> {
    // 生成 key: gc-{32 hex chars}
    let raw_uuid = Uuid::new_v4().to_string().replace('-', "");
    let raw_key = format!("gc-{}", &raw_uuid[..32]);

    let key_prefix = raw_key[..10].to_string();

    // sha256 hash 用于存储
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
    .map_err(|e| err(StatusCode::BAD_REQUEST, e.to_string()))?;

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

/// DELETE /api/keys/:id — 删除（禁用）API Key
async fn delete_key(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    let result = sqlx::query("UPDATE api_keys SET is_active = false WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

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
}

/// GET /api/orgs/:org_id/models — 列出组织的模型配置
async fn list_models(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Vec<ModelConfigRow>>, (StatusCode, Json<ErrorResponse>)> {
    let rows = sqlx::query_as::<_, ModelConfigRow>(
        "SELECT id, org_id, name, protocol, model_patterns, base_url, real_api_key, \
                priority, is_active, created_at, updated_at \
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
        "INSERT INTO model_configs (org_id, name, protocol, model_patterns, base_url, real_api_key, priority) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         RETURNING id, org_id, name, protocol, model_patterns, base_url, real_api_key, \
                   priority, is_active, created_at, updated_at",
    )
    .bind(org_id)
    .bind(&body.name)
    .bind(body.protocol.as_deref().unwrap_or("openai"))
    .bind(&body.model_patterns)
    .bind(&body.base_url)
    .bind(&body.real_api_key)
    .bind(body.priority.unwrap_or(0))
    .fetch_one(&state.pool)
    .await
    .map_err(|e| err(StatusCode::BAD_REQUEST, e.to_string()))?;

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
            updated_at = now() \
         WHERE id = $1 \
         RETURNING id, org_id, name, protocol, model_patterns, base_url, real_api_key, \
                   priority, is_active, created_at, updated_at",
    )
    .bind(id)
    .bind(&body.name)
    .bind(&body.protocol)
    .bind(&body.model_patterns)
    .bind(&body.base_url)
    .bind(&body.real_api_key)
    .bind(body.priority)
    .bind(body.is_active)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
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
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

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

    let total = count_query
        .fetch_one(&state.pool)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // 查询数据
    let data_sql = format!(
        "SELECT id, org_id, api_key_id, provider, model, path, method, is_stream, \
                response_status, status, prompt_tokens, completion_tokens, cached_tokens, \
                (COALESCE(prompt_tokens, 0) + COALESCE(completion_tokens, 0) + COALESCE(cached_tokens, 0)) AS total_tokens, \
                duration_ms, error_message, credit_cost, created_at \
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
                duration_ms, error_message, credit_cost, created_at, updated_at \
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
         RETURNING id, name, slug, credit, is_active, created_at, updated_at"
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

/// GET /api/orgs/:id/credit_logs
async fn list_credit_logs(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<CreditLogSummary>>, (StatusCode, Json<ErrorResponse>)> {
    let rows = sqlx::query_as::<_, CreditLogSummary>(
        "SELECT id, org_id, amount, balance_after, transaction_type, reference_id, created_at \
         FROM credit_logs WHERE org_id = $1 ORDER BY created_at DESC LIMIT 100"
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(rows))
}

#[derive(Deserialize)]
struct StatsQuery {
    days: Option<i32>,
    org_id: Option<Uuid>,
}

#[derive(Serialize)]
struct StatsResponse {
    overview: StatsOverview,
    by_provider: Vec<ProviderStats>,
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
}

#[derive(Serialize, sqlx::FromRow)]
struct ProviderStats {
    provider: String,
    request_count: i64,
    prompt_tokens: i64,
    completion_tokens: i64,
    cached_tokens: i64,
    total_tokens: i64,
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

/// GET /api/stats — 统计数据
async fn get_stats(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Query(q): Query<StatsQuery>,
) -> Result<Json<StatsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let days = q.days.unwrap_or(7).max(1);
    let since = Utc::now() - chrono::TimeDelta::days(days as i64);

    let org_filter = if q.org_id.is_some() {
        "AND org_id = $2"
    } else {
        ""
    };

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
            COALESCE(SUM(credit_cost)::FLOAT8, 0) AS total_credit_cost \
         FROM request_logs WHERE created_at >= $1 {org_filter}"
    );
    let mut overview_query = sqlx::query_as::<_, StatsOverview>(&overview_sql).bind(since);
    if let Some(ref oid) = q.org_id {
        overview_query = overview_query.bind(oid);
    }
    let overview = overview_query
        .fetch_one(&state.pool)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // 按 provider
    let provider_sql = format!(
        "SELECT provider, \
            COUNT(*)::BIGINT AS request_count, \
            COALESCE(SUM(prompt_tokens), 0)::BIGINT AS prompt_tokens, \
            COALESCE(SUM(completion_tokens), 0)::BIGINT AS completion_tokens, \
            COALESCE(SUM(cached_tokens), 0)::BIGINT AS cached_tokens, \
            (COALESCE(SUM(prompt_tokens), 0) + COALESCE(SUM(completion_tokens), 0) + COALESCE(SUM(cached_tokens), 0))::BIGINT AS total_tokens, \
            COUNT(*) FILTER (WHERE status = 'error')::BIGINT AS error_count \
         FROM request_logs WHERE created_at >= $1 {org_filter} \
         GROUP BY provider ORDER BY request_count DESC"
    );
    let mut pq = sqlx::query_as::<_, ProviderStats>(&provider_sql).bind(since);
    if let Some(ref oid) = q.org_id {
        pq = pq.bind(oid);
    }
    let by_provider = pq
        .fetch_all(&state.pool)
        .await
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
         FROM request_logs WHERE created_at >= $1 {org_filter} \
         GROUP BY model ORDER BY request_count DESC"
    );
    let mut mq = sqlx::query_as::<_, ModelStats>(&model_sql).bind(since);
    if let Some(ref oid) = q.org_id {
        mq = mq.bind(oid);
    }
    let by_model = mq
        .fetch_all(&state.pool)
        .await
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
         FROM request_logs WHERE created_at >= $1 {org_filter} \
         GROUP BY date ORDER BY date"
    );
    let mut dq = sqlx::query_as::<_, DailyStats>(&daily_sql).bind(since);
    if let Some(ref oid) = q.org_id {
        dq = dq.bind(oid);
    }
    let daily_stats = dq
        .fetch_all(&state.pool)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(StatsResponse {
        overview,
        by_provider,
        by_model,
        daily_stats,
    }))
}
