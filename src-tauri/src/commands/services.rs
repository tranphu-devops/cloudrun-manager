//! Command đọc thông tin service.

use gcp::run;
use gcp::types::{ProjectLoadSnapshot, RevisionInfo, ServiceDetail, ServiceSummary};
use serde::Serialize;
use tauri::State;

use crate::error::CmdError;
use crate::state::AppState;

type R<T> = Result<T, CmdError>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceListResult {
    pub services: Vec<ServiceSummary>,
    /// Số giây kể từ lúc dữ liệu này được lấy từ GCP.
    ///
    /// Người vận hành cần biết con số đang xem tươi đến mức nào — một dashboard không
    /// nói rõ độ tươi thì dễ bị dùng để ra quyết định trên dữ liệu cũ.
    pub age_seconds: u64,
    /// Các region có service, để nhóm sidebar.
    pub regions: Vec<String>,
}

#[tauri::command]
pub async fn list_services(state: State<'_, AppState>, project: String) -> R<ServiceListResult> {
    state.guard_project(&project).await?;
    let services = run::list_services(&state.gcp, &project).await?;

    let age_seconds = state
        .gcp
        .cache
        .age(&format!("run:{project}:services"))
        .await
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let mut regions: Vec<String> = services.iter().map(|s| s.region.clone()).collect();
    regions.sort();
    regions.dedup();

    Ok(ServiceListResult {
        services,
        age_seconds,
        regions,
    })
}

#[tauri::command]
pub async fn get_service(
    state: State<'_, AppState>,
    project: String,
    region: String,
    service: String,
) -> R<ServiceDetail> {
    state.guard_project(&project).await?;
    Ok(run::get_service(&state.gcp, &project, &region, &service).await?)
}

#[tauri::command]
pub async fn list_revisions(
    state: State<'_, AppState>,
    project: String,
    region: String,
    service: String,
) -> R<Vec<RevisionInfo>> {
    state.guard_project(&project).await?;
    Ok(run::list_revisions(&state.gcp, &project, &region, &service).await?)
}

/// Bỏ cache của project rồi lấy lại danh sách service.
#[tauri::command]
pub async fn refresh_project(
    state: State<'_, AppState>,
    project: String,
) -> R<ServiceListResult> {
    state.guard_project(&project).await?;
    state
        .gcp
        .cache
        .invalidate_prefix(&format!("run:{project}"))
        .await;
    list_services(state, project).await
}

/// Tải của toàn bộ service trong project — chỉ 3 truy vấn Monitoring cho cả project.
///
/// Với ~95 service thì lấy metric từng cái là bất khả thi (95×3 request mỗi lần
/// refresh). Monitoring API gộp theo `service_name` nên một truy vấn cho mỗi metric.
#[tauri::command]
pub async fn project_load(
    state: State<'_, AppState>,
    project: String,
    minutes: Option<i64>,
) -> R<ProjectLoadSnapshot> {
    state.guard_project(&project).await?;
    let range = gcp::monitoring::TimeRange::from_minutes(minutes.unwrap_or(30));
    Ok(gcp::monitoring::fetch_project_load(&state.gcp, &project, range).await)
}
