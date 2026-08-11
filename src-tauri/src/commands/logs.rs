//! Command xem log.

use chrono::{DateTime, Utc};
use gcp::logging::{self, LogQuery, StreamFilter};
use gcp::types::LogPage;
use tauri::State;

use crate::error::CmdError;
use crate::state::AppState;

type R<T> = Result<T, CmdError>;

fn parse_stream(s: Option<&str>) -> StreamFilter {
    match s {
        Some("request") | Some("requests") => StreamFilter::Requests,
        Some("app") => StreamFilter::App,
        _ => StreamFilter::All,
    }
}

/// Lấy một trang log.
///
/// `since` (RFC3339) dùng cho tail: mỗi lần poll chỉ hỏi log mới hơn mốc đã thấy.
/// Frontend gộp kết quả và bỏ trùng theo `insertId` — polling luôn chồng lấn vì log có
/// thể được ghi trễ so với timestamp của nó.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn fetch_logs(
    state: State<'_, AppState>,
    project: String,
    region: String,
    service: String,
    revision: Option<String>,
    min_severity: Option<String>,
    search: Option<String>,
    stream: Option<String>,
    minutes: Option<i64>,
    since: Option<String>,
    page_size: Option<i64>,
    page_token: Option<String>,
) -> R<LogPage> {
    state.guard_project(&project).await?;
    let mut q = LogQuery::new(&project, &region, &service);
    q.revision = revision.filter(|r| !r.is_empty());
    q.min_severity = min_severity.filter(|s| !s.is_empty());
    q.search = search.filter(|s| !s.trim().is_empty());
    q.stream = parse_stream(stream.as_deref());
    q.minutes = minutes.unwrap_or(60);
    q.page_size = page_size.unwrap_or(200);
    q.page_token = page_token.filter(|t| !t.is_empty());

    if let Some(s) = since.as_deref().filter(|s| !s.is_empty()) {
        // `since` sai định dạng thì bỏ qua và dùng cửa sổ `minutes` — thà lấy hơi rộng
        // còn hơn báo lỗi và để người dùng không xem được log nào.
        q.since = DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|d| d.with_timezone(&Utc));
    }

    Ok(logging::fetch_logs(&state.gcp, &q).await?)
}

/// Link mở Log Explorer trên GCP Console với đúng filter đang xem.
///
/// Log Explorer làm được những thứ app này không làm (histogram, saved query, sink),
/// nên đưa đường thoát ra Console là hữu ích hơn là cố nhồi mọi thứ vào app.
#[tauri::command]
pub async fn log_explorer_url(
    project: String,
    region: String,
    service: String,
) -> R<String> {
    let query = format!(
        "resource.type=\"cloud_run_revision\"\nresource.labels.service_name=\"{service}\"\nresource.labels.location=\"{region}\""
    );
    Ok(format!(
        "https://console.cloud.google.com/logs/query;query={}?project={}",
        gcp::client::seg(&query),
        gcp::client::seg(&project)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_stream_nhan_dung_cac_gia_tri() {
        assert!(matches!(parse_stream(Some("app")), StreamFilter::App));
        assert!(matches!(parse_stream(Some("request")), StreamFilter::Requests));
        assert!(matches!(parse_stream(Some("requests")), StreamFilter::Requests));
        assert!(matches!(parse_stream(None), StreamFilter::All));
        assert!(matches!(parse_stream(Some("bậy bạ")), StreamFilter::All));
    }
}
