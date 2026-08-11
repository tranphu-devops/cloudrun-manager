//! Command cho vault credential: import SA key, mở/khoá vault.
//!
//! Bất biến: SA key **không bao giờ** đi ngược ra frontend. Các command ở đây nhận nội dung
//! file key một chiều (import) và trả về **mô tả** (email, project, quyền), không trả key.

use gcp::resourcemanager;
use gcp::sa::ServiceAccountKey;
use serde::Serialize;
use tauri::State;

use crate::audit::{Action, Outcome};
use crate::error::CmdError;
use crate::state::AppState;
use crate::vault::{KdfParams, VaultContents};

type R<T> = Result<T, CmdError>;

impl From<crate::vault::VaultError> for CmdError {
    fn from(e: crate::vault::VaultError) -> Self {
        use crate::vault::VaultError as V;
        let kind = match e {
            V::WrongPassphrase | V::PassphraseTooShort => "vaultPassphrase",
            V::NotFound => "vaultMissing",
            V::Tampered | V::BadMagic | V::BadVersion(_) | V::Truncated(_) | V::Corrupt(_) => {
                "vaultCorrupt"
            }
            V::Io(_) => "other",
            V::NoSuchIndex(_) => "invalid",
        };
        CmdError::new(kind, e.to_string())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialInfo {
    pub client_email: String,
    pub project_id: Option<String>,
    pub private_key_id: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultStatus {
    /// Vault đã tồn tại trên đĩa.
    pub exists: bool,
    /// Vault đang mở khoá trong session này.
    pub unlocked: bool,
    /// Credential đang active (chỉ metadata, không có key).
    pub active: Option<CredentialInfo>,
    pub credential_count: usize,
    /// Nguồn token đang thực sự dùng: `serviceAccount` | `gcloudCli` | `adc`.
    pub effective_source: String,
    pub vault_path: String,
}

async fn status_of(state: &AppState) -> VaultStatus {
    let unlocked = state.unlocked.read().await;
    let active = unlocked.as_ref().and_then(|u| {
        u.contents
            .active()
            .and_then(|json| ServiceAccountKey::parse(json).ok())
            .map(|k| CredentialInfo {
                client_email: k.client_email.clone(),
                project_id: k.project_id.clone(),
                private_key_id: k.private_key_id.clone(),
            })
    });

    VaultStatus {
        exists: state.vault.exists(),
        unlocked: unlocked.is_some(),
        credential_count: unlocked
            .as_ref()
            .map(|u| u.contents.credentials.len())
            .unwrap_or(0),
        effective_source: if state.gcp.auth.has_service_account().await {
            "serviceAccount".into()
        } else {
            "gcloudCli".into()
        },
        vault_path: state.vault.path().display().to_string(),
        active,
    }
}

#[tauri::command]
pub async fn vault_status(state: State<'_, AppState>) -> R<VaultStatus> {
    Ok(status_of(&state).await)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub credential: CredentialInfo,
    /// Lấy token thật thành công — chứng minh key hợp lệ và đồng hồ máy đúng.
    pub token_ok: bool,
    /// Quyền SA có trên project đang ghim. `None` nếu không kiểm được.
    pub granted: Option<Vec<String>>,
    pub missing: Vec<String>,
    pub warnings: Vec<String>,
}

/// Import SA key JSON và ghi vào vault.
///
/// Kiểm ngay tại đây thay vì chỉ lưu: import mà không thử thì lỗi sẽ nổ lúc đang cần dùng.
/// Ba bước kiểm — parse, lấy token thật, và đối chiếu quyền trên project đang ghim.
#[tauri::command]
pub async fn import_service_account(
    state: State<'_, AppState>,
    key_json: String,
    passphrase: String,
) -> R<ImportResult> {
    // 1. Parse + validate định dạng.
    let key = ServiceAccountKey::parse(&key_json).map_err(CmdError::from)?;
    let info = CredentialInfo {
        client_email: key.client_email.clone(),
        project_id: key.project_id.clone(),
        private_key_id: key.private_key_id.clone(),
    };

    // 2. Ghi vào vault trước khi nạp — nếu ghi lỗi thì không đổi trạng thái đang chạy.
    let mut unlocked_guard = state.unlocked.write().await;
    let unlocked = match unlocked_guard.take() {
        // Vault đang mở: thêm vào, không cần passphrase mới.
        Some(mut u) => {
            u.add(key_json.clone());
            state.vault.save(&u)?;
            u
        }
        // Chưa có vault (hoặc đang khoá): tạo mới bằng passphrase vừa nhập.
        None => state.vault.create(
            &passphrase,
            &VaultContents {
                credentials: vec![key_json.clone()],
                active_index: 0,
            },
            KdfParams::default(),
        )?,
    };
    *unlocked_guard = Some(unlocked);
    drop(unlocked_guard);

    // 3. Nạp vào TokenProvider rồi thử lấy token thật.
    state.gcp.auth.set_service_account(Some(key)).await;
    let mut warnings = Vec::new();

    let token_ok = match state.gcp.auth.token().await {
        Ok(_) => true,
        Err(e) => {
            // Key không dùng được: quay về gcloud để app vẫn chạy, và báo rõ.
            state.gcp.auth.set_service_account(None).await;
            return Err(CmdError::from(e));
        }
    };

    // 4. Đối chiếu quyền trên project đang ghim — để biết ngay SA thiếu gì.
    let project = {
        let s = state.settings.read().await;
        s.allowed_projects.first().cloned()
    };

    let (granted, missing) = match &project {
        Some(p) => {
            match resourcemanager::test_permissions(
                &state.gcp,
                p,
                resourcemanager::WANTED_PERMISSIONS,
            )
            .await
            {
                Ok(g) => {
                    let caps = resourcemanager::interpret(&g);
                    (Some(g), caps.missing)
                }
                Err(e) => {
                    warnings.push(format!(
                        "Không kiểm được quyền của SA trên {p}: {e}. App vẫn dùng được — lỗi quyền \
                         sẽ hiện lúc bạn thao tác."
                    ));
                    (None, Vec::new())
                }
            }
        }
        None => (None, Vec::new()),
    };

    // Key thuộc project khác project đang ghim: không phải lỗi (SA cross-project là hợp lệ)
    // nhưng thường là dấu hiệu import sai file.
    if let (Some(kp), Some(p)) = (&info.project_id, &project) {
        if kp != p {
            warnings.push(format!(
                "Key này thuộc project `{kp}` nhưng app đang ghim vào `{p}`. Vẫn dùng được nếu SA \
                 có quyền cross-project, nhưng hãy kiểm tra lại xem có phải bạn chọn đúng file."
            ));
        }
    }

    state
        .record(
            project.as_deref().unwrap_or(""),
            None,
            None,
            Action::ToggleReadOnly,
            vec![format!("Import service account {}", info.client_email)],
            Outcome::Ok,
            "Đã import service account key vào vault và lấy token thành công.",
            None,
            None,
        )
        .await;

    Ok(ImportResult {
        credential: info,
        token_ok,
        granted,
        missing,
        warnings,
    })
}

/// Mở khoá vault và nạp credential active vào TokenProvider.
#[tauri::command]
pub async fn unlock_vault(state: State<'_, AppState>, passphrase: String) -> R<VaultStatus> {
    let unlocked = state.vault.unlock(&passphrase)?;

    let key = match unlocked.contents.active() {
        Some(json) => Some(ServiceAccountKey::parse(json).map_err(CmdError::from)?),
        None => None,
    };

    *state.unlocked.write().await = Some(unlocked);
    state.gcp.auth.set_service_account(key).await;

    Ok(status_of(&state).await)
}

/// Khoá vault: bỏ key khỏi RAM, quay về gcloud.
///
/// Không làm app chết giữa việc — chỉ đổi nguồn token.
#[tauri::command]
pub async fn lock_vault(state: State<'_, AppState>) -> R<VaultStatus> {
    *state.unlocked.write().await = None;
    state.gcp.auth.set_service_account(None).await;
    state.gcp.cache.clear().await;
    Ok(status_of(&state).await)
}

/// Xoá một credential khỏi vault. Vault phải đang mở.
#[tauri::command]
pub async fn remove_credential(state: State<'_, AppState>, index: usize) -> R<VaultStatus> {
    {
        let mut guard = state.unlocked.write().await;
        let u = guard.as_mut().ok_or_else(|| {
            CmdError::new(
                "vaultLocked",
                "Vault đang khoá. Mở khoá trước khi xoá credential.",
            )
        })?;
        u.remove(index)?;
        state.vault.save(u)?;
    }

    // Credential active có thể đã đổi — nạp lại đúng cái đang active.
    let key = {
        let guard = state.unlocked.read().await;
        match guard.as_ref().and_then(|u| u.contents.active()) {
            Some(json) => ServiceAccountKey::parse(json).ok(),
            None => None,
        }
    };
    state.gcp.auth.set_service_account(key).await;

    Ok(status_of(&state).await)
}

/// Danh sách project app được phép thao tác + bật/tắt khoá.
#[tauri::command]
pub async fn set_allowed_projects(
    state: State<'_, AppState>,
    projects: Vec<String>,
    lock: bool,
) -> R<crate::config::Settings> {
    {
        let mut s = state.settings.write().await;
        s.allowed_projects = projects
            .into_iter()
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
        s.project_lock = lock;
        // Project đang chọn không còn được phép → chuyển sang cái đầu trong allowlist,
        // nếu không UI sẽ hiện lỗi projectLocked ở mọi panel mà không rõ vì sao.
        let current_ok = s
            .current_project
            .as_deref()
            .map(|c| s.project_allowed(c))
            .unwrap_or(false);
        if !current_ok {
            s.current_project = s.allowed_projects.first().cloned();
        }
    }
    state.save_settings().await;
    Ok(state.settings.read().await.clone())
}
