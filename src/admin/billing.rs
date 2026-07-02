use axum::{
    extract::State,
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Utc};
use lettre::{
    message::{Mailbox, MultiPart, SinglePart, Attachment, header::ContentType},
    transport::smtp::authentication::Credentials,
    AsyncSmtpTransport, AsyncTransport, Tokio1Executor,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::AuthAdmin;
use crate::state::AppState;
use super::{err, ErrorResponse};

#[derive(Deserialize)]
pub(super) struct ExportUsageReportRequest {
    org_ids: Vec<Uuid>,
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

/// POST /api/usage/export_report — 支持 download（直接下载 PDF）和 email（发送邮件）两种模式
pub(super) async fn export_usage_report(
    State(state): State<Arc<AppState>>,
    _admin: AuthAdmin,
    Json(body): Json<ExportUsageReportRequest>,
) -> Result<axum::response::Response, (StatusCode, Json<ErrorResponse>)> {
    if body.org_ids.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "请至少选择一个企业"));
    }

    let is_email = body.delivery.as_deref() == Some("email");
    let recipient = body.recipient_email.as_deref().unwrap_or("").trim().to_string();
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

    // 查询所有选中组织及其项目，LEFT JOIN 用量数据（无用量的组织/项目也会出现，计数为 0）
    let details = sqlx::query_as::<_, UsageReportDetailRow>(
        "SELECT o.name AS org_name, \
                p.name AS project_name, \
                COALESCE(o.credit_price::FLOAT8, 0) AS credit_price, \
                COUNT(r.id)::BIGINT AS request_count, \
                COALESCE(SUM(r.credit_cost)::FLOAT8, 0) AS total_credit_cost, \
                COALESCE(SUM(r.money_cost)::FLOAT8, 0) AS total_money_cost \
         FROM organizations o \
         LEFT JOIN projects p ON p.org_id = o.id \
         LEFT JOIN request_logs r ON r.org_id = o.id AND r.project_id = p.id \
              AND r.created_at >= $2 AND r.created_at < $3 \
         WHERE o.id = ANY($1) \
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

    if is_email {
        // 邮件模式：批量发送所有组织的 PDF 附件
        let mail_settings = crate::db::get_mail_settings(&state.pool).await;
        send_usage_report_mail(&mail_settings, &recipient, start_time, end_time, &pdf_attachments).await?;
        return Ok(Json(ExportUsageReportResponse {
            ok: true, start_time, end_time,
            org_count: body.org_ids.len(),
            total_requests, total_credit_cost, total_money_cost,
        }).into_response());
    }

    // 下载模式：返回第一个组织的 PDF（前端逐个组织发起请求）
    let att = pdf_attachments.into_iter().next()
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "未找到匹配的组织"))?;
    let encoded_name = percent_encode_filename(&att.filename);
    Ok((
        [(header::CONTENT_TYPE, "application/pdf".to_string()),
         (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{encoded_name}\"; filename*=UTF-8''{encoded_name}"))],
        att.data,
    ).into_response())
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
        let month_start_date = chrono::NaiveDate::parse_from_str(
            &format!("{month_str}-01"), "%Y-%m-%d",
        ).map_err(|_| err(StatusCode::BAD_REQUEST, "月份格式错误，请使用 YYYY-MM"))?;
        let month_end_date = month_start_date
            .checked_add_months(chrono::Months::new(1))
            .ok_or_else(|| err(StatusCode::BAD_REQUEST, "月份无效"))?;
        let month_start = month_start_date.and_hms_opt(0, 0, 0)
            .ok_or_else(|| err(StatusCode::BAD_REQUEST, "月份无效"))?.and_utc();
        let month_end = month_end_date.and_hms_opt(0, 0, 0)
            .ok_or_else(|| err(StatusCode::BAD_REQUEST, "月份无效"))?.and_utc();
        return Ok((month_start, month_end));
    }

    let parse_start = |s: &str| {
        chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").ok()
            .or_else(|| chrono::NaiveDateTime::parse_from_str(&format!("{s} 00:00:00"), "%Y-%m-%d %H:%M:%S").ok())
            .map(|dt| dt.and_utc())
    };
    let parse_end_exclusive = |s: &str| {
        chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").ok()
            .map(|dt| dt.and_utc() + chrono::TimeDelta::seconds(1))
            .or_else(|| {
                chrono::NaiveDateTime::parse_from_str(&format!("{s} 23:59:59"), "%Y-%m-%d %H:%M:%S").ok()
                    .map(|dt| dt.and_utc() + chrono::TimeDelta::seconds(1))
            })
    };

    let start = start_time.and_then(parse_start)
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "请选择月份或完整的开始时间"))?;
    let end = end_time.and_then(parse_end_exclusive)
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
            _ => { out.push('%'); out.push_str(&format!("{b:02X}")); }
        }
    }
    out
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
static EMBEDDED_FONT: &[u8] = include_bytes!("../../fonts/LXGWNeoXiHei.ttf");

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
