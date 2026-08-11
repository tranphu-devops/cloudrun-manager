//! Command về project, xác thực, cấu hình, audit.

use gcp::auth::AuthInfo;
use gcp::monitoring::{self, MetricCheck};
use gcp::resourcemanager::{self, Capabilities};
use gcp::types::ProjectInfo;
use serde::Serialize;
use tauri::State;

use crate::audit::{Action, Outcome};
use crate::config::{EnvLabel, Settings};
use crate::error::CmdError;
use crate::state::AppState;

type R<T> = Result<T, CmdError>;

#[tauri::command]
pub async fn auth_info(state: State<'_, AppState>) -> R<AuthInfo> {
    state.auth_info().await
}

#[tauri::command]
pub async fn list_projects(state: State<'_, AppState>) -> R<Vec<ProjectInfo>> {
    Ok(resourcemanager::list_projects(&state.gcp).await?)
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> R<Settings> {
    Ok(state.settings.read().await.clone())
}

#[tauri::command]
pub async fn set_read_only(state: State<'_, AppState>, value: bool) -> R<Settings> {
    let project = {
        let mut s = state.settings.write().await;
        s.read_only = value;
        s.current_project.clone().unwrap_or_default()
    };
    state.save_settings().await;

    // Tắt read-only là một quyết định đáng ghi lại: nó mở cửa cho mọi thao tác ghi sau đó.
    state
        .record(
            &project,
            None,
            None,
            Action::ToggleReadOnly,
            vec![format!(
                "Read-only: {}",
                if value { "BẬT" } else { "TẮT" }
            )],
            Outcome::Ok,
            if value {
                "Đã bật lại chế độ chỉ đọc."
            } else {
                "Đã tắt chế độ chỉ đọc — từ giờ app có thể ghi lên GCP."
            },
            None,
            None,
        )
        .await;

    Ok(state.settings.read().await.clone())
}

#[tauri::command]
pub async fn set_project_label(
    state: State<'_, AppState>,
    project: String,
    label: EnvLabel,
) -> R<Settings> {
    {
        let mut s = state.settings.write().await;
        s.project_labels.insert(project, label);
    }
    state.save_settings().await;
    Ok(state.settings.read().await.clone())
}

#[tauri::command]
pub async fn set_preferences(
    state: State<'_, AppState>,
    auto_refresh_seconds: Option<u64>,
    log_poll_seconds: Option<u64>,
    reveal_timeout_seconds: Option<u64>,
    metrics_window_minutes: Option<i64>,
) -> R<Settings> {
    {
        let mut s = state.settings.write().await;
        if let Some(v) = auto_refresh_seconds {
            // 0 = tắt. Dưới 10s thì vô nghĩa vì cache đã 15–30s, chỉ tốn quota.
            s.auto_refresh_seconds = if v == 0 { 0 } else { v.clamp(10, 600) };
        }
        if let Some(v) = log_poll_seconds {
            s.log_poll_seconds = v.clamp(2, 60);
        }
        if let Some(v) = reveal_timeout_seconds {
            s.reveal_timeout_seconds = v.clamp(5, 300);
        }
        if let Some(v) = metrics_window_minutes {
            s.metrics_window_minutes = v.clamp(5, 30 * 24 * 60);
        }
    }
    state.save_settings().await;
    Ok(state.settings.read().await.clone())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilitiesResult {
    /// `false` nghĩa là không kiểm tra được quyền (thường vì thiếu quyền gọi
    /// `testIamPermissions`). Khi đó các cờ bên dưới là lạc quan, không phải sự thật.
    pub checked: bool,
    pub note: Option<String>,
    #[serde(flatten)]
    pub caps: Capabilities,
}

/// Cờ lạc quan khi không kiểm tra được quyền.
///
/// Nếu đoán là "không có quyền" thì UI sẽ khoá hết chức năng và app thành vô dụng dù
/// người dùng có quyền thật. Đoán "có quyền" thì tệ nhất là họ bấm và nhận lỗi 403 —
/// mà lỗi 403 của app này đã được diễn giải rõ ràng, nên đó là kết cục chấp nhận được.
fn optimistic() -> Capabilities {
    Capabilities {
        can_list_services: true,
        can_read_service: true,
        can_update_service: true,
        can_read_metrics: true,
        can_read_logs: true,
        can_list_secrets: true,
        can_reveal_secrets: true,
        missing: Vec::new(),
    }
}

#[tauri::command]
pub async fn check_permissions(
    state: State<'_, AppState>,
    project: String,
) -> R<CapabilitiesResult> {
    state.guard_project(&project).await?;
    match resourcemanager::test_permissions(
        &state.gcp,
        &project,
        resourcemanager::WANTED_PERMISSIONS,
    )
    .await
    {
        Ok(granted) => Ok(CapabilitiesResult {
            checked: true,
            note: None,
            caps: resourcemanager::interpret(&granted),
        }),
        Err(e) => Ok(CapabilitiesResult {
            checked: false,
            note: Some(format!(
                "Không kiểm tra được quyền trên project này ({e}). App vẫn dùng bình thường — \
                 nếu thiếu quyền ở đâu thì lỗi sẽ hiện lúc bạn thao tác."
            )),
            caps: optimistic(),
        }),
    }
}

#[tauri::command]
pub async fn select_project(state: State<'_, AppState>, project: String) -> R<Settings> {
    state.guard_project(&project).await?;
    {
        let mut s = state.settings.write().await;
        s.touch_recent(&project);
    }
    state.save_settings().await;
    Ok(state.settings.read().await.clone())
}

/// Đối chiếu tên metric trong code với `metricDescriptors.list` thật của project.
///
/// Đáng chạy khi thêm project mới: metric sai tên không gây lỗi HTTP, chỉ trả series
/// rỗng, nên nếu không kiểm tra thì chart phẳng ở 0 sẽ bị hiểu là "service không có tải".
#[tauri::command]
pub async fn verify_metrics(state: State<'_, AppState>, project: String) -> R<Vec<MetricCheck>> {
    state.guard_project(&project).await?;
    Ok(monitoring::verify_metrics(&state.gcp, &project).await?)
}

#[tauri::command]
pub async fn audit_tail(state: State<'_, AppState>, limit: Option<usize>) -> R<Vec<serde_json::Value>> {
    Ok(state.audit.tail(limit.unwrap_or(200)))
}

#[tauri::command]
pub async fn audit_path(state: State<'_, AppState>) -> R<String> {
    Ok(state.audit.path().display().to_string())
}

/// Xoá toàn bộ cache. Dùng cho nút Reload khi người dùng muốn chắc chắn thấy số mới.
#[tauri::command]
pub async fn clear_cache(state: State<'_, AppState>) -> R<()> {
    state.gcp.cache.clear().await;
    Ok(())
}
