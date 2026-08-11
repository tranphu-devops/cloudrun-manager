//! Command về Secret Manager.
//!
//! Giá trị secret chỉ rời khỏi Rust khi người dùng bấm reveal một cách có ý thức, và
//! lần nào cũng được ghi vào audit log (ghi việc xem, không ghi nội dung).

use gcp::run;
use gcp::secretmanager;
use gcp::types::{SecretInfo, SecretVersionInfo};
use serde::Serialize;
use tauri::State;

use crate::audit::{Action, Outcome};
use crate::error::CmdError;
use crate::state::AppState;

type R<T> = Result<T, CmdError>;

/// Danh sách secret của project, kèm thông tin service nào đang dùng.
///
/// `used_by` tính từ bản `services.list` đã cache — Cloud Run v2 trả về Service đầy đủ
/// trong list nên không cần GET riêng 95 service để biết ai dùng secret nào.
#[tauri::command]
pub async fn list_secrets(state: State<'_, AppState>, project: String) -> R<Vec<SecretInfo>> {
    state.guard_project(&project).await?;
    let mut secrets = secretmanager::list_secrets(&state.gcp, &project).await?;

    // Không lấy được usage thì vẫn trả danh sách secret (used_by rỗng) — thiếu thông
    // tin phụ không nên làm mất thông tin chính.
    if let Ok(usage) = run::secret_usage_map(&state.gcp, &project).await {
        secretmanager::attach_usage(&mut secrets, &usage);
    }

    Ok(secrets)
}

#[tauri::command]
pub async fn list_secret_versions(
    state: State<'_, AppState>,
    project: String,
    secret: String,
) -> R<Vec<SecretVersionInfo>> {
    state.guard_project(&project).await?;
    Ok(secretmanager::list_versions(&state.gcp, &project, &secret).await?)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevealResult {
    pub value: String,
    /// `true` khi nội dung không phải text đọc được (ví dụ file key nhị phân).
    /// UI cảnh báo thay vì hiện một mớ ký tự thay thế.
    pub looks_binary: bool,
    pub byte_len: usize,
    pub line_count: usize,
    /// Số giây UI nên tự ẩn lại.
    pub hide_after_seconds: u64,
}

/// Đọc giá trị một version secret.
///
/// Không cache, không log giá trị. Mỗi lần gọi đều ghi audit: ai, lúc nào, secret nào,
/// version nào — nhưng không ghi nội dung.
#[tauri::command]
pub async fn reveal_secret(
    state: State<'_, AppState>,
    project: String,
    secret: String,
    version: Option<String>,
) -> R<RevealResult> {
    state.guard_project(&project).await?;
    let version = version.unwrap_or_else(|| "latest".to_string());

    let result = secretmanager::access_version(&state.gcp, &project, &secret, &version).await;

    let (outcome, message) = match &result {
        Ok(_) => (Outcome::Ok, "đã xem giá trị secret".to_string()),
        Err(e) => (Outcome::Error, e.to_string()),
    };
    state
        .record(
            &project,
            None,
            None,
            Action::RevealSecret,
            vec![format!("{secret} version {version}")],
            outcome,
            &message,
            None,
            None,
        )
        .await;

    let value = result?;
    let s = value.expose();

    // Ký tự thay thế U+FFFD xuất hiện khi payload không phải UTF-8 hợp lệ.
    let looks_binary = s.contains('\u{FFFD}') || s.bytes().any(|b| b < 0x09);

    let hide_after_seconds = state.settings.read().await.reveal_timeout_seconds;

    Ok(RevealResult {
        looks_binary,
        byte_len: s.len(),
        line_count: s.lines().count(),
        hide_after_seconds,
        value: s.to_string(),
    })
}
