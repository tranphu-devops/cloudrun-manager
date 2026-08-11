//! Xác thực bằng service account key (JWT self-signed → access token).
//!
//! Luồng chuẩn của Google cho SA key:
//!   1. Dựng JWT với `iss = client_email`, `aud = token_uri`, `scope = cloud-platform`
//!   2. Sign RS256 bằng private key trong file key
//!   3. POST `token_uri` với `grant_type=urn:ietf:params:oauth:grant-type:jwt-bearer`
//!   4. Nhận `access_token`
//!
//! # Vì sao tự viết thay vì dùng `gcp_auth` / `yup-oauth2`
//!
//! Đây là ~120 dòng, và nó nằm trên đường đi của credential. Cùng lý do v1 không dùng
//! `tauri-specta`: chỗ nào rủi ro thì kiểm soát trực tiếp, đừng giao cho dependency.
//!
//! # Bất biến
//!
//! `private_key` bọc trong `Secret` nên không rò qua `Debug`/log/panic. Struct này
//! **không** implement `Serialize` — không thể vô tình gửi ra frontend.

use base64::Engine;
use rsa::pkcs1v15::SigningKey;
use rsa::pkcs8::DecodePrivateKey;
use rsa::signature::{SignatureEncoding, Signer};
use rsa::RsaPrivateKey;
use serde::Deserialize;
use sha2::Sha256;

use crate::error::{GcpError, Result};
use crate::secret::Secret;

/// Scope duy nhất app cần. `cloud-platform` bao trọn Run/Monitoring/Logging/Secret
/// Manager/Recommender — xin hẹp hơn sẽ phải liệt kê 6 scope và dễ thiếu.
pub const SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";

const DEFAULT_TOKEN_URI: &str = "https://oauth2.googleapis.com/token";

/// JWT sống 1 giờ — mức tối đa Google nhận.
const JWT_TTL_SECS: i64 = 3600;

/// File SA key sau khi parse. Không derive `Serialize` (xem chú thích module).
#[derive(Clone)]
pub struct ServiceAccountKey {
    pub client_email: String,
    pub project_id: Option<String>,
    pub private_key_id: Option<String>,
    pub token_uri: String,
    private_key_pem: Secret,
}

impl std::fmt::Debug for ServiceAccountKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServiceAccountKey")
            .field("client_email", &self.client_email)
            .field("project_id", &self.project_id)
            .field("private_key_id", &self.private_key_id)
            .field("private_key_pem", &"[redacted]")
            .finish()
    }
}

#[derive(Deserialize)]
struct RawKey {
    #[serde(rename = "type")]
    key_type: Option<String>,
    client_email: Option<String>,
    private_key: Option<String>,
    private_key_id: Option<String>,
    project_id: Option<String>,
    token_uri: Option<String>,
}

impl ServiceAccountKey {
    /// Parse nội dung file JSON do GCP Console / `gcloud iam service-accounts keys create` sinh ra.
    ///
    /// Kiểm từng field và báo lỗi cụ thể thay vì "JSON không hợp lệ" — người dùng hay
    /// import nhầm file (ADC, OAuth client secret, key của project khác) và cần biết
    /// mình đưa sai cái gì.
    pub fn parse(json: &str) -> Result<Self> {
        let raw: RawKey = serde_json::from_str(json).map_err(|e| {
            GcpError::Invalid(format!(
                "File không phải JSON hợp lệ ({e}). Hãy chọn đúng file key do GCP sinh ra, \
                 không phải file đã bị sửa hay copy thiếu."
            ))
        })?;

        match raw.key_type.as_deref() {
            Some("service_account") => {}
            Some("authorized_user") => {
                return Err(GcpError::Invalid(
                    "Đây là file ADC (authorized_user) của tài khoản người dùng, không phải \
                     service account key. Nếu muốn dùng tài khoản cá nhân thì để app đi qua \
                     gcloud (không cần import gì)."
                        .to_string(),
                ))
            }
            Some(other) => {
                return Err(GcpError::Invalid(format!(
                    "File key có `type` là `{other}`, app chỉ nhận `service_account`."
                )))
            }
            None => {
                return Err(GcpError::Invalid(
                    "File thiếu field `type`. Có thể bạn đang chọn OAuth client secret \
                     (file có `installed`/`web`) thay vì service account key."
                        .to_string(),
                ))
            }
        }

        let client_email = raw.client_email.filter(|s| !s.trim().is_empty()).ok_or_else(|| {
            GcpError::Invalid("File key thiếu `client_email`.".to_string())
        })?;

        let pem = raw.private_key.filter(|s| !s.trim().is_empty()).ok_or_else(|| {
            GcpError::Invalid(
                "File key thiếu `private_key`. Key tải từ Console phải có field này — \
                 nếu không có thì file đã bị lược bớt."
                    .to_string(),
            )
        })?;

        if !pem.contains("PRIVATE KEY") {
            return Err(GcpError::Invalid(
                "`private_key` không có dạng PEM (`-----BEGIN PRIVATE KEY-----`)."
                    .to_string(),
            ));
        }

        let key = Self {
            client_email,
            project_id: raw.project_id,
            private_key_id: raw.private_key_id,
            token_uri: raw
                .token_uri
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| DEFAULT_TOKEN_URI.to_string()),
            private_key_pem: Secret::new(pem),
        };

        // Thử parse PEM ngay lúc import: sai định dạng thì phải biết bây giờ, không
        // phải lúc đang cần token.
        key.signing_key()?;
        Ok(key)
    }

    fn signing_key(&self) -> Result<SigningKey<Sha256>> {
        // Key của Google là PKCS#8. Chấp nhận cả PKCS#1 để không kén chọn vô ích.
        let pem = self.private_key_pem.expose();
        let rsa = RsaPrivateKey::from_pkcs8_pem(pem)
            .or_else(|_| {
                use rsa::pkcs1::DecodeRsaPrivateKey;
                RsaPrivateKey::from_pkcs1_pem(pem)
            })
            .map_err(|e| {
                GcpError::Invalid(format!(
                    "Không đọc được private key trong file (không phải RSA PKCS#8/PKCS#1 hợp lệ): {e}"
                ))
            })?;
        Ok(SigningKey::<Sha256>::new(rsa))
    }

    /// Dựng và sign JWT. `now_unix` truyền vào để test được mà không phụ thuộc đồng hồ.
    pub fn build_jwt(&self, now_unix: i64) -> Result<String> {
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;

        let mut header = serde_json::json!({ "alg": "RS256", "typ": "JWT" });
        if let Some(kid) = &self.private_key_id {
            header["kid"] = serde_json::json!(kid);
        }

        let claims = serde_json::json!({
            "iss": self.client_email,
            "scope": SCOPE,
            "aud": self.token_uri,
            "iat": now_unix,
            "exp": now_unix + JWT_TTL_SECS,
        });

        let signing_input = format!(
            "{}.{}",
            b64.encode(serde_json::to_vec(&header).unwrap_or_default()),
            b64.encode(serde_json::to_vec(&claims).unwrap_or_default())
        );

        let sig = self
            .signing_key()?
            .try_sign(signing_input.as_bytes())
            .map_err(|e| GcpError::Invalid(format!("Không sign được JWT: {e}")))?;

        Ok(format!("{signing_input}.{}", b64.encode(sig.to_bytes())))
    }

    /// Đổi JWT lấy access token.
    pub async fn fetch_token(&self, http: &reqwest::Client) -> Result<Secret> {
        let now = chrono::Utc::now().timestamp();
        let assertion = self.build_jwt(now)?;

        let resp = http
            .post(&self.token_uri)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", &assertion),
            ])
            .send()
            .await
            .map_err(|e| {
                GcpError::Network(format!("{e} (khi đổi JWT của service account lấy access token)"))
            })?;

        let status = resp.status().as_u16();
        let body = resp
            .text()
            .await
            .map_err(|e| GcpError::Network(e.to_string()))?;

        if status != 200 {
            return Err(explain_token_error(status, &body, &self.client_email));
        }

        #[derive(Deserialize)]
        struct TokenResp {
            access_token: String,
        }
        let t: TokenResp = serde_json::from_str(&body).map_err(|e| GcpError::Decode {
            api: "oauth2 token (service account)",
            source: e,
        })?;
        Ok(Secret::new(t.access_token))
    }
}

/// Diễn giải lỗi khi đổi JWT lấy token.
///
/// `invalid_grant` là bẫy khó chịu nhất: nó có ba nguyên nhân hoàn toàn khác nhau
/// (đồng hồ lệch, key đã bị xoá, SA bị disable) mà Google trả về cùng một mã. Đoán sai
/// nguyên nhân là mất cả buổi.
fn explain_token_error(status: u16, body: &str, client_email: &str) -> GcpError {
    let low = body.to_ascii_lowercase();

    if low.contains("invalid_grant") {
        return GcpError::Invalid(format!(
            "Google từ chối service account key (`invalid_grant`). Ba nguyên nhân, theo thứ tự \
             hay gặp:\n\n\
             1. **Đồng hồ máy lệch.** JWT bị coi là không hợp lệ nếu máy sai giờ quá ~5 phút. \
             Kiểm tra Cài đặt → Ngày và giờ → bật đồng bộ tự động. Đây là nguyên nhân phổ biến nhất.\n\
             2. **Key đã bị xoá** khỏi service account `{client_email}`. Kiểm tra bằng:\n\
             \x20  gcloud iam service-accounts keys list --iam-account={client_email}\n\
             3. **Service account đã bị disable hoặc xoá.**\n\n\
             Chi tiết từ Google: {}",
            truncate(body, 400)
        ));
    }

    if low.contains("invalid_scope") {
        return GcpError::Invalid(
            "Scope bị từ chối. Service account có thể đang bị giới hạn scope bởi domain-wide \
             policy của tổ chức."
                .to_string(),
        );
    }

    if low.contains("unauthorized_client") {
        return GcpError::Invalid(format!(
            "Service account `{client_email}` không được phép lấy token với scope này. \
             Thường là do policy `constraints/iam.disableServiceAccountKeyCreation` hoặc \
             service account bị chặn ở cấp organization."
        ));
    }

    crate::error::from_http(status, body, "lấy access token bằng service account key")
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use rsa::pkcs8::EncodePrivateKey;

    /// Key RSA 2048 thật, sinh **một lần** cho cả file test.
    ///
    /// Sinh key 2048 ở bản debug mất vài giây; gọi lại trong mỗi test thì bộ test thành
    /// vô dụng vì quá chậm. Không hardcode PEM sẵn vì một PEM nằm trong repo sẽ làm mọi
    /// secret scanner báo động, dù nó chỉ là key rác.
    fn test_key() -> &'static RsaPrivateKey {
        static KEY: std::sync::OnceLock<RsaPrivateKey> = std::sync::OnceLock::new();
        KEY.get_or_init(|| {
            let mut rng = rand::thread_rng();
            RsaPrivateKey::new(&mut rng, 2048).expect("sinh key test")
        })
    }

    /// Dựng SA key JSON thật để test toàn bộ đường parse → sign → verify.
    fn make_key_json(overrides: serde_json::Value) -> (String, RsaPrivateKey) {
        let priv_key = test_key().clone();
        let pem = priv_key
            .to_pkcs8_pem(rsa::pkcs8::LineEnding::LF)
            .expect("pkcs8 pem")
            .to_string();

        let mut v = serde_json::json!({
            "type": "service_account",
            "project_id": "example-project",
            "private_key_id": "abc123",
            "private_key": pem,
            "client_email": "cockpit@example-project.iam.gserviceaccount.com",
            "token_uri": "https://oauth2.googleapis.com/token",
        });
        if let (Some(base), Some(ov)) = (v.as_object_mut(), overrides.as_object()) {
            for (k, val) in ov {
                if val.is_null() {
                    base.remove(k);
                } else {
                    base.insert(k.clone(), val.clone());
                }
            }
        }
        (v.to_string(), priv_key)
    }

    #[test]
    fn parse_key_hop_le() {
        let (json, _) = make_key_json(serde_json::json!({}));
        let k = ServiceAccountKey::parse(&json).unwrap();
        assert_eq!(k.client_email, "cockpit@example-project.iam.gserviceaccount.com");
        assert_eq!(k.project_id.as_deref(), Some("example-project"));
        assert_eq!(k.token_uri, "https://oauth2.googleapis.com/token");
    }

    #[test]
    fn debug_khong_lam_ro_private_key() {
        let (json, _) = make_key_json(serde_json::json!({}));
        let k = ServiceAccountKey::parse(&json).unwrap();
        let dbg = format!("{k:?}");
        assert!(!dbg.contains("PRIVATE KEY"), "Debug rò PEM: {dbg}");
        assert!(!dbg.contains("BEGIN"), "Debug rò PEM: {dbg}");
        assert!(dbg.contains("redacted"));
        // Field không nhạy cảm vẫn phải in ra để debug được.
        assert!(dbg.contains("cockpit@example-project"));
    }

    #[test]
    fn tu_choi_file_adc_voi_message_ro_rang() {
        let json = r#"{"type":"authorized_user","client_id":"x","client_secret":"y","refresh_token":"z"}"#;
        let err = ServiceAccountKey::parse(json).unwrap_err().to_string();
        assert!(err.contains("ADC"), "{err}");
        assert!(err.contains("gcloud"), "phải chỉ ra đường đi thay thế: {err}");
    }

    #[test]
    fn tu_choi_oauth_client_secret() {
        let json = r#"{"installed":{"client_id":"x","client_secret":"y"}}"#;
        let err = ServiceAccountKey::parse(json).unwrap_err().to_string();
        assert!(err.contains("OAuth client secret"), "{err}");
    }

    #[test]
    fn tu_choi_key_thieu_private_key() {
        let (json, _) = make_key_json(serde_json::json!({ "private_key": null }));
        let err = ServiceAccountKey::parse(&json).unwrap_err().to_string();
        assert!(err.contains("private_key"), "{err}");
    }

    #[test]
    fn tu_choi_private_key_khong_phai_pem() {
        let (json, _) = make_key_json(serde_json::json!({ "private_key": "không phải pem" }));
        let err = ServiceAccountKey::parse(&json).unwrap_err().to_string();
        assert!(err.contains("PEM"), "{err}");
    }

    #[test]
    fn tu_choi_pem_dung_dinh_dang_nhung_hong_ruot() {
        // Có chuỗi "PRIVATE KEY" nhưng base64 bên trong là rác — phải bị bắt ngay lúc
        // parse, không đợi tới lúc sign.
        let bad = "-----BEGIN PRIVATE KEY-----\nrác rác rác\n-----END PRIVATE KEY-----\n";
        let (json, _) = make_key_json(serde_json::json!({ "private_key": bad }));
        let err = ServiceAccountKey::parse(&json).unwrap_err().to_string();
        assert!(err.contains("private key"), "{err}");
    }

    #[test]
    fn token_uri_mac_dinh_khi_file_thieu() {
        let (json, _) = make_key_json(serde_json::json!({ "token_uri": null }));
        let k = ServiceAccountKey::parse(&json).unwrap();
        assert_eq!(k.token_uri, "https://oauth2.googleapis.com/token");
    }

    #[test]
    fn jwt_co_dung_ba_phan_va_claims_dung() {
        let (json, _) = make_key_json(serde_json::json!({}));
        let k = ServiceAccountKey::parse(&json).unwrap();
        let jwt = k.build_jwt(1_785_900_000).unwrap();

        let parts: Vec<&str> = jwt.split('.').collect();
        assert_eq!(parts.len(), 3, "JWT phải có 3 phần");

        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let header: serde_json::Value =
            serde_json::from_slice(&b64.decode(parts[0]).unwrap()).unwrap();
        let claims: serde_json::Value =
            serde_json::from_slice(&b64.decode(parts[1]).unwrap()).unwrap();

        assert_eq!(header["alg"], "RS256");
        assert_eq!(header["typ"], "JWT");
        assert_eq!(header["kid"], "abc123", "kid phải lấy từ private_key_id");

        assert_eq!(claims["iss"], "cockpit@example-project.iam.gserviceaccount.com");
        assert_eq!(claims["aud"], "https://oauth2.googleapis.com/token");
        assert_eq!(claims["scope"], SCOPE);
        assert_eq!(claims["iat"], 1_785_900_000);
        assert_eq!(
            claims["exp"].as_i64().unwrap() - claims["iat"].as_i64().unwrap(),
            3600,
            "JWT phải sống đúng 1 giờ — Google từ chối nếu dài hơn"
        );
    }

    #[test]
    fn jwt_khong_co_kid_khi_key_thieu_private_key_id() {
        let (json, _) = make_key_json(serde_json::json!({ "private_key_id": null }));
        let k = ServiceAccountKey::parse(&json).unwrap();
        let jwt = k.build_jwt(0).unwrap();
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let header: serde_json::Value =
            serde_json::from_slice(&b64.decode(jwt.split('.').next().unwrap()).unwrap()).unwrap();
        assert!(header.get("kid").is_none(), "không được thêm kid rỗng");
    }

    #[test]
    fn signature_verify_duoc_bang_public_key() {
        // Kiểm chính chiều sign: nếu signature sai thuật toán hoặc sai input thì
        // Google sẽ trả invalid_grant và rất khó truy. Verify tại đây rẻ hơn nhiều.
        use rsa::pkcs1v15::VerifyingKey;
        use rsa::signature::Verifier;

        let (json, priv_key) = make_key_json(serde_json::json!({}));
        let k = ServiceAccountKey::parse(&json).unwrap();
        let jwt = k.build_jwt(1_785_900_000).unwrap();

        let (signing_input, sig_b64) = jwt.rsplit_once('.').unwrap();
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let sig_bytes = b64.decode(sig_b64).unwrap();

        let vk = VerifyingKey::<Sha256>::new(priv_key.to_public_key());
        let sig = rsa::pkcs1v15::Signature::try_from(sig_bytes.as_slice()).unwrap();
        assert!(
            vk.verify(signing_input.as_bytes(), &sig).is_ok(),
            "signature không verify được bằng public key tương ứng"
        );
    }

    #[test]
    fn invalid_grant_noi_ve_dong_ho_truoc_tien() {
        let body = r#"{"error":"invalid_grant","error_description":"Invalid JWT: Token must be a short-lived token"}"#;
        let err = explain_token_error(400, body, "sa@p.iam.gserviceaccount.com").to_string();
        assert!(err.contains("Đồng hồ máy lệch"), "{err}");
        assert!(
            err.contains("keys list"),
            "phải cho lệnh kiểm tra key còn tồn tại: {err}"
        );
        // Nguyên nhân hay gặp nhất phải đứng trước.
        let pos_clock = err.find("Đồng hồ").unwrap();
        let pos_deleted = err.find("bị xoá").unwrap();
        assert!(pos_clock < pos_deleted, "thứ tự nguyên nhân bị đảo");
    }

    #[test]
    fn unauthorized_client_noi_ve_policy_to_chuc() {
        let body = r#"{"error":"unauthorized_client"}"#;
        let err = explain_token_error(401, body, "sa@p.iam.gserviceaccount.com").to_string();
        assert!(err.contains("organization") || err.contains("policy"), "{err}");
    }

    #[test]
    fn truncate_khong_lam_vo_utf8() {
        let s = "à".repeat(500);
        let out = truncate(&s, 101);
        assert!(out.ends_with('…'));
    }
}
