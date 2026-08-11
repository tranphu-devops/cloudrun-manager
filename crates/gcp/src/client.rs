//! HTTP client dùng chung cho mọi GCP API: gắn bearer token, retry, cache, map lỗi.

use std::sync::Arc;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::auth::{AuthInfo, TokenProvider};
use crate::cache::Cache;
use crate::error::{from_http, GcpError, Result};

const USER_AGENT: &str = concat!("cloud-run-cockpit/", env!("CARGO_PKG_VERSION"));
const REQUEST_TIMEOUT: Duration = Duration::from_secs(45);

/// Backoff cho 429/5xx. Không cần jitter: đây là app desktop một người dùng,
/// không có bầy client cùng đập vào một endpoint.
const BACKOFF: [Duration; 3] = [
    Duration::from_millis(400),
    Duration::from_millis(1200),
    Duration::from_millis(3000),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    /// POST nhưng bản chất là đọc (`projects:search`, `entries:list`) — retry được.
    Post,
    /// POST có tác dụng phụ và **không idempotent** (`jobs:run`, `scheduler:pause`).
    /// Retry một cái như thế có thể tạo hai execution của cùng một job batch.
    PostWrite,
    Patch,
}

impl Method {
    fn is_write(self) -> bool {
        matches!(self, Method::Patch | Method::PostWrite)
    }
}

pub struct GcpClient {
    http: reqwest::Client,
    pub auth: Arc<TokenProvider>,
    pub cache: Cache,
}

impl GcpClient {
    pub fn new() -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|e| GcpError::Network(e.to_string()))?;
        Ok(Self {
            auth: Arc::new(TokenProvider::new(http.clone())),
            http,
            cache: Cache::new(),
        })
    }

    pub async fn auth_info(&self) -> Result<AuthInfo> {
        self.auth.auth_info().await
    }

    /// GET + cache theo `cache_key`. `ttl` = 0 nghĩa là không cache.
    pub async fn get_cached<T: DeserializeOwned>(
        &self,
        url: &str,
        ctx: &str,
        cache_key: &str,
        ttl: Duration,
    ) -> Result<T> {
        if !ttl.is_zero() {
            if let Some(hit) = self.cache.get(cache_key).await {
                return decode(&hit, ctx);
            }
        }
        let body = self.send(Method::Get, url, None, ctx).await?;
        if !ttl.is_zero() {
            self.cache.put(cache_key.to_string(), body.as_str(), ttl).await;
        }
        decode(&body, ctx)
    }

    pub async fn get<T: DeserializeOwned>(&self, url: &str, ctx: &str) -> Result<T> {
        let body = self.send(Method::Get, url, None, ctx).await?;
        decode(&body, ctx)
    }

    pub async fn post<T: DeserializeOwned>(
        &self,
        url: &str,
        payload: &Value,
        ctx: &str,
    ) -> Result<T> {
        let body = self.send(Method::Post, url, Some(payload), ctx).await?;
        decode(&body, ctx)
    }

    /// POST + cache. Một số API (Resource Manager `projects:search`,
    /// Logging `entries:list`) chỉ có dạng POST nhưng bản chất là đọc.
    pub async fn post_cached<T: DeserializeOwned>(
        &self,
        url: &str,
        payload: &Value,
        ctx: &str,
        cache_key: &str,
        ttl: Duration,
    ) -> Result<T> {
        if !ttl.is_zero() {
            if let Some(hit) = self.cache.get(cache_key).await {
                return decode(&hit, ctx);
            }
        }
        let body = self.send(Method::Post, url, Some(payload), ctx).await?;
        if !ttl.is_zero() {
            self.cache.put(cache_key.to_string(), body.as_str(), ttl).await;
        }
        decode(&body, ctx)
    }

    /// POST cho thao tác có tác dụng phụ. **Không retry** kể cả khi lỗi mạng: request có
    /// thể đã tới server và tạo execution rồi mới đứt kết nối.
    pub async fn post_no_retry<T: DeserializeOwned>(
        &self,
        url: &str,
        payload: &Value,
        ctx: &str,
    ) -> Result<T> {
        let body = self.send(Method::PostWrite, url, Some(payload), ctx).await?;
        decode(&body, ctx)
    }

    pub async fn patch<T: DeserializeOwned>(
        &self,
        url: &str,
        payload: &Value,
        ctx: &str,
    ) -> Result<T> {
        let body = self.send(Method::Patch, url, Some(payload), ctx).await?;
        decode(&body, ctx)
    }

    async fn send(
        &self,
        method: Method,
        url: &str,
        payload: Option<&Value>,
        ctx: &str,
    ) -> Result<String> {
        let mut attempt = 0usize;
        let mut refreshed_once = false;

        loop {
            let token = self.auth.token().await?;

            let mut req = match method {
                Method::Get => self.http.get(url),
                Method::Post | Method::PostWrite => self.http.post(url),
                Method::Patch => self.http.patch(url),
            }
            .bearer_auth(token.expose());

            if let Some(p) = payload {
                req = req.json(p);
            }

            let resp = match req.send().await {
                Ok(r) => r,
                Err(e) => {
                    // Lỗi mạng: retry cho GET/POST-đọc, nhưng KHÔNG retry PATCH.
                    // PATCH có thể đã tới server và tạo revision rồi mới đứt kết nối;
                    // gửi lại là rủi ro tạo revision trùng.
                    if !method.is_write() && attempt < BACKOFF.len() {
                        tokio::time::sleep(BACKOFF[attempt]).await;
                        attempt += 1;
                        continue;
                    }
                    return Err(GcpError::Network(format!(
                        "{e} (khi {ctx}). Kiểm tra kết nối mạng / VPN / proxy."
                    )));
                }
            };

            let status = resp.status().as_u16();
            let body = resp
                .text()
                .await
                .map_err(|e| GcpError::Network(format!("{e} (khi đọc response của: {ctx})")))?;

            if (200..300).contains(&status) {
                return Ok(body);
            }

            // Token hết hạn giữa đường: bỏ cache, lấy token mới, thử lại đúng 1 lần.
            // An toàn cả với PATCH vì request trước chắc chắn bị từ chối, chưa đổi gì.
            if status == 401 && !refreshed_once {
                self.auth.invalidate().await;
                refreshed_once = true;
                continue;
            }

            let err = from_http(status, &body, ctx);
            if err.retryable() && !method.is_write() && attempt < BACKOFF.len() {
                tokio::time::sleep(BACKOFF[attempt]).await;
                attempt += 1;
                continue;
            }
            return Err(err);
        }
    }
}

fn decode<T: DeserializeOwned>(body: &str, ctx: &str) -> Result<T> {
    // Response rỗng (ví dụ 200 với body trống) — coi như `null` để type Option/unit
    // vẫn parse được thay vì lỗi "EOF while parsing".
    let src = if body.trim().is_empty() { "null" } else { body };
    serde_json::from_str(src).map_err(|e| {
        tracing::warn!(ctx, "không parse được response JSON");
        GcpError::Decode {
            api: leak_ctx(ctx),
            source: e,
        }
    })
}

/// `GcpError::Decode.api` là `&'static str` cho gọn ở chỗ khai báo; ctx là chuỗi động
/// nên phải leak. Số lượng ctx khác nhau là hữu hạn và nhỏ (mỗi endpoint một cái),
/// nên leak ở đây không tạo rò rỉ tăng dần.
fn leak_ctx(ctx: &str) -> &'static str {
    Box::leak(ctx.to_string().into_boxed_str())
}

/// Escape một segment để nhúng vào URL path.
pub fn seg(s: &str) -> String {
    urlencoding::encode(s).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_va_post_write_duoc_coi_la_write() {
        assert!(Method::Patch.is_write());
        // `jobs:run` là POST nhưng không idempotent — retry sẽ tạo execution thứ hai.
        assert!(
            Method::PostWrite.is_write(),
            "PostWrite phải bị chặn retry, nếu không jobs:run có thể chạy job hai lần"
        );
        assert!(!Method::Get.is_write());
        assert!(!Method::Post.is_write(), "POST-đọc vẫn retry được");
    }

    #[test]
    fn decode_body_rong_thanh_null() {
        let v: Option<u32> = decode("", "test").unwrap();
        assert!(v.is_none());
        let v: Option<u32> = decode("   \n", "test").unwrap();
        assert!(v.is_none());
    }

    #[test]
    fn decode_bao_loi_khi_json_sai() {
        let r: Result<Value> = decode("{not json", "xem service");
        assert!(matches!(r, Err(GcpError::Decode { .. })));
    }

    #[test]
    fn seg_escape_ky_tu_dac_biet() {
        // logName của Cloud Run chứa `/` phải được encode thành %2F.
        assert_eq!(seg("run.googleapis.com/requests"), "run.googleapis.com%2Frequests");
        assert_eq!(seg("simple-name"), "simple-name");
    }
}
