mod auth_handlers;
mod billing;
mod credit_handlers;
mod log_handlers;
mod model_handlers;
mod org_handlers;
mod settings_handlers;
mod stats_handlers;

use axum::{
    http::StatusCode,
    routing::{delete, get, post, put},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

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
// 数据库行结构（跨子模块共享）
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
    cache_write_tokens: Option<i32>,
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
    org_name: String,
    project_id: Option<Uuid>,
    project_name: Option<String>,
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
    cache_write_tokens: Option<i32>,
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

// ============================================================
// 路由注册
// ============================================================

/// 构建管理后台所有 API 路由
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        // 认证（login 不需要 JWT）
        .route("/api/auth/login", post(auth_handlers::login))
        .route("/api/auth/logout", post(auth_handlers::logout))
        .route("/api/auth/me", get(auth_handlers::me))
        .route("/api/auth/password", put(auth_handlers::change_password))
        // 组织管理
        .route("/api/orgs", get(org_handlers::list_orgs).post(org_handlers::create_org))
        .route(
            "/api/orgs/{id}",
            put(org_handlers::update_org).delete(org_handlers::delete_org),
        )
        // 项目管理
        .route(
            "/api/orgs/{org_id}/projects",
            get(org_handlers::list_projects).post(org_handlers::create_project),
        )
        .route(
            "/api/projects/{id}",
            put(org_handlers::update_project).delete(org_handlers::delete_project),
        )
        // API Key 管理（归属项目）
        .route(
            "/api/projects/{project_id}/keys",
            get(org_handlers::list_keys).post(org_handlers::create_key),
        )
        .route("/api/keys/{id}", delete(org_handlers::delete_key))
        // 模型配置管理（保持在组织级别）
        .route(
            "/api/orgs/{org_id}/models",
            get(model_handlers::list_models).post(model_handlers::create_model),
        )
        .route(
            "/api/models/{id}",
            put(model_handlers::update_model).delete(model_handlers::delete_model),
        )
        // 日志与统计
        .route("/api/logs", get(log_handlers::list_logs))
        .route("/api/logs/{id}", get(log_handlers::get_log))
        .route("/api/stats", get(stats_handlers::get_stats))
        // 全局设置
        .route(
            "/api/settings/credit_rates",
            get(settings_handlers::get_credit_rates).put(settings_handlers::update_credit_rates),
        )
        .route("/api/settings/mail", get(settings_handlers::get_mail_settings).put(settings_handlers::update_mail_settings))
        .route("/api/settings/compression", get(settings_handlers::get_compression).put(settings_handlers::update_compression))
        .route("/api/settings/model_credit_rates", get(settings_handlers::list_model_credit_rates).post(settings_handlers::create_model_credit_rate))
        .route("/api/settings/model_credit_rates/{id}", put(settings_handlers::update_model_credit_rate).delete(settings_handlers::delete_model_credit_rate))
        // 用量导出
        .route("/api/usage/export_report", post(billing::export_usage_report))
        // 积分系统
        .route("/api/orgs/{id}/credit", post(credit_handlers::recharge_credit))
        .route("/api/orgs/{id}/credit_logs", get(credit_handlers::list_credit_logs))
}
