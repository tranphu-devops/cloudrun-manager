//! Command cho ước lượng chi phí và recommendation.

use std::collections::BTreeMap;

use gcp::billing::{self, BillingMode, CostEstimate, FreeTierOffset};
use gcp::monitoring::TimeRange;
use gcp::recommender::{self, MarkAction, RecommendationsResult};
use gcp::{jobs, monitoring, run};
use serde::Serialize;
use tauri::State;

use crate::audit::{Action, Outcome};
use crate::error::CmdError;
use crate::state::AppState;

type R<T> = Result<T, CmdError>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CostRow {
    pub name: String,
    pub region: String,
    /// `service` hoặc `job`.
    pub kind: String,
    pub cpu: Option<String>,
    pub memory: Option<String>,
    pub mode: BillingMode,
    pub mode_label: String,
    pub estimate: CostEstimate,
    /// Quy ra mỗi ngày để so sánh giữa các cửa sổ thời gian khác nhau.
    pub per_day: f64,
    pub rps: f64,
    pub min_instances: Option<i64>,
    /// Vì sao tốn — suy từ cấu hình + tải, không phải chỉ con số.
    pub drivers: Vec<String>,
    pub tier2_region: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CostReport {
    pub window_minutes: i64,
    pub rows: Vec<CostRow>,
    pub total_estimate: f64,
    pub total_per_day: f64,
    pub total_per_month: f64,
    pub free_tier: FreeTierOffset,
    /// Bảy nguồn sai số — hiện thẳng trên UI.
    pub error_sources: Vec<String>,
    /// Cảnh báo riêng khi có service ở region tier 2 (đơn giá khác).
    pub warnings: Vec<String>,
    /// `true` khi không lấy được metric — khi đó mọi số là 0 vì THIẾU DỮ LIỆU,
    /// không phải vì không tốn tiền.
    pub usage_unavailable: bool,
    pub note: Option<String>,
}

/// Ước lượng chi phí cho toàn bộ service + job của project.
///
/// Dữ liệu: 2 truy vấn Monitoring cho cả project (instance-giây theo state, tổng request)
/// + bản `services.list`/`jobs.list` đã cache. Không có call nào theo từng service.
#[tauri::command]
pub async fn cost_report(
    state: State<'_, AppState>,
    project: String,
    minutes: Option<i64>,
) -> R<CostReport> {
    state.guard_project(&project).await?;

    let window = minutes.unwrap_or(60 * 24 * 30);
    let range = TimeRange::from_minutes(window);
    let day_factor = 1440.0 / range.minutes as f64;

    let usage_map = monitoring::fetch_usage_by_service(&state.gcp, &project, range).await;
    let usage_unavailable = usage_map.is_err();
    let note = usage_map.as_ref().err().map(|e| {
        format!(
            "Không lấy được metric nên mọi con số dưới đây bằng 0 vì THIẾU DỮ LIỆU, không phải vì \
             không tốn tiền. Nguyên nhân: {e}"
        )
    });
    let usage_map: BTreeMap<String, billing::Usage> = usage_map.unwrap_or_default();

    let load = monitoring::fetch_project_load(&state.gcp, &project, range).await;

    let mut rows: Vec<CostRow> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let (mut tot_cpu_s, mut tot_gib_s, mut tot_req) = (0.0f64, 0.0f64, 0.0f64);

    // --- Services ---
    for svc in run::list_services_raw(&state.gcp, &project).await? {
        let Some(s) = run::summarize(&svc) else { continue };
        let container = gcp::mutate::parse_containers(&svc)
            .ok()
            .and_then(|c| c.into_iter().next());

        let cpu_str = container.as_ref().and_then(|c| c.cpu.clone());
        let mem_str = container.as_ref().and_then(|c| c.memory.clone());
        let cpu_idle = container.as_ref().and_then(|c| c.cpu_idle);

        let mode = BillingMode::from_cpu_idle(cpu_idle);
        let cpu = billing::parse_cpu(cpu_str.as_deref());
        let mem = billing::parse_memory_gib(mem_str.as_deref());
        let usage = usage_map.get(&s.name).copied().unwrap_or_default();
        let est = billing::estimate(usage, cpu, mem, mode);
        let rps = load.rps.get(&s.name).copied().unwrap_or(0.0);

        tot_cpu_s += est.vcpu_seconds;
        tot_gib_s += est.gib_seconds;
        tot_req += usage.requests;

        let tier2 = billing::is_tier2(&s.region);
        if tier2 && !warnings.iter().any(|w| w.contains(&s.region)) {
            warnings.push(format!(
                "Có service ở region tier 2 ({}) — đơn giá cao hơn bảng giá app đang dùng, nên ước \
                 lượng của những service đó THẤP hơn thực tế.",
                s.region
            ));
        }

        rows.push(CostRow {
            drivers: billing::cost_drivers(s.min_instances, rps, mode, cpu, est.total),
            per_day: est.total * day_factor,
            mode_label: mode.label_vi().to_string(),
            name: s.name,
            region: s.region,
            kind: "service".into(),
            cpu: cpu_str,
            memory: mem_str,
            mode,
            estimate: est,
            rps,
            min_instances: s.min_instances,
            tier2_region: tier2,
        });
    }

    // --- Jobs: luôn tính theo instance-based ---
    //
    // Metric của job nằm ở resource type `cloud_run_job`, khác `cloud_run_revision` của
    // service, nên `usage_map` không có job. Thay vì bỏ trống, ước lượng từ số lần chạy/ngày
    // × timeout — thô nhưng đủ để thấy job nào là hố tiền, và được ghi rõ là thô.
    if let Ok(ov) = jobs::overview(&state.gcp, &project).await {
        for j in &ov.jobs {
            let cpu = billing::parse_cpu(j.cpu.as_deref());
            let mem = billing::parse_memory_gib(j.memory.as_deref());
            let runs = j.runs_per_day.unwrap_or(0) as f64;

            // Không biết job chạy bao lâu thật; dùng 60 giây (mức tính tiền tối thiểu của
            // instance-based) làm cận dưới thay vì dùng timeout (thường 3600s) làm cận trên —
            // ước lượng thấp và nói rõ, tốt hơn là báo một con số phóng đại 60×.
            let secs_per_run = 60.0;
            let usage = billing::Usage {
                instance_seconds_active: runs * secs_per_run / day_factor,
                instance_seconds_idle: 0.0,
                requests: 0.0,
            };
            let est = billing::estimate(usage, cpu, mem, BillingMode::InstanceBased);
            tot_cpu_s += est.vcpu_seconds;
            tot_gib_s += est.gib_seconds;

            let mut drivers = Vec::new();
            if runs >= 100.0 {
                drivers.push(format!(
                    "Chạy {runs:.0} lần/ngày — kiểm tra lại cron, đây là mức rất dày cho job batch."
                ));
            }
            drivers.push(
                "Ước lượng của job dùng mốc 60 giây mỗi lần chạy (mức tính tiền tối thiểu), nên là \
                 CẬN DƯỚI. Job chạy lâu hơn sẽ tốn nhiều hơn con số này."
                    .to_string(),
            );

            rows.push(CostRow {
                per_day: est.total * day_factor,
                mode_label: BillingMode::InstanceBased.label_vi().to_string(),
                name: j.name.clone(),
                region: j.region.clone(),
                kind: "job".into(),
                cpu: j.cpu.clone(),
                memory: j.memory.clone(),
                mode: BillingMode::InstanceBased,
                estimate: est,
                rps: 0.0,
                min_instances: None,
                drivers,
                tier2_region: billing::is_tier2(&j.region),
            });
        }
    }

    rows.sort_by(|a, b| {
        b.estimate
            .total
            .partial_cmp(&a.estimate.total)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let total: f64 = rows.iter().map(|r| r.estimate.total).sum();
    let total_per_day = total * day_factor;

    Ok(CostReport {
        window_minutes: range.minutes,
        total_estimate: total,
        total_per_day,
        total_per_month: total_per_day * 30.0,
        // Free tier tính theo request-based (mô hình mặc định) — cận trên của phần được bù.
        free_tier: billing::free_tier_offset(
            tot_cpu_s,
            tot_gib_s,
            tot_req,
            BillingMode::RequestBased,
        ),
        error_sources: billing::ERROR_SOURCES.iter().map(|s| s.to_string()).collect(),
        warnings,
        usage_unavailable,
        note,
        rows,
    })
}

#[tauri::command]
pub async fn recommendations(
    state: State<'_, AppState>,
    project: String,
) -> R<RecommendationsResult> {
    state.guard_project(&project).await?;

    // Region nào có service thì hỏi recommendation ở đó.
    let mut regions: Vec<String> = run::list_services(&state.gcp, &project)
        .await
        .map(|v| v.into_iter().map(|s| s.region).collect())
        .unwrap_or_default();
    regions.sort();
    regions.dedup();
    if regions.is_empty() {
        regions.push("asia-northeast1".to_string());
    }

    Ok(recommender::list_all(&state.gcp, &project, &regions).await)
}

/// Đánh dấu trạng thái một recommendation. **Không** thực hiện thay đổi trên hạ tầng.
#[tauri::command]
pub async fn mark_recommendation(
    state: State<'_, AppState>,
    project: String,
    full_name: String,
    etag: String,
    action: MarkAction,
) -> R<String> {
    // Đánh dấu là ghi (đổi state trên GCP), nên qua guard. Dùng chính id recommendation làm
    // chuỗi xác nhận nếu project cần gõ tên.
    let id = full_name
        .rsplit('/')
        .next()
        .unwrap_or(&full_name)
        .to_string();
    state.guard_write(&project, &id, Some(&id)).await?;

    let result = recommender::mark(&state.gcp, &full_name, &etag, action).await;
    let (outcome, msg) = match &result {
        Ok(m) => (Outcome::Ok, m.clone()),
        Err(e) => (Outcome::Error, e.to_string()),
    };
    state
        .record(
            &project,
            None,
            None,
            Action::MarkRecommendation,
            vec![format!("{action:?} recommendation {id}")],
            outcome,
            &msg,
            None,
            None,
        )
        .await;
    result.map_err(CmdError::from)
}
