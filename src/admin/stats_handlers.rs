use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;
use rust_decimal::Decimal;

use crate::auth::AuthAdmin;
use crate::state::AppState;
use super::{err, ErrorResponse};

#[derive(Deserialize)]
pub(super) struct StatsQuery {
    days: Option<i32>,
    org_id: Option<Uuid>,
    project_id: Option<Uuid>,
    api_key_id: Option<Uuid>,
    model: Option<String>,
    start_time: Option<String>,
    end_time: Option<String>,
}

#[derive(Serialize)]
pub(super) struct StatsResponse {
    overview: StatsOverview,
    by_org: Vec<OrgStats>,
    by_project: Vec<ProjectStats>,
    by_model: Vec<ModelStats>,
    daily_stats: Vec<DailyStats>,
}

#[derive(Serialize, sqlx::FromRow)]
pub(super) struct StatsOverview {
    total_requests: i64,
    success_requests: i64,
    error_requests: i64,
    prompt_tokens: i64,
    completion_tokens: i64,
    cached_tokens: i64,
    cache_write_tokens: i64,
    total_tokens: i64,
    avg_duration_ms: f64,
    total_credit_cost: Decimal,
    compressed_requests: i64,
    est_tokens_saved: i64,
}

#[derive(Serialize, sqlx::FromRow)]
pub(super) struct OrgStats {
    org_id: Uuid,
    org_name: String,
    request_count: i64,
    prompt_tokens: i64,
    completion_tokens: i64,
    cached_tokens: i64,
    cache_write_tokens: i64,
    total_tokens: i64,
    total_credit_cost: Decimal,
    error_count: i64,
}

#[derive(Serialize, sqlx::FromRow)]
pub(super) struct ProjectStats {
    project_id: Option<Uuid>,
    project_name: String,
    org_name: String,
    request_count: i64,
    prompt_tokens: i64,
    completion_tokens: i64,
    cached_tokens: i64,
    cache_write_tokens: i64,
    total_tokens: i64,
    total_credit_cost: Decimal,
    error_count: i64,
}

#[derive(Serialize, sqlx::FromRow)]
pub(super) struct ModelStats {
    model: String,
    request_count: i64,
    prompt_tokens: i64,
    completion_tokens: i64,
    cached_tokens: i64,
    cache_write_tokens: i64,
    total_tokens: i64,
    avg_duration_ms: f64,
    total_credit_cost: Decimal,
}

#[derive(Serialize, sqlx::FromRow)]
pub(super) struct DailyStats {
    date: chrono::NaiveDate,
    request_count: i64,
    prompt_tokens: i64,
    completion_tokens: i64,
    cached_tokens: i64,
    cache_write_tokens: i64,
    total_tokens: i64,
    error_count: i64,
    credit_cost: Decimal,
}

/// GET /api/stats — 统计数据（支持组织/Key/模型筛选）
pub(super) async fn get_stats(
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
            COALESCE(SUM(cache_write_tokens), 0)::BIGINT AS cache_write_tokens, \
            COALESCE(SUM(COALESCE(total_tokens, COALESCE(prompt_tokens, 0) + COALESCE(completion_tokens, 0))), 0)::BIGINT AS total_tokens, \
            COALESCE(AVG(duration_ms)::FLOAT8, 0) AS avg_duration_ms, \
            COALESCE(SUM(credit_cost), 0) AS total_credit_cost, \
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
            COALESCE(SUM(r.cache_write_tokens), 0)::BIGINT AS cache_write_tokens, \
            COALESCE(SUM(COALESCE(r.total_tokens, COALESCE(r.prompt_tokens, 0) + COALESCE(r.completion_tokens, 0))), 0)::BIGINT AS total_tokens, \
            COALESCE(SUM(r.credit_cost), 0) AS total_credit_cost, \
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
            COALESCE(SUM(r.cache_write_tokens), 0)::BIGINT AS cache_write_tokens, \
            COALESCE(SUM(COALESCE(r.total_tokens, COALESCE(r.prompt_tokens, 0) + COALESCE(r.completion_tokens, 0))), 0)::BIGINT AS total_tokens, \
            COALESCE(SUM(r.credit_cost), 0) AS total_credit_cost, \
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
            COALESCE(SUM(cache_write_tokens), 0)::BIGINT AS cache_write_tokens, \
            COALESCE(SUM(COALESCE(total_tokens, COALESCE(prompt_tokens, 0) + COALESCE(completion_tokens, 0))), 0)::BIGINT AS total_tokens, \
            COALESCE(AVG(duration_ms)::FLOAT8, 0) AS avg_duration_ms, \
            COALESCE(SUM(credit_cost), 0) AS total_credit_cost \
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
            COALESCE(SUM(cache_write_tokens), 0)::BIGINT AS cache_write_tokens, \
            COALESCE(SUM(COALESCE(total_tokens, COALESCE(prompt_tokens, 0) + COALESCE(completion_tokens, 0))), 0)::BIGINT AS total_tokens, \
            COUNT(*) FILTER (WHERE status = 'error')::BIGINT AS error_count, \
            COALESCE(SUM(credit_cost), 0) AS credit_cost \
         FROM request_logs WHERE created_at >= $1 {extra_where} \
         GROUP BY date ORDER BY date"
    );
    let daily_stats = bind_filters!(sqlx::query_as::<_, DailyStats>(&daily_sql).bind(since))
        .fetch_all(&state.pool).await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(StatsResponse { overview, by_org, by_project, by_model, daily_stats }))
}
