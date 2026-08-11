//! Error type + phần quan trọng nhất: map lỗi GCP thành hướng dẫn hành động cụ thể.
//!
//! Lý do dành hẳn một module cho việc này: message gốc của GCP cho mấy lỗi hay gặp
//! (403 vì thiếu `iam.serviceAccounts.actAs`, 409 vì etag cũ) mơ hồ đến mức người
//! đọc không biết phải làm gì tiếp. Ở tầng vận hành, một message nói đúng "phải
//! làm gì" tiết kiệm được cả buổi debug.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum GcpError {
    #[error("Không tìm thấy gcloud CLI trên máy. Cài Google Cloud SDK rồi chạy `gcloud auth login`, sau đó mở lại app.")]
    GcloudNotFound,

    #[error("gcloud chạy lỗi: {0}")]
    GcloudFailed(String),

    #[error("Chưa đăng nhập gcloud. Chạy `gcloud auth login` trong terminal rồi bấm Reload trong app.")]
    NotAuthenticated,

    #[error("Lỗi mạng: {0}")]
    Network(String),

    #[error("Không đọc được JSON từ API {api}: {source}")]
    Decode {
        api: &'static str,
        #[source]
        source: serde_json::Error,
    },

    /// Lỗi HTTP đã được diễn giải sang tiếng Việt kèm hướng xử lý.
    #[error("{message}")]
    Api {
        status: u16,
        /// Message đã diễn giải, hiển thị trực tiếp cho người dùng.
        message: String,
        /// Message gốc của Google, để trong khối "chi tiết kỹ thuật" có thể mở ra.
        raw: String,
        /// Lỗi này có đáng retry tự động hay không.
        retryable: bool,
    },

    /// Service đã bị người khác thay đổi giữa lúc mình đang sửa.
    /// Tách riêng khỏi `Api` vì frontend phải xử lý khác: bắt buộc reload, không retry.
    #[error("Service đã bị thay đổi bởi người/tiến trình khác sau khi bạn mở nó. Bấm Reload để lấy bản mới nhất rồi sửa lại — không tự động ghi đè để tránh mất thay đổi của người khác.")]
    Conflict { raw: String },

    #[error("Đang ở chế độ Read-only. Tắt Read-only ở góc trên phải nếu bạn thực sự muốn ghi.")]
    ReadOnly,

    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, GcpError>;

impl GcpError {
    pub fn retryable(&self) -> bool {
        match self {
            GcpError::Network(_) => true,
            GcpError::Api { retryable, .. } => *retryable,
            _ => false,
        }
    }

    /// Chi tiết kỹ thuật để hiện trong khối collapse, không phải message chính.
    pub fn raw_detail(&self) -> Option<&str> {
        match self {
            GcpError::Api { raw, .. } | GcpError::Conflict { raw } => Some(raw),
            _ => None,
        }
    }

    pub fn status(&self) -> Option<u16> {
        match self {
            GcpError::Api { status, .. } => Some(*status),
            GcpError::Conflict { .. } => Some(409),
            _ => None,
        }
    }
}

/// Dựng `GcpError` từ HTTP status + body của Google API, kèm diễn giải.
///
/// `context` là mô tả ngắn việc đang làm ("sửa env của service checkout"), dùng để
/// message cuối cùng nói rõ thao tác nào thất bại.
pub fn from_http(status: u16, body: &str, context: &str) -> GcpError {
    let raw = extract_google_message(body).unwrap_or_else(|| truncate(body, 2000));

    // 409 luôn là conflict etag trong luồng của app này.
    if status == 409 {
        return GcpError::Conflict { raw };
    }

    let message = match status {
        401 => "Access token hết hạn hoặc không hợp lệ. App sẽ tự lấy token mới — nếu vẫn lỗi, chạy `gcloud auth login`.".to_string(),

        403 => explain_403(&raw, context),

        404 => format!(
            "Không tìm thấy tài nguyên khi {context}. Kiểm tra lại project ID và region — service có thể đã bị xoá hoặc nằm ở region khác."
        ),

        429 => format!(
            "Bị giới hạn tốc độ (quota) khi {context}. App sẽ tự thử lại. Nếu xảy ra liên tục, giảm tần suất auto-refresh trong Settings."
        ),

        400 => format!(
            "Yêu cầu không hợp lệ khi {context}. Thường là giá trị nhập sai định dạng (ví dụ memory phải là `512Mi`/`1Gi`, cpu phải là `1`/`2`/`0.5`). Chi tiết: {raw}"
        ),

        s if s >= 500 => format!(
            "Lỗi phía Google (HTTP {s}) khi {context}. Đây không phải lỗi cấu hình của bạn — thử lại sau ít phút."
        ),

        s => format!("Lỗi HTTP {s} khi {context}: {raw}"),
    };

    GcpError::Api {
        status,
        message,
        raw,
        retryable: matches!(status, 429 | 500 | 502 | 503 | 504),
    }
}

/// 403 là lỗi khó đoán nhất. Ba nhánh chính, mỗi nhánh cần hành động khác nhau.
fn explain_403(raw: &str, context: &str) -> String {
    let low = raw.to_ascii_lowercase();

    // Nhánh 1: thiếu actAs trên runtime service account.
    // Đây là lỗi mà `roles/run.developer` một mình KHÔNG giải quyết được, và là
    // lý do phổ biến nhất khiến "tôi có quyền developer mà vẫn không deploy được".
    if low.contains("actas")
        || low.contains("act as")
        || (low.contains("iam.serviceaccounts") && low.contains("permission"))
    {
        return format!(
            "Thiếu quyền `iam.serviceAccounts.actAs` trên runtime service account của service (khi {context}).\n\n\
             `roles/run.developer` một mình là KHÔNG đủ để tạo revision mới — bạn còn phải được phép \"đóng vai\" \
             service account mà service đang chạy dưới danh nghĩa.\n\n\
             Cách sửa — cấp `roles/iam.serviceAccountUser` trên đúng SA đó:\n\
             gcloud iam service-accounts add-iam-policy-binding RUNTIME_SA_EMAIL \\\n\
             \x20 --member=user:YOUR_EMAIL --role=roles/iam.serviceAccountUser --project=PROJECT_ID\n\n\
             Tên runtime SA xem ở tab Overview, dòng \"Service account\"."
        );
    }

    // Nhánh 2: API chưa enable (Google trả 403 chứ không phải 404 cho case này).
    if low.contains("has not been used in project")
        || low.contains("is disabled")
        || low.contains("service_disabled")
        || low.contains("api has not been")
    {
        return format!(
            "API cần dùng chưa được enable trên project (khi {context}).\n\n\
             Chạy: gcloud services enable run.googleapis.com monitoring.googleapis.com \
             logging.googleapis.com secretmanager.googleapis.com cloudresourcemanager.googleapis.com \
             --project=PROJECT_ID\n\n\
             Chi tiết: {raw}"
        );
    }

    // Nhánh 3: thiếu role thông thường.
    format!(
        "Không đủ quyền khi {context}.\n\n\
         Role tối thiểu cần có:\n\
         • Xem service/revision → roles/run.viewer\n\
         • Sửa env/scaling → roles/run.developer (+ actAs trên runtime SA)\n\
         • Xem metrics → roles/monitoring.viewer\n\
         • Xem log → roles/logging.viewer\n\
         • Xem secret metadata → roles/secretmanager.viewer\n\
         • Xem giá trị secret → roles/secretmanager.secretAccessor\n\n\
         Chi tiết từ Google: {raw}"
    )
}

/// Google trả lỗi dạng `{"error": {"code":403, "message":"...", "status":"..."}}`.
/// Móc lấy `message`; nếu body không phải JSON thì trả None để caller dùng raw body.
fn extract_google_message(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let err = v.get("error")?;
    // Một số API trả `error` là string thay vì object.
    if let Some(s) = err.as_str() {
        return Some(s.to_string());
    }
    let msg = err.get("message")?.as_str()?.to_string();

    // Gom thêm `details[].reason` nếu có — chỗ này chứa manh mối như SERVICE_DISABLED.
    let mut out = msg;
    if let Some(details) = err.get("details").and_then(|d| d.as_array()) {
        let reasons: Vec<&str> = details
            .iter()
            .filter_map(|d| d.get("reason").and_then(|r| r.as_str()))
            .collect();
        if !reasons.is_empty() {
            out.push_str(" [reason: ");
            out.push_str(&reasons.join(", "));
            out.push(']');
        }
    }
    Some(out)
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}… (đã cắt)", &s[..end])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conflict_409_thanh_loai_rieng_va_khong_retry() {
        let e = from_http(409, r#"{"error":{"code":409,"message":"etag mismatch"}}"#, "sửa env");
        assert!(matches!(e, GcpError::Conflict { .. }));
        assert!(!e.retryable(), "409 tuyệt đối không được auto-retry, sẽ ghi đè mất thay đổi của người khác");
        assert!(e.to_string().contains("Reload"));
    }

    #[test]
    fn phat_hien_loi_actas_va_huong_dan_cu_the() {
        let body = r#"{"error":{"code":403,"message":"Permission 'iam.serviceAccounts.actAs' denied on service account run-sa@p.iam.gserviceaccount.com"}}"#;
        let e = from_http(403, body, "tạo revision mới");
        let msg = e.to_string();
        assert!(msg.contains("iam.serviceAccountUser"), "phải chỉ ra role cần cấp: {msg}");
        assert!(msg.contains("add-iam-policy-binding"), "phải cho lệnh sửa cụ thể: {msg}");
    }

    #[test]
    fn phat_hien_api_chua_enable() {
        let body = r#"{"error":{"code":403,"message":"Cloud Run Admin API has not been used in project 123 before or it is disabled","details":[{"reason":"SERVICE_DISABLED"}]}}"#;
        let e = from_http(403, body, "liệt kê service");
        let msg = e.to_string();
        assert!(msg.contains("gcloud services enable"), "phải hướng dẫn enable API: {msg}");
    }

    #[test]
    fn loi_403_thong_thuong_liet_ke_role_can_thiet() {
        let body = r#"{"error":{"code":403,"message":"Permission denied on resource"}}"#;
        let e = from_http(403, body, "xem log");
        let msg = e.to_string();
        assert!(msg.contains("roles/logging.viewer"), "{msg}");
        // Không được rơi vào nhánh actAs: nhánh đó đưa ra lệnh sửa cụ thể, ở đây thì
        // chưa biết thiếu quyền gì nên chỉ liệt kê role. (Danh sách role vẫn nhắc actAs
        // như một dòng thông tin — đó là khác, nên kiểm tra bằng dấu hiệu riêng của nhánh.)
        assert!(
            !msg.contains("add-iam-policy-binding"),
            "không được nhầm sang nhánh actAs: {msg}"
        );
    }

    #[test]
    fn loi_429_va_5xx_duoc_danh_dau_retryable() {
        assert!(from_http(429, "{}", "x").retryable());
        assert!(from_http(503, "{}", "x").retryable());
        assert!(!from_http(400, "{}", "x").retryable());
        assert!(!from_http(403, "{}", "x").retryable());
        assert!(!from_http(404, "{}", "x").retryable());
    }

    #[test]
    fn gom_duoc_reason_trong_details() {
        let body = r#"{"error":{"message":"boom","details":[{"reason":"CONSUMER_INVALID"}]}}"#;
        let got = extract_google_message(body).unwrap();
        assert!(got.contains("CONSUMER_INVALID"), "{got}");
    }

    #[test]
    fn body_khong_phai_json_van_khong_panic() {
        let e = from_http(502, "<html>Bad Gateway</html>", "xem metrics");
        assert!(e.to_string().contains("Lỗi phía Google"));
    }

    #[test]
    fn truncate_khong_lam_vo_utf8() {
        // Cắt giữa ký tự tiếng Việt nhiều byte không được panic.
        let s = "à".repeat(3000);
        let out = truncate(&s, 1001);
        assert!(out.ends_with("(đã cắt)"));
    }
}
