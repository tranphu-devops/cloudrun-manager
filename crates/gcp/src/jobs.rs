//! Cloud Run Jobs (Admin API v2) + Cloud Scheduler, và phần join hai nguồn.
//!
//! # Ràng buộc từ hạ tầng thật
//!
//! `example-project` có 196 job tên `job001`…`job233` — tên hoàn toàn không mang thông tin.
//! Toàn bộ dùng **cùng một image** (`.../batch/dev-env:NNNNN`), không có `args`, và
//! phân biệt nhau bằng env `ID`/`JOB_ID`. Nên grid Jobs không thể dựa vào tên hay image;
//! thứ nhận diện được một job là **cron + đường dẫn source + lần chạy cuối**.
//!
//! # Bẫy: template lồng hai lớp
//!
//! ```text
//! Service:  service.template.containers[0]
//! Job:      job.template.template.containers[0]
//!                 │        └─ TaskTemplate: containers, maxRetries, timeout
//!                 └─ ExecutionTemplate: taskCount, parallelism
//! ```
//!
//! Code viết theo trí nhớ từ Service sẽ đọc `job.template.containers` và nhận `None`,
//! rồi báo "job không có container" cho một job hoàn toàn bình thường.
//!
//! # Ít call cho 196 job
//!
//! `jobs.list` trả về **Job đầy đủ kèm `latestCreatedExecution.completionStatus`**, nên
//! không cần gọi `executions.list` cho từng job. Cộng thêm 1 call Scheduler mỗi region là đủ
//! dữ liệu cho cả grid. Đây là cùng bài học với sidebar service ở v1.
//!
//! # Bẫy: v2 không nhận `locations/-` cho jobs
//!
//! `services.list` cho phép wildcard `locations/-` (list mọi region trong một call), nhưng
//! `jobs.list` thì **không** — gọi với `-` trả HTTP 400 "invalid argument". Nên jobs phải
//! list theo từng region; region suy từ danh sách service (xem `job_regions`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::client::{seg, GcpClient};
use crate::cronlint::{self, EnvSecretFinding, Finding, Severity};
use crate::error::{GcpError, Result};
use crate::types::{EnvEntry, Health};

const RUN_BASE: &str = "https://run.googleapis.com/v2";
const SCHED_BASE: &str = "https://cloudscheduler.googleapis.com/v1";

fn short(name: &str) -> String {
    name.rsplit('/').next().unwrap_or(name).to_string()
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ExecStatus {
    Succeeded,
    Failed,
    Cancelled,
    Running,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulerJob {
    /// Tên ngắn, ví dụ `batch-dev-env-job204`.
    pub name: String,
    pub region: String,
    pub schedule: String,
    /// **Bắt buộc hiện kèm cron.** `0 15 * * *` là nửa đêm JST nếu timeZone là UTC,
    /// nhưng là 15h chiều nếu timeZone là Asia/Tokyo — cron không có timezone là
    /// thông tin sai.
    pub time_zone: String,
    /// `ENABLED` | `PAUSED` | `DISABLED` | `UPDATE_FAILED`
    pub state: String,
    /// Tên Cloud Run job mà nó gọi, rút từ `httpTarget.uri`. `None` nếu target không
    /// phải `jobs/{x}:run`.
    pub target_job: Option<String>,
    pub last_attempt_time: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobRow {
    pub name: String,
    pub region: String,
    pub image: Option<String>,
    /// Từ annotation `batch/source` — đường dẫn file yaml trong repo deploy.
    ///
    /// Đây là convention của pipeline nhóm vận hành, không phải field của Cloud Run. Với 196 job
    /// tên vô nghĩa thì đây là thứ nhận diện tốt nhất, nên vẫn surface — chỉ là code
    /// không được phụ thuộc vào việc nó tồn tại.
    pub source_path: Option<String>,
    /// Từ annotation `batch/schedule`. Đối chiếu với Scheduler để phát hiện lệch.
    pub declared_schedule: Option<String>,
    pub task_count: Option<i64>,
    pub parallelism: Option<i64>,
    pub max_retries: Option<i64>,
    pub timeout: Option<String>,
    pub cpu: Option<String>,
    pub memory: Option<String>,
    pub service_account: Option<String>,
    pub execution_count: Option<i64>,
    pub last_execution: Option<String>,
    pub last_execution_status: ExecStatus,
    pub last_execution_time: Option<String>,
    pub health: Health,
    pub health_message: Option<String>,
    pub labels: BTreeMap<String, String>,
    /// Scheduler đang trỏ tới job này (có thể nhiều hơn một).
    pub schedulers: Vec<SchedulerJob>,
    /// Số lần chạy mỗi ngày, suy từ cron. `None` khi không phân tích được hoặc không có lịch.
    pub runs_per_day: Option<u32>,
    /// Kết quả linter cron + đối chiếu.
    pub findings: Vec<Finding>,
    /// Env plain trông như secret.
    pub env_secrets: Vec<EnvSecretFinding>,
    pub env_count: usize,
    pub secret_env_count: usize,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct JobsOverview {
    pub jobs: Vec<JobRow>,
    /// Scheduler trỏ tới job không tồn tại — mỗi lần fire là một lỗi im lặng.
    pub orphan_schedulers: Vec<SchedulerJob>,
    pub total_runs_per_day: u32,
    /// Không lấy được danh sách Scheduler (thiếu quyền / API chưa enable). Khi đó cột
    /// cron trống vì **thiếu dữ liệu**, không phải vì job không có lịch — phải nói rõ.
    pub scheduler_unavailable: bool,
    pub scheduler_note: Option<String>,
}

// ---------------------------------------------------------------------------
// Parse Job
// ---------------------------------------------------------------------------

/// Container đầu tiên của job. Đây là chỗ dễ sai nhất — xem chú thích module.
fn task_container(job: &Value) -> Option<&Value> {
    job.get("template")?            // ExecutionTemplate
        .get("template")?           // TaskTemplate  ← lớp mà Service không có
        .get("containers")?
        .as_array()?
        .first()
}

fn task_template(job: &Value) -> Option<&Value> {
    job.get("template")?.get("template")
}

fn health_of(job: &Value) -> (Health, Option<String>) {
    let reconciling = job
        .get("reconciling")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let tc = job.get("terminalCondition");
    let state = tc
        .and_then(|c| c.get("state"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let msg = tc
        .and_then(|c| c.get("message"))
        .and_then(|v| v.as_str())
        .map(String::from);

    let h = match state {
        "CONDITION_SUCCEEDED" if reconciling => Health::Reconciling,
        "CONDITION_SUCCEEDED" => Health::Ready,
        "CONDITION_FAILED" => Health::NotReady,
        "CONDITION_PENDING" | "CONDITION_RECONCILING" => Health::Reconciling,
        _ if reconciling => Health::Reconciling,
        _ => Health::Unknown,
    };
    (h, msg)
}

fn exec_status(raw: Option<&str>) -> ExecStatus {
    match raw.unwrap_or("") {
        s if s.contains("SUCCEEDED") => ExecStatus::Succeeded,
        s if s.contains("CANCELLED") => ExecStatus::Cancelled,
        s if s.contains("FAILED") => ExecStatus::Failed,
        "" => ExecStatus::Unknown,
        _ => ExecStatus::Running,
    }
}

/// Đọc env của job dưới dạng cặp tên/giá trị plain (bỏ qua secret-ref) để đưa vào scanner.
fn plain_env_pairs(container: &Value) -> Vec<(String, String)> {
    container
        .get("env")
        .and_then(|e| e.as_array())
        .map(|arr| {
            arr.iter()
                .filter(|e| e.get("valueSource").is_none())
                .filter_map(|e| {
                    let n = e.get("name")?.as_str()?.to_string();
                    let v = e.get("value").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    Some((n, v))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn env_entries(container: &Value) -> Vec<EnvEntry> {
    crate::mutate::parse_env(container)
}

// ---------------------------------------------------------------------------
// Join + lint — hàm thuần, đây là chỗ được test kỹ
// ---------------------------------------------------------------------------

/// Ghép Job với Scheduler, chạy linter, và tìm scheduler mồ côi.
///
/// Là hàm thuần (không I/O) nên test được toàn bộ logic đối chiếu mà không cần GCP.
pub fn build_overview(jobs_raw: &[Value], schedulers: &[SchedulerJob]) -> JobsOverview {
    let style = cronlint::majority_step_style(schedulers.iter().map(|s| s.schedule.as_str()));

    // job name -> các scheduler trỏ tới nó
    let mut by_target: BTreeMap<String, Vec<SchedulerJob>> = BTreeMap::new();
    for s in schedulers {
        if let Some(t) = &s.target_job {
            by_target.entry(t.clone()).or_default().push(s.clone());
        }
    }

    let mut rows: Vec<JobRow> = Vec::with_capacity(jobs_raw.len());
    let mut total_runs = 0u32;

    for job in jobs_raw {
        let Some(full) = job.get("name").and_then(|v| v.as_str()) else {
            continue;
        };
        let parts: Vec<&str> = full.split('/').collect();
        if parts.len() < 6 {
            continue;
        }
        let region = parts[3].to_string();
        let name = parts[5].to_string();

        let container = task_container(job);
        let tt = task_template(job);
        let limits = container
            .and_then(|c| c.get("resources"))
            .and_then(|r| r.get("limits"));

        let annotations = crate::mutate::string_map(job.get("annotations"));
        let mine = by_target.get(&name).cloned().unwrap_or_default();

        // Số lần chạy: lấy tổng của tất cả scheduler đang ENABLED trỏ tới job này.
        let mut runs: Option<u32> = None;
        for s in mine.iter().filter(|s| s.state == "ENABLED") {
            if let Some(n) = cronlint::runs_per_day(&s.schedule) {
                runs = Some(runs.unwrap_or(0) + n);
            }
        }
        total_runs += runs.unwrap_or(0);

        // --- findings ---
        let mut findings: Vec<Finding> = Vec::new();

        for s in &mine {
            for f in cronlint::lint_schedule(&s.schedule, style) {
                findings.push(Finding {
                    message: format!("[{}] {}", s.name, f.message),
                    ..f
                });
            }
        }

        if mine.is_empty() {
            findings.push(Finding {
                severity: Severity::Warn,
                code: "job.noSchedule",
                message:
                    "Không có Cloud Scheduler nào trỏ tới job này nên nó không tự chạy. \
                     Hoặc job đã chết và nên xoá, hoặc nó được chạy tay / gọi từ nơi khác."
                        .to_string(),
                suggestion: None,
            });
        }

        if mine.iter().all(|s| s.state != "ENABLED") && !mine.is_empty() {
            findings.push(Finding {
                severity: Severity::Info,
                code: "job.allSchedulesPaused",
                message: format!(
                    "Tất cả {} lịch của job này đang không ở trạng thái ENABLED.",
                    mine.len()
                ),
                suggestion: None,
            });
        }

        // Lệch giữa annotation `batch/schedule` (khai báo trong repo) và Scheduler thật.
        // Lệch nghĩa là repo và hạ tầng không còn khớp — sửa repo sẽ không có tác dụng,
        // hoặc ngược lại, deploy lần sau sẽ ghi đè thay đổi làm tay.
        if let Some(declared) = annotations.get("batch/schedule") {
            for s in mine.iter() {
                if s.schedule.trim() != declared.trim() {
                    findings.push(Finding {
                        severity: Severity::Warn,
                        code: "job.scheduleDrift",
                        message: format!(
                            "Cron khai báo trong repo (`{declared}`) khác cron đang chạy thật trên \
                             Scheduler `{}` (`{}`). Sửa repo sẽ không đổi hành vi hiện tại, còn \
                             deploy lần sau sẽ ghi đè thay đổi làm tay.",
                            s.name, s.schedule
                        ),
                        suggestion: None,
                    });
                }
            }
        }

        // Nhiều timeZone khác nhau trong cùng một job là chuyện gần như luôn sai.
        let zones: Vec<&str> = {
            let mut z: Vec<&str> = mine.iter().map(|s| s.time_zone.as_str()).collect();
            z.sort();
            z.dedup();
            z
        };
        if zones.len() > 1 {
            findings.push(Finding {
                severity: Severity::Warn,
                code: "job.mixedTimeZone",
                message: format!(
                    "Các lịch của job này dùng timezone khác nhau ({}). Rất dễ dẫn tới hiểu sai \
                     giờ chạy.",
                    zones.join(", ")
                ),
                suggestion: None,
            });
        }

        let env = container.map(env_entries).unwrap_or_default();
        let env_secrets = container
            .map(|c| cronlint::scan_env_secrets(&plain_env_pairs(c)))
            .unwrap_or_default();

        let (health, health_message) = health_of(job);
        let last = job.get("latestCreatedExecution");

        rows.push(JobRow {
            secret_env_count: env
                .iter()
                .filter(|e| e.kind == crate::types::EnvKind::SecretRef)
                .count(),
            env_count: env.len(),
            name,
            region,
            image: container
                .and_then(|c| c.get("image"))
                .and_then(|v| v.as_str())
                .map(String::from),
            source_path: annotations.get("batch/source").cloned(),
            declared_schedule: annotations.get("batch/schedule").cloned(),
            task_count: job
                .get("template")
                .and_then(|t| t.get("taskCount"))
                .and_then(|v| v.as_i64()),
            parallelism: job
                .get("template")
                .and_then(|t| t.get("parallelism"))
                .and_then(|v| v.as_i64()),
            max_retries: tt.and_then(|t| t.get("maxRetries")).and_then(as_i64_loose),
            timeout: tt
                .and_then(|t| t.get("timeout"))
                .and_then(|v| v.as_str())
                .map(String::from),
            cpu: limits
                .and_then(|l| l.get("cpu"))
                .and_then(|v| v.as_str())
                .map(String::from),
            memory: limits
                .and_then(|l| l.get("memory"))
                .and_then(|v| v.as_str())
                .map(String::from),
            service_account: tt
                .and_then(|t| t.get("serviceAccount"))
                .and_then(|v| v.as_str())
                .map(String::from),
            execution_count: job.get("executionCount").and_then(as_i64_loose),
            last_execution: last
                .and_then(|e| e.get("name"))
                .and_then(|v| v.as_str())
                .map(short),
            last_execution_status: exec_status(
                last.and_then(|e| e.get("completionStatus"))
                    .and_then(|v| v.as_str()),
            ),
            last_execution_time: last
                .and_then(|e| e.get("completionTime").or_else(|| e.get("createTime")))
                .and_then(|v| v.as_str())
                .map(String::from),
            health,
            health_message,
            labels: crate::mutate::string_map(job.get("labels")),
            schedulers: mine,
            runs_per_day: runs,
            findings,
            env_secrets,
        });
    }

    rows.sort_by(|a, b| a.region.cmp(&b.region).then(a.name.cmp(&b.name)));

    let existing: std::collections::HashSet<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    let orphan_schedulers: Vec<SchedulerJob> = schedulers
        .iter()
        .filter(|s| match &s.target_job {
            Some(t) => !existing.contains(t.as_str()),
            // Target không phải `jobs/x:run` thì không phải mồ côi, chỉ là không liên quan.
            None => false,
        })
        .cloned()
        .collect();

    JobsOverview {
        jobs: rows,
        orphan_schedulers,
        total_runs_per_day: total_runs,
        scheduler_unavailable: false,
        scheduler_note: None,
    }
}

/// Protobuf serialize int64 thành string; `executionCount`/`maxRetries` có thể là cả hai dạng.
fn as_i64_loose(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| v.as_str()?.parse().ok())
}

/// Rút tên Cloud Run job từ `httpTarget.uri` của Scheduler.
///
/// URI thật: `https://asia-northeast1-run.googleapis.com/v2/projects/P/locations/L/jobs/job204:run`
pub fn target_job_from_uri(uri: &str) -> Option<String> {
    let after = uri.rsplit_once("/jobs/")?.1;
    let name = after.strip_suffix(":run").unwrap_or(after);
    let name = name.split(['?', '#']).next().unwrap_or(name);
    if name.is_empty() || name.contains('/') {
        None
    } else {
        Some(name.to_string())
    }
}

// ---------------------------------------------------------------------------
// API
// ---------------------------------------------------------------------------

/// Region có Cloud Run trong project, suy từ danh sách service (đã cache cho sidebar).
///
/// Vì sao cần: `jobs.list` của Admin v2 **không** nhận wildcard `locations/-` (trả 400
/// "invalid argument"), khác với `services.list`. Nên phải list job theo từng region. Job gần
/// như luôn nằm cùng region với service, và `list_services_raw` dùng chung cache nên đây
/// thường là cache hit chứ không phải call thừa.
async fn job_regions(client: &GcpClient, project: &str) -> Result<Vec<String>> {
    let services = crate::run::list_services_raw(client, project).await?;
    let mut regions: Vec<String> = services
        .iter()
        .filter_map(|s| s.get("name")?.as_str()?.split('/').nth(3).map(String::from))
        .collect();
    regions.sort();
    regions.dedup();
    // Project chưa có service nào để suy region: dùng region mặc định (app đang khoá vào
    // asia-northeast1). Không có service nhưng có job là trường hợp hiếm.
    if regions.is_empty() {
        regions.push("asia-northeast1".to_string());
    }
    Ok(regions)
}

/// JSON thô của toàn bộ Job trong project, mọi region.
///
/// List theo từng region vì v2 không cho wildcard `-` với jobs (xem `job_regions`).
pub async fn list_jobs_raw(client: &GcpClient, project: &str) -> Result<Vec<Value>> {
    let cache_key = format!("jobs:{project}:list");
    if let Some(hit) = client.cache.get(&cache_key).await {
        if let Ok(v) = serde_json::from_str::<Vec<Value>>(&hit) {
            return Ok(v);
        }
    }

    let regions = job_regions(client, project).await?;

    let mut all = Vec::new();
    let ctx = format!("liệt kê Cloud Run Job của project {project}");

    for region in &regions {
        let mut token: Option<String> = None;
        loop {
            let mut url =
                format!("{RUN_BASE}/projects/{project}/locations/{region}/jobs?pageSize=200");
            if let Some(t) = &token {
                url.push_str("&pageToken=");
                url.push_str(&seg(t));
            }

            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Page {
                #[serde(default)]
                jobs: Vec<Value>,
                #[serde(default)]
                next_page_token: Option<String>,
            }
            let page: Page = client.get(&url, &ctx).await?;
            all.extend(page.jobs);

            match page.next_page_token {
                Some(t) if !t.is_empty() => token = Some(t),
                _ => break,
            }
            if all.len() > 5000 {
                break;
            }
        }
    }

    if let Ok(s) = serde_json::to_string(&all) {
        client
            .cache
            .put(cache_key, s.as_str(), crate::ttl::SERVICES)
            .await;
    }
    Ok(all)
}

/// Scheduler job của một region. Cloud Scheduler **không** nhận wildcard `-` cho location.
pub async fn list_scheduler_jobs(
    client: &GcpClient,
    project: &str,
    region: &str,
) -> Result<Vec<SchedulerJob>> {
    let mut out = Vec::new();
    let mut token: Option<String> = None;
    let ctx = format!("liệt kê Cloud Scheduler job ở {region}");

    loop {
        let mut url =
            format!("{SCHED_BASE}/projects/{project}/locations/{region}/jobs?pageSize=500");
        if let Some(t) = &token {
            url.push_str("&pageToken=");
            url.push_str(&seg(t));
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Page {
            #[serde(default)]
            jobs: Vec<Value>,
            #[serde(default)]
            next_page_token: Option<String>,
        }
        let cache_key = format!(
            "sched:{project}:{region}:{}",
            token.as_deref().unwrap_or("p0")
        );
        let page: Page = client
            .get_cached(&url, &ctx, &cache_key, crate::ttl::SERVICES)
            .await?;

        for j in &page.jobs {
            let Some(full) = j.get("name").and_then(|v| v.as_str()) else {
                continue;
            };
            out.push(SchedulerJob {
                name: short(full),
                region: region.to_string(),
                schedule: j
                    .get("schedule")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                time_zone: j
                    .get("timeZone")
                    .and_then(|v| v.as_str())
                    // Cloud Scheduler mặc định UTC khi không khai báo.
                    .unwrap_or("UTC")
                    .to_string(),
                state: j
                    .get("state")
                    .and_then(|v| v.as_str())
                    .unwrap_or("STATE_UNSPECIFIED")
                    .to_string(),
                target_job: j
                    .get("httpTarget")
                    .and_then(|h| h.get("uri"))
                    .and_then(|v| v.as_str())
                    .and_then(target_job_from_uri),
                last_attempt_time: j
                    .get("lastAttemptTime")
                    .and_then(|v| v.as_str())
                    .map(String::from),
            });
        }

        match page.next_page_token {
            Some(t) if !t.is_empty() => token = Some(t),
            _ => break,
        }
        if out.len() > 5000 {
            break;
        }
    }
    Ok(out)
}

/// Toàn cảnh Jobs: 1 call Run + 1 call Scheduler mỗi region.
///
/// Scheduler lỗi (thiếu quyền, API chưa enable) **không** làm fail cả màn hình — grid vẫn
/// hiện job, chỉ là cột cron trống và có cờ `scheduler_unavailable` để UI nói rõ đây là
/// thiếu dữ liệu chứ không phải job không có lịch.
pub async fn overview(client: &GcpClient, project: &str) -> Result<JobsOverview> {
    let jobs = list_jobs_raw(client, project).await?;

    let mut regions: Vec<String> = jobs
        .iter()
        .filter_map(|j| {
            let full = j.get("name")?.as_str()?;
            full.split('/').nth(3).map(String::from)
        })
        .collect();
    regions.sort();
    regions.dedup();

    let mut schedulers = Vec::new();
    let mut failed: Option<String> = None;
    for r in &regions {
        match list_scheduler_jobs(client, project, r).await {
            Ok(mut s) => schedulers.append(&mut s),
            Err(e) => failed = Some(e.to_string()),
        }
    }

    let mut ov = build_overview(&jobs, &schedulers);
    if let Some(msg) = failed {
        if schedulers.is_empty() {
            ov.scheduler_unavailable = true;
            ov.scheduler_note = Some(format!(
                "Không lấy được danh sách Cloud Scheduler nên cột lịch đang trống vì THIẾU DỮ LIỆU, \
                 không phải vì job không có lịch. Nguyên nhân: {msg}"
            ));
        } else {
            ov.scheduler_note = Some(format!("Một số region không lấy được Scheduler: {msg}"));
        }
    }
    Ok(ov)
}

pub async fn get_job_raw(
    client: &GcpClient,
    project: &str,
    region: &str,
    job: &str,
) -> Result<Value> {
    let url = format!("{RUN_BASE}/projects/{project}/locations/{region}/jobs/{}", seg(job));
    let ctx = format!("xem chi tiết job {job}");
    client
        .get_cached(
            &url,
            &ctx,
            &format!("jobs:{project}:{region}:{job}"),
            crate::ttl::SERVICE_DETAIL,
        )
        .await
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunJobOutcome {
    pub operation: Option<String>,
    pub execution: Option<String>,
    pub message: String,
}

/// Chạy job ngay (`jobs:run`).
///
/// # Không idempotent
///
/// Khác hẳn sửa env (gửi hai lần cho cùng kết quả), gọi cái này hai lần sẽ tạo **hai
/// execution** và một job batch có thể xử lý trùng dữ liệu. Vì vậy:
///
/// - Tầng gọi phải qua `guard_write` và có xác nhận riêng.
/// - **Không bao giờ auto-retry.** `client.rs` chỉ chặn retry cho `PATCH`; đây là `POST`
///   nên phải dùng `post_no_retry`.
pub async fn run_job(
    client: &GcpClient,
    project: &str,
    region: &str,
    job: &str,
) -> Result<RunJobOutcome> {
    let url = format!(
        "{RUN_BASE}/projects/{project}/locations/{region}/jobs/{}:run",
        seg(job)
    );
    let op: Value = client
        .post_no_retry(&url, &serde_json::json!({}), &format!("chạy job {job}"))
        .await?;

    client.cache.invalidate_prefix(&format!("jobs:{project}")).await;

    let execution = op
        .get("metadata")
        .and_then(|m| m.get("name"))
        .or_else(|| op.get("response").and_then(|r| r.get("name")))
        .and_then(|v| v.as_str())
        .map(short);

    Ok(RunJobOutcome {
        operation: op.get("name").and_then(|v| v.as_str()).map(String::from),
        message: match &execution {
            Some(e) => format!("Đã tạo execution {e}. Theo dõi tiến trình ở tab Log."),
            None => "Đã gửi yêu cầu chạy job. Xem tab Log để theo dõi.".to_string(),
        },
        execution,
    })
}

/// Tạm dừng / bật lại một Cloud Scheduler job.
///
/// Đưa vào v2 vì đây chính là hành động cần làm khi gặp cron chạy loạn: dừng ngay rồi sửa
/// sau. Thao tác **đảo lại được**, khác hẳn xoá.
pub async fn set_scheduler_paused(
    client: &GcpClient,
    project: &str,
    region: &str,
    scheduler_job: &str,
    paused: bool,
) -> Result<String> {
    let verb = if paused { "pause" } else { "resume" };
    let url = format!(
        "{SCHED_BASE}/projects/{project}/locations/{region}/jobs/{}:{verb}",
        seg(scheduler_job)
    );
    let ctx = format!(
        "{} lịch {scheduler_job}",
        if paused { "tạm dừng" } else { "bật lại" }
    );
    let resp: Value = client
        .post_no_retry(&url, &serde_json::json!({}), &ctx)
        .await?;

    client.cache.invalidate_prefix(&format!("sched:{project}")).await;
    client.cache.invalidate_prefix(&format!("jobs:{project}")).await;

    let state = resp
        .get("state")
        .and_then(|v| v.as_str())
        .unwrap_or("không rõ");
    Ok(format!(
        "Lịch `{scheduler_job}` giờ ở trạng thái {state}."
    ))
}

/// Bảo đảm job tồn tại trước khi làm gì với nó — message rõ hơn 404 thô.
pub fn require_job<'a>(ov: &'a JobsOverview, name: &str) -> Result<&'a JobRow> {
    ov.jobs.iter().find(|j| j.name == name).ok_or_else(|| {
        GcpError::Invalid(format!(
            "Không có job nào tên `{name}` trong project. Bấm Reload nếu job vừa được tạo."
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Job dựng theo đúng shape v2, đối chiếu từ `job204` thật của `example-project`.
    fn job(name: &str, extra: Value) -> Value {
        let mut v = json!({
          "name": format!("projects/example-project/locations/asia-northeast1/jobs/{name}"),
          "labels": { "env": "dev-env", "group": "job", "tier": "batch" },
          "annotations": {
            "batch/schedule": "* 17 * * *",
            "batch/source": format!("deployments/cloud-run/base/{name}/job.yaml"),
            "batch/suspended": "false"
          },
          "executionCount": 1324,
          "template": {
            "taskCount": 1,
            "parallelism": 1,
            "template": {
              "maxRetries": 0,
              "timeout": "3600s",
              "serviceAccount": "batch-sa@example-project.iam.gserviceaccount.com",
              "containers": [{
                "image": "asia-northeast1-docker.pkg.dev/example-develop/images/batch/dev-env:18273",
                "resources": { "limits": { "cpu": "1", "memory": "2Gi" } },
                "env": [
                  { "name": "ID", "value": name },
                  { "name": "SPRING_PROFILES_ACTIVE", "value": "dev-env" },
                  { "name": "STRIPE_API_KEY", "value": "sk_test_51HI6vlAlDlJA4baC6atQhvlR8Yd" },
                  { "name": "DB_PASS", "valueSource": { "secretKeyRef": { "secret": "s", "version": "latest" } } }
                ]
              }]
            }
          },
          "terminalCondition": { "type": "Ready", "state": "CONDITION_SUCCEEDED" },
          "latestCreatedExecution": {
            "name": format!("projects/p/locations/l/executions/{name}-x25jp"),
            "createTime": "2026-08-04T08:59:04Z",
            "completionTime": "2026-08-04T08:59:28Z",
            "completionStatus": "EXECUTION_SUCCEEDED"
          },
          "etag": "\"abc\""
        });
        if let (Some(base), Some(ov)) = (v.as_object_mut(), extra.as_object()) {
            for (k, val) in ov {
                base.insert(k.clone(), val.clone());
            }
        }
        v
    }

    fn sched(name: &str, target: &str, schedule: &str, state: &str) -> SchedulerJob {
        SchedulerJob {
            name: name.into(),
            region: "asia-northeast1".into(),
            schedule: schedule.into(),
            time_zone: "UTC".into(),
            state: state.into(),
            target_job: Some(target.into()),
            last_attempt_time: None,
        }
    }

    // --- template lồng hai lớp -------------------------------------------

    #[test]
    fn doc_dung_container_o_lop_template_template() {
        let j = job("job204", json!({}));
        let c = task_container(&j).expect("phải tìm được container ở template.template");
        assert!(c["image"].as_str().unwrap().contains("batch/dev-env"));
    }

    #[test]
    fn doc_sai_mot_lop_thi_khong_thay_gi() {
        // Chốt lại chính cái bẫy: đường của Service không dùng được cho Job.
        let j = job("job204", json!({}));
        assert!(
            j.get("template").and_then(|t| t.get("containers")).is_none(),
            "job KHÔNG có template.containers — Service mới có. Nếu assert này fail thì \
             Google đã đổi shape và cần đọc lại toàn bộ module."
        );
    }

    #[test]
    fn max_retries_va_timeout_lay_tu_task_template() {
        let ov = build_overview(&[job("job204", json!({}))], &[]);
        let r = &ov.jobs[0];
        assert_eq!(r.max_retries, Some(0));
        assert_eq!(r.timeout.as_deref(), Some("3600s"));
        assert_eq!(r.task_count, Some(1), "taskCount ở ExecutionTemplate");
        assert_eq!(r.cpu.as_deref(), Some("1"));
        assert_eq!(r.memory.as_deref(), Some("2Gi"));
        assert_eq!(
            r.service_account.as_deref(),
            Some("batch-sa@example-project.iam.gserviceaccount.com")
        );
    }

    #[test]
    fn execution_count_dang_string_van_doc_duoc() {
        // Protobuf serialize int64 thành string ở một số endpoint.
        let ov = build_overview(&[job("j", json!({ "executionCount": "999" }))], &[]);
        assert_eq!(ov.jobs[0].execution_count, Some(999));
    }

    #[test]
    fn lay_duoc_trang_thai_lan_chay_cuoi_tu_list_khong_can_goi_executions() {
        let ov = build_overview(&[job("job204", json!({}))], &[]);
        let r = &ov.jobs[0];
        assert_eq!(r.last_execution_status, ExecStatus::Succeeded);
        assert_eq!(r.last_execution.as_deref(), Some("job204-x25jp"));
        assert_eq!(r.last_execution_time.as_deref(), Some("2026-08-04T08:59:28Z"));
    }

    #[test]
    fn execution_that_bai_duoc_nhan_dien() {
        let j = job(
            "j",
            json!({ "latestCreatedExecution": { "name": "p/executions/j-1", "completionStatus": "EXECUTION_FAILED" } }),
        );
        assert_eq!(build_overview(&[j], &[]).jobs[0].last_execution_status, ExecStatus::Failed);
    }

    #[test]
    fn chua_chay_lan_nao_thi_status_unknown() {
        let mut j = job("j", json!({}));
        j.as_object_mut().unwrap().remove("latestCreatedExecution");
        let r = &build_overview(&[j], &[]).jobs[0];
        assert_eq!(r.last_execution_status, ExecStatus::Unknown);
        assert!(r.last_execution.is_none());
    }

    // --- nhận diện job ----------------------------------------------------

    #[test]
    fn surface_duoc_source_path_vi_ten_job_vo_nghia() {
        let r = &build_overview(&[job("job204", json!({}))], &[]).jobs[0];
        assert_eq!(
            r.source_path.as_deref(),
            Some("deployments/cloud-run/base/job204/job.yaml"),
            "với 196 job tên jobNNN thì đây là thứ nhận diện tốt nhất"
        );
    }

    #[test]
    fn job_thieu_annotation_van_hoat_dong() {
        // `batch/source` là convention của pipeline nhóm vận hành, không phải field Cloud Run.
        let mut j = job("j", json!({}));
        j.as_object_mut().unwrap().remove("annotations");
        let r = &build_overview(&[j], &[]).jobs[0];
        assert!(r.source_path.is_none());
        assert!(r.declared_schedule.is_none());
        assert_eq!(r.name, "j", "vẫn phải đọc được job");
    }

    // --- join + linter ----------------------------------------------------

    #[test]
    fn ghep_scheduler_vao_dung_job_va_bat_loi_minute_wildcard() {
        let ov = build_overview(
            &[job("job204", json!({})), job("job215", json!({}))],
            &[
                sched("batch-dev-env-job204", "job204", "* 17 * * *", "ENABLED"),
                sched("batch-dev-env-job215", "job215", "0 17 * * *", "ENABLED"),
            ],
        );

        let j204 = ov.jobs.iter().find(|j| j.name == "job204").unwrap();
        let j215 = ov.jobs.iter().find(|j| j.name == "job215").unwrap();

        assert_eq!(j204.runs_per_day, Some(60));
        assert_eq!(j215.runs_per_day, Some(1));

        assert!(
            j204.findings.iter().any(|f| f.code == "cron.minuteWildcard"),
            "{:?}",
            j204.findings
        );
        assert!(
            !j215.findings.iter().any(|f| f.severity == Severity::High),
            "job đúng không được bị báo High: {:?}",
            j215.findings
        );
        // Message phải nêu tên scheduler để biết sửa cái nào trong ~190 cái.
        let m = &j204
            .findings
            .iter()
            .find(|f| f.code == "cron.minuteWildcard")
            .unwrap()
            .message;
        assert!(m.contains("batch-dev-env-job204"), "{m}");
    }

    #[test]
    fn tong_so_lan_chay_moi_ngay_cua_ca_project() {
        let ov = build_overview(
            &[job("a", json!({})), job("b", json!({}))],
            &[
                sched("s-a", "a", "*/5 * * * *", "ENABLED"),  // 288
                sched("s-b", "b", "0 17 * * *", "ENABLED"),   // 1
            ],
        );
        assert_eq!(ov.total_runs_per_day, 289);
    }

    #[test]
    fn lich_paused_khong_tinh_vao_so_lan_chay() {
        let ov = build_overview(
            &[job("a", json!({}))],
            &[sched("s-a", "a", "*/5 * * * *", "PAUSED")],
        );
        assert_eq!(ov.jobs[0].runs_per_day, None, "lịch đang dừng thì không chạy lần nào");
        assert_eq!(ov.total_runs_per_day, 0);
        assert!(ov.jobs[0]
            .findings
            .iter()
            .any(|f| f.code == "job.allSchedulesPaused"));
    }

    #[test]
    fn job_khong_co_lich_bi_canh_bao() {
        let ov = build_overview(&[job("job999", json!({}))], &[]);
        let f = &ov.jobs[0].findings;
        assert!(f.iter().any(|x| x.code == "job.noSchedule"), "{f:?}");
    }

    #[test]
    fn scheduler_tro_toi_job_khong_ton_tai_la_mo_coi() {
        // Mỗi lần fire là một lỗi im lặng — 196 job vs ~190 scheduler nên phải đối chiếu
        // cả hai chiều.
        let ov = build_overview(
            &[job("job001", json!({}))],
            &[
                sched("s-ok", "job001", "0 17 * * *", "ENABLED"),
                sched("s-orphan", "job-da-bi-xoa", "0 18 * * *", "ENABLED"),
            ],
        );
        assert_eq!(ov.orphan_schedulers.len(), 1);
        assert_eq!(ov.orphan_schedulers[0].name, "s-orphan");
    }

    #[test]
    fn phat_hien_lech_giua_cron_khai_bao_trong_repo_va_cron_thuc_te() {
        // annotation nói `* 17 * * *`, Scheduler thật lại là `0 17 * * *`.
        let ov = build_overview(
            &[job("job204", json!({}))],
            &[sched("s", "job204", "0 17 * * *", "ENABLED")],
        );
        let f = ov.jobs[0]
            .findings
            .iter()
            .find(|x| x.code == "job.scheduleDrift")
            .expect("phải phát hiện lệch");
        assert!(f.message.contains("* 17 * * *"), "{}", f.message);
        assert!(f.message.contains("0 17 * * *"), "{}", f.message);
    }

    #[test]
    fn khong_bao_lech_khi_khai_bao_khop_thuc_te() {
        let ov = build_overview(
            &[job("job204", json!({}))],
            &[sched("s", "job204", "* 17 * * *", "ENABLED")],
        );
        assert!(!ov.jobs[0]
            .findings
            .iter()
            .any(|x| x.code == "job.scheduleDrift"));
    }

    #[test]
    fn nhieu_timezone_cho_cung_mot_job_bi_canh_bao() {
        let mut s1 = sched("s1", "a", "0 17 * * *", "ENABLED");
        let mut s2 = sched("s2", "a", "0 18 * * *", "ENABLED");
        s1.time_zone = "UTC".into();
        s2.time_zone = "Asia/Tokyo".into();
        let ov = build_overview(&[job("a", json!({}))], &[s1, s2]);
        assert!(ov.jobs[0]
            .findings
            .iter()
            .any(|f| f.code == "job.mixedTimeZone"));
    }

    #[test]
    fn nhieu_scheduler_cung_tro_mot_job_thi_cong_don_so_lan() {
        let ov = build_overview(
            &[job("a", json!({}))],
            &[
                sched("s1", "a", "0 17 * * *", "ENABLED"),
                sched("s2", "a", "0 18 * * *", "ENABLED"),
            ],
        );
        assert_eq!(ov.jobs[0].schedulers.len(), 2);
        assert_eq!(ov.jobs[0].runs_per_day, Some(2));
    }

    // --- env secret scanner ----------------------------------------------

    #[test]
    fn bat_duoc_stripe_key_plain_trong_env_cua_job() {
        let ov = build_overview(&[job("job204", json!({}))], &[]);
        let s = &ov.jobs[0].env_secrets;
        assert_eq!(s.len(), 1, "{s:?}");
        assert_eq!(s[0].env_name, "STRIPE_API_KEY");
        // Không được in giá trị đầy đủ.
        let dumped = serde_json::to_string(s).unwrap();
        assert!(!dumped.contains("AlDlJA4baC"), "rò secret: {dumped}");
    }

    #[test]
    fn dem_dung_env_plain_va_env_secret_ref() {
        let r = &build_overview(&[job("job204", json!({}))], &[]).jobs[0];
        assert_eq!(r.env_count, 4);
        assert_eq!(r.secret_env_count, 1, "DB_PASS là secretKeyRef");
    }

    // --- target_job_from_uri ---------------------------------------------

    #[test]
    fn rut_dung_ten_job_tu_uri_that_cua_scheduler() {
        assert_eq!(
            target_job_from_uri(
                "https://asia-northeast1-run.googleapis.com/v2/projects/example-project/locations/asia-northeast1/jobs/job208:run"
            ),
            Some("job208".to_string())
        );
    }

    #[test]
    fn uri_khong_phai_job_run_thi_tra_none() {
        assert_eq!(target_job_from_uri("https://example.com/webhook"), None);
        assert_eq!(target_job_from_uri("https://x/v2/projects/p/services/s"), None);
    }

    #[test]
    fn uri_thieu_hau_to_run_van_rut_duoc_ten() {
        assert_eq!(
            target_job_from_uri("https://x/v2/projects/p/locations/l/jobs/job042"),
            Some("job042".to_string())
        );
    }

    #[test]
    fn job_thieu_name_bi_bo_qua_khong_panic() {
        let ov = build_overview(&[json!({ "labels": {} }), job("ok", json!({}))], &[]);
        assert_eq!(ov.jobs.len(), 1);
        assert_eq!(ov.jobs[0].name, "ok");
    }

    #[test]
    fn danh_sach_job_duoc_sap_xep_on_dinh() {
        let ov = build_overview(
            &[job("job010", json!({})), job("job002", json!({})), job("job001", json!({}))],
            &[],
        );
        let names: Vec<&str> = ov.jobs.iter().map(|j| j.name.as_str()).collect();
        assert_eq!(names, vec!["job001", "job002", "job010"]);
    }
}
