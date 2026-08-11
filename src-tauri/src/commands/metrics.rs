//! Command lấy dữ liệu biểu đồ.

use gcp::monitoring::{self, MetricSpec, TimeRange};
use gcp::types::ChartData;
use serde::Serialize;
use tauri::State;

use crate::error::CmdError;
use crate::state::AppState;

type R<T> = Result<T, CmdError>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceCharts {
    pub instances: ChartData,
    pub rps: ChartData,
    pub by_class: ChartData,
    pub latency_p50: ChartData,
    pub latency_p95: ChartData,
    pub latency_p99: ChartData,
    pub cpu: ChartData,
    pub memory: ChartData,
    pub startup: ChartData,
    /// Độ rộng mỗi điểm dữ liệu, để chú thích dưới chart.
    pub alignment_seconds: i64,
    pub window_minutes: i64,
}

/// Lấy toàn bộ chart của một service.
///
/// Chín truy vấn chạy song song. `fetch_chart` không trả `Result` — metric nào lỗi thì
/// tự mang cờ `unavailable` kèm lý do, để một metric thiếu quyền không làm trắng cả trang.
#[tauri::command]
pub async fn service_charts(
    state: State<'_, AppState>,
    project: String,
    region: String,
    service: String,
    minutes: Option<i64>,
) -> R<ServiceCharts> {
    state.guard_project(&project).await?;
    let window = match minutes {
        Some(m) => m,
        None => state.settings.read().await.metrics_window_minutes,
    };
    let range = TimeRange::from_minutes(window);

    let g = &state.gcp;
    let (p, r, s) = (project.as_str(), region.as_str(), service.as_str());

    // Bind ra biến trước: `&MetricSpec::foo()` ngay trong `join!` tạo temporary bị drop
    // trước khi future được await.
    let (spec_inst, spec_rps, spec_class) = (
        MetricSpec::instance_count(),
        MetricSpec::requests_per_second(),
        MetricSpec::requests_by_class(),
    );
    let (spec_p50, spec_p95, spec_p99) = (
        MetricSpec::latency(50),
        MetricSpec::latency(95),
        MetricSpec::latency(99),
    );
    let (spec_cpu, spec_mem, spec_start) = (
        MetricSpec::cpu_utilization(),
        MetricSpec::memory_utilization(),
        MetricSpec::startup_latency(),
    );

    let (instances, rps, by_class, p50, p95, p99, cpu, memory, startup) = tokio::join!(
        monitoring::fetch_chart(g, p, r, s, &spec_inst, range),
        monitoring::fetch_chart(g, p, r, s, &spec_rps, range),
        monitoring::fetch_chart(g, p, r, s, &spec_class, range),
        monitoring::fetch_chart(g, p, r, s, &spec_p50, range),
        monitoring::fetch_chart(g, p, r, s, &spec_p95, range),
        monitoring::fetch_chart(g, p, r, s, &spec_p99, range),
        monitoring::fetch_chart(g, p, r, s, &spec_cpu, range),
        monitoring::fetch_chart(g, p, r, s, &spec_mem, range),
        monitoring::fetch_chart(g, p, r, s, &spec_start, range),
    );

    Ok(ServiceCharts {
        instances,
        rps,
        by_class,
        latency_p50: p50,
        latency_p95: p95,
        latency_p99: p99,
        cpu,
        memory,
        startup,
        alignment_seconds: range.alignment_seconds(),
        window_minutes: range.minutes,
    })
}
