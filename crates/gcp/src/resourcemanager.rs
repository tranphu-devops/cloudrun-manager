//! Cloud Resource Manager API v3 — liệt kê project.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::client::{seg, GcpClient};
use crate::error::Result;
use crate::ttl;
use crate::types::ProjectInfo;

const BASE: &str = "https://cloudresourcemanager.googleapis.com/v3";

/// Liệt kê project mà account hiện tại nhìn thấy.
///
/// Dùng `projects:search` (v3) thay vì `projects.list` (v1): v3 trả về theo quyền của
/// caller thay vì phải chỉ định parent, đúng nhu cầu "cho tôi xem những gì tôi được xem".
pub async fn list_projects(client: &GcpClient) -> Result<Vec<ProjectInfo>> {
    let mut all: Vec<Value> = Vec::new();
    let mut token: Option<String> = None;

    loop {
        let mut url = format!("{BASE}/projects:search?query={}&pageSize=500", seg("state:ACTIVE"));
        if let Some(t) = &token {
            url.push_str("&pageToken=");
            url.push_str(&seg(t));
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Resp {
            #[serde(default)]
            projects: Vec<Value>,
            #[serde(default)]
            next_page_token: Option<String>,
        }

        let cache_key = format!("projects:{}", token.as_deref().unwrap_or("p0"));
        let resp: Resp = client
            .get_cached(&url, "liệt kê GCP project", &cache_key, ttl::PROJECTS)
            .await?;
        all.extend(resp.projects);

        match resp.next_page_token {
            Some(t) if !t.is_empty() => token = Some(t),
            _ => break,
        }
        if all.len() > 5000 {
            break;
        }
    }

    let mut out: Vec<ProjectInfo> = all
        .iter()
        .filter_map(|p| {
            let project_id = p.get("projectId")?.as_str()?.to_string();
            let display_name = p
                .get("displayName")
                .and_then(|v| v.as_str())
                .unwrap_or(&project_id)
                .to_string();
            Some(ProjectInfo {
                state: p
                    .get("state")
                    .and_then(|v| v.as_str())
                    .unwrap_or("ACTIVE")
                    .to_string(),
                project_id,
                display_name,
            })
        })
        .collect();

    out.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    Ok(out)
}

/// Kiểm tra caller có những quyền nào trên project.
///
/// Dùng để UI biết trước nên hiện hay khoá nút sửa, thay vì để người dùng bấm rồi mới
/// nhận 403. Danh sách permission truyền vào tối đa 100 cái theo giới hạn của API.
pub async fn test_permissions(
    client: &GcpClient,
    project: &str,
    permissions: &[&str],
) -> Result<Vec<String>> {
    let url = format!("{BASE}/projects/{project}:testIamPermissions");
    let body = json!({ "permissions": permissions });

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Resp {
        #[serde(default)]
        permissions: Vec<String>,
    }
    let resp: Resp = client
        .post(&url, &body, "kiểm tra quyền trên project")
        .await?;
    Ok(resp.permissions)
}

/// Permission app quan tâm.
pub const WANTED_PERMISSIONS: &[&str] = &[
    "run.services.list",
    "run.services.get",
    "run.services.update",
    "monitoring.timeSeries.list",
    "logging.logEntries.list",
    "secretmanager.secrets.list",
    "secretmanager.versions.access",
];

/// Diễn giải kết quả `test_permissions` thành thông tin dùng được cho UI.
#[derive(Debug, Clone, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub can_list_services: bool,
    pub can_read_service: bool,
    pub can_update_service: bool,
    pub can_read_metrics: bool,
    pub can_read_logs: bool,
    pub can_list_secrets: bool,
    pub can_reveal_secrets: bool,
    /// Những gì thiếu, diễn giải sang tiếng Việt để hiện thành gợi ý.
    pub missing: Vec<String>,
}

pub fn interpret(granted: &[String]) -> Capabilities {
    let has = |p: &str| granted.iter().any(|g| g == p);

    let caps = Capabilities {
        can_list_services: has("run.services.list"),
        can_read_service: has("run.services.get"),
        can_update_service: has("run.services.update"),
        can_read_metrics: has("monitoring.timeSeries.list"),
        can_read_logs: has("logging.logEntries.list"),
        can_list_secrets: has("secretmanager.secrets.list"),
        can_reveal_secrets: has("secretmanager.versions.access"),
        missing: Vec::new(),
    };

    let mut missing = Vec::new();
    if !caps.can_list_services || !caps.can_read_service {
        missing.push("Xem service — cần roles/run.viewer".to_string());
    }
    if !caps.can_update_service {
        missing.push(
            "Sửa env/scaling — cần roles/run.developer và iam.serviceAccounts.actAs trên runtime SA"
                .to_string(),
        );
    }
    if !caps.can_read_metrics {
        missing.push("Xem biểu đồ tải — cần roles/monitoring.viewer".to_string());
    }
    if !caps.can_read_logs {
        missing.push("Xem log — cần roles/logging.viewer".to_string());
    }
    if !caps.can_list_secrets {
        missing.push("Xem danh sách secret — cần roles/secretmanager.viewer".to_string());
    }
    if !caps.can_reveal_secrets {
        missing.push(
            "Xem giá trị secret — cần roles/secretmanager.secretAccessor (có thể cố tình không cấp trên prod)"
                .to_string(),
        );
    }

    Capabilities { missing, ..caps }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpret_day_du_quyen() {
        let granted: Vec<String> = WANTED_PERMISSIONS.iter().map(|s| s.to_string()).collect();
        let c = interpret(&granted);
        assert!(c.can_update_service);
        assert!(c.can_reveal_secrets);
        assert!(c.missing.is_empty(), "{:?}", c.missing);
    }

    #[test]
    fn interpret_chi_co_quyen_doc() {
        let granted = vec![
            "run.services.list".to_string(),
            "run.services.get".to_string(),
            "monitoring.timeSeries.list".to_string(),
            "logging.logEntries.list".to_string(),
        ];
        let c = interpret(&granted);
        assert!(c.can_list_services);
        assert!(c.can_read_logs);
        assert!(!c.can_update_service);
        assert!(!c.can_reveal_secrets);

        let joined = c.missing.join(" | ");
        assert!(joined.contains("run.developer"), "{joined}");
        assert!(
            joined.contains("actAs"),
            "gợi ý sửa phải nhắc actAs, đây là chỗ hay bị thiếu: {joined}"
        );
        assert!(joined.contains("secretAccessor"), "{joined}");
        assert!(
            !joined.contains("roles/run.viewer"),
            "đã có quyền đọc thì không được báo thiếu: {joined}"
        );
    }

    #[test]
    fn interpret_khong_co_quyen_gi() {
        let c = interpret(&[]);
        assert_eq!(c.missing.len(), 6);
        assert!(!c.can_list_services);
    }

    #[test]
    fn wanted_permissions_trong_gioi_han_api() {
        assert!(WANTED_PERMISSIONS.len() <= 100);
    }
}
