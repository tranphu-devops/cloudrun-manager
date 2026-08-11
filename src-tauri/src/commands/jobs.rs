//! Command cho Cloud Run Jobs.

use gcp::jobs::{self, JobsOverview, RunJobOutcome};
use serde::Serialize;
use tauri::State;

use crate::audit::{Action, Outcome};
use crate::error::CmdError;
use crate::state::AppState;

type R<T> = Result<T, CmdError>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobsResult {
    #[serde(flatten)]
    pub overview: JobsOverview,
    pub age_seconds: u64,
}

/// Toàn cảnh Jobs: 2 call cho 196 job (Run list + Scheduler list mỗi region).
#[tauri::command]
pub async fn jobs_overview(state: State<'_, AppState>, project: String) -> R<JobsResult> {
    state.guard_project(&project).await?;
    let overview = jobs::overview(&state.gcp, &project).await?;
    let age_seconds = state
        .gcp
        .cache
        .age(&format!("jobs:{project}:list"))
        .await
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Ok(JobsResult {
        overview,
        age_seconds,
    })
}

#[tauri::command]
pub async fn refresh_jobs(state: State<'_, AppState>, project: String) -> R<JobsResult> {
    state.guard_project(&project).await?;
    state
        .gcp
        .cache
        .invalidate_prefix(&format!("jobs:{project}"))
        .await;
    state
        .gcp
        .cache
        .invalidate_prefix(&format!("sched:{project}"))
        .await;
    jobs_overview(state, project).await
}

#[tauri::command]
pub async fn get_job(
    state: State<'_, AppState>,
    project: String,
    region: String,
    job: String,
) -> R<serde_json::Value> {
    state.guard_project(&project).await?;
    Ok(jobs::get_job_raw(&state.gcp, &project, &region, &job).await?)
}

/// Chạy job ngay.
///
/// # Vì sao cần thêm một lớp so với sửa env
///
/// `jobs:run` **không idempotent**: gọi hai lần tạo hai execution, và một job batch có thể
/// xử lý trùng dữ liệu. Nên ngoài `guard_write` (read-only + gõ tên) còn:
/// - Chặn nếu job đang có execution chạy dở, trừ khi `force = true`.
/// - Không auto-retry (đã xử lý ở tầng `post_no_retry`).
#[tauri::command]
pub async fn run_job(
    state: State<'_, AppState>,
    project: String,
    region: String,
    job: String,
    confirm_text: Option<String>,
    force: bool,
) -> R<RunJobOutcome> {
    // Dùng tên job làm "tên tài nguyên" cần gõ để xác nhận.
    state
        .guard_write(&project, &job, confirm_text.as_deref())
        .await?;

    // Kiểm trạng thái hiện tại trước khi chạy — chạy chồng lên execution đang dở là đúng
    // cái cách tạo ra xử lý trùng.
    if !force {
        let ov = jobs::overview(&state.gcp, &project).await?;
        let row = jobs::require_job(&ov, &job).map_err(CmdError::from)?;
        if row.last_execution_status == gcp::jobs::ExecStatus::Running {
            return Err(CmdError::new(
                "jobRunning",
                format!(
                    "Job `{job}` đang có execution chạy dở ({}). Chạy thêm một lần nữa bây giờ có \
                     thể làm job xử lý trùng dữ liệu.\n\nNếu bạn chắc là an toàn, bật \"Chạy dù \
                     đang có execution\" rồi thử lại.",
                    row.last_execution.as_deref().unwrap_or("không rõ tên")
                ),
            ));
        }
    }

    let result = jobs::run_job(&state.gcp, &project, &region, &job).await;

    match result {
        Ok(o) => {
            state
                .record(
                    &project,
                    Some(&region),
                    Some(&job),
                    Action::RunJob,
                    vec![format!(
                        "chạy tay job {job}{}",
                        if force { " (force)" } else { "" }
                    )],
                    Outcome::Ok,
                    &o.message,
                    o.execution.clone(),
                    o.operation.clone(),
                )
                .await;
            Ok(o)
        }
        Err(e) => {
            let err: CmdError = e.into();
            state
                .record(
                    &project,
                    Some(&region),
                    Some(&job),
                    Action::RunJob,
                    vec![format!("chạy tay job {job}")],
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

/// Tạm dừng / bật lại một Cloud Scheduler job.
///
/// Đây chính là hành động cần khi gặp cron chạy loạn: dừng ngay rồi sửa sau. Thao tác đảo
/// lại được nên chỉ cần `guard_write` bình thường.
#[tauri::command]
pub async fn set_schedule_paused(
    state: State<'_, AppState>,
    project: String,
    region: String,
    scheduler_job: String,
    paused: bool,
    confirm_text: Option<String>,
) -> R<String> {
    state
        .guard_write(&project, &scheduler_job, confirm_text.as_deref())
        .await?;

    let result =
        jobs::set_scheduler_paused(&state.gcp, &project, &region, &scheduler_job, paused).await;

    let (outcome, msg) = match &result {
        Ok(m) => (Outcome::Ok, m.clone()),
        Err(e) => (Outcome::Error, e.to_string()),
    };
    state
        .record(
            &project,
            Some(&region),
            Some(&scheduler_job),
            Action::SetSchedulePaused,
            vec![format!(
                "{} lịch {scheduler_job}",
                if paused { "tạm dừng" } else { "bật lại" }
            )],
            outcome,
            &msg,
            None,
            None,
        )
        .await;

    result.map_err(CmdError::from)
}
