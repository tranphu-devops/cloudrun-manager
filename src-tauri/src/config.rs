//! Cấu hình lưu trên máy: nhãn môi trường của project, read-only mode, project gần đây.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Nhãn môi trường quyết định mức độ "hỏi lại" trước khi ghi.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum EnvLabel {
    Dev,
    Staging,
    Prod,
    /// Chưa gắn nhãn.
    #[default]
    Unknown,
}

impl EnvLabel {
    /// Ghi vào môi trường này có cần gõ tên service để xác nhận không.
    ///
    /// `Unknown` cũng cần — thà làm người dùng gõ thêm một lần trên project dev chưa
    /// gắn nhãn, còn hơn để họ sửa nhầm prod vì app đoán sai. Gắn nhãn một lần là xong.
    pub fn requires_typed_confirm(self) -> bool {
        matches!(self, EnvLabel::Prod | EnvLabel::Unknown)
    }

    pub fn is_prod(self) -> bool {
        matches!(self, EnvLabel::Prod)
    }
}

/// Ngôn ngữ hiển thị của UI.
///
/// Chỉ ảnh hưởng tầng React. Message lỗi sinh từ Rust (`gcp::error`, cron lint, nguồn sai
/// số chi phí) vẫn là tiếng Việt — dịch chúng cần đổi `CmdError` thành key + tham số, là
/// một việc riêng.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Language {
    /// Mặc định: repo công khai nên người đọc đầu tiên nhiều khả năng không đọc được
    /// tiếng Việt.
    #[default]
    En,
    Vi,
    Ja,
}

fn default_language() -> Language {
    Language::default()
}

/// Đoán nhãn từ tên project — chỉ để GỢI Ý, không tự áp dụng cho nhánh nguy hiểm.
///
/// Đoán "prod" thì an toàn (chỉ làm app hỏi kỹ hơn). Đoán "dev" mà sai thì bỏ mất lớp
/// xác nhận trên production, nên nhánh dev cố tình giữ hẹp: chỉ nhận những từ khoá
/// không thể là production.
pub fn suggest_label(project_id: &str) -> EnvLabel {
    let id = project_id.to_ascii_lowercase();

    // Nhánh prod rộng tay: nghi ngờ là prod thì cứ coi là prod.
    for k in ["prod", "production", "master", "live", "main"] {
        if id.contains(k) {
            return EnvLabel::Prod;
        }
    }
    for k in ["stg", "staging", "stage", "uat", "preprod", "pre-prod"] {
        if id.contains(k) {
            return EnvLabel::Staging;
        }
    }
    // Nhánh dev hẹp: phải có từ khoá rõ ràng.
    for k in ["dev", "develop", "development", "sandbox", "test", "local", "demo"] {
        if id.contains(k) {
            return EnvLabel::Dev;
        }
    }
    EnvLabel::Unknown
}

/// Project duy nhất app được phép thao tác khi mới cài.
///
/// Ý đồ: ghim app vào **một** project để nó không thể chạm tới staging/production của
/// bạn ngay cả khi bấm nhầm dropdown. Đây là giá trị placeholder — sửa hằng này (hoặc
/// sửa allowlist trong **⚙ Cài đặt → Project được phép thao tác**) thành project ID
/// thật của bạn trước khi dùng. Để nguyên placeholder thì app chặn mọi thao tác, đó là
/// hướng fail an toàn.
pub const DEFAULT_ALLOWED_PROJECT: &str = "example-project";

fn default_allowed_projects() -> Vec<String> {
    vec![DEFAULT_ALLOWED_PROJECT.to_string()]
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Mặc định BẬT. Người dùng phải tắt có ý thức mới ghi được.
    pub read_only: bool,
    /// Danh sách project app được phép thao tác. Rỗng + `project_lock = true` nghĩa là
    /// không làm được gì — nên mặc định luôn có `DEFAULT_ALLOWED_PROJECT`.
    ///
    /// `serde(default)` là bắt buộc: người dùng đã chạy v1 nên `settings.json` trên máy
    /// họ không có field này. Thiếu default thì cả file parse fail, `Settings::load` rơi
    /// về mặc định và họ mất sạch nhãn project đã gắn — an toàn nhưng gây khó chịu vô ích.
    #[serde(default = "default_allowed_projects")]
    pub allowed_projects: Vec<String>,
    /// Mặc định BẬT. Tắt đi thì app thao tác được mọi project account nhìn thấy.
    #[serde(default = "default_true")]
    pub project_lock: bool,
    /// Ngôn ngữ UI. `serde(default)` bắt buộc: `settings.json` của bản cũ không có field
    /// này, thiếu default thì cả file parse fail và người dùng mất sạch nhãn đã gắn.
    #[serde(default = "default_language")]
    pub language: Language,
    pub project_labels: BTreeMap<String, EnvLabel>,
    pub recent_projects: Vec<String>,
    pub current_project: Option<String>,
    /// 0 = tắt auto refresh.
    pub auto_refresh_seconds: u64,
    pub log_poll_seconds: u64,
    /// Giá trị secret tự ẩn sau bao nhiêu giây.
    pub reveal_timeout_seconds: u64,
    /// Số phút mặc định của cửa sổ metric.
    pub metrics_window_minutes: i64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            read_only: true,
            allowed_projects: vec![DEFAULT_ALLOWED_PROJECT.to_string()],
            project_lock: true,
            language: Language::default(),
            project_labels: BTreeMap::new(),
            recent_projects: Vec::new(),
            current_project: Some(DEFAULT_ALLOWED_PROJECT.to_string()),
            auto_refresh_seconds: 30,
            log_poll_seconds: 3,
            reveal_timeout_seconds: 30,
            metrics_window_minutes: 60,
        }
    }
}

impl Settings {
    pub fn label_for(&self, project_id: &str) -> EnvLabel {
        self.project_labels
            .get(project_id)
            .copied()
            .unwrap_or_else(|| suggest_label(project_id))
    }

    /// Project này có được phép thao tác không.
    pub fn project_allowed(&self, project_id: &str) -> bool {
        !self.project_lock || self.allowed_projects.iter().any(|p| p == project_id)
    }

    pub fn touch_recent(&mut self, project_id: &str) {
        self.recent_projects.retain(|p| p != project_id);
        self.recent_projects.insert(0, project_id.to_string());
        self.recent_projects.truncate(8);
        self.current_project = Some(project_id.to_string());
    }

    pub fn load(path: &PathBuf) -> Self {
        match std::fs::read_to_string(path) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_else(|_| {
                // File cấu hình hỏng: quay về mặc định (tức là read-only BẬT) chứ không
                // panic. Mất cấu hình còn hơn mở khoá ghi vì đọc lỗi.
                Settings::default()
            }),
            Err(_) => Settings::default(),
        }
    }

    pub fn save(&self, path: &PathBuf) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(self)
            .unwrap_or_else(|_| "{}".to_string());
        std::fs::write(path, json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_bat_theo_mac_dinh() {
        // Đây là lựa chọn an toàn quan trọng nhất của app.
        assert!(Settings::default().read_only);
    }

    #[test]
    fn doan_nhan_theo_tu_khoa_trong_ten_project() {
        assert_eq!(suggest_label("example-prod"), EnvLabel::Prod);
        assert_eq!(suggest_label("example-staging"), EnvLabel::Staging);
        assert_eq!(suggest_label("example-develop"), EnvLabel::Dev);
        assert_eq!(suggest_label("example-develop-vn"), EnvLabel::Dev);
        assert_eq!(suggest_label("example-sandbox"), EnvLabel::Dev);
        assert_eq!(suggest_label("example-demo"), EnvLabel::Dev);
    }

    #[test]
    fn ten_project_khong_ro_thi_de_unknown_chu_khong_doan_la_dev() {
        // `example-project` và `example-1115` không có từ khoá nào — đoán "dev" ở đây là
        // đúng cái cách làm mất lớp bảo vệ trên một project có thể là production.
        assert_eq!(suggest_label("example-project"), EnvLabel::Unknown);
        assert_eq!(suggest_label("example-1115"), EnvLabel::Unknown);
        assert_eq!(suggest_label("quiet-meadow-123456-a7"), EnvLabel::Unknown);
    }

    #[test]
    fn unknown_van_phai_go_ten_de_xac_nhan() {
        assert!(EnvLabel::Unknown.requires_typed_confirm());
        assert!(EnvLabel::Prod.requires_typed_confirm());
        assert!(!EnvLabel::Dev.requires_typed_confirm());
        assert!(!EnvLabel::Staging.requires_typed_confirm());
    }

    #[test]
    fn nhan_do_nguoi_dung_dat_thang_duoc_uu_tien_hon_doan() {
        let mut s = Settings::default();
        assert_eq!(s.label_for("example-prod"), EnvLabel::Prod);
        s.project_labels
            .insert("example-prod".into(), EnvLabel::Dev);
        assert_eq!(s.label_for("example-prod"), EnvLabel::Dev);
    }

    #[test]
    fn recent_projects_moi_nhat_len_dau_va_khong_trung() {
        let mut s = Settings::default();
        s.touch_recent("a");
        s.touch_recent("b");
        s.touch_recent("a");
        assert_eq!(s.recent_projects, vec!["a", "b"]);
        assert_eq!(s.current_project.as_deref(), Some("a"));
    }

    #[test]
    fn recent_projects_gioi_han_8() {
        let mut s = Settings::default();
        for i in 0..20 {
            s.touch_recent(&format!("p{i}"));
        }
        assert_eq!(s.recent_projects.len(), 8);
        assert_eq!(s.recent_projects[0], "p19");
    }

    #[test]
    fn file_cau_hinh_hong_thi_ve_mac_dinh_an_toan() {
        let dir = std::env::temp_dir().join("crc-test-config");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("broken.json");
        std::fs::write(&p, "{ đây không phải json").unwrap();

        let s = Settings::load(&p);
        assert!(
            s.read_only,
            "đọc lỗi cấu hình không được dẫn tới việc mở khoá ghi"
        );
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn luu_va_doc_lai_giu_nguyen_gia_tri() {
        let dir = std::env::temp_dir().join("crc-test-config");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("roundtrip.json");

        let mut s = Settings {
            read_only: false,
            ..Default::default()
        };
        s.project_labels.insert("example-project".into(), EnvLabel::Dev);
        s.touch_recent("example-project");
        s.save(&p).unwrap();

        let back = Settings::load(&p);
        assert!(!back.read_only);
        assert_eq!(back.label_for("example-project"), EnvLabel::Dev);
        assert_eq!(back.current_project.as_deref(), Some("example-project"));
        let _ = std::fs::remove_file(&p);
    }
}

#[cfg(test)]
mod allowlist_tests {
    use super::*;

    #[test]
    fn mac_dinh_ghim_vao_dung_mot_project() {
        let s = Settings::default();
        assert!(s.project_lock, "khoá project phải bật mặc định");
        assert_eq!(s.allowed_projects, vec!["example-project"]);
        assert_eq!(s.current_project.as_deref(), Some("example-project"));
    }

    #[test]
    fn staging_va_master_bi_chan() {
        let s = Settings::default();
        assert!(s.project_allowed("example-project"));
        // Đây là lý do tồn tại của tính năng này.
        assert!(!s.project_allowed("example-prod"));
        assert!(!s.project_allowed("example-staging"));
        assert!(!s.project_allowed("example-develop"));
        // Tên user hay gõ nhầm cũng không được lọt qua.
        assert!(!s.project_allowed("example-dev-project"));
    }

    #[test]
    fn tat_khoa_thi_cho_qua_het() {
        let s = Settings {
            project_lock: false,
            ..Default::default()
        };
        assert!(s.project_allowed("example-prod"));
    }

    #[test]
    fn allowlist_rong_ma_van_bat_khoa_thi_chan_tat_ca() {
        let s = Settings {
            allowed_projects: vec![],
            ..Default::default()
        };
        assert!(!s.project_allowed("example-project"));
    }

    #[test]
    fn ngon_ngu_mac_dinh_la_tieng_anh() {
        assert_eq!(Settings::default().language, Language::En);
    }

    #[test]
    fn ngon_ngu_luu_va_doc_lai_duoc() {
        let dir = std::env::temp_dir().join("crc-test-config");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("language.json");

        let s = Settings {
            language: Language::Vi,
            ..Default::default()
        };
        s.save(&p).unwrap();
        assert_eq!(Settings::load(&p).language, Language::Vi);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn settings_json_thieu_language_van_parse_duoc() {
        // File của bản trước khi có i18n. Thiếu `serde(default)` thì cả file hỏng và
        // người dùng mất nhãn project đã gắn.
        let old = r#"{"readOnly":false,"projectLabels":{"example-prod":"prod"},
                      "recentProjects":[],"currentProject":null,
                      "autoRefreshSeconds":30,"logPollSeconds":3,
                      "revealTimeoutSeconds":30,"metricsWindowMinutes":60}"#;
        let s: Settings = serde_json::from_str(old).expect("file cũ phải parse được");
        assert_eq!(s.language, Language::En);
        assert_eq!(s.label_for("example-prod"), EnvLabel::Prod);
    }

    #[test]
    fn settings_json_tu_v1_van_doc_duoc_va_giu_nhan_da_gan() {
        // Người dùng đã chạy v1, file trên máy họ không có allowedProjects/projectLock.
        let old = r#"{"readOnly":false,"projectLabels":{"example-prod":"prod"},
                      "recentProjects":["example-project"],"currentProject":"example-project",
                      "autoRefreshSeconds":30,"logPollSeconds":3,
                      "revealTimeoutSeconds":30,"metricsWindowMinutes":60}"#;
        let s: Settings = serde_json::from_str(old).expect("bản v1 phải parse được");

        // Nhãn cũ được giữ.
        assert_eq!(s.label_for("example-prod"), EnvLabel::Prod);
        assert!(!s.read_only, "lựa chọn cũ của người dùng được giữ");
        // Field mới lấy mặc định an toàn.
        assert!(s.project_lock, "khoá project phải bật cho file cũ");
        assert_eq!(s.allowed_projects, vec!["example-project"]);
        assert!(!s.project_allowed("example-prod"));
    }
}
