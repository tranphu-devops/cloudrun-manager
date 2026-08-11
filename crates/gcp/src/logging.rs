//! Cloud Logging API v2 — xem log của service.
//!
//! # Vì sao là polling chứ không phải streaming
//!
//! `entries.tail` (streaming thật) chỉ có trên gRPC bidi, KHÔNG có bản REST. App này
//! đi qua REST nên "live tail" ở đây là polling mỗi vài giây, dedupe theo `insertId`.
//! UI phải ghi rõ "cập nhật mỗi 3s" thay vì gọi là realtime — nói quá về độ tươi của
//! dữ liệu vận hành là cách nhanh nhất để người dùng ra quyết định sai.

use chrono::{DateTime, SecondsFormat, Utc};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::client::GcpClient;
use crate::error::{GcpError, Result};
use crate::types::{LogEntry, LogPage};

const BASE: &str = "https://logging.googleapis.com/v2";

/// Log name của Cloud Run.
const LOG_REQUESTS: &str = "run.googleapis.com%2Frequests";
const LOG_STDOUT: &str = "run.googleapis.com%2Fstdout";
const LOG_STDERR: &str = "run.googleapis.com%2Fstderr";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamFilter {
    All,
    /// Access log (`run.googleapis.com/requests`).
    Requests,
    /// stdout + stderr của app.
    App,
}

#[derive(Debug, Clone)]
pub struct LogQuery {
    pub project: String,
    pub region: String,
    pub service: String,
    /// Chỉ lấy log của một revision cụ thể.
    pub revision: Option<String>,
    /// `DEFAULT`, `INFO`, `WARNING`, `ERROR`… lọc `severity >= giá trị này`.
    pub min_severity: Option<String>,
    /// Tìm chuỗi trong nội dung log.
    pub search: Option<String>,
    pub stream: StreamFilter,
    /// Chỉ lấy log mới hơn mốc này (dùng cho tail).
    pub since: Option<DateTime<Utc>>,
    /// Cửa sổ thời gian tính theo phút khi không có `since`.
    pub minutes: i64,
    pub page_size: i64,
    pub page_token: Option<String>,
}

impl LogQuery {
    pub fn new(project: &str, region: &str, service: &str) -> Self {
        Self {
            project: project.to_string(),
            region: region.to_string(),
            service: service.to_string(),
            revision: None,
            min_severity: None,
            search: None,
            stream: StreamFilter::All,
            since: None,
            minutes: 60,
            page_size: 200,
            page_token: None,
        }
    }
}

/// Escape giá trị nhúng vào chuỗi filter của Logging query language.
///
/// Người dùng gõ dấu `"` trong ô tìm kiếm sẽ làm hỏng cú pháp filter (và về nguyên tắc
/// là một lỗ chèn query). Escape `\` trước rồi mới tới `"`, đúng thứ tự.
fn esc(v: &str) -> String {
    v.replace('\\', "\\\\").replace('"', "\\\"")
}

const ALLOWED_SEVERITY: &[&str] = &[
    "DEFAULT",
    "DEBUG",
    "INFO",
    "NOTICE",
    "WARNING",
    "ERROR",
    "CRITICAL",
    "ALERT",
    "EMERGENCY",
];

pub fn build_filter(q: &LogQuery) -> Result<String> {
    let mut parts: Vec<String> = vec![
        r#"resource.type="cloud_run_revision""#.to_string(),
        format!(r#"resource.labels.service_name="{}""#, esc(&q.service)),
        format!(r#"resource.labels.location="{}""#, esc(&q.region)),
    ];

    if let Some(rev) = &q.revision {
        parts.push(format!(
            r#"resource.labels.revision_name="{}""#,
            esc(rev)
        ));
    }

    match q.stream {
        StreamFilter::All => {}
        StreamFilter::Requests => parts.push(format!(
            r#"logName="projects/{}/logs/{}""#,
            esc(&q.project),
            LOG_REQUESTS
        )),
        StreamFilter::App => parts.push(format!(
            r#"(logName="projects/{p}/logs/{out}" OR logName="projects/{p}/logs/{err}")"#,
            p = esc(&q.project),
            out = LOG_STDOUT,
            err = LOG_STDERR
        )),
    }

    if let Some(sev) = &q.min_severity {
        let up = sev.to_ascii_uppercase();
        // Whitelist thay vì nội suy chuỗi vào filter.
        if !ALLOWED_SEVERITY.contains(&up.as_str()) {
            return Err(GcpError::Invalid(format!(
                "Severity `{sev}` không hợp lệ. Chọn một trong: {}.",
                ALLOWED_SEVERITY.join(", ")
            )));
        }
        if up != "DEFAULT" {
            parts.push(format!("severity>={up}"));
        }
    }

    if let Some(s) = &q.search {
        let s = s.trim();
        if !s.is_empty() {
            // Tìm toàn văn trong entry (Logging hỗ trợ `:` cho substring).
            parts.push(format!(r#""{}""#, esc(s)));
        }
    }

    let start = q
        .since
        .unwrap_or_else(|| Utc::now() - chrono::Duration::minutes(q.minutes.clamp(1, 60 * 24 * 30)));
    parts.push(format!(
        r#"timestamp>="{}""#,
        start.to_rfc3339_opts(SecondsFormat::Millis, true)
    ));

    Ok(parts.join(" AND "))
}

/// Rút nội dung log thành một dòng để hiển thị trong bảng.
fn flatten_message(e: &Value) -> String {
    if let Some(t) = e.get("textPayload").and_then(|v| v.as_str()) {
        return t.trim_end().to_string();
    }

    if let Some(jp) = e.get("jsonPayload") {
        // Log có cấu trúc: ưu tiên field nội dung thông dụng.
        for key in ["message", "msg", "log", "event", "error", "detail"] {
            if let Some(s) = jp.get(key).and_then(|v| v.as_str()) {
                return s.trim_end().to_string();
            }
        }
        // Không có field quen thuộc: in compact để vẫn đọc được thay vì hiện rỗng.
        return serde_json::to_string(jp).unwrap_or_default();
    }

    if let Some(hr) = e.get("httpRequest") {
        let method = hr
            .get("requestMethod")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let url = hr.get("requestUrl").and_then(|v| v.as_str()).unwrap_or("");
        // Chỉ giữ path để bảng không bị đẩy ngang bởi domain lặp lại.
        let path = url
            .split_once("://")
            .and_then(|(_, rest)| rest.split_once('/'))
            .map(|(_, p)| format!("/{p}"))
            .unwrap_or_else(|| url.to_string());
        let status = hr
            .get("status")
            .and_then(|v| v.as_i64())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "-".into());
        return format!("{method} {path} → {status}");
    }

    if let Some(p) = e.get("protoPayload") {
        return serde_json::to_string(p).unwrap_or_default();
    }

    String::new()
}

fn latency_ms(e: &Value) -> Option<f64> {
    // Logging trả latency dạng `"0.123456s"`.
    let s = e.get("httpRequest")?.get("latency")?.as_str()?;
    s.strip_suffix('s')?.parse::<f64>().ok().map(|v| v * 1000.0)
}

fn parse_entry(e: &Value) -> Option<LogEntry> {
    let log_name = e.get("logName").and_then(|v| v.as_str()).unwrap_or("");
    let stream = if log_name.ends_with("requests") {
        "request"
    } else {
        "app"
    };

    let hr = e.get("httpRequest");

    Some(LogEntry {
        insert_id: e
            .get("insertId")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        timestamp: e
            .get("timestamp")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        severity: e
            .get("severity")
            .and_then(|v| v.as_str())
            .unwrap_or("DEFAULT")
            .to_string(),
        revision: e
            .get("resource")
            .and_then(|r| r.get("labels"))
            .and_then(|l| l.get("revision_name"))
            .and_then(|v| v.as_str())
            .map(String::from),
        message: flatten_message(e),
        stream: stream.to_string(),
        http_status: hr.and_then(|h| h.get("status")).and_then(|v| v.as_i64()),
        http_method: hr
            .and_then(|h| h.get("requestMethod"))
            .and_then(|v| v.as_str())
            .map(String::from),
        http_path: hr
            .and_then(|h| h.get("requestUrl"))
            .and_then(|v| v.as_str())
            .map(String::from),
        latency_ms: latency_ms(e),
        raw: e.clone(),
    })
}

pub async fn fetch_logs(client: &GcpClient, q: &LogQuery) -> Result<LogPage> {
    let filter = build_filter(q)?;

    let body = json!({
        "resourceNames": [format!("projects/{}", q.project)],
        "filter": filter,
        // `timestamp desc` là khuyến nghị của Google cho log vừa ghi — nhanh hơn asc.
        "orderBy": "timestamp desc",
        "pageSize": q.page_size.clamp(1, 1000),
        "pageToken": q.page_token.clone().unwrap_or_default(),
    });

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Resp {
        #[serde(default)]
        entries: Vec<Value>,
        #[serde(default)]
        next_page_token: Option<String>,
    }

    let url = format!("{BASE}/entries:list");
    let ctx = format!("xem log của service {}", q.service);
    // Log không cache: mở tab log là để xem cái mới nhất.
    let resp: Resp = client.post(&url, &body, &ctx).await?;

    Ok(LogPage {
        entries: resp.entries.iter().filter_map(parse_entry).collect(),
        next_page_token: resp.next_page_token.filter(|t| !t.is_empty()),
    })
}

/// Gộp trang log mới vào danh sách đang có, bỏ trùng theo `insertId`.
///
/// Polling luôn có phần chồng lấn (log ghi trễ, cùng mốc timestamp) nên dedupe là
/// bắt buộc, không phải tối ưu.
pub fn merge_dedupe(existing: &mut Vec<LogEntry>, incoming: Vec<LogEntry>, cap: usize) {
    use std::collections::HashSet;
    let seen: HashSet<String> = existing.iter().map(|e| e.insert_id.clone()).collect();

    for e in incoming {
        // Entry không có insertId (hiếm) thì vẫn nhận, chấp nhận rủi ro trùng.
        if e.insert_id.is_empty() || !seen.contains(&e.insert_id) {
            existing.push(e);
        }
    }

    // Mới nhất lên đầu.
    existing.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    existing.dedup_by(|a, b| !a.insert_id.is_empty() && a.insert_id == b.insert_id);
    if existing.len() > cap {
        existing.truncate(cap);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q() -> LogQuery {
        LogQuery::new("example-project", "asia-northeast1", "gateway")
    }

    #[test]
    fn filter_co_du_dieu_kien_co_ban() {
        let f = build_filter(&q()).unwrap();
        assert!(f.contains(r#"resource.type="cloud_run_revision""#), "{f}");
        assert!(f.contains(r#"resource.labels.service_name="gateway""#), "{f}");
        assert!(f.contains(r#"resource.labels.location="asia-northeast1""#), "{f}");
        assert!(f.contains("timestamp>="), "{f}");
    }

    #[test]
    fn escape_dau_ngoac_kep_trong_o_tim_kiem() {
        // Không escape thì filter sai cú pháp và về nguyên tắc là chèn query được.
        let mut query = q();
        query.search = Some(r#"user said "hi" \ ok"#.into());
        let f = build_filter(&query).unwrap();
        assert!(f.contains(r#"\""#), "dấu ngoặc kép chưa được escape: {f}");
        assert!(f.contains(r#"\\"#), "dấu backslash chưa được escape: {f}");
        // Không được để lọt dấu " chưa escape làm đóng chuỗi sớm.
        assert!(!f.contains(r#" said "hi" "#), "{f}");
    }

    #[test]
    fn severity_dung_whitelist() {
        let mut query = q();
        query.min_severity = Some("error".into());
        let f = build_filter(&query).unwrap();
        assert!(f.contains("severity>=ERROR"), "{f}");

        query.min_severity = Some(r#"ERROR" OR "x"#.into());
        assert!(
            build_filter(&query).is_err(),
            "severity phải qua whitelist, không được nội suy chuỗi tuỳ ý"
        );
    }

    #[test]
    fn severity_default_khong_them_dieu_kien() {
        let mut query = q();
        query.min_severity = Some("DEFAULT".into());
        let f = build_filter(&query).unwrap();
        assert!(!f.contains("severity"), "DEFAULT nghĩa là không lọc: {f}");
    }

    #[test]
    fn stream_requests_va_app_dung_logname() {
        let mut query = q();
        query.stream = StreamFilter::Requests;
        let f = build_filter(&query).unwrap();
        assert!(f.contains("run.googleapis.com%2Frequests"), "{f}");

        query.stream = StreamFilter::App;
        let f = build_filter(&query).unwrap();
        assert!(f.contains("stdout"), "{f}");
        assert!(f.contains("stderr"), "{f}");
        assert!(f.contains(" OR "), "app log phải gộp cả stdout và stderr: {f}");
    }

    #[test]
    fn loc_theo_revision() {
        let mut query = q();
        query.revision = Some("gateway-00041-abc".into());
        let f = build_filter(&query).unwrap();
        assert!(f.contains(r#"resource.labels.revision_name="gateway-00041-abc""#), "{f}");
    }

    #[test]
    fn flatten_text_payload() {
        let e = serde_json::json!({ "textPayload": "hello world\n" });
        assert_eq!(flatten_message(&e), "hello world");
    }

    #[test]
    fn flatten_json_payload_uu_tien_field_message() {
        let e = serde_json::json!({ "jsonPayload": { "message": "boom", "extra": 1 } });
        assert_eq!(flatten_message(&e), "boom");

        let e = serde_json::json!({ "jsonPayload": { "msg": "from msg" } });
        assert_eq!(flatten_message(&e), "from msg");
    }

    #[test]
    fn flatten_json_payload_khong_co_field_quen_thi_in_compact() {
        // Không được trả rỗng — người dùng sẽ nghĩ log trống.
        let e = serde_json::json!({ "jsonPayload": { "weird": { "a": 1 } } });
        let m = flatten_message(&e);
        assert!(m.contains("weird"), "{m}");
    }

    #[test]
    fn flatten_http_request_rut_gon_thanh_path() {
        let e = serde_json::json!({
            "httpRequest": {
                "requestMethod": "GET",
                "requestUrl": "https://gateway-x-an.a.run.app/api/v1/users?page=2",
                "status": 200
            }
        });
        assert_eq!(flatten_message(&e), "GET /api/v1/users?page=2 → 200");
    }

    #[test]
    fn doc_duoc_latency() {
        let e = serde_json::json!({ "httpRequest": { "latency": "0.123456s" } });
        let ms = latency_ms(&e).unwrap();
        assert!((ms - 123.456).abs() < 0.001, "{ms}");
        assert!(latency_ms(&serde_json::json!({})).is_none());
    }

    #[test]
    fn phan_biet_stream_request_va_app() {
        let req = serde_json::json!({
            "logName": "projects/p/logs/run.googleapis.com%2Frequests",
            "insertId": "a", "timestamp": "2026-08-05T01:00:00Z"
        });
        assert_eq!(parse_entry(&req).unwrap().stream, "request");

        let app = serde_json::json!({
            "logName": "projects/p/logs/run.googleapis.com%2Fstderr",
            "insertId": "b", "timestamp": "2026-08-05T01:00:00Z"
        });
        assert_eq!(parse_entry(&app).unwrap().stream, "app");
    }

    fn entry(id: &str, ts: &str) -> LogEntry {
        LogEntry {
            insert_id: id.into(),
            timestamp: ts.into(),
            severity: "INFO".into(),
            revision: None,
            message: id.into(),
            stream: "app".into(),
            http_status: None,
            http_method: None,
            http_path: None,
            latency_ms: None,
            raw: Value::Null,
        }
    }

    #[test]
    fn merge_bo_trung_theo_insert_id() {
        // Polling luôn chồng lấn: cùng entry sẽ về hai lần.
        let mut have = vec![entry("b", "2026-08-05T01:00:02Z"), entry("a", "2026-08-05T01:00:01Z")];
        let incoming = vec![
            entry("c", "2026-08-05T01:00:03Z"),
            entry("b", "2026-08-05T01:00:02Z"),
        ];
        merge_dedupe(&mut have, incoming, 100);

        let ids: Vec<&str> = have.iter().map(|e| e.insert_id.as_str()).collect();
        assert_eq!(ids, vec!["c", "b", "a"], "phải bỏ trùng và giữ mới nhất lên đầu");
    }

    #[test]
    fn merge_gioi_han_so_dong_giu_lai() {
        let mut have: Vec<LogEntry> = (0..10)
            .map(|i| entry(&format!("id{i}"), &format!("2026-08-05T01:00:{i:02}Z")))
            .collect();
        merge_dedupe(&mut have, vec![entry("new", "2026-08-05T01:01:00Z")], 5);
        assert_eq!(have.len(), 5);
        assert_eq!(have[0].insert_id, "new");
    }
}
