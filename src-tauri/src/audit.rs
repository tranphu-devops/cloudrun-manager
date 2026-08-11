//! Audit log cục bộ: mọi thao tác ghi và mọi lần xem giá trị secret đều được ghi lại.
//!
//! Không thay thế Cloud Audit Logs của GCP — mục đích khác: trả lời nhanh câu
//! "chiều nay tôi sửa gì trên service nào" ngay trên máy, kèm diff, không phải đi query
//! Log Explorer. Ghi dạng JSONL để `tail`/`grep`/`jq` được.
//!
//! Bất biến quan trọng: **không bao giờ ghi giá trị secret vào file này.** Ghi lại việc
//! ai xem secret nào là hữu ích; ghi lại nội dung secret là biến audit log thành nơi rò rỉ.

use std::io::Write;
use std::path::PathBuf;

use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    UpdateEnv,
    UpdateScaling,
    /// Dry-run (`validateOnly=true`), không tạo revision.
    ValidateOnly,
    RevealSecret,
    ToggleReadOnly,
    /// Chạy tay một Cloud Run Job. Không idempotent — đáng ghi lại nhất trong nhóm này.
    RunJob,
    /// Tạm dừng / bật lại một Cloud Scheduler job.
    SetSchedulePaused,
    /// Đánh dấu trạng thái một recommendation (không thay đổi hạ tầng).
    MarkRecommendation,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    Ok,
    Error,
    /// Đã gửi nhưng chưa biết kết quả (operation còn đang chạy).
    Pending,
    Blocked,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub ts: String,
    pub account: String,
    /// Khác `account` khi đang impersonate service account.
    pub effective_identity: String,
    pub project: String,
    pub env_label: String,
    pub region: Option<String>,
    pub service: Option<String>,
    pub action: Action,
    /// Mô tả thay đổi dạng câu, ví dụ `LOG_LEVEL: info → debug`.
    /// Với secret chỉ ghi tên + version, không ghi giá trị.
    pub changes: Vec<String>,
    pub outcome: Outcome,
    pub message: String,
    pub new_revision: Option<String>,
    pub operation: Option<String>,
}

pub struct AuditLog {
    path: PathBuf,
}

impl AuditLog {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Ghi một dòng. Lỗi ghi file không được làm fail thao tác chính — người dùng vừa
    /// sửa xong service, báo "thất bại" chỉ vì không ghi được log là sai và gây hoảng.
    /// Nhưng vẫn phải để lại dấu vết ở stderr.
    pub fn append(&self, rec: &Record) {
        if let Err(e) = self.try_append(rec) {
            eprintln!("[audit] không ghi được audit log ({}): {e}", self.path.display());
        }
    }

    fn try_append(&self, rec: &Record) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let line = serde_json::to_string(rec)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(f, "{line}")
    }

    /// Đọc N dòng cuối để hiện trong màn hình History.
    pub fn tail(&self, n: usize) -> Vec<serde_json::Value> {
        let Ok(content) = std::fs::read_to_string(&self.path) else {
            return Vec::new();
        };
        let mut out: Vec<serde_json::Value> = content
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        out.reverse();
        out.truncate(n);
        out
    }
}

pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(action: Action, changes: Vec<String>) -> Record {
        Record {
            ts: now_iso(),
            account: "you@example.com".into(),
            effective_identity: "you@example.com".into(),
            project: "example-project".into(),
            env_label: "dev".into(),
            region: Some("asia-northeast1".into()),
            service: Some("gateway".into()),
            action,
            changes,
            outcome: Outcome::Ok,
            message: "ok".into(),
            new_revision: Some("gateway-00042-abc".into()),
            operation: None,
        }
    }

    #[test]
    fn ghi_va_doc_lai_duoc() {
        let dir = std::env::temp_dir().join("crc-test-audit-1");
        let _ = std::fs::remove_dir_all(&dir);
        let log = AuditLog::new(dir.join("audit.jsonl"));

        log.append(&rec(Action::UpdateEnv, vec!["LOG_LEVEL: info → debug".into()]));
        log.append(&rec(Action::UpdateScaling, vec!["Min instances: 1 → 2".into()]));

        let tail = log.tail(10);
        assert_eq!(tail.len(), 2);
        // Mới nhất lên đầu.
        assert_eq!(tail[0]["action"], serde_json::json!("updateScaling"));
        assert_eq!(tail[0]["project"], serde_json::json!("example-project"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tail_gioi_han_so_dong() {
        let dir = std::env::temp_dir().join("crc-test-audit-2");
        let _ = std::fs::remove_dir_all(&dir);
        let log = AuditLog::new(dir.join("audit.jsonl"));
        for i in 0..10 {
            log.append(&rec(Action::UpdateEnv, vec![format!("change {i}")]));
        }
        assert_eq!(log.tail(3).len(), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dong_hong_khong_lam_sap_tail() {
        let dir = std::env::temp_dir().join("crc-test-audit-3");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("audit.jsonl");
        std::fs::write(&p, "{\"ts\":\"x\",\"action\":\"updateEnv\"}\nkhông phải json\n\n").unwrap();

        let log = AuditLog::new(p);
        // Bỏ qua dòng hỏng, vẫn đọc được dòng hợp lệ.
        assert_eq!(log.tail(10).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ghi_ra_thu_muc_khong_ton_tai_thi_tu_tao() {
        let dir = std::env::temp_dir().join("crc-test-audit-4").join("sâu").join("hơn");
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("crc-test-audit-4"));
        let log = AuditLog::new(dir.join("audit.jsonl"));
        log.append(&rec(Action::UpdateEnv, vec![]));
        assert_eq!(log.tail(1).len(), 1);
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("crc-test-audit-4"));
    }

    #[test]
    fn reveal_secret_duoc_ghi_nhung_khong_kem_gia_tri() {
        // Bất biến của module: audit ghi "ai xem secret nào", không ghi nội dung.
        let dir = std::env::temp_dir().join("crc-test-audit-5");
        let _ = std::fs::remove_dir_all(&dir);
        let log = AuditLog::new(dir.join("audit.jsonl"));

        log.append(&rec(
            Action::RevealSecret,
            // Đây là dạng `changes` mà command reveal tạo ra: chỉ tên + version.
            vec!["gateway-db-password version latest".into()],
        ));

        let raw = std::fs::read_to_string(dir.join("audit.jsonl")).unwrap();
        assert!(raw.contains("revealSecret"));
        assert!(raw.contains("gateway-db-password"));
        assert!(
            !raw.contains("version latest\",\"value"),
            "audit log không được có field giá trị secret"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
