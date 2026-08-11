//! Read-model gửi ra frontend. Tất cả `camelCase` để TS dùng trực tiếp.
//!
//! Đây là model cho việc ĐỌC/HIỂN THỊ. Đường GHI không dùng những struct này —
//! xem `mutate.rs` để biết lý do (tóm gọn: struct chặt sẽ làm mất field mình chưa biết).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Health {
    Ready,
    NotReady,
    Reconciling,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    pub project_id: String,
    pub display_name: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceSummary {
    /// ID ngắn, ví dụ `gateway`.
    pub name: String,
    /// `projects/{p}/locations/{loc}/services/{id}`
    pub full_name: String,
    pub project_id: String,
    pub region: String,
    pub uri: Option<String>,
    pub health: Health,
    pub health_message: Option<String>,
    pub latest_ready_revision: Option<String>,
    pub latest_created_revision: Option<String>,
    pub image: Option<String>,
    pub min_instances: Option<i64>,
    pub max_instances: Option<i64>,
    pub last_modifier: Option<String>,
    pub update_time: Option<String>,
    /// `true` khi traffic bị ghim cứng vào revision cụ thể thay vì LATEST.
    ///
    /// Cực kỳ quan trọng: khi cờ này bật, sửa env sẽ tạo revision mới nhưng revision
    /// đó KHÔNG nhận traffic — người dùng thấy "thành công" mà thực tế không có gì
    /// thay đổi. UI phải cảnh báo.
    pub traffic_pinned: bool,
    pub env_count: usize,
    pub secret_env_count: usize,
    /// Số container (>1 nghĩa là có sidecar).
    pub container_count: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EnvKind {
    Plain,
    SecretRef,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvEntry {
    pub name: String,
    pub kind: EnvKind,
    /// Chỉ có với `Plain`.
    pub value: Option<String>,
    /// Chỉ có với `SecretRef` — tên secret (đã rút gọn về id nếu là đường dẫn đầy đủ).
    pub secret: Option<String>,
    /// Chỉ có với `SecretRef` — `latest` hoặc số version.
    pub version: Option<String>,
}

impl EnvEntry {
    pub fn plain(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind: EnvKind::Plain,
            value: Some(value.into()),
            secret: None,
            version: None,
        }
    }

    pub fn secret_ref(
        name: impl Into<String>,
        secret: impl Into<String>,
        version: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            kind: EnvKind::SecretRef,
            value: None,
            secret: Some(secret.into()),
            version: Some(version.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretVolumeMount {
    pub volume_name: String,
    pub secret: String,
    pub mount_path: Option<String>,
    pub items: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrafficEntry {
    /// `LATEST` hoặc `REVISION`.
    pub kind: String,
    pub revision: Option<String>,
    pub percent: i64,
    pub tag: Option<String>,
    pub uri: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConditionView {
    pub r#type: String,
    pub state: String,
    pub message: Option<String>,
    pub reason: Option<String>,
    pub last_transition_time: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerView {
    pub index: usize,
    pub name: Option<String>,
    pub image: Option<String>,
    pub cpu: Option<String>,
    pub memory: Option<String>,
    pub cpu_idle: Option<bool>,
    pub startup_cpu_boost: Option<bool>,
    pub port: Option<i64>,
    pub env: Vec<EnvEntry>,
    pub command: Vec<String>,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceDetail {
    pub summary: ServiceSummary,
    /// Bắt buộc gửi lại khi PATCH để chặn lost-update.
    pub etag: String,
    pub description: Option<String>,
    pub service_account: Option<String>,
    pub ingress: Option<String>,
    pub launch_stage: Option<String>,
    pub execution_environment: Option<String>,
    pub concurrency: Option<i64>,
    pub timeout: Option<String>,
    pub session_affinity: Option<bool>,
    pub vpc_egress: Option<String>,
    pub vpc_connector: Option<String>,
    pub cloudsql_instances: Vec<String>,
    pub containers: Vec<ContainerView>,
    pub secret_volumes: Vec<SecretVolumeMount>,
    pub traffic: Vec<TrafficEntry>,
    pub conditions: Vec<ConditionView>,
    pub labels: BTreeMap<String, String>,
    pub annotations: BTreeMap<String, String>,
    /// Tên revision dự đoán sẽ được tạo nếu apply thay đổi bây giờ.
    pub next_revision_hint: Option<String>,
    /// JSON thô của Service, giữ nguyên để làm read-modify-write.
    /// Không hiển thị trực tiếp; frontend gửi ngược lại khi apply.
    pub raw: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionInfo {
    pub name: String,
    pub create_time: Option<String>,
    pub image: Option<String>,
    pub health: Health,
    pub health_message: Option<String>,
    pub min_instances: Option<i64>,
    pub max_instances: Option<i64>,
    pub cpu: Option<String>,
    pub memory: Option<String>,
    pub concurrency: Option<i64>,
    pub log_uri: Option<String>,
    /// % traffic đang nhận (0 nếu không nhận).
    pub traffic_percent: i64,
    pub is_latest_ready: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretInfo {
    pub name: String,
    pub create_time: Option<String>,
    pub labels: BTreeMap<String, String>,
    pub replication: Option<String>,
    /// Service nào trong project đang tham chiếu secret này.
    pub used_by: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretVersionInfo {
    pub version: String,
    pub state: String,
    pub create_time: Option<String>,
    pub destroy_time: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub insert_id: String,
    pub timestamp: String,
    pub severity: String,
    pub revision: Option<String>,
    /// Nội dung đã làm phẳng để hiển thị một dòng.
    pub message: String,
    /// `request` (access log) hoặc `app` (stdout/stderr).
    pub stream: String,
    pub http_status: Option<i64>,
    pub http_method: Option<String>,
    pub http_path: Option<String>,
    pub latency_ms: Option<f64>,
    /// Payload gốc để mở rộng xem chi tiết.
    pub raw: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogPage {
    pub entries: Vec<LogEntry>,
    pub next_page_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeSeriesPoint {
    /// Epoch milliseconds.
    pub t: i64,
    pub v: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesData {
    /// Nhãn phân biệt series trong cùng một chart (ví dụ `active`/`idle`, `2xx`/`5xx`).
    pub label: String,
    pub points: Vec<TimeSeriesPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartData {
    pub metric: String,
    pub unit: String,
    pub series: Vec<SeriesData>,
    /// `true` nếu Monitoring API không biết metric này (tên sai / chưa có dữ liệu).
    /// Chart rỗng vì lý do này phải nói rõ, không được im lặng vẽ đường phẳng.
    pub unavailable: bool,
    pub note: Option<String>,
}

/// Tổng quan tải của TẤT CẢ service trong project, lấy bằng một call duy nhất.
///
/// Với ~95 service như `example-project`, gọi metric từng service là 95 request —
/// không khả thi. Monitoring API cho phép group theo `resource.label.service_name`
/// nên một truy vấn là đủ cho cả sidebar.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProjectLoadSnapshot {
    /// service_name -> số instance hiện tại.
    pub instances: BTreeMap<String, f64>,
    /// service_name -> request/giây.
    pub rps: BTreeMap<String, f64>,
    /// service_name -> tỉ lệ lỗi 5xx (0.0 - 1.0).
    pub error_rate: BTreeMap<String, f64>,
    /// Metric nào không lấy được, để UI không hiện số 0 gây hiểu nhầm là "không có tải".
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScalingUpdate {
    pub min_instances: Option<i64>,
    pub max_instances: Option<i64>,
    pub cpu: Option<String>,
    pub memory: Option<String>,
    pub concurrency: Option<i64>,
    /// Duration dạng `300s`.
    pub timeout: Option<String>,
    pub cpu_idle: Option<bool>,
    pub startup_cpu_boost: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum EnvChange {
    Added {
        name: String,
        value: String,
    },
    Removed {
        name: String,
        /// `None` khi entry bị xoá là secret-ref (không in giá trị).
        value: Option<String>,
    },
    Changed {
        name: String,
        before: String,
        after: String,
    },
    /// Đổi version của một secret-ref.
    SecretVersionChanged {
        name: String,
        secret: String,
        before: String,
        after: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyPreview {
    pub env_changes: Vec<EnvChange>,
    pub scaling_changes: Vec<String>,
    pub next_revision_hint: Option<String>,
    pub traffic_pinned: bool,
    /// Cảnh báo dạng câu hoàn chỉnh, hiện nguyên văn cho người dùng.
    pub warnings: Vec<String>,
}
