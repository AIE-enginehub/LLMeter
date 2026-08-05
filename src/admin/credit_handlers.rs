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
use super::{err, CreditLogSummary, ErrorResponse, OrgRow};

#[derive(Deserialize)]
pub(super) struct RechargeRequest {
    /// 兼容协议计费和旧客户端：直接填写积分。
    amount: Option<rust_decimal::Decimal>,
    /// 标准价格计费推荐按人民币充值，系统固定换算为 100 积分/元。
    amount_yuan: Option<rust_decimal::Decimal>,
    note: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct CreditLogQuery {
    page: Option<i64>,
    page_size: Option<i64>,
    r#type: Option<String>,
}

#[derive(Serialize)]
pub(super) struct CreditLogPage {
    data: Vec<CreditLogSummary>,
    total: i64,
    page: i64,
    page_size: i64,
}

/// POST /api/orgs/:id/credit — 组织积分充值
pub(super) async fn recharge_credit(
    State(state): State<Arc<AppState>>,
    admin: AuthAdmin,
    Path(id): Path<Uuid>,
    Json(body): Json<RechargeRequest>,
) -> Result<Json<OrgRow>, (StatusCode, Json<ErrorResponse>)> {
    let billing_mode: String = sqlx::query_scalar("SELECT billing_mode FROM organizations WHERE id = $1")
        .bind(id).fetch_optional(&state.pool).await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "Organization not found"))?;
    let amount = if billing_mode == "standard_pricing" {
        body.amount_yuan
            .map(|yuan| (yuan * rust_decimal::Decimal::from(100u32)).round_dp(6))
            .or(body.amount)
    } else {
        body.amount
    }
    .ok_or_else(|| err(StatusCode::BAD_REQUEST, "充值金额不能为空"))?;
    if amount.is_zero() {
        return Err(err(StatusCode::BAD_REQUEST, "Amount cannot be zero"));
    }

    let mut tx = state.pool.begin().await.map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let row = sqlx::query_as::<_, OrgRow>(
        "UPDATE organizations SET credit = credit + $1, updated_at = now() WHERE id = $2 \
         RETURNING id, name, slug, credit, overdraft_limit, credit_price, billing_mode, is_active, \
                   COALESCE((SELECT ABS(SUM(amount)) FROM credit_logs WHERE org_id = id AND transaction_type = 'consume'), 0) AS total_consumed, \
                   created_at, updated_at"
    )
    .bind(amount)
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
    .bind(amount)
    .bind(row.credit)
    .bind(ref_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    tx.commit().await.map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(row))
}

/// GET /api/orgs/:id/credit_logs?page=1&page_size=20&type=consume
pub(super) async fn list_credit_logs(
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
