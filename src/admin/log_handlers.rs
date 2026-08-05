use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, Utc};
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
    /// 浏览器 Date.getTimezoneOffset()：UTC - 本地时间，单位分钟。
    timezone_offset: Option<i32>,
    sort_order: Option<String>,
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
    let sort_order = if q.sort_order.as_deref() == Some("asc") {
        "ASC"
    } else {
        "DESC"
    };

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
    // 日期选择器使用浏览器本地日期，而数据库存储 UTC。必须先按浏览器时区
    // 把本地自然日换算为 UTC，否则中国时区选择 23 日会查到 24 日 00:00–08:00。
    let timezone_offset = q.timezone_offset.unwrap_or(0).clamp(-14 * 60, 14 * 60);
    let start_ts = q
        .start_time
        .as_deref()
        .and_then(|s| parse_start_time(s, timezone_offset));
    let end_bound = q
        .end_time
        .as_deref()
        .and_then(|s| parse_end_time(s, timezone_offset));
    if start_ts.is_some() {
        conditions.push(format!("created_at >= ${param_idx}"));
        param_idx += 1;
    }
    if let Some((_, exclusive)) = end_bound {
        conditions.push(format!(
            "created_at {} ${param_idx}",
            if exclusive { "<" } else { "<=" }
        ));
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
    if let Some((ref v, _)) = end_bound { count_query = count_query.bind(v); }

    let total = count_query
        .fetch_one(&state.pool)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // 查询数据
    let data_sql = format!(
        "SELECT r.id, r.org_id, COALESCE(o.name, 'unknown') AS org_name, \
                r.project_id, p.name AS project_name, r.api_key_id, r.provider, r.model, r.path, r.method, r.is_stream, \
                r.response_status, r.status, r.prompt_tokens, r.completion_tokens, r.cached_tokens, r.cache_write_tokens, \
                COALESCE(r.total_tokens, COALESCE(r.prompt_tokens, 0) + COALESCE(r.completion_tokens, 0)) AS total_tokens, \
                r.duration_ms, r.error_message, r.credit_cost, r.money_cost, r.billing_mode, r.price_version_id, \
                r.official_cost, r.official_currency, r.exchange_rate, r.official_cost_cny, r.price_multiplier, \
                r.is_long_context, r.compressed, r.est_tokens_saved, r.created_at \
         FROM (SELECT * FROM request_logs {where_clause}) r \
         LEFT JOIN organizations o ON o.id = r.org_id \
         LEFT JOIN projects p ON p.id = r.project_id \
         ORDER BY r.created_at {sort_order}, r.id {sort_order} \
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
    if let Some((ref v, _)) = end_bound { data_query = data_query.bind(v); }
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

fn local_naive_to_utc(value: NaiveDateTime, timezone_offset: i32) -> Option<DateTime<Utc>> {
    value
        .checked_add_signed(Duration::minutes(timezone_offset as i64))
        .map(|value| value.and_utc())
}

fn parse_start_time(value: &str, timezone_offset: i32) -> Option<DateTime<Utc>> {
    let local = NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S")
        .ok()
        .or_else(|| {
            NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .ok()?
                .and_hms_opt(0, 0, 0)
        })?;
    local_naive_to_utc(local, timezone_offset)
}

/// 日期值使用“次日 00:00 前”的半开区间，避免漏掉 23:59:59.xxxxxx。
/// 带具体时间的旧 API 参数仍保留包含结束时刻的行为。
fn parse_end_time(value: &str, timezone_offset: i32) -> Option<(DateTime<Utc>, bool)> {
    if let Ok(local) = NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S") {
        return local_naive_to_utc(local, timezone_offset).map(|value| (value, false));
    }

    let next_midnight = NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .ok()?
        .succ_opt()?
        .and_hms_opt(0, 0, 0)?;
    local_naive_to_utc(next_midnight, timezone_offset).map(|value| (value, true))
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
                prompt_tokens, completion_tokens, cached_tokens, cache_write_tokens, total_tokens, \
                duration_ms, error_message, credit_cost, money_cost, billing_mode, price_version_id, \
                official_cost, official_currency, exchange_rate, official_cost_cny, price_multiplier, is_long_context, \
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

#[cfg(test)]
mod tests {
    use super::{parse_end_time, parse_start_time};

    #[test]
    fn china_local_date_is_converted_to_correct_utc_range() {
        let start = parse_start_time("2026-07-23", -480).unwrap();
        let (end, exclusive) = parse_end_time("2026-07-23", -480).unwrap();

        assert_eq!(start.to_rfc3339(), "2026-07-22T16:00:00+00:00");
        assert_eq!(end.to_rfc3339(), "2026-07-23T16:00:00+00:00");
        assert!(exclusive);
    }

    #[test]
    fn explicit_end_time_remains_inclusive() {
        let (end, exclusive) =
            parse_end_time("2026-07-23 20:30:00", -480).unwrap();

        assert_eq!(end.to_rfc3339(), "2026-07-23T12:30:00+00:00");
        assert!(!exclusive);
    }
}
