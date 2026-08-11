//! State dùng chung giữa các command.

use std::path::PathBuf;

use gcp::auth::AuthInfo;
use gcp::GcpClient;
use tokio::sync::RwLock;

use crate::audit::{Action, AuditLog, Outcome, Record};
use crate::config::{EnvLabel, Settings};
use crate::error::CmdError;
use crate::vault::{UnlockedVault, Vault};

pub struct AppState {
    pub gcp: GcpClient,
    pub settings: RwLock<Settings>,
    pub settings_path: PathBuf,
    pub audit: AuditLog,
    pub vault: Vault,
    /// `Some` khi vault đã mở khoá. Giữ khoá dẫn xuất ở đây để thêm/xoá credential
    /// không phải hỏi lại passphrase.
    pub unlocked: RwLock<Option<UnlockedVault>>,
}

impl AppState {
    pub fn new(config_dir: PathBuf, data_dir: PathBuf) -> Result<Self, String> {
        let settings_path = config_dir.join("settings.json");
        let settings = Settings::load(&settings_path);
        let gcp = GcpClient::new().map_err(|e| e.to_string())?;

        Ok(Self {
            gcp,
            settings: RwLock::new(settings),
            settings_path,
            audit: AuditLog::new(data_dir.join("audit.jsonl")),
            vault: Vault::new(data_dir.join("credentials.vault")),
            unlocked: RwLock::new(None),
        })
    }

    pub async fn save_settings(&self) {
        let s = self.settings.read().await;
        if let Err(e) = s.save(&self.settings_path) {
            eprintln!("[settings] không lưu được cấu hình: {e}");
        }
    }

    /// Cổng cho MỌI command nhận tham số `project` — kể cả command chỉ đọc.
    ///
    /// Người dùng yêu cầu ghim app vào một project để staging/master không thể bị chạm
    /// tới. Kiểm ở tầng Rust chứ không chỉ ẩn dropdown: chỉ ẩn UI thì một lời gọi IPC từ
    /// devtools, hoặc một bug state ở frontend, là đủ để đi vòng qua.
    pub async fn guard_project(&self, project: &str) -> Result<(), CmdError> {
        let s = self.settings.read().await;
        if s.project_allowed(project) {
            return Ok(());
        }
        let allowed = if s.allowed_projects.is_empty() {
            "(chưa có project nào)".to_string()
        } else {
            s.allowed_projects.join(", ")
        };
        Err(CmdError::new(
            "projectLocked",
            format!(
                "App đang được ghim vào project {allowed} nên không thao tác với `{project}`.\n\n\
                 Đây là lớp chặn ở tầng Rust, không phải chỉ ẩn nút trên UI — mục đích là \
                 staging và master không thể bị chạm tới do nhầm lẫn.\n\n\
                 Muốn mở rộng phạm vi: Cài đặt → Project được phép."
            ),
        ))
    }

    /// Cổng duy nhất cho mọi thao tác ghi.
    ///
    /// Kiểm tra ở tầng Rust chứ không chỉ ở UI: nếu chỉ khoá nút bấm thì một lỗi state
    /// ở frontend, hoặc devtools, là đủ để bỏ qua toàn bộ lớp bảo vệ. Ở đây thì không.
    pub async fn guard_write(
        &self,
        project: &str,
        service: &str,
        confirm_text: Option<&str>,
    ) -> Result<EnvLabel, CmdError> {
        // Ghi thì phải qua cả hai cổng.
        self.guard_project(project).await?;

        let s = self.settings.read().await;

        if s.read_only {
            return Err(CmdError::read_only());
        }

        let label = s.label_for(project);
        if label.requires_typed_confirm() {
            let ok = confirm_text
                .map(|t| t.trim() == service)
                .unwrap_or(false);
            if !ok {
                return Err(CmdError::needs_confirm(
                    service,
                    match label {
                        EnvLabel::Prod => "prod",
                        EnvLabel::Unknown => "chưa gắn nhãn",
                        EnvLabel::Staging => "staging",
                        EnvLabel::Dev => "dev",
                    },
                ));
            }
        }
        Ok(label)
    }

    pub async fn auth_info(&self) -> Result<AuthInfo, CmdError> {
        Ok(self.gcp.auth_info().await?)
    }

    /// Ghi audit. Lấy account hiện tại; nếu không lấy được thì vẫn ghi với `unknown`
    /// thay vì bỏ qua bản ghi — thiếu tên người còn hơn mất dấu vết thao tác.
    #[allow(clippy::too_many_arguments)]
    pub async fn record(
        &self,
        project: &str,
        region: Option<&str>,
        service: Option<&str>,
        action: Action,
        changes: Vec<String>,
        outcome: Outcome,
        message: &str,
        new_revision: Option<String>,
        operation: Option<String>,
    ) {
        let (account, effective) = match self.gcp.auth_info().await {
            Ok(i) => (i.account.clone(), i.effective_identity().to_string()),
            Err(_) => ("unknown".to_string(), "unknown".to_string()),
        };
        let env_label = {
            let s = self.settings.read().await;
            format!("{:?}", s.label_for(project)).to_lowercase()
        };

        self.audit.append(&Record {
            ts: crate::audit::now_iso(),
            account,
            effective_identity: effective,
            project: project.to_string(),
            env_label,
            region: region.map(String::from),
            service: service.map(String::from),
            action,
            changes,
            outcome,
            message: message.to_string(),
            new_revision,
            operation,
        });
    }
}

#[cfg(test)]
mod guard_tests {
    use super::*;
    use crate::config::Settings;

    /// Dựng AppState trong thư mục tạm. Không gọi GCP nên test chạy offline.
    fn state(name: &str, settings: Settings) -> AppState {
        let dir = std::env::temp_dir().join("crc-guard-test").join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        AppState {
            gcp: GcpClient::new().unwrap(),
            settings: RwLock::new(settings),
            settings_path: dir.join("settings.json"),
            audit: AuditLog::new(dir.join("audit.jsonl")),
            vault: Vault::new(dir.join("credentials.vault")),
            unlocked: RwLock::new(None),
        }
    }

    #[tokio::test]
    async fn cho_qua_project_duoc_ghim() {
        let st = state("allowed", Settings::default());
        assert!(st.guard_project("example-project").await.is_ok());
    }

    #[tokio::test]
    async fn chan_master_va_staging_ke_ca_khi_chi_doc() {
        let st = state("blocked", Settings::default());
        for p in ["example-prod", "example-staging", "example-develop"] {
            let err = st.guard_project(p).await.unwrap_err();
            assert_eq!(err.kind, "projectLocked", "{p} phải bị chặn");
            assert!(err.message.contains("example-project"));
        }
    }

    #[tokio::test]
    async fn ghi_vao_project_ngoai_allowlist_bi_chan_truoc_ca_read_only() {
        // Thứ tự quan trọng: guard_project phải chạy TRƯỚC kiểm read-only, để lỗi trả về
        // nói đúng vấn đề thật (sai project) chứ không phải "đang ở chế độ chỉ đọc".
        let st = state(
            "order",
            Settings {
                read_only: true,
                ..Default::default()
            },
        );
        let err = st
            .guard_write("example-prod", "gateway", Some("gateway"))
            .await
            .unwrap_err();
        assert_eq!(err.kind, "projectLocked", "phải báo sai project, không phải readOnly");
    }

    #[tokio::test]
    async fn ghi_vao_project_duoc_phep_van_bi_read_only_chan() {
        let st = state("ro", Settings::default());
        let err = st
            .guard_write("example-project", "gateway", Some("gateway"))
            .await
            .unwrap_err();
        assert_eq!(err.kind, "readOnly");
    }

    #[tokio::test]
    async fn tat_khoa_project_thi_qua_duoc() {
        let st = state(
            "unlocked-proj",
            Settings {
                project_lock: false,
                ..Default::default()
            },
        );
        assert!(st.guard_project("example-prod").await.is_ok());
    }
}
