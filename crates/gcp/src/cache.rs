//! TTL cache cho response body.
//!
//! Cache ở dạng chuỗi JSON thô thay vì struct đã parse: đơn giản, không cần `Any`
//! downcast, và chi phí parse lại nhỏ hơn hẳn so với một round-trip mạng.
//!
//! Có cache là điều kiện bắt buộc ở đây: project `example-project` có ~95 service.
//! Không cache thì mỗi lần đổi tab là một loạt call mới và sẽ đụng quota Monitoring API.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;

struct Entry {
    body: Arc<str>,
    stored_at: Instant,
    ttl: Duration,
}

impl Entry {
    fn is_fresh(&self) -> bool {
        self.stored_at.elapsed() < self.ttl
    }
}

#[derive(Default)]
pub struct Cache {
    map: RwLock<HashMap<String, Entry>>,
}

impl Cache {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn get(&self, key: &str) -> Option<Arc<str>> {
        let m = self.map.read().await;
        m.get(key).filter(|e| e.is_fresh()).map(|e| e.body.clone())
    }

    /// Tuổi của entry (kể cả khi đã hết hạn) — dùng để UI hiện "dữ liệu cũ Ns".
    /// Người vận hành luôn cần biết con số đang xem tươi đến mức nào.
    pub async fn age(&self, key: &str) -> Option<Duration> {
        let m = self.map.read().await;
        m.get(key).map(|e| e.stored_at.elapsed())
    }

    pub async fn put(&self, key: impl Into<String>, body: impl Into<Arc<str>>, ttl: Duration) {
        let mut m = self.map.write().await;
        m.insert(
            key.into(),
            Entry {
                body: body.into(),
                stored_at: Instant::now(),
                ttl,
            },
        );
    }

    /// Xoá mọi entry có key bắt đầu bằng `prefix`.
    ///
    /// Dùng ngay sau khi PATCH thành công: sửa env của một service làm sai lệch cả
    /// bản detail của service đó lẫn danh sách service của project, nên phải bỏ cả
    /// cụm theo prefix chứ không thể xoá đúng một key.
    pub async fn invalidate_prefix(&self, prefix: &str) {
        let mut m = self.map.write().await;
        m.retain(|k, _| !k.starts_with(prefix));
    }

    pub async fn clear(&self) {
        self.map.write().await.clear();
    }

    /// Dọn entry đã hết hạn. Gọi định kỳ để cache không phình theo số service đã xem.
    pub async fn evict_stale(&self) {
        let mut m = self.map.write().await;
        m.retain(|_, e| e.is_fresh());
    }

    pub async fn len(&self) -> usize {
        self.map.read().await.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.map.read().await.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn tra_ve_gia_tri_khi_con_han() {
        let c = Cache::new();
        c.put("k", "value", Duration::from_secs(60)).await;
        assert_eq!(c.get("k").await.as_deref(), Some("value"));
    }

    #[tokio::test]
    async fn khong_tra_ve_khi_het_han() {
        let c = Cache::new();
        c.put("k", "value", Duration::from_millis(1)).await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(c.get("k").await.is_none(), "entry hết hạn phải coi như miss");
        // Nhưng age vẫn đọc được để UI báo dữ liệu cũ.
        assert!(c.age("k").await.is_some());
    }

    #[tokio::test]
    async fn invalidate_prefix_chi_xoa_dung_cum() {
        let c = Cache::new();
        let ttl = Duration::from_secs(60);
        c.put("run:example-project:services", "a", ttl).await;
        c.put("run:example-project:svc:gateway", "b", ttl).await;
        c.put("run:example-staging:services", "c", ttl).await;
        c.put("projects:list", "d", ttl).await;

        c.invalidate_prefix("run:example-project").await;

        assert!(c.get("run:example-project:services").await.is_none());
        assert!(c.get("run:example-project:svc:gateway").await.is_none());
        assert_eq!(
            c.get("run:example-staging:services").await.as_deref(),
            Some("c"),
            "project khác không được bị ảnh hưởng"
        );
        assert_eq!(c.get("projects:list").await.as_deref(), Some("d"));
    }

    #[tokio::test]
    async fn evict_stale_don_dung_entry_het_han() {
        let c = Cache::new();
        c.put("cu", "x", Duration::from_millis(1)).await;
        c.put("moi", "y", Duration::from_secs(60)).await;
        tokio::time::sleep(Duration::from_millis(20)).await;

        c.evict_stale().await;
        assert_eq!(c.len().await, 1);
        assert_eq!(c.get("moi").await.as_deref(), Some("y"));
    }
}
