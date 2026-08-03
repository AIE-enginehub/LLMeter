use axum::{
    Json,
    extract::State,
    http::{StatusCode, header},
    response::IntoResponse,
};
use chrono::{DateTime, FixedOffset, Utc};
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Tokio1Executor,
    message::{Attachment, Mailbox, MultiPart, SinglePart, header::ContentType},
    transport::smtp::authentication::Credentials,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use super::{ErrorResponse, err};
use crate::auth::AuthAdmin;
use crate::state::AppState;

#[derive(Deserialize)]
pub(super) struct ExportUsageReportRequest {
    org_id: Uuid,
    #[serde(default)]
    project_ids: Vec<Uuid>,
    month: Option<String>,
    start_time: Option<String>,
    end_time: Option<String>,
    /// "download" | "email"，默认 "download"
    delivery: Option<String>,
    /// 仅 email 模式必填
    recipient_email: Option<String>,
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

/// 按组织+项目+API Key 维度的汇总行
#[derive(sqlx::FromRow)]
struct UsageReportDetailRow {
    org_name: String,
    project_id: Option<Uuid>,
    project_name: Option<String>,
    api_key_id: Option<Uuid>,
    api_key_name: Option<String>,
    request_count: i64,
    total_credit_cost: f64,
    total_money_cost: f64,
}

struct KeyUsage {
    name: String,
    request_count: i64,
    credit_cost: f64,
    money_cost: f64,
}

struct ProjectUsage {
    id: Option<Uuid>,
    name: String,
    keys: Vec<KeyUsage>,
    request_count: i64,
    credit_cost: f64,
    money_cost: f64,
}

impl ProjectUsage {
    fn has_usage(&self) -> bool {
        self.request_count != 0
            || self.credit_cost.abs() > f64::EPSILON
            || self.money_cost.abs() > f64::EPSILON
    }
}

/// POST /api/usage/export_report — 支持 download（直接下载 PDF）和 email（发送邮件）两种模式
pub(super) async fn export_usage_report(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Json(body): Json<ExportUsageReportRequest>,
) -> Result<axum::response::Response, (StatusCode, Json<ErrorResponse>)> {
    let is_email = body.delivery.as_deref() == Some("email");
    let recipient = body
        .recipient_email
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_string();
    if is_email && recipient.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "收件邮箱不能为空"));
    }

    let (start_time, end_time) = parse_report_time_range(
        body.month.as_deref(),
        body.start_time.as_deref(),
        body.end_time.as_deref(),
    )?;

    if is_email {
        let mail_settings = crate::db::get_mail_settings(&state.pool).await;
        validate_mail_settings(&mail_settings)?;
    }

    let org_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM organizations WHERE id = $1)",
    )
    .bind(body.org_id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !org_exists {
        return Err(err(StatusCode::BAD_REQUEST, "所选企业不存在，请刷新后重试"));
    }

    let mut project_ids = body.project_ids.clone();
    project_ids.sort_unstable();
    project_ids.dedup();
    if !project_ids.is_empty() {
        let owned_projects: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::BIGINT FROM projects WHERE org_id = $1 AND id = ANY($2)",
        )
        .bind(body.org_id)
        .bind(&project_ids)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        if owned_projects != project_ids.len() as i64 {
            return Err(err(
                StatusCode::BAD_REQUEST,
                "所选项目不属于当前企业，请刷新后重试",
            ));
        }
    }

    // 空数组表示导出该企业的全部项目。
    let project_filter = if project_ids.is_empty() {
        None
    } else {
        Some(project_ids)
    };

    // 查询所选企业下的全部或指定项目及 API Key。
    let details = sqlx::query_as::<_, UsageReportDetailRow>(
        "SELECT o.name AS org_name, \
                p.id AS project_id, \
                p.name AS project_name, \
                k.id AS api_key_id, \
                k.name AS api_key_name, \
                COUNT(r.id)::BIGINT AS request_count, \
                COALESCE(SUM(r.credit_cost)::FLOAT8, 0) AS total_credit_cost, \
                COALESCE(SUM(r.money_cost)::FLOAT8, 0) AS total_money_cost \
         FROM organizations o \
         LEFT JOIN projects p ON p.org_id = o.id \
         LEFT JOIN api_keys k ON k.project_id = p.id \
         LEFT JOIN request_logs r ON r.api_key_id = k.id \
              AND r.created_at >= $2 AND r.created_at < $3 \
         WHERE o.id = $1 \
           AND ($4::UUID[] IS NULL OR p.id = ANY($4)) \
         GROUP BY o.name, p.id, p.name, k.id, k.name \
         ORDER BY o.name, p.name, k.name",
    )
    .bind(body.org_id)
    .bind(start_time)
    .bind(end_time)
    .bind(project_filter)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // 按组织分组
    let mut org_groups: Vec<(String, Vec<&UsageReportDetailRow>)> = Vec::new();
    for row in &details {
        if let Some(group) = org_groups
            .iter_mut()
            .find(|(name, _)| name == &row.org_name)
        {
            group.1.push(row);
        } else {
            org_groups.push((row.org_name.clone(), vec![row]));
        }
    }

    let period_start = start_time.format("%Y-%m-%d").to_string();
    let period_end = (end_time - chrono::TimeDelta::seconds(1))
        .format("%Y-%m-%d")
        .to_string();
    let generated_now = to_beijing_time(Utc::now());
    let generated_at = generated_now.format("%Y-%m-%d %H:%M").to_string();
    let period_tag = format!(
        "{}_{}",
        start_time.format("%Y%m%d"),
        (end_time - chrono::TimeDelta::seconds(1)).format("%Y%m%d")
    );

    // 为每个组织生成独立 PDF
    let mut pdf_attachments: Vec<PdfAttachment> = Vec::new();
    let mut total_requests: i64 = 0;
    let mut total_credit_cost: f64 = 0.0;
    let mut total_money_cost: f64 = 0.0;

    for (org_name, rows) in &org_groups {
        let bill_no = generate_bill_no(&generated_now);
        let mut projects: Vec<ProjectUsage> = Vec::new();
        for row in rows {
            let project_id = row.project_id;
            let project_idx = projects.iter().position(|p| p.id == project_id);
            let idx = match project_idx {
                Some(idx) => idx,
                None => {
                    projects.push(ProjectUsage {
                        id: project_id,
                        name: row
                            .project_name
                            .clone()
                            .unwrap_or_else(|| "(未分配项目)".to_string()),
                        keys: Vec::new(),
                        request_count: 0,
                        credit_cost: 0.0,
                        money_cost: 0.0,
                    });
                    projects.len() - 1
                }
            };

            let project = &mut projects[idx];
            project.request_count += row.request_count;
            project.credit_cost += row.total_credit_cost;
            project.money_cost += row.total_money_cost;
            if row.api_key_id.is_some() {
                project.keys.push(KeyUsage {
                    name: row
                        .api_key_name
                        .clone()
                        .unwrap_or_else(|| "未命名 Key".to_string()),
                    request_count: row.request_count,
                    credit_cost: row.total_credit_cost,
                    money_cost: row.total_money_cost,
                });
            }
        }
        let org_requests: i64 = rows.iter().map(|r| r.request_count).sum();
        let org_credit: f64 = rows.iter().map(|r| r.total_credit_cost).sum();
        let org_money: f64 = rows.iter().map(|r| r.total_money_cost).sum();

        total_requests += org_requests;
        total_credit_cost += org_credit;
        total_money_cost += org_money;

        let pdf_data = generate_org_invoice_pdf(
            &bill_no,
            &period_start,
            &period_end,
            &generated_at,
            org_name,
            &projects,
            org_requests,
            org_credit,
            org_money,
        )
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;

        let filename = format!("流量账单_{org_name}_{period_tag}.pdf");
        pdf_attachments.push(PdfAttachment {
            filename,
            data: pdf_data,
        });
    }

    if is_email {
        // 邮件模式：发送当前企业的 PDF 附件
        let mail_settings = crate::db::get_mail_settings(&state.pool).await;
        send_usage_report_mail(
            &mail_settings,
            &recipient,
            start_time,
            end_time,
            &pdf_attachments,
        )
        .await?;
        return Ok(Json(ExportUsageReportResponse {
            ok: true,
            start_time,
            end_time,
            org_count: 1,
            total_requests,
            total_credit_cost,
            total_money_cost,
        })
        .into_response());
    }

    // 下载模式：返回当前企业的 PDF
    let att = pdf_attachments
        .into_iter()
        .next()
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "未找到匹配的组织"))?;
    let encoded_name = percent_encode_filename(&att.filename);
    Ok((
        [
            (header::CONTENT_TYPE, "application/pdf".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{encoded_name}\"; filename*=UTF-8''{encoded_name}"),
            ),
        ],
        att.data,
    )
        .into_response())
}

fn parse_report_time_range(
    month: Option<&str>,
    start_time: Option<&str>,
    end_time: Option<&str>,
) -> Result<(DateTime<Utc>, DateTime<Utc>), (StatusCode, Json<ErrorResponse>)> {
    let has_custom = start_time.is_some() || end_time.is_some();
    if month.is_some() && has_custom {
        return Err(err(StatusCode::BAD_REQUEST, "月份和自定义时间段只能二选一"));
    }

    if let Some(month_str) = month {
        let month_start_date =
            chrono::NaiveDate::parse_from_str(&format!("{month_str}-01"), "%Y-%m-%d")
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

/// RFC 5987 百分号编码：将非 ASCII 及特殊字符转为 %XX，确保 Content-Disposition 中文件名正确传输
fn percent_encode_filename(name: &str) -> String {
    let mut out = String::with_capacity(name.len() * 3);
    for b in name.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' => out.push(b as char),
            _ => {
                out.push('%');
                out.push_str(&format!("{b:02X}"));
            }
        }
    }
    out
}

fn validate_mail_settings(
    settings: &crate::db::MailSettings,
) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
    if settings.outbound.host.trim().is_empty() {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "请先在系统设置中配置发件服务器地址",
        ));
    }
    if settings.outbound.sender_email.trim().is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "请先在系统设置中配置发件邮箱"));
    }
    Ok(())
}

/// 内嵌的霞鹜新晰黑字体（静态 TTF 黑体，编译时打包，无运行时依赖）
static EMBEDDED_FONT: &[u8] = include_bytes!("../../fonts/LXGWNeoXiHei.ttf");

fn to_beijing_time(time: DateTime<Utc>) -> DateTime<FixedOffset> {
    let offset = FixedOffset::east_opt(8 * 60 * 60).expect("UTC+8 is a valid fixed offset");
    time.with_timezone(&offset)
}

fn generate_bill_no(generated_at: &DateTime<FixedOffset>) -> String {
    let ts = generated_at.format("%Y%m%d%H%M%S").to_string();
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
    let reader =
        font_subset::FontReader::new(EMBEDDED_FONT).map_err(|e| format!("字体读取失败: {e}"))?;
    let font = reader.read().map_err(|e| format!("字体解析失败: {e}"))?;
    let chars: BTreeSet<char> = text.chars().collect();
    let subset = font
        .subset(&chars)
        .map_err(|e| format!("字体子集化失败: {e}"))?;
    Ok(subset.to_opentype())
}

fn format_integer(value: i64) -> String {
    let negative = value < 0;
    let digits = value.unsigned_abs().to_string();
    let mut grouped = String::new();
    for (idx, ch) in digits.chars().rev().enumerate() {
        if idx > 0 && idx % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    let result: String = grouped.chars().rev().collect();
    if negative {
        format!("-{result}")
    } else {
        result
    }
}

fn format_decimal(value: f64) -> String {
    let negative = value < 0.0;
    let raw = format!("{:.2}", value.abs());
    let (integer, fraction) = raw.split_once('.').unwrap_or((&raw, "00"));
    let integer = format_integer(integer.parse::<i64>().unwrap_or(0));
    if negative {
        format!("-{integer}.{fraction}")
    } else {
        format!("{integer}.{fraction}")
    }
}

fn truncate_label(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut result: String = value.chars().take(max_chars.saturating_sub(3)).collect();
    result.push_str("...");
    result
}

/// 估算 PDF 文本宽度，用于让表格中的数字共享同一条右边界。
fn pdf_text_width_mm(value: &str, font_size: f32) -> f32 {
    let em_width: f32 = value
        .chars()
        .map(|ch| match ch {
            '0'..='9' => 0.56,
            ',' | '.' | ':' => 0.28,
            '-' | ' ' => 0.32,
            '¥' => 0.82,
            ch if ch.is_ascii() => 0.55,
            _ => 1.0,
        })
        .sum();
    em_width * font_size * 0.352_778
}

fn right_aligned_x(value: &str, right_edge: f32, font_size: f32) -> f32 {
    right_edge - pdf_text_width_mm(value, font_size)
}

/// 生成单个组织的账单 PDF（项目小计 + API Key 明细，支持自动分页）
fn generate_org_invoice_pdf(
    bill_no: &str,
    period_start: &str,
    period_end: &str,
    generated_at: &str,
    org_name: &str,
    projects: &[ProjectUsage],
    total_requests: i64,
    total_credit: f64,
    total_money: f64,
) -> Result<Vec<u8>, String> {
    use printpdf::*;

    // ── 预先收集所有文本，用于字体子集化 ──
    let mut all_text = String::new();
    all_text.push_str("流量账单");
    all_text.push_str(&format!("账单编号：{bill_no}"));
    all_text.push_str(&format!("账单周期：{period_start} ～ {period_end}"));
    all_text.push_str(&format!("生成时间：{generated_at}"));
    all_text.push_str(&format!("客户：{org_name}"));
    all_text.push_str("总调用次数Credit 用量应付金额费用明细调用次数金额 (¥)项目：项目合计暂无 API Key 用量流量账单 - 费用明细（续）流量账单 - 费用汇总客户账单周期（续）");
    all_text.push_str(&format!(
        "{}{}¥{}",
        format_integer(total_requests),
        format_decimal(total_credit),
        format_decimal(total_money)
    ));
    all_text.push_str("合计续第页此账单由系统自动生成，如有疑问请联系管理员。");
    for project in projects.iter().filter(|project| project.has_usage()) {
        all_text.push_str(&project.name);
        all_text.push_str(&format!(
            "{}{}{}",
            format_integer(project.request_count),
            format_decimal(project.credit_cost),
            format_decimal(project.money_cost)
        ));
        for key in &project.keys {
            all_text.push_str(&key.name);
            all_text.push_str(&format!(
                "{}{}{}",
                format_integer(key.request_count),
                format_decimal(key.credit_cost),
                format_decimal(key.money_cost)
            ));
        }
    }

    let font_data = subset_font(&all_text)?;

    let (doc, page1, layer1) = PdfDocument::new("Traffic Invoice", Mm(210.0), Mm(297.0), "Layer 1");
    let font = doc
        .add_external_font(&mut std::io::Cursor::new(&font_data))
        .map_err(|e| format!("子集字体加载失败: {e}"))?;

    let mut layer = doc.get_page(page1).get_layer(layer1);

    let lx: f32 = 22.0;
    let rx: f32 = 188.0;
    let r2: f32 = 125.0;
    let r3: f32 = 158.0;
    let r4: f32 = 185.0;
    let mut y: f32 = 270.0;
    let mut page_number: usize = 1;

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
    macro_rules! right_text {
        ($t:expr, $sz:expr, $right:expr, $yy:expr) => {{
            let value = $t;
            text!(
                value,
                $sz,
                right_aligned_x(value, $right, $sz as f32),
                $yy
            );
        }};
    }

    // ── 页脚 ──
    fill!(0.45);
    text!(
        "此账单由系统自动生成，如有疑问请联系管理员。",
        7.0,
        lx,
        18.0
    );
    text!(&format!("第 {page_number} 页"), 7.0, 172.0, 18.0);

    // ── 标题 ──
    fill!(0.0);
    text!("流量账单", 22.0, lx, y);
    y -= 16.0;

    // ── 账单信息 ──
    fill!(0.25);
    text!(&format!("账单编号：{bill_no}"), 9.0, lx, y);
    text!(
        &format!("账单周期：{period_start} ～ {period_end}"),
        9.0,
        108.0,
        y
    );
    y -= 5.5;
    text!(&format!("生成时间：{generated_at}"), 9.0, lx, y);
    fill!(0.0);
    text!(&format!("客户：{org_name}"), 9.0, 108.0, y);
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
    text!(&format_integer(total_requests), 18.0, lx, y - 5.0);
    text!(&format_decimal(total_credit), 18.0, 78.0, y - 5.0);
    text!(
        &format!("¥{}", format_decimal(total_money)),
        18.0,
        138.0,
        y - 5.0
    );
    y -= 22.0;

    // ── 分割线 ──
    hline!(y, 0.6, 0.7);
    y -= 12.0;

    // ── 明细标题与数值列表头 ──
    fill!(0.0);
    text!("费用明细", 12.0, lx, y);
    y -= 10.0;
    fill!(0.2);
    right_text!("调用次数", 8.5, r2, y);
    right_text!("Credit 用量", 8.5, r3, y);
    right_text!("金额 (¥)", 8.5, r4, y);
    y -= 3.5;
    hline!(y, 0.4, 0.6);
    y -= 8.0;

    // ── 数据行：项目分组 + Key 明细 + 组尾小计 ──
    for project in projects.iter().filter(|project| project.has_usage()) {
        let group_height = 10.0 + (project.keys.len().max(1) as f32 * 9.0) + 12.0;
        let should_start_new_page = y < 55.0 || (group_height <= 190.0 && y - group_height < 38.0);
        if should_start_new_page {
            let (page, layer_index) = doc.add_page(Mm(210.0), Mm(297.0), "Details");
            layer = doc.get_page(page).get_layer(layer_index);
            page_number += 1;
            fill!(0.45);
            text!(
                "此账单由系统自动生成，如有疑问请联系管理员。",
                7.0,
                lx,
                18.0
            );
            text!(&format!("第 {page_number} 页"), 7.0, 172.0, 18.0);
            fill!(0.0);
            text!("流量账单 - 费用明细（续）", 16.0, lx, 270.0);
            fill!(0.35);
            text!(&format!("客户：{org_name}"), 8.0, lx, 260.0);
            text!(
                &format!("账单周期：{period_start} ～ {period_end}"),
                8.0,
                110.0,
                260.0
            );
            hline!(253.0, 0.6, 0.7);
            fill!(0.2);
            right_text!("调用次数", 8.5, r2, 243.0);
            right_text!("Credit 用量", 8.5, r3, 243.0);
            right_text!("金额 (¥)", 8.5, r4, 243.0);
            hline!(239.5, 0.4, 0.6);
            y = 231.5;
        }

        // 深浅分明的项目标题栏，与下方 Key 明细形成清晰层级。
        hline!(y + 0.8, 19.0, 0.90);
        fill!(0.05);
        text!(
            &truncate_label(&format!("项目：{}", project.name), 40),
            10.0,
            lx + 4.0,
            y
        );
        y -= 10.5;

        if project.keys.is_empty() {
            fill!(0.5);
            text!("暂无 API Key 用量", 8.0, lx + 8.0, y);
            y -= 9.0;
        } else {
            for key in &project.keys {
                if y < 50.0 {
                    let (page, layer_index) = doc.add_page(Mm(210.0), Mm(297.0), "Details");
                    layer = doc.get_page(page).get_layer(layer_index);
                    page_number += 1;
                    fill!(0.45);
                    text!(
                        "此账单由系统自动生成，如有疑问请联系管理员。",
                        7.0,
                        lx,
                        18.0
                    );
                    text!(&format!("第 {page_number} 页"), 7.0, 172.0, 18.0);
                    fill!(0.0);
                    text!("流量账单 - 费用明细（续）", 16.0, lx, 270.0);
                    fill!(0.35);
                    text!(&format!("客户：{org_name}"), 8.0, lx, 260.0);
                    text!(
                        &format!("账单周期：{period_start} ～ {period_end}"),
                        8.0,
                        110.0,
                        260.0
                    );
                    hline!(253.0, 0.6, 0.7);
                    fill!(0.2);
                    right_text!("调用次数", 8.5, r2, 243.0);
                    right_text!("Credit 用量", 8.5, r3, 243.0);
                    right_text!("金额 (¥)", 8.5, r4, 243.0);
                    hline!(239.5, 0.4, 0.6);
                    y = 231.5;

                    hline!(y + 0.8, 19.0, 0.90);
                    fill!(0.05);
                    text!(
                        &truncate_label(&format!("项目：{}（续）", project.name), 38),
                        10.0,
                        lx + 4.0,
                        y
                    );
                    y -= 10.5;
                }

                fill!(0.18);
                text!(&truncate_label(&key.name, 36), 8.6, lx + 10.0, y);
                fill!(0.28);
                let request_count = format_integer(key.request_count);
                let credit_cost = format_decimal(key.credit_cost);
                let money_cost = format_decimal(key.money_cost);
                right_text!(&request_count, 8.3, r2, y);
                right_text!(&credit_cost, 8.3, r3, y);
                right_text!(&money_cost, 8.3, r4, y);
                y -= 8.0;
            }
        }

        // 小计统一放在分组底部，形成明确的阅读闭环。
        hline!(y + 2.5, 0.35, 0.82);
        fill!(0.12);
        text!("项目合计", 8.5, lx + 10.0, y - 1.0);
        let project_requests = format_integer(project.request_count);
        let project_credit = format_decimal(project.credit_cost);
        let project_money = format_decimal(project.money_cost);
        right_text!(&project_requests, 8.5, r2, y - 1.0);
        right_text!(&project_credit, 8.5, r3, y - 1.0);
        right_text!(&project_money, 8.5, r4, y - 1.0);
        y -= 11.0;
    }

    if y < 45.0 {
        let (page, layer_index) = doc.add_page(Mm(210.0), Mm(297.0), "Summary");
        layer = doc.get_page(page).get_layer(layer_index);
        page_number += 1;
        fill!(0.45);
        text!(
            "此账单由系统自动生成，如有疑问请联系管理员。",
            7.0,
            lx,
            18.0
        );
        text!(&format!("第 {page_number} 页"), 7.0, 172.0, 18.0);
        fill!(0.0);
        text!("流量账单 - 费用汇总", 16.0, lx, 270.0);
        fill!(0.35);
        text!(&format!("客户：{org_name}"), 8.0, lx, 260.0);
        text!(
            &format!("账单周期：{period_start} ～ {period_end}"),
            8.0,
            110.0,
            260.0
        );
        y = 245.0;
    }

    // ── 合计粗线 ──
    y += 3.0;
    hline!(y, 1.2, 0.0);
    y -= 9.0;

    // ── 合计行 ──
    fill!(0.0);
    text!("合计", 11.0, lx, y);
    let total_requests = format_integer(total_requests);
    let total_credit = format_decimal(total_credit);
    let total_money = format!("¥{}", format_decimal(total_money));
    right_text!(&total_requests, 11.0, r2, y);
    right_text!(&total_credit, 11.0, r3, y);
    right_text!(&total_money, 11.0, r4, y);

    doc.save_to_bytes()
        .map_err(|e| format!("PDF 保存失败: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_request_defaults_to_all_projects() {
        let org_id = Uuid::new_v4();
        let request: ExportUsageReportRequest = serde_json::from_value(serde_json::json!({
            "org_id": org_id,
            "month": "2026-08",
            "delivery": "download"
        }))
        .unwrap();

        assert_eq!(request.org_id, org_id);
        assert!(request.project_ids.is_empty());
    }

    #[test]
    fn invoice_generation_time_uses_beijing_timezone() {
        let utc = DateTime::parse_from_rfc3339("2026-08-03T06:21:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let beijing = to_beijing_time(utc);

        assert_eq!(beijing.format("%Y-%m-%d %H:%M").to_string(), "2026-08-03 14:21");
        assert!(generate_bill_no(&beijing).starts_with("LLM-20260803142100-"));
    }

    #[test]
    fn invoice_numbers_use_grouping_separators() {
        assert_eq!(format_integer(10240), "10,240");
        assert_eq!(format_decimal(249295.15), "249,295.15");
        assert_eq!(format_decimal(-6232.38), "-6,232.38");
    }

    #[test]
    fn invoice_with_project_and_key_details_is_generated() {
        let projects = vec![ProjectUsage {
            id: None,
            name: "客服".to_string(),
            keys: vec![KeyUsage {
                name: "生产环境".to_string(),
                request_count: 9146,
                credit_cost: 223194.45,
                money_cost: 5579.86,
            }],
            request_count: 9146,
            credit_cost: 223194.45,
            money_cost: 5579.86,
        }];

        let pdf = generate_org_invoice_pdf(
            "LLM-TEST-0001",
            "2026-07-01",
            "2026-07-31",
            "2026-07-21 10:15",
            "测试组织",
            &projects,
            9146,
            223194.45,
            5579.86,
        )
        .unwrap();

        assert!(pdf.starts_with(b"%PDF-"));
        assert!(pdf.len() > 10_000);
    }

    #[test]
    fn invoice_with_many_keys_spans_multiple_pages() {
        let keys: Vec<KeyUsage> = (1..=50)
            .map(|index| KeyUsage {
                name: format!("API Key {index}"),
                request_count: index,
                credit_cost: index as f64 * 1.25,
                money_cost: index as f64 * 0.5,
            })
            .collect();
        let projects = vec![ProjectUsage {
            id: None,
            name: "多 Key 项目".to_string(),
            keys,
            request_count: 1275,
            credit_cost: 1593.75,
            money_cost: 637.5,
        }];

        let pdf = generate_org_invoice_pdf(
            "LLM-TEST-0002",
            "2026-07-01",
            "2026-07-31",
            "2026-07-21 10:15",
            "测试组织",
            &projects,
            1275,
            1593.75,
            637.5,
        )
        .unwrap();

        let pdf_text = String::from_utf8_lossy(&pdf);
        let marker = "/Type/Pages/Count ";
        let count_start = pdf_text.find(marker).unwrap() + marker.len();
        let page_count: usize = pdf_text[count_start..]
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .collect::<String>()
            .parse()
            .unwrap();
        assert!(page_count > 1);
    }

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
        "流量账单 {} - {}",
        start_time.format("%Y-%m-%d"),
        (end_time - chrono::TimeDelta::seconds(1)).format("%Y-%m-%d")
    );

    let att_count = attachments.len();
    let body_text = format!("请查收附件中的流量账单（共 {att_count} 份）。");

    let mut mp = MultiPart::mixed().singlepart(SinglePart::plain(body_text));

    for att in attachments {
        let ct: ContentType = "application/pdf".parse().unwrap();
        mp = mp.singlepart(Attachment::new(att.filename.clone()).body(att.data.clone(), ct));
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
    mailer.send(email).await.map_err(|e| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("邮件发送失败: {e}"),
        )
    })?;

    Ok(())
}
