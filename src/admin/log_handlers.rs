use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::AuthAdmin;
use crate::state::AppState;
use super::{err, ErrorResponse, LogRow, LogSummaryRow};

#[derive(Deserialize)]
pub(super) struct LogQuery {
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
pub(super) struct PaginatedLogs {
    data: Vec<LogSummaryRow>,
    total: i64,
    page: i64,
    page_size: i64,
}

/// GET /api/logs — 分页查询日志列表
pub(super) async fn list_logs(
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
pub(super) async fn get_log(
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
