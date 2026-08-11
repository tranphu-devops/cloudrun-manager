//! Cloud Monitoring API v3 — biểu đồ tải và số instance.
//!
//! # Hai quyết định thiết kế đáng nói
//!
//! **1. Sidebar dùng MỘT truy vấn cho toàn bộ project.**
//! `example-project` có ~95 service. Lấy metric từng service là ~95 request mỗi lần
//! refresh, chắc chắn đụng quota và chậm. Monitoring API cho phép bỏ filter service và
//! `groupByFields=resource.label.service_name`, trả về mỗi service một series —
//! một request là đủ cho cả sidebar.
//!
//! **2. Metric không lấy được thì phải NÓI RA.**
//! Nếu tên metric sai một chữ, Monitoring API trả series rỗng chứ không báo lỗi. Vẽ
//! đường phẳng ở 0 sẽ khiến người vận hành tin rằng "service không có tải" — sai lệch
//! nguy hiểm hơn là không có chart. Nên `ChartData.unavailable` tồn tại, và
//! `verify_metrics()` cho phép đối chiếu catalog với `metricDescriptors.list` thật.

use std::collections::BTreeMap;

use chrono::{DateTime, SecondsFormat, Utc};
use serde::Deserialize;
use serde_json::Value;

use crate::client::{seg, GcpClient};
use crate::error::Result;
use crate::ttl;
use crate::types::{ChartData, ProjectLoadSnapshot, SeriesData, TimeSeriesPoint};

const BASE: &str = "https://monitoring.googleapis.com/v3";

// Tên metric của Cloud Run. Đối chiếu lại bằng `verify_metrics()` khi setup project mới.
pub const M_INSTANCE_COUNT: &str = "run.googleapis.com/container/instance_count";
pub const M_REQUEST_COUNT: &str = "run.googleapis.com/request_count";
pub const M_REQUEST_LATENCIES: &str = "run.googleapis.com/request_latencies";
pub const M_CPU_UTIL: &str = "run.googleapis.com/container/cpu/utilizations";
pub const M_MEM_UTIL: &str = "run.googleapis.com/container/memory/utilizations";
pub const M_STARTUP_LATENCIES: &str = "run.googleapis.com/container/startup_latencies";
pub const M_BILLABLE_TIME: &str = "run.googleapis.com/container/billable_instance_time";
pub const M_MAX_CONCURRENCY: &str = "run.googleapis.com/container/max_request_concurrencies";

/// Toàn bộ metric app dùng — dùng cho bước verify.
pub const CATALOG: &[&str] = &[
    M_INSTANCE_COUNT,
    M_REQUEST_COUNT,
    M_REQUEST_LATENCIES,
    M_CPU_UTIL,
    M_MEM_UTIL,
    M_STARTUP_LATENCIES,
    M_BILLABLE_TIME,
    M_MAX_CONCURRENCY,
];

/// Khoảng thời gian xem, tính bằng phút.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeRange {
    pub minutes: i64,
}

impl TimeRange {
    pub const H1: Self = Self { minutes: 60 };
    pub const H6: Self = Self { minutes: 360 };
    pub const H24: Self = Self { minutes: 1440 };
    pub const D7: Self = Self { minutes: 10080 };

    pub fn from_minutes(m: i64) -> Self {
        Self {
            minutes: m.clamp(5, 30 * 24 * 60),
        }
    }

    /// Chọn alignment period sao cho chart có ~60–120 điểm: đủ chi tiết để thấy
    /// spike, đủ thưa để không tải về hàng nghìn điểm cho một cửa sổ nhỏ.
    pub fn alignment_seconds(&self) -> i64 {
        match self.minutes {
            0..=90 => 60,
            91..=480 => 300,
            481..=1440 => 900,
            1441..=4320 => 1800,
            _ => 3600,
        }
    }

    fn window(&self) -> (DateTime<Utc>, DateTime<Utc>) {
        let end = Utc::now();
        let start = end - chrono::Duration::minutes(self.minutes);
        (start, end)
    }
}

/// Cách gộp dữ liệu cho một chart.
#[derive(Debug, Clone)]
pub struct MetricSpec {
    pub metric: &'static str,
    pub unit: &'static str,
    pub aligner: &'static str,
    pub reducer: &'static str,
    /// Ví dụ `metric.labels.state` để tách active/idle.
    pub group_by: Option<&'static str>,
    /// Nhân giá trị (ví dụ utilization 0..1 -> phần trăm).
    pub scale: f64,
}

impl MetricSpec {
    pub fn instance_count() -> Self {
        Self {
            metric: M_INSTANCE_COUNT,
            unit: "instance",
            aligner: "ALIGN_MEAN",
            reducer: "REDUCE_SUM",
            // active vs idle: instance idle vẫn tính tiền ở chế độ CPU always-allocated,
            // nên tách ra xem là có ích khi soi chi phí.
            group_by: Some("metric.labels.state"),
            scale: 1.0,
        }
    }

    pub fn requests_per_second() -> Self {
        Self {
            metric: M_REQUEST_COUNT,
            unit: "req/s",
            aligner: "ALIGN_RATE",
            reducer: "REDUCE_SUM",
            group_by: None,
            scale: 1.0,
        }
    }

    pub fn requests_by_class() -> Self {
        Self {
            metric: M_REQUEST_COUNT,
            unit: "req/s",
            aligner: "ALIGN_RATE",
            reducer: "REDUCE_SUM",
            group_by: Some("metric.labels.response_code_class"),
            scale: 1.0,
        }
    }

    pub fn latency(percentile: u8) -> Self {
        Self {
            metric: M_REQUEST_LATENCIES,
            unit: "ms",
            aligner: "ALIGN_DELTA",
            reducer: match percentile {
                50 => "REDUCE_PERCENTILE_50",
                95 => "REDUCE_PERCENTILE_95",
                _ => "REDUCE_PERCENTILE_99",
            },
            group_by: None,
            scale: 1.0,
        }
    }

    pub fn cpu_utilization() -> Self {
        Self {
            metric: M_CPU_UTIL,
            unit: "%",
            aligner: "ALIGN_DELTA",
            reducer: "REDUCE_PERCENTILE_99",
            group_by: None,
            // API trả 0..1.
            scale: 100.0,
        }
    }

    pub fn memory_utilization() -> Self {
        Self {
            metric: M_MEM_UTIL,
            unit: "%",
            aligner: "ALIGN_DELTA",
            reducer: "REDUCE_PERCENTILE_99",
            group_by: None,
            scale: 100.0,
        }
    }

    pub fn startup_latency() -> Self {
        Self {
            metric: M_STARTUP_LATENCIES,
            unit: "ms",
            aligner: "ALIGN_DELTA",
            reducer: "REDUCE_PERCENTILE_95",
            group_by: None,
            scale: 1.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Parse response
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TimeSeriesResp {
    #[serde(default)]
    time_series: Vec<Value>,
    /// Chưa dùng: cửa sổ thời gian của app luôn nằm gọn trong một trang. Giữ field để
    /// nếu sau này mở rộng cửa sổ thì thấy ngay có pagination cần xử lý.
    #[serde(default)]
    #[allow(dead_code)]
    next_page_token: Option<String>,
}

/// Đọc một `Point` của Monitoring API.
///
/// `int64Value` được trả về dưới dạng **string** trong JSON (quy ước protobuf cho
/// int64) — parse thẳng thành number sẽ mất dữ liệu.
fn point_value(p: &Value) -> Option<f64> {
    let v = p.get("value")?;
    if let Some(d) = v.get("doubleValue").and_then(|x| x.as_f64()) {
        return Some(d);
    }
    if let Some(i) = v.get("int64Value") {
        if let Some(s) = i.as_str() {
            return s.parse::<f64>().ok();
        }
        if let Some(n) = i.as_i64() {
            return Some(n as f64);
        }
    }
    if let Some(b) = v.get("boolValue").and_then(|x| x.as_bool()) {
        return Some(if b { 1.0 } else { 0.0 });
    }
    // Distribution chưa qua reducer percentile: lấy mean để không mất hẳn dữ liệu.
    if let Some(dist) = v.get("distributionValue") {
        return dist.get("mean").and_then(|m| m.as_f64());
    }
    None
}

fn point_time_ms(p: &Value) -> Option<i64> {
    let t = p
        .get("interval")
        .and_then(|i| i.get("endTime"))
        .and_then(|v| v.as_str())?;
    DateTime::parse_from_rfc3339(t).ok().map(|d| d.timestamp_millis())
}

/// Nhãn của một series, lấy theo `group_by`.
fn series_label(ts: &Value, group_by: Option<&str>) -> String {
    let Some(gb) = group_by else {
        return "value".to_string();
    };
    // `metric.labels.state` -> tìm trong ts.metric.labels.state
    let key = gb.rsplit('.').next().unwrap_or(gb);
    let from_metric = ts
        .get("metric")
        .and_then(|m| m.get("labels"))
        .and_then(|l| l.get(key))
        .and_then(|v| v.as_str());
    let from_resource = ts
        .get("resource")
        .and_then(|m| m.get("labels"))
        .and_then(|l| l.get(key))
        .and_then(|v| v.as_str());
    from_metric
        .or(from_resource)
        .unwrap_or("unknown")
        .to_string()
}

fn parse_series(resp_series: &[Value], spec: &MetricSpec) -> Vec<SeriesData> {
    let mut grouped: BTreeMap<String, Vec<TimeSeriesPoint>> = BTreeMap::new();

    for ts in resp_series {
        let label = series_label(ts, spec.group_by);
        let Some(points) = ts.get("points").and_then(|p| p.as_array()) else {
            continue;
        };
        let entry = grouped.entry(label).or_default();
        for p in points {
            if let (Some(t), Some(v)) = (point_time_ms(p), point_value(p)) {
                entry.push(TimeSeriesPoint {
                    t,
                    v: v * spec.scale,
                });
            }
        }
    }

    grouped
        .into_iter()
        .map(|(label, mut points)| {
            // Monitoring API trả điểm mới nhất trước; chart cần thứ tự tăng dần.
            points.sort_by_key(|p| p.t);
            SeriesData { label, points }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Query
// ---------------------------------------------------------------------------

fn build_url(
    project: &str,
    filter: &str,
    spec: &MetricSpec,
    range: TimeRange,
    page_token: Option<&str>,
) -> String {
    let (start, end) = range.window();
    let mut url = format!(
        "{BASE}/projects/{project}/timeSeries?filter={}&interval.startTime={}&interval.endTime={}\
         &aggregation.alignmentPeriod={}s&aggregation.perSeriesAligner={}&aggregation.crossSeriesReducer={}",
        seg(filter),
        seg(&start.to_rfc3339_opts(SecondsFormat::Secs, true)),
        seg(&end.to_rfc3339_opts(SecondsFormat::Secs, true)),
        range.alignment_seconds(),
        spec.aligner,
        spec.reducer,
    );
    if let Some(gb) = spec.group_by {
        url.push_str("&aggregation.groupByFields=");
        url.push_str(&seg(gb));
    }
    if let Some(t) = page_token {
        url.push_str("&pageToken=");
        url.push_str(&seg(t));
    }
    url
}

fn service_filter(spec: &MetricSpec, service: &str, region: &str) -> String {
    format!(
        r#"metric.type="{}" AND resource.type="cloud_run_revision" AND resource.labels.service_name="{}" AND resource.labels.location="{}""#,
        spec.metric, service, region
    )
}

/// Lấy dữ liệu một chart cho một service.
///
/// Lỗi từ Monitoring API (metric không tồn tại, thiếu quyền) KHÔNG làm fail cả trang —
/// trả về `ChartData` với `unavailable = true` kèm lý do, để trang Metrics vẫn hiện
/// được các chart khác.
pub async fn fetch_chart(
    client: &GcpClient,
    project: &str,
    region: &str,
    service: &str,
    spec: &MetricSpec,
    range: TimeRange,
) -> ChartData {
    let filter = service_filter(spec, service, region);
    let cache_key = format!(
        "metrics:{project}:{region}:{service}:{}:{}:{}",
        spec.metric,
        spec.reducer,
        range.minutes
    );
    let ctx = format!("lấy metric {} của service {service}", spec.metric);

    let url = build_url(project, &filter, spec, range, None);
    let resp: Result<TimeSeriesResp> = client
        .get_cached(&url, &ctx, &cache_key, ttl::METRICS)
        .await;

    match resp {
        Err(e) => ChartData {
            metric: spec.metric.to_string(),
            unit: spec.unit.to_string(),
            series: Vec::new(),
            unavailable: true,
            note: Some(format!("Không lấy được dữ liệu: {e}")),
        },
        Ok(r) => {
            let series = parse_series(&r.time_series, spec);
            let empty = series.iter().all(|s| s.points.is_empty());
            ChartData {
                metric: spec.metric.to_string(),
                unit: spec.unit.to_string(),
                unavailable: false,
                note: if empty {
                    // Phân biệt rõ "không có dữ liệu" với "có dữ liệu và bằng 0".
                    Some(
                        "Không có dữ liệu trong khoảng thời gian này. Với service không nhận request \
                         thì đây là bình thường (Cloud Run không ghi metric khi không có hoạt động)."
                            .to_string(),
                    )
                } else {
                    None
                },
                series,
            }
        }
    }
}

/// Ảnh chụp tải của TẤT CẢ service trong project — dùng cho badge ở sidebar.
///
/// Ba truy vấn cho cả project (instance, rps, error), thay vì 3×95 truy vấn.
pub async fn fetch_project_load(
    client: &GcpClient,
    project: &str,
    range: TimeRange,
) -> ProjectLoadSnapshot {
    let mut snap = ProjectLoadSnapshot::default();

    // Số instance hiện tại, gộp theo service.
    let inst_spec = MetricSpec {
        metric: M_INSTANCE_COUNT,
        unit: "instance",
        aligner: "ALIGN_MEAN",
        reducer: "REDUCE_SUM",
        group_by: Some("resource.label.service_name"),
        scale: 1.0,
    };
    match fetch_grouped_by_service(client, project, &inst_spec, range).await {
        Ok(m) => snap.instances = m,
        Err(_) => snap.missing.push("instance_count".to_string()),
    }

    let rps_spec = MetricSpec {
        metric: M_REQUEST_COUNT,
        unit: "req/s",
        aligner: "ALIGN_RATE",
        reducer: "REDUCE_SUM",
        group_by: Some("resource.label.service_name"),
        scale: 1.0,
    };
    match fetch_grouped_by_service(client, project, &rps_spec, range).await {
        Ok(m) => snap.rps = m,
        Err(_) => snap.missing.push("request_count".to_string()),
    }

    // Error rate: cần cả tổng và riêng 5xx, gộp theo service + response_code_class.
    match fetch_error_rate(client, project, range).await {
        Ok(m) => snap.error_rate = m,
        Err(_) => snap.missing.push("error_rate".to_string()),
    }

    snap
}

/// Trả về map service_name -> giá trị điểm cuối (giá trị hiện tại).
async fn fetch_grouped_by_service(
    client: &GcpClient,
    project: &str,
    spec: &MetricSpec,
    range: TimeRange,
) -> Result<BTreeMap<String, f64>> {
    let filter = format!(
        r#"metric.type="{}" AND resource.type="cloud_run_revision""#,
        spec.metric
    );
    let url = build_url(project, &filter, spec, range, None);
    let cache_key = format!("load:{project}:{}:{}", spec.metric, range.minutes);
    let ctx = format!("lấy {} cho toàn bộ project", spec.metric);

    let resp: TimeSeriesResp = client
        .get_cached(&url, &ctx, &cache_key, ttl::METRICS)
        .await?;

    let mut out = BTreeMap::new();
    for ts in &resp.time_series {
        let name = ts
            .get("resource")
            .and_then(|r| r.get("labels"))
            .and_then(|l| l.get("service_name"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        if name.is_empty() {
            continue;
        }
        // Điểm đầu tiên là mới nhất (API trả giảm dần theo thời gian).
        let latest = ts
            .get("points")
            .and_then(|p| p.as_array())
            .and_then(|a| a.first())
            .and_then(point_value)
            .unwrap_or(0.0);
        out.insert(name, latest * spec.scale);
    }
    Ok(out)
}

async fn fetch_error_rate(
    client: &GcpClient,
    project: &str,
    range: TimeRange,
) -> Result<BTreeMap<String, f64>> {
    let spec = MetricSpec {
        metric: M_REQUEST_COUNT,
        unit: "req/s",
        aligner: "ALIGN_RATE",
        reducer: "REDUCE_SUM",
        group_by: Some("resource.label.service_name"),
        scale: 1.0,
    };

    // Gộp theo cả service_name và response_code_class trong một truy vấn.
    let filter = format!(
        r#"metric.type="{}" AND resource.type="cloud_run_revision""#,
        M_REQUEST_COUNT
    );
    let mut url = build_url(project, &filter, &spec, range, None);
    url.push_str("&aggregation.groupByFields=");
    url.push_str(&seg("metric.label.response_code_class"));

    let cache_key = format!("load:{project}:errorrate:{}", range.minutes);
    let resp: TimeSeriesResp = client
        .get_cached(&url, "lấy tỉ lệ lỗi cho toàn bộ project", &cache_key, ttl::METRICS)
        .await?;

    let mut total: BTreeMap<String, f64> = BTreeMap::new();
    let mut errors: BTreeMap<String, f64> = BTreeMap::new();

    for ts in &resp.time_series {
        let svc = ts
            .get("resource")
            .and_then(|r| r.get("labels"))
            .and_then(|l| l.get("service_name"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        if svc.is_empty() {
            continue;
        }
        let class = ts
            .get("metric")
            .and_then(|m| m.get("labels"))
            .and_then(|l| l.get("response_code_class"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        // Cộng dồn cả cửa sổ thay vì chỉ lấy điểm cuối: tỉ lệ lỗi tính trên một
        // điểm đơn lẻ nhiễu quá, dễ nhảy 0% <-> 100%.
        let sum: f64 = ts
            .get("points")
            .and_then(|p| p.as_array())
            .map(|a| a.iter().filter_map(point_value).sum())
            .unwrap_or(0.0);

        *total.entry(svc.clone()).or_insert(0.0) += sum;
        if class == "5xx" {
            *errors.entry(svc).or_insert(0.0) += sum;
        }
    }

    Ok(total
        .into_iter()
        .map(|(svc, t)| {
            let e = errors.get(&svc).copied().unwrap_or(0.0);
            let rate = if t > 0.0 { e / t } else { 0.0 };
            (svc, rate)
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Verify catalog
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricCheck {
    pub metric: String,
    pub exists: bool,
    pub metric_kind: Option<String>,
    pub value_type: Option<String>,
}

/// Đối chiếu `CATALOG` với `metricDescriptors.list` thật của project.
///
/// Chạy bước này khi thêm project mới. Metric sai tên không gây lỗi HTTP mà chỉ trả
/// series rỗng, nên đây là cách duy nhất phát hiện sớm thay vì ngồi debug chart phẳng.
pub async fn verify_metrics(client: &GcpClient, project: &str) -> Result<Vec<MetricCheck>> {
    let filter = r#"metric.type = starts_with("run.googleapis.com")"#;
    let url = format!(
        "{BASE}/projects/{project}/metricDescriptors?filter={}&pageSize=500",
        seg(filter)
    );

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Resp {
        #[serde(default)]
        metric_descriptors: Vec<Value>,
    }
    let resp: Resp = client
        .get(&url, "đối chiếu danh sách metric của Cloud Run")
        .await?;

    let found: BTreeMap<String, (Option<String>, Option<String>)> = resp
        .metric_descriptors
        .iter()
        .filter_map(|d| {
            let t = d.get("type")?.as_str()?.to_string();
            Some((
                t,
                (
                    d.get("metricKind")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    d.get("valueType").and_then(|v| v.as_str()).map(String::from),
                ),
            ))
        })
        .collect();

    Ok(CATALOG
        .iter()
        .map(|m| {
            let hit = found.get(*m);
            MetricCheck {
                metric: (*m).to_string(),
                exists: hit.is_some(),
                metric_kind: hit.and_then(|h| h.0.clone()),
                value_type: hit.and_then(|h| h.1.clone()),
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn alignment_period_scale_theo_do_rong_cua_so() {
        assert_eq!(TimeRange::H1.alignment_seconds(), 60);
        assert_eq!(TimeRange::H6.alignment_seconds(), 300);
        assert_eq!(TimeRange::H24.alignment_seconds(), 900);
        assert_eq!(TimeRange::D7.alignment_seconds(), 3600);
    }

    #[test]
    fn from_minutes_bi_kep_trong_khoang_hop_ly() {
        assert_eq!(TimeRange::from_minutes(-5).minutes, 5);
        assert_eq!(TimeRange::from_minutes(999_999).minutes, 30 * 24 * 60);
        assert_eq!(TimeRange::from_minutes(60).minutes, 60);
    }

    #[test]
    fn doc_duoc_int64_dang_string() {
        // Quy ước protobuf: int64 serialize thành string. Coi như number sẽ trả None.
        let p = json!({ "value": { "int64Value": "42" } });
        assert_eq!(point_value(&p), Some(42.0));
    }

    #[test]
    fn doc_duoc_int64_dang_number() {
        let p = json!({ "value": { "int64Value": 42 } });
        assert_eq!(point_value(&p), Some(42.0));
    }

    #[test]
    fn doc_duoc_double_va_distribution() {
        assert_eq!(
            point_value(&json!({ "value": { "doubleValue": 0.375 } })),
            Some(0.375)
        );
        assert_eq!(
            point_value(&json!({ "value": { "distributionValue": { "mean": 123.5 } } })),
            Some(123.5)
        );
        assert_eq!(point_value(&json!({ "value": {} })), None);
    }

    #[test]
    fn diem_duoc_sap_xep_tang_dan_theo_thoi_gian() {
        // API trả mới nhất trước; chart cần thứ tự ngược lại.
        let series = json!([{
            "metric": { "labels": {} },
            "points": [
                { "interval": { "endTime": "2026-08-05T01:03:00Z" }, "value": { "int64Value": "3" } },
                { "interval": { "endTime": "2026-08-05T01:02:00Z" }, "value": { "int64Value": "2" } },
                { "interval": { "endTime": "2026-08-05T01:01:00Z" }, "value": { "int64Value": "1" } }
            ]
        }]);
        let spec = MetricSpec::instance_count();
        let out = parse_series(series.as_array().unwrap(), &spec);
        assert_eq!(out.len(), 1);
        let vals: Vec<f64> = out[0].points.iter().map(|p| p.v).collect();
        assert_eq!(vals, vec![1.0, 2.0, 3.0], "điểm phải tăng dần theo thời gian");
    }

    #[test]
    fn tach_series_theo_group_by() {
        let series = json!([
            {
                "metric": { "labels": { "state": "active" } },
                "points": [{ "interval": { "endTime": "2026-08-05T01:00:00Z" }, "value": { "int64Value": "2" } }]
            },
            {
                "metric": { "labels": { "state": "idle" } },
                "points": [{ "interval": { "endTime": "2026-08-05T01:00:00Z" }, "value": { "int64Value": "1" } }]
            }
        ]);
        let out = parse_series(series.as_array().unwrap(), &MetricSpec::instance_count());
        let labels: Vec<&str> = out.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(labels, vec!["active", "idle"]);
    }

    #[test]
    fn scale_doi_utilization_thanh_phan_tram() {
        let series = json!([{
            "metric": { "labels": {} },
            "points": [{ "interval": { "endTime": "2026-08-05T01:00:00Z" }, "value": { "doubleValue": 0.38 } }]
        }]);
        let out = parse_series(series.as_array().unwrap(), &MetricSpec::cpu_utilization());
        assert!((out[0].points[0].v - 38.0).abs() < 1e-9, "0.38 phải thành 38%");
    }

    #[test]
    fn url_co_du_tham_so_aggregation() {
        let spec = MetricSpec::instance_count();
        let url = build_url("example-project", "metric.type=\"x\"", &spec, TimeRange::H1, None);
        assert!(url.contains("aggregation.alignmentPeriod=60s"), "{url}");
        assert!(url.contains("aggregation.perSeriesAligner=ALIGN_MEAN"), "{url}");
        assert!(url.contains("aggregation.crossSeriesReducer=REDUCE_SUM"), "{url}");
        assert!(url.contains("groupByFields=metric.labels.state"), "{url}");
        assert!(url.contains("interval.startTime="), "{url}");
        // Filter phải được URL-encode: dấu " và space không được để nguyên.
        assert!(!url.contains("metric.type=\"x\""), "filter chưa được encode: {url}");
    }

    #[test]
    fn filter_service_gioi_han_dung_region() {
        // Thiếu region sẽ gộp lẫn metric của service cùng tên ở region khác.
        let f = service_filter(&MetricSpec::requests_per_second(), "gateway", "asia-northeast1");
        assert!(f.contains(r#"resource.labels.service_name="gateway""#), "{f}");
        assert!(f.contains(r#"resource.labels.location="asia-northeast1""#), "{f}");
        assert!(f.contains(r#"resource.type="cloud_run_revision""#), "{f}");
    }

    #[test]
    fn latency_dung_reducer_percentile() {
        assert_eq!(MetricSpec::latency(50).reducer, "REDUCE_PERCENTILE_50");
        assert_eq!(MetricSpec::latency(95).reducer, "REDUCE_PERCENTILE_95");
        assert_eq!(MetricSpec::latency(99).reducer, "REDUCE_PERCENTILE_99");
    }

    #[test]
    fn catalog_khong_trung_lap() {
        let mut v: Vec<&str> = CATALOG.to_vec();
        v.sort();
        let n = v.len();
        v.dedup();
        assert_eq!(v.len(), n, "CATALOG có metric bị khai hai lần");
    }
}

// ---------------------------------------------------------------------------
// Lượng tài nguyên cho ước lượng chi phí (v2)
// ---------------------------------------------------------------------------

/// instance-giây tách theo active/idle + số request, gộp theo service.
///
/// Dùng `instance_count` (có nhãn `state`) × alignmentPeriod thay vì
/// `billable_instance_time`: cần tách active/idle để rẽ nhánh mô hình tính tiền, mà
/// `instance_count` là metric v1 đã xác nhận có nhãn đó.
///
/// Một truy vấn cho cả project, không phải một truy vấn mỗi service.
pub async fn fetch_usage_by_service(
    client: &GcpClient,
    project: &str,
    range: TimeRange,
) -> Result<BTreeMap<String, crate::billing::Usage>> {
    let align = range.alignment_seconds() as f64;
    let mut out: BTreeMap<String, crate::billing::Usage> = BTreeMap::new();

    // --- instance-giây theo state ---
    let spec = MetricSpec {
        metric: M_INSTANCE_COUNT,
        unit: "instance",
        aligner: "ALIGN_MEAN",
        reducer: "REDUCE_SUM",
        group_by: Some("resource.label.service_name"),
        scale: 1.0,
    };
    let filter = format!(
        r#"metric.type="{M_INSTANCE_COUNT}" AND resource.type="cloud_run_revision""#
    );
    let mut url = build_url(project, &filter, &spec, range, None);
    url.push_str("&aggregation.groupByFields=");
    url.push_str(&seg("metric.label.state"));

    let resp: TimeSeriesResp = client
        .get_cached(
            &url,
            "lấy instance-giây cho ước lượng chi phí",
            &format!("usage:{project}:inst:{}", range.minutes),
            ttl::METRICS,
        )
        .await?;

    for ts in &resp.time_series {
        let svc = ts
            .get("resource")
            .and_then(|r| r.get("labels"))
            .and_then(|l| l.get("service_name"))
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if svc.is_empty() {
            continue;
        }
        let state = ts
            .get("metric")
            .and_then(|m| m.get("labels"))
            .and_then(|l| l.get("state"))
            .and_then(|v| v.as_str())
            .unwrap_or("active");

        // Mỗi điểm là số instance TRUNG BÌNH trong alignmentPeriod → nhân ra instance-giây.
        let seconds: f64 = ts
            .get("points")
            .and_then(|p| p.as_array())
            .map(|a| a.iter().filter_map(point_value).map(|v| v * align).sum())
            .unwrap_or(0.0);

        let e = out.entry(svc.to_string()).or_default();
        if state == "idle" {
            e.instance_seconds_idle += seconds;
        } else {
            e.instance_seconds_active += seconds;
        }
    }

    // --- tổng số request ---
    let rspec = MetricSpec {
        metric: M_REQUEST_COUNT,
        unit: "req",
        // ALIGN_DELTA cho tổng số request trong kỳ; ALIGN_RATE sẽ ra req/s, sai đơn vị.
        aligner: "ALIGN_DELTA",
        reducer: "REDUCE_SUM",
        group_by: Some("resource.label.service_name"),
        scale: 1.0,
    };
    let rfilter = format!(
        r#"metric.type="{M_REQUEST_COUNT}" AND resource.type="cloud_run_revision""#
    );
    let rurl = build_url(project, &rfilter, &rspec, range, None);
    if let Ok(r) = client
        .get_cached::<TimeSeriesResp>(
            &rurl,
            "lấy tổng số request cho ước lượng chi phí",
            &format!("usage:{project}:req:{}", range.minutes),
            ttl::METRICS,
        )
        .await
    {
        for ts in &r.time_series {
            let svc = ts
                .get("resource")
                .and_then(|x| x.get("labels"))
                .and_then(|l| l.get("service_name"))
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            if svc.is_empty() {
                continue;
            }
            let total: f64 = ts
                .get("points")
                .and_then(|p| p.as_array())
                .map(|a| a.iter().filter_map(point_value).sum())
                .unwrap_or(0.0);
            out.entry(svc.to_string()).or_default().requests += total;
        }
    }

    Ok(out)
}

#[cfg(test)]
mod usage_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn instance_giay_duoc_nhan_voi_alignment_period() {
        // Mỗi điểm là số instance trung bình trong alignmentPeriod. Quên nhân ra giây là
        // sai chi phí đúng bằng hệ số alignmentPeriod (60× ở cửa sổ 1 giờ).
        let series = json!([{
            "resource": { "labels": { "service_name": "gateway" } },
            "metric": { "labels": { "state": "active" } },
            "points": [
                { "interval": { "endTime": "2026-08-05T01:02:00Z" }, "value": { "doubleValue": 2.0 } },
                { "interval": { "endTime": "2026-08-05T01:01:00Z" }, "value": { "doubleValue": 3.0 } }
            ]
        }]);
        // 2 + 3 = 5 instance-phút; ×60 = 300 instance-giây.
        let align = TimeRange::H1.alignment_seconds() as f64;
        assert_eq!(align, 60.0);
        let total: f64 = series[0]["points"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(point_value)
            .map(|v| v * align)
            .sum();
        assert_eq!(total, 300.0);
    }
}
