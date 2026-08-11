//! Tầng truy cập GCP API cho Cloud Run Cockpit.
//!
//! Crate này cố tình KHÔNG phụ thuộc Tauri: nhờ vậy toàn bộ logic rủi ro
//! (read-modify-write service, parse env, diff, validate) chạy được dưới `cargo test`
//! trên bất kỳ máy nào, không cần dựng webview.

pub mod auth;
pub mod billing;
pub mod cache;
pub mod client;
pub mod cronlint;
pub mod error;
pub mod jobs;
pub mod logging;
pub mod monitoring;
pub mod mutate;
pub mod recommender;
pub mod resourcemanager;
pub mod run;
pub mod sa;
pub mod secret;
pub mod secretmanager;
pub mod types;

pub use client::GcpClient;
pub use error::{GcpError, Result};

/// TTL cache tập trung một chỗ để dễ chỉnh khi cần đổi độ tươi của dữ liệu.
pub mod ttl {
    use std::time::Duration;

    /// Danh sách project gần như không đổi.
    pub const PROJECTS: Duration = Duration::from_secs(3600);
    /// Danh sách service: đủ ngắn để thấy service mới, đủ dài để đổi tab không gọi lại.
    pub const SERVICES: Duration = Duration::from_secs(30);
    /// Chi tiết service.
    pub const SERVICE_DETAIL: Duration = Duration::from_secs(15);
    pub const REVISIONS: Duration = Duration::from_secs(30);
    /// Metric của Monitoring API là dữ liệu 1 phút/điểm, cache 60s không mất gì.
    pub const METRICS: Duration = Duration::from_secs(60);
    pub const SECRETS: Duration = Duration::from_secs(300);
    /// Log không cache: người ta mở tab log là để xem cái mới nhất.
    pub const NONE: Duration = Duration::ZERO;
}
