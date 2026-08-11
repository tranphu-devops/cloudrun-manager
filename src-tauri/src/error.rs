//! Error type gửi ra frontend.
//!
//! Frontend cần hai thứ từ một lỗi: câu để hiện cho người dùng, và một `kind` để biết
//! phải xử lý thế nào (conflict thì bắt reload, auth thì hiện hướng dẫn `gcloud auth
//! login`, permission thì hiện role cần cấp). Nên `kind` là chuỗi ổn định, không phải
//! thứ để hiển thị.

use gcp::GcpError;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CmdError {
    /// Câu hiện trực tiếp cho người dùng.
    pub message: String,
    /// Chi tiết kỹ thuật, để trong khối có thể mở ra.
    pub detail: Option<String>,
    /// `auth` | `permission` | `conflict` | `readOnly` | `needsConfirm` | `network` | `invalid` | `notFound` | `rateLimit` | `other`
    pub kind: String,
    pub status: Option<u16>,
}

impl CmdError {
    pub fn new(kind: &str, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            detail: None,
            kind: kind.to_string(),
            status: None,
        }
    }

    pub fn read_only() -> Self {
        Self::new(
            "readOnly",
            "Đang ở chế độ Read-only nên app không gửi thay đổi nào lên GCP. \
             Tắt Read-only ở thanh trên nếu bạn thực sự muốn ghi.",
        )
    }

    pub fn needs_confirm(service: &str, label: &str) -> Self {
        Self::new(
            "needsConfirm",
            format!(
                "Project này đang được gắn nhãn `{label}` nên thao tác ghi cần xác nhận: \
                 gõ đúng tên service `{service}` vào ô xác nhận. \
                 (Nếu đây là project dev, hãy gắn nhãn Dev cho nó để không phải gõ mỗi lần.)"
            ),
        )
    }
}

impl From<GcpError> for CmdError {
    fn from(e: GcpError) -> Self {
        let kind = match &e {
            GcpError::GcloudNotFound | GcpError::NotAuthenticated | GcpError::GcloudFailed(_) => {
                "auth"
            }
            GcpError::Conflict { .. } => "conflict",
            GcpError::ReadOnly => "readOnly",
            GcpError::Network(_) => "network",
            GcpError::Invalid(_) => "invalid",
            GcpError::Decode { .. } => "other",
            GcpError::Api { status, .. } => match status {
                401 => "auth",
                403 => "permission",
                404 => "notFound",
                429 => "rateLimit",
                _ => "other",
            },
        };

        Self {
            message: e.to_string(),
            detail: e.raw_detail().map(String::from),
            status: e.status(),
            kind: kind.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conflict_duoc_gan_kind_rieng() {
        // Frontend dựa vào kind này để bắt buộc reload thay vì cho retry.
        let e: CmdError = GcpError::Conflict { raw: "etag".into() }.into();
        assert_eq!(e.kind, "conflict");
        assert_eq!(e.status, Some(409));
    }

    #[test]
    fn loi_403_thanh_permission() {
        let e: CmdError = gcp::error::from_http(403, "{}", "sửa env").into();
        assert_eq!(e.kind, "permission");
        assert_eq!(e.status, Some(403));
    }

    #[test]
    fn loi_401_thanh_auth() {
        let e: CmdError = gcp::error::from_http(401, "{}", "x").into();
        assert_eq!(e.kind, "auth");
    }

    #[test]
    fn loi_429_thanh_rate_limit() {
        let e: CmdError = gcp::error::from_http(429, "{}", "x").into();
        assert_eq!(e.kind, "rateLimit");
    }

    #[test]
    fn chua_dang_nhap_thanh_auth() {
        let e: CmdError = GcpError::NotAuthenticated.into();
        assert_eq!(e.kind, "auth");
        assert!(e.message.contains("gcloud auth login"));
    }

    #[test]
    fn needs_confirm_noi_ro_phai_go_gi() {
        let e = CmdError::needs_confirm("gateway", "prod");
        assert_eq!(e.kind, "needsConfirm");
        assert!(e.message.contains("gateway"));
        assert!(e.message.contains("prod"));
    }
}
