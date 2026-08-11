//! Cloud Run Admin API v2.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::client::GcpClient;
use crate::error::{GcpError, Result};
use crate::mutate;
use crate::ttl;
use crate::types::{
    ConditionView, Health, RevisionInfo, ServiceDetail, ServiceSummary,
};

const BASE: &str = "https://run.googleapis.com/v2";

/// Poll operation tối đa bao lâu trước khi trả về "đang triển khai".
/// Revision khởi động chậm (pull image, startup probe) nên phải rộng tay.
const OP_WAIT: Duration = Duration::from_secs(120);
const OP_POLL_INTERVAL: Duration = Duration::from_secs(2);

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Tách `projects/{p}/locations/{loc}/services/{id}`.
pub fn parse_service_name(full: &str) -> Option<(String, String, String)> {
    let parts: Vec<&str> = full.split('/').collect();
    if parts.len() < 6 || parts[0] != "projects" || parts[2] != "locations" {
        return None;
    }
    Some((
        parts[1].to_string(),
        parts[3].to_string(),
        parts[5].to_string(),
    ))
}

pub fn service_full_name(project: &str, region: &str, name: &str) -> String {
    format!("projects/{project}/locations/{region}/services/{name}")
}

fn short(name: &str) -> String {
    name.rsplit('/').next().unwrap_or(name).to_string()
}

/// Suy ra tình trạng service từ `terminalCondition` + cờ `reconciling`.
fn health_of(svc: &Value) -> (Health, Option<String>) {
    let reconciling = svc
        .get("reconciling")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let tc = svc.get("terminalCondition");
    let state = tc
        .and_then(|c| c.get("state"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let message = tc
        .and_then(|c| c.get("message"))
        .and_then(|v| v.as_str())
        .map(String::from);

    let health = match state {
        "CONDITION_SUCCEEDED" if reconciling => Health::Reconciling,
        "CONDITION_SUCCEEDED" => Health::Ready,
        "CONDITION_FAILED" => Health::NotReady,
        "CONDITION_PENDING" | "CONDITION_RECONCILING" => Health::Reconciling,
        _ if reconciling => Health::Reconciling,
        _ => Health::Unknown,
    };
    (health, message)
}

fn conditions_of(svc: &Value) -> Vec<ConditionView> {
    let mut out = Vec::new();

    let mut push = |c: &Value| {
        if let Some(t) = c.get("type").and_then(|v| v.as_str()) {
            out.push(ConditionView {
                r#type: t.to_string(),
                state: c
                    .get("state")
                    .and_then(|v| v.as_str())
                    .unwrap_or("UNKNOWN")
                    .to_string(),
                message: c.get("message").and_then(|v| v.as_str()).map(String::from),
                reason: c
                    .get("reason")
                    .or_else(|| c.get("revisionReason"))
                    .or_else(|| c.get("executionReason"))
                    .and_then(|v| v.as_str())
                    .map(String::from),
                last_transition_time: c
                    .get("lastTransitionTime")
                    .and_then(|v| v.as_str())
                    .map(String::from),
            });
        }
    };

    if let Some(tc) = svc.get("terminalCondition") {
        push(tc);
    }
    if let Some(arr) = svc.get("conditions").and_then(|c| c.as_array()) {
        for c in arr {
            push(c);
        }
    }
    out
}

pub fn summarize(svc: &Value) -> Option<ServiceSummary> {
    let full_name = svc.get("name")?.as_str()?.to_string();
    let (project_id, region, name) = parse_service_name(&full_name)?;

    let (health, health_message) = health_of(svc);

    let containers = svc
        .get("template")
        .and_then(|t| t.get("containers"))
        .and_then(|c| c.as_array());

    let first = containers.and_then(|c| c.first());
    let env = first.map(mutate::parse_env).unwrap_or_default();
    let secret_env_count = env
        .iter()
        .filter(|e| e.kind == crate::types::EnvKind::SecretRef)
        .count();

    let scaling = svc.get("template").and_then(|t| t.get("scaling"));

    Some(ServiceSummary {
        name,
        full_name,
        project_id,
        region,
        uri: svc.get("uri").and_then(|v| v.as_str()).map(String::from),
        health,
        health_message,
        latest_ready_revision: svc
            .get("latestReadyRevision")
            .and_then(|v| v.as_str())
            .map(short),
        latest_created_revision: svc
            .get("latestCreatedRevision")
            .and_then(|v| v.as_str())
            .map(short),
        image: first
            .and_then(|c| c.get("image"))
            .and_then(|v| v.as_str())
            .map(String::from),
        min_instances: scaling
            .and_then(|s| s.get("minInstanceCount"))
            .and_then(|v| v.as_i64()),
        max_instances: scaling
            .and_then(|s| s.get("maxInstanceCount"))
            .and_then(|v| v.as_i64()),
        last_modifier: svc
            .get("lastModifier")
            .and_then(|v| v.as_str())
            .map(String::from),
        update_time: svc
            .get("updateTime")
            .and_then(|v| v.as_str())
            .map(String::from),
        traffic_pinned: mutate::is_traffic_pinned(svc),
        env_count: env.len(),
        secret_env_count,
        container_count: containers.map(|c| c.len()).unwrap_or(0),
    })
}

pub fn detail_from_raw(svc: &Value) -> Result<ServiceDetail> {
    let summary = summarize(svc).ok_or_else(|| {
        GcpError::Invalid(
            "Response của Cloud Run thiếu field `name` đúng định dạng nên không đọc được service."
                .to_string(),
        )
    })?;

    let tpl = svc.get("template");

    Ok(ServiceDetail {
        etag: svc
            .get("etag")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        description: svc
            .get("description")
            .and_then(|v| v.as_str())
            .map(String::from),
        service_account: tpl
            .and_then(|t| t.get("serviceAccount"))
            .and_then(|v| v.as_str())
            .map(String::from),
        ingress: svc.get("ingress").and_then(|v| v.as_str()).map(String::from),
        launch_stage: svc
            .get("launchStage")
            .and_then(|v| v.as_str())
            .map(String::from),
        execution_environment: tpl
            .and_then(|t| t.get("executionEnvironment"))
            .and_then(|v| v.as_str())
            .map(String::from),
        concurrency: tpl
            .and_then(|t| t.get("maxInstanceRequestConcurrency"))
            .and_then(|v| v.as_i64()),
        timeout: tpl
            .and_then(|t| t.get("timeout"))
            .and_then(|v| v.as_str())
            .map(String::from),
        session_affinity: tpl
            .and_then(|t| t.get("sessionAffinity"))
            .and_then(|v| v.as_bool()),
        vpc_egress: tpl
            .and_then(|t| t.get("vpcAccess"))
            .and_then(|v| v.get("egress"))
            .and_then(|v| v.as_str())
            .map(String::from),
        vpc_connector: tpl
            .and_then(|t| t.get("vpcAccess"))
            .and_then(|v| v.get("connector"))
            .and_then(|v| v.as_str())
            .map(short),
        cloudsql_instances: tpl
            .and_then(|t| t.get("volumes"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.get("cloudSqlInstance"))
                    .filter_map(|c| c.get("instances"))
                    .filter_map(|i| i.as_array())
                    .flatten()
                    .filter_map(|s| s.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        containers: mutate::parse_containers(svc)?,
        secret_volumes: mutate::parse_secret_volumes(svc),
        traffic: mutate::parse_traffic(svc),
        conditions: conditions_of(svc),
        labels: mutate::string_map(svc.get("labels")),
        annotations: mutate::string_map(svc.get("annotations")),
        next_revision_hint: mutate::predict_next_revision(
            svc.get("latestCreatedRevision").and_then(|v| v.as_str()),
        ),
        summary,
        raw: svc.clone(),
    })
}

// ---------------------------------------------------------------------------
// List / Get
// ---------------------------------------------------------------------------

/// Liệt kê toàn bộ service của project, mọi region, bằng wildcard `locations/-`.
///
/// Không hardcode region: `example-project` hiện đều ở `asia-northeast1` nhưng service
/// mới có thể được deploy ở region khác và app không được bỏ sót.
pub async fn list_services(client: &GcpClient, project: &str) -> Result<Vec<ServiceSummary>> {
    let raw = list_services_raw(client, project).await?;
    // Sắp xếp theo region rồi tên để sidebar ổn định giữa các lần refresh.
    let mut summaries: Vec<ServiceSummary> = raw.iter().filter_map(summarize).collect();
    summaries.sort_by(|a, b| a.region.cmp(&b.region).then(a.name.cmp(&b.name)));
    Ok(summaries)
}

/// JSON thô của toàn bộ service trong project.
///
/// `services.list` của Cloud Run v2 trả về **Service đầy đủ**, không phải bản rút gọn.
/// Nhờ vậy những thứ như "secret nào đang được service nào dùng" tính được từ đây,
/// không cần GET riêng từng service (95 request cho `example-project`).
pub async fn list_services_raw(client: &GcpClient, project: &str) -> Result<Vec<Value>> {
    let cache_key = format!("run:{project}:services");
    if let Some(hit) = client.cache.get(&cache_key).await {
        if let Ok(raw) = serde_json::from_str::<Vec<Value>>(&hit) {
            return Ok(raw);
        }
    }

    let mut all: Vec<Value> = Vec::new();
    let mut page_token: Option<String> = None;
    let ctx = format!("liệt kê Cloud Run service của project {project}");

    loop {
        let mut url = format!("{BASE}/projects/{project}/locations/-/services?pageSize=200");
        if let Some(t) = &page_token {
            url.push_str("&pageToken=");
            url.push_str(&crate::client::seg(t));
        }

        // `serde` field ở đây là camelCase trong JSON.
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Page {
            #[serde(default)]
            services: Vec<Value>,
            #[serde(default)]
            next_page_token: Option<String>,
        }
        let page: Page = client.get(&url, &ctx).await?;
        all.extend(page.services);

        match page.next_page_token {
            Some(t) if !t.is_empty() => page_token = Some(t),
            _ => break,
        }
        // Chặn vòng lặp vô hạn nếu API trả token lặp lại.
        if all.len() > 5000 {
            break;
        }
    }

    if let Ok(s) = serde_json::to_string(&all) {
        client.cache.put(cache_key, s.as_str(), ttl::SERVICES).await;
    }
    Ok(all)
}

/// Map tên service -> danh sách secret nó tham chiếu, tính từ bản list đã cache.
pub async fn secret_usage_map(
    client: &GcpClient,
    project: &str,
) -> Result<std::collections::BTreeMap<String, Vec<String>>> {
    let raw = list_services_raw(client, project).await?;
    Ok(raw
        .iter()
        .filter_map(|s| {
            let full = s.get("name")?.as_str()?;
            let (_, _, name) = parse_service_name(full)?;
            let used = mutate::referenced_secrets(s);
            if used.is_empty() {
                None
            } else {
                Some((name, used))
            }
        })
        .collect())
}

pub async fn get_service_raw(
    client: &GcpClient,
    project: &str,
    region: &str,
    name: &str,
) -> Result<Value> {
    let full = service_full_name(project, region, name);
    let url = format!("{BASE}/{full}");
    let ctx = format!("xem chi tiết service {name} ({region})");
    let cache_key = format!("run:{project}:svc:{region}:{name}");
    client
        .get_cached(&url, &ctx, &cache_key, ttl::SERVICE_DETAIL)
        .await
}

/// GET bỏ qua cache.
///
/// Bắt buộc dùng trước khi ghi: read-modify-write trên bản cache 15 giây cũ là đúng
/// cái cửa sổ để lost-update xảy ra. Lấy bản tươi rồi mới so etag với bản người dùng
/// đang xem.
pub async fn get_service_raw_fresh(
    client: &GcpClient,
    project: &str,
    region: &str,
    name: &str,
) -> Result<Value> {
    let cache_key = format!("run:{project}:svc:{region}:{name}");
    client.cache.invalidate_prefix(&cache_key).await;
    get_service_raw(client, project, region, name).await
}

pub async fn get_service(
    client: &GcpClient,
    project: &str,
    region: &str,
    name: &str,
) -> Result<ServiceDetail> {
    let raw = get_service_raw(client, project, region, name).await?;
    detail_from_raw(&raw)
}

// ---------------------------------------------------------------------------
// Revisions
// ---------------------------------------------------------------------------

pub async fn list_revisions(
    client: &GcpClient,
    project: &str,
    region: &str,
    service: &str,
) -> Result<Vec<RevisionInfo>> {
    let full = service_full_name(project, region, service);
    let url = format!("{BASE}/{full}/revisions?pageSize=100");
    let ctx = format!("liệt kê revision của service {service}");
    let cache_key = format!("run:{project}:revs:{region}:{service}");

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Resp {
        #[serde(default)]
        revisions: Vec<Value>,
    }
    let resp: Resp = client
        .get_cached(&url, &ctx, &cache_key, ttl::REVISIONS)
        .await?;

    // % traffic lấy từ service, không có ở revision.
    let svc = get_service_raw(client, project, region, service).await.ok();
    let traffic: std::collections::BTreeMap<String, i64> = svc
        .as_ref()
        .map(|s| {
            mutate::parse_traffic(s)
                .into_iter()
                .filter_map(|t| t.revision.map(|r| (r, t.percent)))
                .collect()
        })
        .unwrap_or_default();
    let latest_ready = svc
        .as_ref()
        .and_then(|s| s.get("latestReadyRevision"))
        .and_then(|v| v.as_str())
        .map(short);
    // Khi traffic đi về LATEST thì revision latest-ready nhận 100%.
    let latest_percent: i64 = svc
        .as_ref()
        .map(|s| {
            mutate::parse_traffic(s)
                .into_iter()
                .filter(|t| t.kind == "LATEST")
                .map(|t| t.percent)
                .sum()
        })
        .unwrap_or(0);

    let mut out: Vec<RevisionInfo> = resp
        .revisions
        .iter()
        .map(|r| {
            let name = r
                .get("name")
                .and_then(|v| v.as_str())
                .map(short)
                .unwrap_or_default();
            let (health, health_message) = health_of(r);
            let c0 = r
                .get("containers")
                .and_then(|c| c.as_array())
                .and_then(|a| a.first());
            let limits = c0.and_then(|c| c.get("resources")).and_then(|r| r.get("limits"));
            let is_latest_ready = latest_ready.as_deref() == Some(name.as_str());

            RevisionInfo {
                traffic_percent: traffic
                    .get(&name)
                    .copied()
                    .unwrap_or(if is_latest_ready { latest_percent } else { 0 }),
                is_latest_ready,
                create_time: r
                    .get("createTime")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                image: c0
                    .and_then(|c| c.get("image"))
                    .and_then(|v| v.as_str())
                    .map(String::from),
                health,
                health_message,
                min_instances: r
                    .get("scaling")
                    .and_then(|s| s.get("minInstanceCount"))
                    .and_then(|v| v.as_i64()),
                max_instances: r
                    .get("scaling")
                    .and_then(|s| s.get("maxInstanceCount"))
                    .and_then(|v| v.as_i64()),
                cpu: limits
                    .and_then(|l| l.get("cpu"))
                    .and_then(|v| v.as_str())
                    .map(String::from),
                memory: limits
                    .and_then(|l| l.get("memory"))
                    .and_then(|v| v.as_str())
                    .map(String::from),
                concurrency: r
                    .get("maxInstanceRequestConcurrency")
                    .and_then(|v| v.as_i64()),
                log_uri: r.get("logUri").and_then(|v| v.as_str()).map(String::from),
                name,
            }
        })
        .collect();

    // Mới nhất lên đầu.
    out.sort_by(|a, b| b.create_time.cmp(&a.create_time));
    Ok(out)
}

// ---------------------------------------------------------------------------
// Patch
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchOutcome {
    /// Tên operation, để tra lại nếu cần.
    pub operation: Option<String>,
    /// `true` nếu operation đã hoàn tất trong lúc chờ.
    pub done: bool,
    /// Revision mới, chỉ có khi operation xong.
    pub new_revision: Option<String>,
    /// Câu mô tả trạng thái, hiện trực tiếp cho người dùng.
    pub message: String,
}

/// Gửi PATCH. `validate_only = true` là dry-run: Cloud Run kiểm tra payload và trả lỗi
/// nếu sai nhưng KHÔNG tạo revision. Rất đáng chạy trước khi apply thật.
pub async fn patch_service(
    client: &GcpClient,
    project: &str,
    region: &str,
    name: &str,
    payload: &Value,
    validate_only: bool,
) -> Result<PatchOutcome> {
    // Không có etag thì không có gì chặn lost-update. Từ chối ngay tại đây.
    mutate::require_etag(payload)?;

    let full = service_full_name(project, region, name);
    let url = format!(
        "{BASE}/{full}{}",
        if validate_only {
            "?validateOnly=true"
        } else {
            ""
        }
    );
    let ctx = if validate_only {
        format!("kiểm tra trước thay đổi cho service {name}")
    } else {
        format!("cập nhật service {name}")
    };

    let op: Value = client.patch(&url, payload, &ctx).await?;

    if validate_only {
        return Ok(PatchOutcome {
            operation: None,
            done: true,
            new_revision: None,
            message: "Cấu hình hợp lệ. Cloud Run chấp nhận payload này (chưa tạo revision nào)."
                .to_string(),
        });
    }

    // Ghi thành công -> cache của project này không còn đúng.
    client.cache.invalidate_prefix(&format!("run:{project}")).await;

    let op_name = op.get("name").and_then(|v| v.as_str()).map(String::from);

    // Trường hợp API trả về done ngay.
    if op.get("done").and_then(|v| v.as_bool()) == Some(true) {
        return Ok(finish_outcome(&op, op_name));
    }

    let Some(op_name) = op_name else {
        return Ok(PatchOutcome {
            operation: None,
            done: false,
            new_revision: None,
            message: "Đã gửi thay đổi. Cloud Run không trả về tên operation nên app không theo dõi được tiến trình — xem tab Revisions để kiểm tra."
                .to_string(),
        });
    };

    match wait_operation(client, &op_name).await {
        Ok(done_op) => Ok(finish_outcome(&done_op, Some(op_name))),
        Err(GcpError::Invalid(msg)) => Ok(PatchOutcome {
            operation: Some(op_name),
            done: false,
            new_revision: None,
            message: msg,
        }),
        Err(e) => Err(e),
    }
}

fn finish_outcome(op: &Value, op_name: Option<String>) -> PatchOutcome {
    // Operation thất bại: revision đã được tạo nhưng không khởi động được.
    // Đây là ca người dùng cần biết ngay, đừng báo "thành công".
    if let Some(err) = op.get("error") {
        let msg = err
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("không rõ nguyên nhân");
        return PatchOutcome {
            operation: op_name,
            done: true,
            new_revision: None,
            message: format!(
                "Cloud Run đã nhận thay đổi nhưng revision mới KHÔNG khởi động được: {msg}\n\n\
                 Service vẫn đang chạy revision cũ (Cloud Run chỉ chuyển traffic khi revision mới \
                 healthy). Xem tab Logs để tìm nguyên nhân — hay gặp nhất là thiếu quyền đọc secret, \
                 env sai định dạng, hoặc app crash khi khởi động."
            ),
        };
    }

    let new_revision = op
        .get("response")
        .and_then(|r| r.get("latestCreatedRevision"))
        .and_then(|v| v.as_str())
        .map(short);

    PatchOutcome {
        message: match &new_revision {
            Some(r) => format!("Đã tạo và triển khai xong revision {r}."),
            None => "Thay đổi đã được áp dụng.".to_string(),
        },
        operation: op_name,
        done: true,
        new_revision,
    }
}

/// Poll operation tới khi `done`. Hết thời gian thì trả `GcpError::Invalid` mang message
/// "vẫn đang triển khai" — caller coi đó là kết quả chấp nhận được, không phải lỗi.
pub async fn wait_operation(client: &GcpClient, op_name: &str) -> Result<Value> {
    let url = format!("{BASE}/{op_name}");
    let started = Instant::now();

    loop {
        tokio::time::sleep(OP_POLL_INTERVAL).await;

        let op: Value = client.get(&url, "theo dõi tiến trình triển khai").await?;
        if op.get("done").and_then(|v| v.as_bool()) == Some(true) {
            return Ok(op);
        }

        if started.elapsed() > OP_WAIT {
            return Err(GcpError::Invalid(format!(
                "Đã gửi thay đổi và Cloud Run đang triển khai, nhưng sau {}s vẫn chưa xong nên app \
                 dừng theo dõi. Thay đổi KHÔNG bị mất — mở tab Revisions để xem tiến trình.",
                OP_WAIT.as_secs()
            )));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tach_dung_ten_service() {
        let got =
            parse_service_name("projects/example-project/locations/asia-northeast1/services/gateway");
        assert_eq!(
            got,
            Some((
                "example-project".into(),
                "asia-northeast1".into(),
                "gateway".into()
            ))
        );
        assert_eq!(parse_service_name("rác"), None);
        assert_eq!(parse_service_name("projects/p/locations/l"), None);
    }

    #[test]
    fn health_ready() {
        let svc = json!({ "terminalCondition": { "state": "CONDITION_SUCCEEDED" }, "reconciling": false });
        assert_eq!(health_of(&svc).0, Health::Ready);
    }

    #[test]
    fn health_dang_reconcile_khong_bao_la_ready() {
        // Service vừa được sửa: condition vẫn SUCCEEDED của revision cũ nhưng đang
        // reconcile revision mới. Báo "Ready" ở đây sẽ làm người dùng tưởng đã xong.
        let svc = json!({ "terminalCondition": { "state": "CONDITION_SUCCEEDED" }, "reconciling": true });
        assert_eq!(health_of(&svc).0, Health::Reconciling);
    }

    #[test]
    fn health_failed_giu_lai_message() {
        let svc = json!({
            "terminalCondition": { "state": "CONDITION_FAILED", "message": "Revision failed to start" }
        });
        let (h, m) = health_of(&svc);
        assert_eq!(h, Health::NotReady);
        assert_eq!(m.as_deref(), Some("Revision failed to start"));
    }

    #[test]
    fn outcome_operation_loi_khong_bao_thanh_cong() {
        let op = json!({
            "name": "projects/p/locations/l/operations/op1",
            "done": true,
            "error": { "code": 9, "message": "Revision is not ready and cannot serve traffic" }
        });
        let out = finish_outcome(&op, Some("op1".into()));
        assert!(out.message.contains("KHÔNG khởi động được"), "{}", out.message);
        assert!(
            out.message.contains("revision cũ"),
            "phải nói rõ service vẫn đang chạy bản cũ: {}",
            out.message
        );
        assert!(out.new_revision.is_none());
    }

    #[test]
    fn outcome_thanh_cong_lay_ten_revision_moi() {
        let op = json!({
            "done": true,
            "response": {
                "latestCreatedRevision": "projects/p/locations/l/revisions/gateway-00042-abc"
            }
        });
        let out = finish_outcome(&op, None);
        assert_eq!(out.new_revision.as_deref(), Some("gateway-00042-abc"));
        assert!(out.message.contains("gateway-00042-abc"));
    }

    #[test]
    fn summarize_doc_du_thong_tin_sidebar() {
        let svc = json!({
            "name": "projects/example-project/locations/asia-northeast1/services/gateway",
            "uri": "https://gateway-x-an.a.run.app",
            "terminalCondition": { "state": "CONDITION_SUCCEEDED" },
            "template": {
                "scaling": { "minInstanceCount": 1, "maxInstanceCount": 10 },
                "containers": [{
                    "image": "img:v1",
                    "env": [
                        { "name": "A", "value": "1" },
                        { "name": "S", "valueSource": { "secretKeyRef": { "secret": "s", "version": "latest" } } }
                    ]
                }]
            },
            "traffic": [{ "type": "TRAFFIC_TARGET_ALLOCATION_TYPE_LATEST", "percent": 100 }]
        });
        let s = summarize(&svc).unwrap();
        assert_eq!(s.name, "gateway");
        assert_eq!(s.region, "asia-northeast1");
        assert_eq!(s.project_id, "example-project");
        assert_eq!(s.env_count, 2);
        assert_eq!(s.secret_env_count, 1);
        assert_eq!(s.min_instances, Some(1));
        assert!(!s.traffic_pinned);
        assert_eq!(s.container_count, 1);
    }

    #[test]
    fn summarize_bo_qua_service_thieu_name() {
        assert!(summarize(&json!({ "uri": "x" })).is_none());
    }
}
