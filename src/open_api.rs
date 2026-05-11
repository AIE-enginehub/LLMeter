//! 开放 API — 供各组织通过 API Key 查询 Credit 消耗情况

use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;

use crate::state::AppState;

/// 注册开放 API 路由
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/open-api/credit/balance", get(credit_balance))
        .route("/open-api/credit/usage", get(credit_usage))
        .route("/open-api/credit/logs", get(credit_logs))
}

// ============================================================
// 公共部分
// ============================================================

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

fn err(status: StatusCode, msg: impl Into<String>) -> (StatusCode, Json<ErrorResponse>) {
    (status, Json(ErrorResponse { error: msg.into() }))
}

/// 从 Authorization Header 提取 API Key 并验证，返回 (org_id, org_name, org_slug, credit)
async fn authenticate(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<(uuid::Uuid, String, String, rust_decimal::Decimal), (StatusCode, Json<ErrorResponse>)> {
    let raw_key = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "Missing Authorization header"))?;

    let mut hasher = Sha256::new();
    hasher.update(raw_key.as_bytes());
    let key_hash = format!("{:x}", hasher.finalize());

    let (_api_key, org) = crate::db::find_api_key_by_hash(&state.pool, &key_hash)
        .await
        .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "Invalid or inactive API Key"))?;

    Ok((org.id, org.name, org.slug, org.credit))
}

// ============================================================
// GET /open-api/credit/balance
// ============================================================

#[derive(Serialize)]
struct BalanceResponse {
    org_name: String,
    slug: String,
    credit_balance: String,
}

async fn credit_balance(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> Result<Json<BalanceResponse>, (StatusCode, Json<ErrorResponse>)> {
    let (_org_id, name, slug, credit) = authenticate(&state, &headers).await?;

    Ok(Json(BalanceResponse {
        org_name: name,
        slug,
        credit_balance: format!("{:.4}", credit),
    }))
}

// ============================================================
// GET /open-api/credit/usage
// ============================================================

#[derive(Deserialize)]
struct TimeRangeQuery {
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
struct UsageResponse {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    total_credit_cost: String,
    total_requests: i64,
    daily: Vec<DailyCredit>,
}

#[derive(Serialize, sqlx::FromRow)]
struct DailyCredit {
    date: chrono::NaiveDate,
    credit_cost: f64,
    request_count: i64,
}

async fn credit_usage(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Query(q): Query<TimeRangeQuery>,
) -> Result<Json<UsageResponse>, (StatusCode, Json<ErrorResponse>)> {
    let (org_id, ..) = authenticate(&state, &headers).await?;

    let end = q.end.unwrap_or_else(Utc::now);
    let start = q.start.unwrap_or_else(|| end - chrono::TimeDelta::days(30));

    #[derive(sqlx::FromRow)]
    struct Summary { total_cost: f64, total_req: i64 }

    let summary = sqlx::query_as::<_, Summary>(
        "SELECT COALESCE(SUM(credit_cost)::FLOAT8, 0) AS total_cost, \
                COUNT(*)::BIGINT AS total_req \
         FROM request_logs WHERE org_id = $1 AND created_at >= $2 AND created_at <= $3"
    )
    .bind(org_id).bind(start).bind(end)
    .fetch_one(&state.pool).await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let daily = sqlx::query_as::<_, DailyCredit>(
        "SELECT created_at::DATE AS date, \
                COALESCE(SUM(credit_cost)::FLOAT8, 0) AS credit_cost, \
                COUNT(*)::BIGINT AS request_count \
         FROM request_logs WHERE org_id = $1 AND created_at >= $2 AND created_at <= $3 \
         GROUP BY date ORDER BY date"
    )
    .bind(org_id).bind(start).bind(end)
    .fetch_all(&state.pool).await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(UsageResponse {
        start, end,
        total_credit_cost: format!("{:.4}", summary.total_cost),
        total_requests: summary.total_req,
        daily,
    }))
}

// ============================================================
// GET /open-api/credit/logs
// ============================================================

#[derive(Deserialize)]
struct CreditLogQuery {
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
    page: Option<i64>,
    page_size: Option<i64>,
}

#[derive(Serialize)]
struct CreditLogResponse {
    data: Vec<CreditLogItem>,
    total: i64,
    page: i64,
    page_size: i64,
}

#[derive(Serialize, sqlx::FromRow)]
struct CreditLogItem {
    id: uuid::Uuid,
    amount: rust_decimal::Decimal,
    balance_after: rust_decimal::Decimal,
    transaction_type: String,
    note: Option<String>,
    created_at: DateTime<Utc>,
}

async fn credit_logs(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Query(q): Query<CreditLogQuery>,
) -> Result<Json<CreditLogResponse>, (StatusCode, Json<ErrorResponse>)> {
    let (org_id, ..) = authenticate(&state, &headers).await?;

    let end = q.end.unwrap_or_else(Utc::now);
    let start = q.start.unwrap_or_else(|| end - chrono::TimeDelta::days(30));
    let page = q.page.unwrap_or(1).max(1);
    let page_size = q.page_size.unwrap_or(50).clamp(1, 100);
    let offset = (page - 1) * page_size;

    let total = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::BIGINT FROM credit_logs \
         WHERE org_id = $1 AND created_at >= $2 AND created_at <= $3"
    )
    .bind(org_id).bind(start).bind(end)
    .fetch_one(&state.pool).await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let data = sqlx::query_as::<_, CreditLogItem>(
        "SELECT id, amount, balance_after, transaction_type, \
                reference_id AS note, created_at \
         FROM credit_logs \
         WHERE org_id = $1 AND created_at >= $2 AND created_at <= $3 \
         ORDER BY created_at DESC LIMIT $4 OFFSET $5"
    )
    .bind(org_id).bind(start).bind(end)
    .bind(page_size).bind(offset)
    .fetch_all(&state.pool).await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(CreditLogResponse { data, total, page, page_size }))
}
