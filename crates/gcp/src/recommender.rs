//! Recommender API v1 — đề xuất từ Google về Cloud Run và IAM.
//!
//! # Ranh giới cố ý: app ĐÁNH DẤU trạng thái, không TỰ ÁP DỤNG
//!
//! Recommender có `markClaimed` / `markSucceeded` / `markDismissed`. Đó là đánh dấu để
//! theo dõi, không phải thực hiện thay đổi. App giữ ranh giới này:
//!
//! - `markDismissed` — rủi ro thấp, cho phép.
//! - `markSucceeded` khi thực tế chưa làm gì → tự phá hệ thống theo dõi của chính mình.
//!   UI phải nói rõ nút này nghĩa là "tôi đã làm xong", không phải "hãy làm việc này".
//! - **Không có nút áp dụng tự động.** Với `CostRecommender` (đổi `cpuIdle`), việc đúng là
//!   mở tab Scaling của service đó với giá trị đề xuất điền sẵn, rồi đi qua đúng đường ghi
//!   đã có diff + dry-run + etag + audit. Thêm đường ghi thứ hai bỏ qua các lớp đó là tự
//!   phá thiết kế của v1.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::client::{seg, GcpClient};
use crate::error::Result;

const BASE: &str = "https://recommender.googleapis.com/v1";

/// Recommender liên quan tới Cloud Run + IAM, kèm location áp dụng.
///
/// `location`: `None` = theo region của project; `Some("global")` = chỉ ở global.
pub const RECOMMENDERS: &[(&str, &str, Option<&str>)] = &[
    (
        "google.run.service.CostRecommender",
        "Cấu hình CPU allocation chưa tối ưu",
        None,
    ),
    (
        "google.run.service.IdentityRecommender",
        "Service account của service chưa hợp lý",
        None,
    ),
    (
        "google.run.service.SecurityRecommender",
        "Vấn đề bảo mật trong cấu hình service",
        None,
    ),
    (
        "google.iam.policy.Recommender",
        "Quyền IAM cấp thừa, không dùng tới",
        Some("global"),
    ),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recommendation {
    /// Tên đầy đủ, cần cho các lệnh mark*.
    pub full_name: String,
    pub id: String,
    pub recommender: String,
    pub location: String,
    /// `COST` | `SECURITY` | `PERFORMANCE` | `RELIABILITY` | `MANAGEABILITY`
    pub category: String,
    /// `P1`..`P4` — P1 cao nhất.
    pub priority: String,
    pub description: String,
    /// `ACTIVE` | `CLAIMED` | `SUCCEEDED` | `FAILED` | `DISMISSED`
    pub state: String,
    /// Tiết kiệm/chi phí ước tính mỗi tháng, USD. Dấu âm = tiết kiệm.
    pub monthly_cost_impact: Option<f64>,
    /// Tên resource bị ảnh hưởng, rút gọn. Dùng để link sang màn service.
    pub target_resource: Option<String>,
    /// Etag, bắt buộc khi gọi mark*.
    pub etag: String,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RecommendationsResult {
    pub items: Vec<Recommendation>,
    /// `true` khi Recommender API chưa được enable trên project.
    ///
    /// Đây là trạng thái thật của `example-project` lúc lập kế hoạch v2, nên phải xử lý tử tế
    /// thay vì hiện màn trắng. `error.rs` đã nhận diện `SERVICE_DISABLED` và in ra lệnh
    /// enable, nên chỉ cần chuyển message đó lên UI.
    pub api_disabled: bool,
    /// Recommender nào không lấy được và vì sao.
    pub errors: Vec<String>,
}

fn parse_one(v: &Value, recommender: &str, location: &str) -> Option<Recommendation> {
    let full = v.get("name")?.as_str()?.to_string();

    // Chi phí: `primaryImpact.costProjection.cost` dạng Money {units, nanos, currencyCode}.
    let money = v
        .get("primaryImpact")
        .and_then(|p| p.get("costProjection"))
        .and_then(|c| c.get("cost"));
    let monthly_cost_impact = money.map(|m| {
        let units = m
            .get("units")
            .and_then(|u| u.as_str().and_then(|s| s.parse::<f64>().ok()).or_else(|| u.as_f64()))
            .unwrap_or(0.0);
        let nanos = m.get("nanos").and_then(|n| n.as_f64()).unwrap_or(0.0);
        units + nanos / 1e9
    });

    Some(Recommendation {
        id: full.rsplit('/').next().unwrap_or(&full).to_string(),
        recommender: recommender.to_string(),
        location: location.to_string(),
        category: v
            .get("primaryImpact")
            .and_then(|p| p.get("category"))
            .and_then(|c| c.as_str())
            .unwrap_or("UNSPECIFIED")
            .to_string(),
        priority: v
            .get("priority")
            .and_then(|p| p.as_str())
            .unwrap_or("PRIORITY_UNSPECIFIED")
            .to_string(),
        description: v
            .get("description")
            .and_then(|d| d.as_str())
            .unwrap_or_default()
            .to_string(),
        state: v
            .get("stateInfo")
            .and_then(|s| s.get("state"))
            .and_then(|s| s.as_str())
            .unwrap_or("ACTIVE")
            .to_string(),
        monthly_cost_impact,
        target_resource: v
            .get("content")
            .and_then(|c| c.get("operationGroups"))
            .and_then(|g| g.as_array())
            .and_then(|a| a.first())
            .and_then(|g| g.get("operations"))
            .and_then(|o| o.as_array())
            .and_then(|a| a.first())
            .and_then(|o| o.get("resource"))
            .and_then(|r| r.as_str())
            .map(|s| s.rsplit('/').next().unwrap_or(s).to_string()),
        etag: v
            .get("etag")
            .and_then(|e| e.as_str())
            .unwrap_or_default()
            .to_string(),
        full_name: full,
    })
}

/// Lấy recommendation của mọi recommender × mọi location liên quan.
///
/// Một recommender lỗi **không** làm fail cả màn hình — ghi vào `errors` và tiếp tục.
pub async fn list_all(
    client: &GcpClient,
    project: &str,
    regions: &[String],
) -> RecommendationsResult {
    let mut out = RecommendationsResult::default();

    for (rec, _desc, fixed_loc) in RECOMMENDERS {
        let locations: Vec<String> = match fixed_loc {
            Some(l) => vec![(*l).to_string()],
            None => regions.to_vec(),
        };

        for loc in locations {
            let url = format!(
                "{BASE}/projects/{project}/locations/{}/recommenders/{}/recommendations?pageSize=200",
                seg(&loc),
                seg(rec)
            );
            let ctx = format!("lấy recommendation {rec} ở {loc}");

            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Resp {
                #[serde(default)]
                recommendations: Vec<Value>,
            }

            match client
                .get_cached::<Resp>(
                    &url,
                    &ctx,
                    &format!("rec:{project}:{loc}:{rec}"),
                    crate::ttl::SECRETS,
                )
                .await
            {
                Ok(r) => {
                    for item in &r.recommendations {
                        if let Some(p) = parse_one(item, rec, &loc) {
                            out.items.push(p);
                        }
                    }
                }
                Err(e) => {
                    let msg = e.to_string();
                    // `SERVICE_DISABLED` → 403 kèm hướng dẫn enable, error.rs đã lo phần đó.
                    if msg.contains("chưa được enable") || msg.contains("services enable") {
                        out.api_disabled = true;
                        if out.errors.is_empty() {
                            out.errors.push(msg);
                        }
                    } else {
                        out.errors.push(format!("{rec} @ {loc}: {msg}"));
                    }
                }
            }
        }
    }

    // P1 lên trước, rồi tới cái tiết kiệm nhiều nhất.
    out.items.sort_by(|a, b| {
        a.priority.cmp(&b.priority).then_with(|| {
            b.monthly_cost_impact
                .unwrap_or(0.0)
                .abs()
                .partial_cmp(&a.monthly_cost_impact.unwrap_or(0.0).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    });
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MarkAction {
    Dismissed,
    Claimed,
    Succeeded,
    Failed,
}

impl MarkAction {
    fn verb(self) -> &'static str {
        match self {
            MarkAction::Dismissed => "markDismissed",
            MarkAction::Claimed => "markClaimed",
            MarkAction::Succeeded => "markSucceeded",
            MarkAction::Failed => "markFailed",
        }
    }
}

/// Đánh dấu trạng thái một recommendation. **Không** thực hiện thay đổi nào trên hạ tầng.
pub async fn mark(
    client: &GcpClient,
    full_name: &str,
    etag: &str,
    action: MarkAction,
) -> Result<String> {
    let url = format!("{BASE}/{full_name}:{}", action.verb());
    let body = serde_json::json!({ "etag": etag });
    let resp: Value = client
        .post_no_retry(&url, &body, "đánh dấu trạng thái recommendation")
        .await?;

    client.cache.invalidate_prefix("rec:").await;
    Ok(resp
        .get("stateInfo")
        .and_then(|s| s.get("state"))
        .and_then(|s| s.as_str())
        .unwrap_or("đã cập nhật")
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_recommendation_day_du() {
        let v = json!({
          "name": "projects/123/locations/asia-northeast1/recommenders/google.run.service.CostRecommender/recommendations/abc-123",
          "description": "Tắt CPU always-allocated để giảm chi phí",
          "priority": "P2",
          "etag": "\"e1\"",
          "stateInfo": { "state": "ACTIVE" },
          "primaryImpact": {
            "category": "COST",
            "costProjection": { "cost": { "currencyCode": "USD", "units": "-12", "nanos": -500000000 } }
          },
          "content": {
            "operationGroups": [{ "operations": [{
              "resource": "//run.googleapis.com/projects/p/locations/l/services/gateway"
            }]}]
          }
        });
        let r = parse_one(&v, "google.run.service.CostRecommender", "asia-northeast1").unwrap();
        assert_eq!(r.id, "abc-123");
        assert_eq!(r.category, "COST");
        assert_eq!(r.priority, "P2");
        assert_eq!(r.state, "ACTIVE");
        assert_eq!(r.target_resource.as_deref(), Some("gateway"));
        assert_eq!(r.etag, "\"e1\"");
        // Money: units + nanos/1e9, dấu âm = tiết kiệm.
        assert!((r.monthly_cost_impact.unwrap() - (-12.5)).abs() < 1e-9);
    }

    #[test]
    fn parse_recommendation_thieu_field_khong_panic() {
        let v = json!({ "name": "projects/1/locations/l/recommenders/r/recommendations/x" });
        let r = parse_one(&v, "r", "l").unwrap();
        assert_eq!(r.id, "x");
        assert_eq!(r.state, "ACTIVE", "thiếu stateInfo thì coi như ACTIVE");
        assert!(r.monthly_cost_impact.is_none());
        assert!(r.target_resource.is_none());
        assert_eq!(r.priority, "PRIORITY_UNSPECIFIED");
    }

    #[test]
    fn thieu_name_thi_bo_qua() {
        assert!(parse_one(&json!({ "description": "x" }), "r", "l").is_none());
    }

    #[test]
    fn iam_recommender_chi_o_global() {
        let iam = RECOMMENDERS
            .iter()
            .find(|(id, _, _)| *id == "google.iam.policy.Recommender")
            .unwrap();
        assert_eq!(iam.2, Some("global"), "IAM recommender không có bản theo region");

        let cost = RECOMMENDERS
            .iter()
            .find(|(id, _, _)| id.contains("CostRecommender"))
            .unwrap();
        assert_eq!(cost.2, None, "Cloud Run recommender là theo region");
    }

    #[test]
    fn moi_recommender_deu_co_mo_ta_tieng_viet() {
        for (id, desc, _) in RECOMMENDERS {
            assert!(!desc.is_empty(), "{id} thiếu mô tả");
            assert!(desc.len() > 15, "{id} mô tả quá ngắn để hiểu");
        }
    }

    #[test]
    fn mark_action_dung_ten_method_cua_api() {
        assert_eq!(MarkAction::Dismissed.verb(), "markDismissed");
        assert_eq!(MarkAction::Claimed.verb(), "markClaimed");
        assert_eq!(MarkAction::Succeeded.verb(), "markSucceeded");
        assert_eq!(MarkAction::Failed.verb(), "markFailed");
    }
}
