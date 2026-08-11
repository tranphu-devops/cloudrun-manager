//! Command ghi: sửa env và sửa scaling/resource.
//!
//! # Luồng an toàn (giống nhau cho cả hai)
//!
//! 1. `guard_write` — chặn nếu đang Read-only, đòi gõ tên service nếu project là
//!    prod/chưa gắn nhãn. Kiểm ở Rust, không chỉ ở UI.
//! 2. **GET lại bản tươi**, bỏ qua cache. Read-modify-write trên bản cache 15 giây cũ
//!    là đúng cái cửa sổ để ghi đè mất thay đổi của người khác.
//! 3. So `etag` bản tươi với etag người dùng đang xem trên màn hình. Khác nhau → dừng,
//!    báo conflict, KHÔNG tự merge.
//! 4. Dựng payload từ bản tươi (giữ nguyên mọi field không liên quan).
//! 5. `validateOnly=true` nếu là dry-run, hoặc PATCH thật rồi chờ operation.
//! 6. Ghi audit kể cả khi thất bại.

use gcp::mutate as m;
use gcp::run;
use gcp::types::{ApplyPreview, EnvEntry, ScalingUpdate};
use serde::Serialize;
use serde_json::Value;
use tauri::State;

use crate::audit::{Action, Outcome};
use crate::error::CmdError;
use crate::state::AppState;

type R<T> = Result<T, CmdError>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    pub preview: ApplyPreview,
    pub outcome: run::PatchOutcome,
    /// Etag mới sau khi ghi — frontend dùng cho lần sửa tiếp theo mà không cần reload.
    pub new_etag: Option<String>,
    /// `true` khi chỉ kiểm tra, không tạo revision.
    pub validated_only: bool,
}

/// Lấy bản tươi và kiểm tra etag khớp với bản người dùng đang xem.
async fn fresh_and_check(
    state: &AppState,
    project: &str,
    region: &str,
    service: &str,
    expected_etag: &str,
) -> R<Value> {
    let fresh = run::get_service_raw_fresh(&state.gcp, project, region, service).await?;

    let actual = fresh.get("etag").and_then(|v| v.as_str()).unwrap_or("");

    // Etag rỗng thì không có gì để so — coi như không đủ điều kiện ghi an toàn.
    if actual.is_empty() {
        return Err(CmdError::new(
            "invalid",
            "Cloud Run không trả về etag cho service này nên app không bảo đảm được bạn đang sửa \
             trên bản mới nhất. Bấm Reload rồi thử lại.",
        ));
    }

    if !expected_etag.is_empty() && actual != expected_etag {
        let modifier = fresh
            .get("lastModifier")
            .and_then(|v| v.as_str())
            .unwrap_or("ai đó");
        let when = fresh
            .get("updateTime")
            .and_then(|v| v.as_str())
            .unwrap_or("vừa rồi");
        return Err(CmdError {
            kind: "conflict".to_string(),
            status: Some(409),
            detail: Some(format!("etag đang xem: {expected_etag} — etag hiện tại: {actual}")),
            message: format!(
                "Service `{service}` đã bị thay đổi sau khi bạn mở nó (bởi {modifier}, lúc {when}).\n\n\
                 App dừng lại thay vì ghi đè, vì ghi tiếp sẽ xoá mất thay đổi của họ. \
                 Bấm Reload để lấy bản mới nhất rồi áp lại sửa đổi của bạn."
            ),
        });
    }

    Ok(fresh)
}

fn env_of(svc: &Value, container_index: usize) -> R<Vec<EnvEntry>> {
    let containers = m::parse_containers(svc).map_err(CmdError::from)?;
    let c = containers.get(container_index).ok_or_else(|| {
        CmdError::new(
            "invalid",
            format!(
                "Service chỉ có {} container, không có container thứ {}.",
                containers.len(),
                container_index + 1
            ),
        )
    })?;
    Ok(c.env.clone())
}

/// Xem trước thay đổi env — không gọi API ghi nào.
#[tauri::command]
pub async fn preview_env(
    state: State<'_, AppState>,
    project: String,
    region: String,
    service: String,
    container_index: usize,
    env: Vec<EnvEntry>,
) -> R<ApplyPreview> {
    state.guard_project(&project).await?;
    // Dùng bản cache là đủ cho preview: đây chỉ là để người dùng nhìn diff, còn kiểm
    // tra etag thật thì làm ở bước apply.
    let svc = run::get_service_raw(&state.gcp, &project, &region, &service).await?;
    let before = env_of(&svc, container_index)?;

    // Validate sớm để lỗi nhập liệu hiện ngay ở khung preview, không đợi tới lúc bấm Apply.
    m::validate_env_list(&env).map_err(CmdError::from)?;

    Ok(m::build_preview(&svc, &before, &env, vec![]))
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn apply_env(
    state: State<'_, AppState>,
    project: String,
    region: String,
    service: String,
    container_index: usize,
    env: Vec<EnvEntry>,
    expected_etag: String,
    confirm_text: Option<String>,
    validate_only: bool,
) -> R<ApplyResult> {
    // Dry-run cũng phải qua guard: nó không tạo revision, nhưng vẫn là hành động
    // "chuẩn bị ghi" và không nên chạy được khi đang Read-only, để trạng thái app rõ ràng.
    state
        .guard_write(&project, &service, confirm_text.as_deref())
        .await?;

    let fresh = fresh_and_check(&state, &project, &region, &service, &expected_etag).await?;
    let before = env_of(&fresh, container_index)?;
    let preview = m::build_preview(&fresh, &before, &env, vec![]);

    // Không có gì thay đổi: đừng tạo revision rỗng.
    if preview.env_changes.is_empty() && !validate_only {
        return Ok(ApplyResult {
            outcome: run::PatchOutcome {
                operation: None,
                done: true,
                new_revision: None,
                message: "Không có thay đổi nào so với cấu hình hiện tại — app không tạo revision mới."
                    .to_string(),
            },
            new_etag: fresh.get("etag").and_then(|v| v.as_str()).map(String::from),
            validated_only: false,
            preview,
        });
    }

    let payload = m::apply_env(&fresh, container_index, &env).map_err(CmdError::from)?;
    let changes = describe_env_changes(&preview);

    let result = run::patch_service(
        &state.gcp,
        &project,
        &region,
        &service,
        &payload,
        validate_only,
    )
    .await;

    finish(
        &state,
        &project,
        &region,
        &service,
        if validate_only {
            Action::ValidateOnly
        } else {
            Action::UpdateEnv
        },
        changes,
        preview,
        validate_only,
        result,
    )
    .await
}

#[tauri::command]
pub async fn preview_scaling(
    state: State<'_, AppState>,
    project: String,
    region: String,
    service: String,
    container_index: usize,
    update: ScalingUpdate,
) -> R<ApplyPreview> {
    state.guard_project(&project).await?;
    let svc = run::get_service_raw(&state.gcp, &project, &region, &service).await?;
    m::validate_scaling(&update).map_err(CmdError::from)?;

    let changes = m::describe_scaling_changes(&svc, container_index, &update);
    let env = env_of(&svc, container_index)?;
    Ok(m::build_preview(&svc, &env, &env, changes))
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn apply_scaling(
    state: State<'_, AppState>,
    project: String,
    region: String,
    service: String,
    container_index: usize,
    update: ScalingUpdate,
    expected_etag: String,
    confirm_text: Option<String>,
    validate_only: bool,
) -> R<ApplyResult> {
    state
        .guard_write(&project, &service, confirm_text.as_deref())
        .await?;

    let fresh = fresh_and_check(&state, &project, &region, &service, &expected_etag).await?;

    let changes = m::describe_scaling_changes(&fresh, container_index, &update);
    let env = env_of(&fresh, container_index)?;
    let preview = m::build_preview(&fresh, &env, &env, changes.clone());

    if changes.is_empty() && !validate_only {
        return Ok(ApplyResult {
            outcome: run::PatchOutcome {
                operation: None,
                done: true,
                new_revision: None,
                message: "Các giá trị bạn nhập giống hệt cấu hình hiện tại — app không tạo revision mới."
                    .to_string(),
            },
            new_etag: fresh.get("etag").and_then(|v| v.as_str()).map(String::from),
            validated_only: false,
            preview,
        });
    }

    let payload = m::apply_scaling(&fresh, container_index, &update).map_err(CmdError::from)?;

    let result = run::patch_service(
        &state.gcp,
        &project,
        &region,
        &service,
        &payload,
        validate_only,
    )
    .await;

    finish(
        &state,
        &project,
        &region,
        &service,
        if validate_only {
            Action::ValidateOnly
        } else {
            Action::UpdateScaling
        },
        changes,
        preview,
        validate_only,
        result,
    )
    .await
}

/// Ghi audit rồi trả kết quả. Thất bại cũng phải được ghi lại — dấu vết thao tác thất
/// bại là thứ hay cần nhất khi truy nguyên sự cố.
#[allow(clippy::too_many_arguments)]
async fn finish(
    state: &AppState,
    project: &str,
    region: &str,
    service: &str,
    action: Action,
    changes: Vec<String>,
    preview: ApplyPreview,
    validate_only: bool,
    result: gcp::Result<run::PatchOutcome>,
) -> R<ApplyResult> {
    match result {
        Ok(outcome) => {
            let ok = outcome.done && outcome.new_revision.is_some();
            let is_failure = outcome.message.contains("KHÔNG khởi động được");

            state
                .record(
                    project,
                    Some(region),
                    Some(service),
                    action,
                    changes,
                    if is_failure {
                        Outcome::Error
                    } else if ok || validate_only {
                        Outcome::Ok
                    } else {
                        Outcome::Pending
                    },
                    &outcome.message,
                    outcome.new_revision.clone(),
                    outcome.operation.clone(),
                )
                .await;

            // Ghi xong thì etag cũ đã hết hiệu lực; lấy bản mới cho lần sửa tiếp theo.
            let new_etag = if validate_only {
                None
            } else {
                run::get_service_raw_fresh(&state.gcp, project, region, service)
                    .await
                    .ok()
                    .and_then(|s| s.get("etag").and_then(|v| v.as_str()).map(String::from))
            };

            Ok(ApplyResult {
                preview,
                outcome,
                new_etag,
                validated_only: validate_only,
            })
        }
        Err(e) => {
            let err: CmdError = e.into();
            state
                .record(
                    project,
                    Some(region),
                    Some(service),
                    action,
                    changes,
                    Outcome::Error,
                    &err.message,
                    None,
                    None,
                )
                .await;
            Err(err)
        }
    }
}

/// Mô tả thay đổi env dạng câu cho audit log. Không bao giờ chứa giá trị secret.
fn describe_env_changes(preview: &ApplyPreview) -> Vec<String> {
    use gcp::types::EnvChange::*;
    preview
        .env_changes
        .iter()
        .map(|c| match c {
            Added { name, value } => format!("thêm {name} = {value}"),
            Removed { name, value } => match value {
                Some(v) => format!("xoá {name} (giá trị cũ: {v})"),
                None => format!("xoá {name} (biến lấy từ secret)"),
            },
            Changed {
                name,
                before,
                after,
            } => format!("{name}: {before} → {after}"),
            SecretVersionChanged {
                name,
                secret,
                before,
                after,
            } => format!("{name} (secret {secret}): version {before} → {after}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gcp::types::{ApplyPreview, EnvChange};

    fn preview_with(changes: Vec<EnvChange>) -> ApplyPreview {
        ApplyPreview {
            env_changes: changes,
            scaling_changes: vec![],
            next_revision_hint: None,
            traffic_pinned: false,
            warnings: vec![],
        }
    }

    #[test]
    fn mo_ta_thay_doi_env_cho_audit() {
        let p = preview_with(vec![
            EnvChange::Changed {
                name: "LOG_LEVEL".into(),
                before: "info".into(),
                after: "debug".into(),
            },
            EnvChange::Added {
                name: "NEW".into(),
                value: "1".into(),
            },
        ]);
        let d = describe_env_changes(&p);
        assert_eq!(d[0], "LOG_LEVEL: info → debug");
        assert_eq!(d[1], "thêm NEW = 1");
    }

    #[test]
    fn audit_khong_ghi_gia_tri_bien_secret_bi_xoa() {
        let p = preview_with(vec![EnvChange::Removed {
            name: "DB_PASSWORD".into(),
            value: None,
        }]);
        let d = describe_env_changes(&p);
        assert_eq!(d[0], "xoá DB_PASSWORD (biến lấy từ secret)");
        assert!(!d[0].contains('='), "không được có dấu = kèm giá trị: {}", d[0]);
    }

    #[test]
    fn audit_ghi_doi_version_secret_khong_ghi_gia_tri() {
        let p = preview_with(vec![EnvChange::SecretVersionChanged {
            name: "JWT".into(),
            secret: "jwt-key".into(),
            before: "3".into(),
            after: "latest".into(),
        }]);
        let d = describe_env_changes(&p);
        assert_eq!(d[0], "JWT (secret jwt-key): version 3 → latest");
    }
}
