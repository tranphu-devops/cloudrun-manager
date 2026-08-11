//! Lấy access token cho GCP API.
//!
//! Thứ tự ưu tiên:
//!   1. `gcloud auth print-access-token` — thừa hưởng luôn account đang active và cả
//!      cấu hình `auth/impersonate_service_account` nếu team đang dùng.
//!   2. ADC (`application_default_credentials.json`) dạng `authorized_user` — refresh
//!      token grant trực tiếp, dùng khi máy không có gcloud trong PATH.
//!
//! Service account: KHÔNG cần code riêng. Chạy
//! `gcloud auth activate-service-account --key-file=key.json` rồi app đi qua nhánh (1)
//! là xong — `print-access-token` sẽ trả token của SA. Nhờ vậy v1 không phải nhúng
//! thư viện crypto để tự sign JWT, và cũng không phải giữ file key trong app.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Deserialize;
use tokio::sync::{OnceCell, RwLock};

use crate::error::{GcpError, Result};
use crate::sa::ServiceAccountKey;
use crate::secret::Secret;

/// Token GCP sống 60 phút. Cache 50 phút để luôn còn biên an toàn 10 phút.
const TOKEN_TTL: Duration = Duration::from_secs(50 * 60);

/// gcloud khởi động Python nên khá chậm; 20s là đủ rộng cho máy đang tải nặng
/// mà vẫn không treo UI vô hạn.
const GCLOUD_TIMEOUT: Duration = Duration::from_secs(20);

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TokenSource {
    /// Service account key đã được mở khoá từ vault. Ưu tiên cao nhất.
    ServiceAccount,
    GcloudCli,
    Adc,
}

/// Thông tin hiển thị trên thanh trạng thái: đang chạy dưới danh nghĩa ai.
/// Quan trọng khi team dùng impersonation — người dùng phải biết mình đang là ai
/// trước khi bấm sửa gì trên prod.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthInfo {
    pub account: String,
    pub source: TokenSource,
    /// `Some(sa_email)` nếu `gcloud config get-value auth/impersonate_service_account` có giá trị.
    pub impersonating: Option<String>,
    /// Project mặc định theo cấu hình gcloud (hoặc project_id trong SA key).
    pub default_project: Option<String>,
    pub gcloud_path: Option<String>,
    /// `true` khi đang dùng service account. Frontend hiện badge khác nhau cho hai
    /// trường hợp — người dùng phải biết mình đang là ai trước khi bấm sửa.
    pub using_service_account: bool,
}

impl AuthInfo {
    /// Danh tính hiệu lực thật sự khi gọi API.
    pub fn effective_identity(&self) -> &str {
        self.impersonating.as_deref().unwrap_or(&self.account)
    }
}

struct CachedToken {
    token: Secret,
    fetched_at: Instant,
}

pub struct TokenProvider {
    cached: RwLock<Option<CachedToken>>,
    gcloud: OnceCell<Option<PathBuf>>,
    http: reqwest::Client,
    /// Service account đang mở khoá. `None` = chưa import hoặc vault đang khoá,
    /// khi đó rơi về gcloud.
    sa: RwLock<Option<ServiceAccountKey>>,
}

impl TokenProvider {
    pub fn new(http: reqwest::Client) -> Self {
        Self {
            cached: RwLock::new(None),
            gcloud: OnceCell::new(),
            http,
            sa: RwLock::new(None),
        }
    }

    /// Trả token còn hiệu lực, lấy mới nếu cần.
    pub async fn token(&self) -> Result<Secret> {
        if let Some(c) = self.cached.read().await.as_ref() {
            if c.fetched_at.elapsed() < TOKEN_TTL {
                return Ok(c.token.clone());
            }
        }

        // Nhiều request song song có thể cùng vào đây; giữ write lock trong lúc fetch
        // để chỉ đúng một lần gọi gcloud, tránh spawn 7 process cùng lúc khi
        // trang Metrics load 7 chart một phát.
        let mut guard = self.cached.write().await;
        if let Some(c) = guard.as_ref() {
            if c.fetched_at.elapsed() < TOKEN_TTL {
                return Ok(c.token.clone());
            }
        }

        let token = self.fetch_fresh().await?;
        *guard = Some(CachedToken {
            token: token.clone(),
            fetched_at: Instant::now(),
        });
        Ok(token)
    }

    /// Xoá cache. Gọi khi API trả 401 để lần sau lấy token mới.
    pub async fn invalidate(&self) {
        *self.cached.write().await = None;
    }

    /// Nạp / bỏ service account. Bỏ luôn token đang cache vì token cũ thuộc danh tính cũ —
    /// giữ lại là đường dẫn tới việc thao tác dưới danh nghĩa không như UI đang hiện.
    pub async fn set_service_account(&self, key: Option<ServiceAccountKey>) {
        *self.sa.write().await = key;
        self.invalidate().await;
    }

    pub async fn has_service_account(&self) -> bool {
        self.sa.read().await.is_some()
    }

    pub async fn service_account_email(&self) -> Option<String> {
        self.sa.read().await.as_ref().map(|k| k.client_email.clone())
    }

    async fn fetch_fresh(&self) -> Result<Secret> {
        // Service account đứng trước gcloud: đã import và mở khoá thì đó là ý muốn rõ ràng.
        if let Some(key) = self.sa.read().await.as_ref() {
            return key.fetch_token(&self.http).await;
        }

        match self.gcloud_path().await {
            Some(path) => {
                let out = self
                    .run_gcloud(path, &["auth", "print-access-token"])
                    .await?;
                let t = out.trim();
                if t.is_empty() {
                    return Err(GcpError::NotAuthenticated);
                }
                Ok(Secret::new(t))
            }
            None => self.token_from_adc().await,
        }
    }

    /// Đường dẫn gcloud, resolve một lần rồi cache.
    async fn gcloud_path(&self) -> Option<PathBuf> {
        self.gcloud
            .get_or_init(|| async { resolve_gcloud() })
            .await
            .clone()
    }

    async fn run_gcloud(&self, path: PathBuf, args: &[&str]) -> Result<String> {
        let mut cmd = tokio::process::Command::new(&path);
        cmd.args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        // Không có cờ này thì mỗi lần refresh token sẽ nháy một cửa sổ console đen
        // trên Windows — với auto-refresh mỗi 30s thì không dùng nổi.
        #[cfg(windows)]
        {
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let fut = cmd.output();
        let out = match tokio::time::timeout(GCLOUD_TIMEOUT, fut).await {
            Err(_) => {
                return Err(GcpError::GcloudFailed(format!(
                    "gcloud không phản hồi trong {}s (lệnh: gcloud {}). Thử chạy tay lệnh này trong terminal để xem nó treo ở đâu.",
                    GCLOUD_TIMEOUT.as_secs(),
                    args.join(" ")
                )))
            }
            Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(GcpError::GcloudNotFound)
            }
            Ok(Err(e)) => return Err(GcpError::GcloudFailed(e.to_string())),
            Ok(Ok(o)) => o,
        };

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let low = stderr.to_ascii_lowercase();
            if low.contains("do not have valid credentials")
                || low.contains("reauthentication")
                || low.contains("please run")
                || low.contains("credentials were not found")
            {
                return Err(GcpError::NotAuthenticated);
            }
            return Err(GcpError::GcloudFailed(stderr.trim().to_string()));
        }

        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// `gcloud config get-value X` trả về chuỗi "(unset)" khi chưa set — phải lọc,
    /// nếu không sẽ đi hỏi API với project tên là "(unset)".
    async fn gcloud_config(&self, path: &Path, key: &str) -> Option<String> {
        let out = self
            .run_gcloud(path.to_path_buf(), &["config", "get-value", key])
            .await
            .ok()?;
        normalize_config_value(&out)
    }

    pub async fn auth_info(&self) -> Result<AuthInfo> {
        if let Some(key) = self.sa.read().await.as_ref() {
            return Ok(AuthInfo {
                account: key.client_email.clone(),
                source: TokenSource::ServiceAccount,
                // SA không impersonate ai — nó chính là danh tính.
                impersonating: None,
                default_project: key.project_id.clone(),
                gcloud_path: None,
                using_service_account: true,
            });
        }

        match self.gcloud_path().await {
            Some(path) => {
                let account = self
                    .gcloud_config(&path, "account")
                    .await
                    .ok_or(GcpError::NotAuthenticated)?;
                Ok(AuthInfo {
                    account,
                    source: TokenSource::GcloudCli,
                    impersonating: self
                        .gcloud_config(&path, "auth/impersonate_service_account")
                        .await,
                    default_project: self.gcloud_config(&path, "project").await,
                    gcloud_path: Some(path.display().to_string()),
                    using_service_account: false,
                })
            }
            None => {
                let adc = load_adc()?;
                Ok(AuthInfo {
                    account: adc.describe_account(),
                    source: TokenSource::Adc,
                    impersonating: None,
                    default_project: adc.quota_project_id.clone(),
                    gcloud_path: None,
                    using_service_account: false,
                })
            }
        }
    }

    async fn token_from_adc(&self) -> Result<Secret> {
        let adc = load_adc()?;
        let refresh_token = adc
            .refresh_token
            .as_deref()
            .ok_or_else(|| match adc.cred_type.as_deref() {
                Some("service_account") => GcpError::Invalid(
                    "File ADC là service account key. App v1 không tự sign JWT — hãy nạp key vào gcloud rồi mở lại app:\n\n\
                     gcloud auth activate-service-account --key-file=đường-dẫn-key.json\n\n\
                     Sau đó `gcloud auth print-access-token` sẽ trả token của SA và app dùng được ngay."
                        .to_string(),
                ),
                _ => GcpError::NotAuthenticated,
            })?;

        let client_id = adc.client_id.as_deref().ok_or(GcpError::NotAuthenticated)?;
        let client_secret = adc
            .client_secret
            .as_deref()
            .ok_or(GcpError::NotAuthenticated)?;

        let resp = self
            .http
            .post("https://oauth2.googleapis.com/token")
            .form(&[
                ("client_id", client_id),
                ("client_secret", client_secret),
                ("refresh_token", refresh_token),
                ("grant_type", "refresh_token"),
            ])
            .send()
            .await
            .map_err(|e| GcpError::Network(e.to_string()))?;

        let status = resp.status().as_u16();
        let body = resp
            .text()
            .await
            .map_err(|e| GcpError::Network(e.to_string()))?;

        if status != 200 {
            return Err(crate::error::from_http(
                status,
                &body,
                "đổi refresh token của ADC lấy access token",
            ));
        }

        #[derive(Deserialize)]
        struct TokenResp {
            access_token: String,
        }
        let t: TokenResp = serde_json::from_str(&body).map_err(|e| GcpError::Decode {
            api: "oauth2 token",
            source: e,
        })?;
        Ok(Secret::new(t.access_token))
    }
}

// ---------------------------------------------------------------------------
// Resolve gcloud
// ---------------------------------------------------------------------------

/// Tên file gcloud khả dĩ, theo thứ tự ưu tiên.
///
/// Trên Windows, Cloud SDK cài ra `gcloud.cmd` (batch wrapper gọi Python), KHÔNG có
/// `gcloud` không đuôi. `Command::new("gcloud")` vì thế fail sạch — đây là lỗi số 1
/// khiến app kiểu này chết ngay bước đầu trên Windows.
fn gcloud_candidates() -> &'static [&'static str] {
    #[cfg(windows)]
    {
        &["gcloud.cmd", "gcloud.exe", "gcloud.bat", "gcloud"]
    }
    #[cfg(not(windows))]
    {
        &["gcloud"]
    }
}

/// Các thư mục cài đặt mặc định, dò thêm khi PATH không có (hay gặp khi app được
/// khởi chạy từ shortcut chứ không từ terminal — PATH có thể khác).
fn extra_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    #[cfg(windows)]
    {
        let sdk_tail = Path::new("Google")
            .join("Cloud SDK")
            .join("google-cloud-sdk")
            .join("bin");
        for var in ["LOCALAPPDATA", "APPDATA", "ProgramFiles", "ProgramFiles(x86)"] {
            if let Ok(base) = std::env::var(var) {
                dirs.push(Path::new(&base).join(&sdk_tail));
            }
        }
        if let Ok(up) = std::env::var("USERPROFILE") {
            dirs.push(Path::new(&up).join("google-cloud-sdk").join("bin"));
            dirs.push(
                Path::new(&up)
                    .join("AppData")
                    .join("Local")
                    .join("Google")
                    .join("Cloud SDK")
                    .join("google-cloud-sdk")
                    .join("bin"),
            );
        }
    }

    #[cfg(not(windows))]
    {
        if let Ok(home) = std::env::var("HOME") {
            dirs.push(Path::new(&home).join("google-cloud-sdk").join("bin"));
        }
        dirs.push(PathBuf::from("/usr/lib/google-cloud-sdk/bin"));
        dirs.push(PathBuf::from("/usr/local/bin"));
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
        dirs.push(PathBuf::from("/opt/homebrew/share/google-cloud-sdk/bin"));
        dirs.push(PathBuf::from("/snap/bin"));
    }

    dirs
}

fn resolve_gcloud() -> Option<PathBuf> {
    let path_dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();

    for dir in path_dirs.into_iter().chain(extra_search_dirs()) {
        for name in gcloud_candidates() {
            let cand = dir.join(name);
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// ADC
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Default)]
pub struct AdcFile {
    #[serde(rename = "type")]
    pub cred_type: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub refresh_token: Option<String>,
    pub quota_project_id: Option<String>,
    /// Có ở service account key.
    pub client_email: Option<String>,
}

impl AdcFile {
    fn describe_account(&self) -> String {
        self.client_email
            .clone()
            .unwrap_or_else(|| "ADC (authorized_user)".to_string())
    }
}

pub fn adc_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("GOOGLE_APPLICATION_CREDENTIALS") {
        if !p.trim().is_empty() {
            return Some(PathBuf::from(p));
        }
    }

    #[cfg(windows)]
    {
        let base = std::env::var("APPDATA").ok()?;
        Some(
            Path::new(&base)
                .join("gcloud")
                .join("application_default_credentials.json"),
        )
    }
    #[cfg(not(windows))]
    {
        let home = std::env::var("HOME").ok()?;
        Some(
            Path::new(&home)
                .join(".config")
                .join("gcloud")
                .join("application_default_credentials.json"),
        )
    }
}

fn load_adc() -> Result<AdcFile> {
    let path = adc_path().ok_or(GcpError::GcloudNotFound)?;
    let raw = std::fs::read_to_string(&path).map_err(|_| GcpError::GcloudNotFound)?;
    serde_json::from_str(&raw).map_err(|e| GcpError::Decode {
        api: "application_default_credentials.json",
        source: e,
    })
}

/// `gcloud config get-value` in ra `(unset)` (kèm cảnh báo ở stderr) khi key chưa set.
/// Không lọc thì app sẽ đi query project tên `(unset)` và nhận 403 khó hiểu.
fn normalize_config_value(raw: &str) -> Option<String> {
    let v = raw.trim();
    if v.is_empty() || v == "(unset)" || v.eq_ignore_ascii_case("none") {
        None
    } else {
        Some(v.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loc_duoc_gia_tri_unset_cua_gcloud_config() {
        assert_eq!(normalize_config_value("(unset)\n"), None);
        assert_eq!(normalize_config_value("  \n"), None);
        assert_eq!(normalize_config_value("None"), None);
        assert_eq!(
            normalize_config_value("example-project\n"),
            Some("example-project".to_string())
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_uu_tien_gcloud_cmd() {
        // Đây là bug chí tử trên Windows: Cloud SDK không ship `gcloud` không đuôi.
        assert_eq!(gcloud_candidates()[0], "gcloud.cmd");
    }

    #[test]
    fn candidates_khong_rong() {
        assert!(!gcloud_candidates().is_empty());
    }

    #[test]
    fn adc_service_account_key_duoc_nhan_dien() {
        let json = r#"{"type":"service_account","client_email":"sa@p.iam.gserviceaccount.com","private_key":"x"}"#;
        let adc: AdcFile = serde_json::from_str(json).unwrap();
        assert_eq!(adc.cred_type.as_deref(), Some("service_account"));
        assert!(adc.refresh_token.is_none());
        assert_eq!(adc.describe_account(), "sa@p.iam.gserviceaccount.com");
    }

    #[test]
    fn adc_authorized_user_parse_duoc() {
        let json = r#"{"type":"authorized_user","client_id":"cid","client_secret":"cs","refresh_token":"rt","quota_project_id":"example-project"}"#;
        let adc: AdcFile = serde_json::from_str(json).unwrap();
        assert_eq!(adc.refresh_token.as_deref(), Some("rt"));
        assert_eq!(adc.quota_project_id.as_deref(), Some("example-project"));
    }

    #[test]
    fn effective_identity_uu_tien_impersonation() {
        let mut info = AuthInfo {
            account: "you@example.com".into(),
            source: TokenSource::GcloudCli,
            impersonating: None,
            default_project: None,
            gcloud_path: None,
            using_service_account: false,
        };
        assert_eq!(info.effective_identity(), "you@example.com");

        info.impersonating = Some("deployer@example-project.iam.gserviceaccount.com".into());
        assert_eq!(
            info.effective_identity(),
            "deployer@example-project.iam.gserviceaccount.com",
            "khi impersonate thì danh tính hiệu lực phải là SA, không phải user"
        );
    }
}
