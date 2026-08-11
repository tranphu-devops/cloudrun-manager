//! Secret Manager API v1.
//!
//! Nguyên tắc bảo mật của module này:
//!   • Metadata (tên, version, ngày tạo) được cache bình thường.
//!   • **Giá trị secret KHÔNG BAO GIỜ được cache** và không đi qua `Cache`.
//!   • Giá trị trả về bọc trong `Secret` để không lỡ in ra log.

use std::collections::BTreeMap;

use base64::Engine;
use serde::Deserialize;
use serde_json::Value;

use crate::client::{seg, GcpClient};
use crate::error::{GcpError, Result};
use crate::secret::Secret;
use crate::ttl;
use crate::types::{SecretInfo, SecretVersionInfo};

const BASE: &str = "https://secretmanager.googleapis.com/v1";

fn short(name: &str) -> String {
    name.rsplit('/').next().unwrap_or(name).to_string()
}

/// Liệt kê secret của project (chỉ metadata).
///
/// `used_by` để rỗng ở đây; tầng trên đối chiếu với danh sách service để điền vào,
/// vì Secret Manager không biết ai đang dùng secret của nó.
pub async fn list_secrets(client: &GcpClient, project: &str) -> Result<Vec<SecretInfo>> {
    let mut all: Vec<Value> = Vec::new();
    let mut token: Option<String> = None;
    let ctx = format!("liệt kê secret của project {project}");

    loop {
        let mut url = format!("{BASE}/projects/{project}/secrets?pageSize=500");
        if let Some(t) = &token {
            url.push_str("&pageToken=");
            url.push_str(&seg(t));
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Resp {
            #[serde(default)]
            secrets: Vec<Value>,
            #[serde(default)]
            next_page_token: Option<String>,
        }

        let cache_key = format!("secrets:{project}:{}", token.as_deref().unwrap_or("p0"));
        let resp: Resp = client
            .get_cached(&url, &ctx, &cache_key, ttl::SECRETS)
            .await?;
        all.extend(resp.secrets);

        match resp.next_page_token {
            Some(t) if !t.is_empty() => token = Some(t),
            _ => break,
        }
        if all.len() > 10_000 {
            break;
        }
    }

    let mut out: Vec<SecretInfo> = all
        .iter()
        .filter_map(|s| {
            let full = s.get("name")?.as_str()?;
            Some(SecretInfo {
                name: short(full),
                create_time: s
                    .get("createTime")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                labels: crate::mutate::string_map(s.get("labels")),
                replication: s
                    .get("replication")
                    .map(|r| {
                        if r.get("automatic").is_some() {
                            "automatic".to_string()
                        } else if let Some(um) = r.get("userManaged") {
                            let n = um
                                .get("replicas")
                                .and_then(|x| x.as_array())
                                .map(|a| a.len())
                                .unwrap_or(0);
                            format!("user-managed ({n} replica)")
                        } else {
                            "unknown".to_string()
                        }
                    }),
                used_by: Vec::new(),
            })
        })
        .collect();

    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

pub async fn list_versions(
    client: &GcpClient,
    project: &str,
    secret: &str,
) -> Result<Vec<SecretVersionInfo>> {
    let url = format!(
        "{BASE}/projects/{project}/secrets/{}/versions?pageSize=100",
        seg(secret)
    );
    let ctx = format!("liệt kê version của secret {secret}");
    let cache_key = format!("secretver:{project}:{secret}");

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Resp {
        #[serde(default)]
        versions: Vec<Value>,
    }
    let resp: Resp = client
        .get_cached(&url, &ctx, &cache_key, ttl::SECRETS)
        .await?;

    let mut out: Vec<SecretVersionInfo> = resp
        .versions
        .iter()
        .filter_map(|v| {
            let full = v.get("name")?.as_str()?;
            Some(SecretVersionInfo {
                version: short(full),
                state: v
                    .get("state")
                    .and_then(|s| s.as_str())
                    .unwrap_or("STATE_UNSPECIFIED")
                    .to_string(),
                create_time: v
                    .get("createTime")
                    .and_then(|s| s.as_str())
                    .map(String::from),
                destroy_time: v
                    .get("destroyTime")
                    .and_then(|s| s.as_str())
                    .map(String::from),
            })
        })
        .collect();

    // Version là số; sort số học để v10 không đứng trước v9.
    out.sort_by_key(|v| std::cmp::Reverse(v.version.parse::<u64>().unwrap_or(0)));
    Ok(out)
}

/// Đọc giá trị của một version secret.
///
/// KHÔNG cache, KHÔNG log. Chỉ gọi khi người dùng bấm reveal một cách có ý thức.
pub async fn access_version(
    client: &GcpClient,
    project: &str,
    secret: &str,
    version: &str,
) -> Result<Secret> {
    let url = format!(
        "{BASE}/projects/{project}/secrets/{}/versions/{}:access",
        seg(secret),
        seg(version)
    );
    let ctx = format!("đọc giá trị secret {secret} (version {version})");

    // Dùng `get` (không cache) — có cache ở đây là lỗi bảo mật, không phải tối ưu.
    let resp: Value = client.get(&url, &ctx).await?;

    let b64 = resp
        .get("payload")
        .and_then(|p| p.get("data"))
        .and_then(|d| d.as_str())
        .ok_or_else(|| {
            GcpError::Invalid(format!(
                "Secret Manager không trả về payload cho {secret} version {version}. \
                 Có thể version này đã bị disable hoặc destroy."
            ))
        })?;

    // Secret Manager dùng base64 standard, nhưng chấp nhận cả URL-safe cho chắc.
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(b64))
        .map_err(|e| {
            GcpError::Invalid(format!("Không giải mã được payload secret (base64): {e}"))
        })?;

    // Secret có thể là binary (ví dụ file key). Hiển thị lossy để không panic,
    // và tầng UI sẽ cảnh báo nếu nội dung không phải text.
    Ok(Secret::new(String::from_utf8_lossy(&bytes).into_owned()))
}

/// Điền `used_by` cho danh sách secret, dựa trên các service đang tham chiếu.
///
/// `service_refs`: tên service -> danh sách secret nó dùng.
pub fn attach_usage(
    secrets: &mut [SecretInfo],
    service_refs: &BTreeMap<String, Vec<String>>,
) {
    let mut reverse: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for (svc, used) in service_refs {
        for s in used {
            reverse.entry(s.as_str()).or_default().push(svc.clone());
        }
    }
    for s in secrets.iter_mut() {
        if let Some(users) = reverse.get(s.name.as_str()) {
            let mut u = users.clone();
            u.sort();
            u.dedup();
            s.used_by = u;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(name: &str) -> SecretInfo {
        SecretInfo {
            name: name.into(),
            create_time: None,
            labels: BTreeMap::new(),
            replication: None,
            used_by: Vec::new(),
        }
    }

    #[test]
    fn rut_ngan_ten_tu_resource_path() {
        assert_eq!(short("projects/p/secrets/abc/versions/3"), "3");
        assert_eq!(short("projects/p/secrets/abc"), "abc");
    }

    #[test]
    fn attach_usage_dien_dung_service_dang_dung() {
        let mut secrets = vec![
            info("gateway-db-password"),
            info("jwt-signing-key"),
            info("khong-ai-dung"),
        ];
        let mut refs = BTreeMap::new();
        refs.insert(
            "gateway".to_string(),
            vec!["gateway-db-password".to_string(), "jwt-signing-key".to_string()],
        );
        refs.insert("admin".to_string(), vec!["jwt-signing-key".to_string()]);

        attach_usage(&mut secrets, &refs);

        assert_eq!(secrets[0].used_by, vec!["gateway"]);
        assert_eq!(
            secrets[1].used_by,
            vec!["admin", "gateway"],
            "secret dùng bởi nhiều service phải liệt kê đủ và sắp xếp ổn định"
        );
        assert!(
            secrets[2].used_by.is_empty(),
            "secret không ai dùng phải để rỗng — đây là tín hiệu để dọn dẹp"
        );
    }

    #[test]
    fn attach_usage_khong_lap_service_trung() {
        let mut secrets = vec![info("s1")];
        let mut refs = BTreeMap::new();
        refs.insert("svc".to_string(), vec!["s1".to_string(), "s1".to_string()]);
        attach_usage(&mut secrets, &refs);
        assert_eq!(secrets[0].used_by, vec!["svc"]);
    }

    #[test]
    fn sort_version_theo_so_khong_theo_chuoi() {
        // Sort chuỗi sẽ cho "10" < "9"; phải sort số học.
        let mut v = [
            SecretVersionInfo {
                version: "9".into(),
                state: "ENABLED".into(),
                create_time: None,
                destroy_time: None,
            },
            SecretVersionInfo {
                version: "10".into(),
                state: "ENABLED".into(),
                create_time: None,
                destroy_time: None,
            },
        ];
        v.sort_by_key(|x: &SecretVersionInfo| {
            std::cmp::Reverse(x.version.parse::<u64>().unwrap_or(0))
        });
        assert_eq!(v[0].version, "10");
    }
}
