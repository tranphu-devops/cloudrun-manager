//! Kiểm tra biểu thức cron của Cloud Scheduler và quét env tìm giá trị trông như secret.
//!
//! # Vì sao module này tồn tại
//!
//! Khảo sát `example-project` (196 Cloud Run Job, ~190 Scheduler job) tìm ra 5 job để trống
//! trường phút — `* 17 * * *` thay vì `0 17 * * *` — nên chạy **60 lần trong một giờ**.
//! Tiền không đáng kể; vấn đề là execution chồng lấn khi job chạy quá 60 giây, dẫn tới xử
//! lý trùng dữ liệu. Loại lỗi này không có cách nào phát hiện bằng mắt khi bạn có 190 dòng
//! cron, nên phải để máy soát.
//!
//! Cùng lý do, `scan_env_secrets` quét giá trị env dạng plain: job204 trong project thật
//! có `STRIPE_API_KEY` là secret key nằm thẳng trong cấu hình, không qua Secret Manager.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    /// Gần như chắc chắn là lỗi, cần sửa.
    High,
    /// Đáng xem lại, có thể là chủ ý.
    Warn,
    /// Chỉ để biết.
    Info,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub severity: Severity,
    /// Mã ổn định để UI nhóm/filter, không phải để hiển thị.
    pub code: &'static str,
    /// Câu hoàn chỉnh hiện cho người dùng.
    pub message: String,
    /// Gợi ý sửa, nếu suy ra được.
    pub suggestion: Option<String>,
}

/// Số lần chạy mỗi ngày, suy ra từ cron. `None` khi không phân tích được.
///
/// Chỉ tính các trường hợp thường gặp — đủ để xếp hạng "job nào chạy nhiều nhất", không
/// nhằm thay thế một cron parser đầy đủ.
pub fn runs_per_day(schedule: &str) -> Option<u32> {
    let f: Vec<&str> = schedule.split_whitespace().collect();
    if f.len() != 5 {
        return None;
    }
    let (min, hour, dom, _mon, dow) = (f[0], f[1], f[2], f[3], f[4]);

    let per_hour = count_field(min, 60)?;
    let hours = count_field(hour, 24)?;

    // Ngày trong tháng / thứ trong tuần: chỉ xử lý trường hợp đơn giản. Có giới hạn thì
    // số lần/ngày trung bình giảm, nhưng ở đây ta muốn "số lần trong ngày nó chạy" để so
    // sánh mức độ dày, nên bỏ qua dom/dow khi cả hai là `*`.
    if dom != "*" || dow != "*" {
        // Không nhân thêm; trả về số lần trong một ngày mà nó có chạy.
        return Some(per_hour * hours);
    }
    Some(per_hour * hours)
}

/// Đếm số giá trị một trường cron khớp, trong phạm vi `range`.
fn count_field(field: &str, range: u32) -> Option<u32> {
    if field == "*" {
        return Some(range);
    }
    // `*/N` và biến thể `0/N` (GNU/Quartz) — cùng nghĩa trên Cloud Scheduler.
    if let Some(step) = field
        .strip_prefix("*/")
        .or_else(|| field.strip_prefix("0/"))
    {
        let n: u32 = step.parse().ok()?;
        if n == 0 {
            return None;
        }
        return Some(range.div_ceil(n));
    }
    // Danh sách `a,b,c`
    if field.contains(',') {
        return Some(field.split(',').filter(|s| !s.trim().is_empty()).count() as u32);
    }
    // Khoảng `a-b`
    if let Some((a, b)) = field.split_once('-') {
        let a: u32 = a.trim().parse().ok()?;
        let b: u32 = b.trim().parse().ok()?;
        if b < a {
            return None;
        }
        return Some(b - a + 1);
    }
    // Một giá trị cụ thể (số, hoặc tên thứ như SUN/FRI).
    Some(1)
}

/// Kiểm một biểu thức cron.
///
/// `style_majority` = dạng step phổ biến nhất trong project (`"*/"` hoặc `"0/"`), dùng để
/// báo lệch style. Truyền `None` nếu không muốn kiểm.
pub fn lint_schedule(schedule: &str, style_majority: Option<&str>) -> Vec<Finding> {
    let mut out = Vec::new();
    let f: Vec<&str> = schedule.split_whitespace().collect();

    if f.len() != 5 {
        out.push(Finding {
            severity: Severity::High,
            code: "cron.malformed",
            message: format!(
                "Cron `{schedule}` có {} trường, cron chuẩn cần đúng 5 (phút giờ ngày tháng thứ).",
                f.len()
            ),
            suggestion: None,
        });
        return out;
    }

    let (min, hour) = (f[0], f[1]);

    // Bẫy chính: phút là `*` nhưng giờ thì cụ thể → 60 lần trong giờ đó.
    if min == "*" && hour != "*" {
        out.push(Finding {
            severity: Severity::High,
            code: "cron.minuteWildcard",
            message: format!(
                "Trường phút là `*` nên job chạy **60 lần** trong giờ {hour}, mỗi phút một lần. \
                 Đây gần như luôn là gõ thiếu số 0. Rủi ro thật không phải tiền mà là chồng lấn: \
                 execution nào chạy quá 60 giây sẽ bị lần kế tiếp khởi động lên trước khi xong."
            ),
            suggestion: Some(format!("0 {} {} {} {}", f[1], f[2], f[3], f[4])),
        });
    }

    // Phút `*` và giờ `*` = mỗi phút, 1440 lần/ngày. Có thể là chủ ý (near-realtime).
    if min == "*" && hour == "*" {
        out.push(Finding {
            severity: Severity::Warn,
            code: "cron.everyMinute",
            message:
                "Cron này chạy mỗi phút (1.440 lần/ngày). Nếu là chủ ý thì bỏ qua, nhưng hãy \
                 chắc rằng job chạy xong dưới 60 giây."
                    .to_string(),
            suggestion: None,
        });
    }

    if matches!(min, "*/1" | "0/1") {
        out.push(Finding {
            severity: Severity::Info,
            code: "cron.everyMinute",
            message:
                "Chạy mỗi phút (1.440 lần/ngày) — mục tốn nhiều tiền nhất trong nhóm batch. \
                 Xác nhận là chủ ý."
                    .to_string(),
            suggestion: None,
        });
    }

    // Lệch style so với đa số project. Không phải lỗi, nhưng cron không nhất quán làm
    // người đọc phải dừng lại nghĩ mỗi lần.
    if let Some(major) = style_majority {
        let uses = |p: &str| f.iter().any(|x| x.starts_with(p));
        let other = if major == "*/" { "0/" } else { "*/" };
        if uses(other) && !uses(major) {
            out.push(Finding {
                severity: Severity::Info,
                code: "cron.styleMismatch",
                message: format!(
                    "Dùng `{other}N` trong khi phần lớn cron của project dùng `{major}N`. \
                     Hai dạng cùng nghĩa trên Cloud Scheduler, chỉ là không nhất quán."
                ),
                suggestion: Some(schedule.replace(other, major)),
            });
        }
    }

    out
}

/// Dạng step phổ biến nhất trong tập cron, để làm mốc so sánh style.
pub fn majority_step_style<'a, I: IntoIterator<Item = &'a str>>(schedules: I) -> Option<&'static str> {
    let (mut star, mut zero) = (0usize, 0usize);
    for s in schedules {
        for field in s.split_whitespace() {
            if field.starts_with("*/") {
                star += 1;
            } else if field.starts_with("0/") {
                zero += 1;
            }
        }
    }
    if star == 0 && zero == 0 {
        None
    } else if star >= zero {
        Some("*/")
    } else {
        Some("0/")
    }
}

// ---------------------------------------------------------------------------
// Quét env tìm giá trị trông như secret
// ---------------------------------------------------------------------------

/// Tiền tố khoá của các dịch vụ phổ biến. Khớp tiền tố là bằng chứng chắc chắn nhất —
/// không phải suy đoán theo entropy.
const KEY_PREFIXES: &[(&str, &str)] = &[
    ("sk_live_", "Stripe secret key (LIVE)"),
    ("sk_test_", "Stripe secret key (test)"),
    ("rk_live_", "Stripe restricted key (LIVE)"),
    ("whsec_", "Stripe webhook secret"),
    ("SG.", "SendGrid API key"),
    ("AIza", "Google API key"),
    ("ghp_", "GitHub personal access token"),
    ("gho_", "GitHub OAuth token"),
    ("github_pat_", "GitHub fine-grained token"),
    ("xoxb-", "Slack bot token"),
    ("xoxp-", "Slack user token"),
    ("AKIA", "AWS access key id"),
    ("-----BEGIN", "private key / certificate dạng PEM"),
    ("eyJ", "JWT hoặc JSON base64 (có thể là service account key)"),
];

/// Tên biến gợi ý đây là chỗ đặt bí mật.
const SENSITIVE_NAME_PARTS: &[&str] = &[
    "SECRET", "PASSWORD", "PASSWD", "TOKEN", "APIKEY", "API_KEY", "PRIVATE_KEY",
    "CREDENTIAL", "AUTH", "SIGNING", "CERT",
];

/// Giá trị rõ ràng vô hại dù tên biến có chữ nhạy cảm.
fn is_obviously_harmless(value: &str) -> bool {
    let v = value.trim();
    if v.is_empty() {
        return true;
    }
    // Cờ bật/tắt, số, cổng, đường dẫn file, tên biến trỏ tới chỗ khác.
    matches!(
        v.to_ascii_lowercase().as_str(),
        "true" | "false" | "yes" | "no" | "on" | "off" | "none" | "null" | "real" | "fake" | "mock"
    ) || v.chars().all(|c| c.is_ascii_digit())
        || v.starts_with('/')
        || v.starts_with("./")
        // Đường dẫn Windows hoặc URL trỏ tới nơi chứa secret thì bản thân nó không phải secret.
        || v.starts_with("http://")
        || v.starts_with("https://")
        || v.starts_with("mongodb://")
        || v.starts_with("redis://")
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvSecretFinding {
    pub severity: Severity,
    pub env_name: String,
    /// Vì sao nghi ngờ. **Không bao giờ chứa giá trị thật.**
    pub reason: String,
    /// Vài ký tự đầu để nhận diện, tối đa 6 — vừa đủ để tìm ra biến, không đủ để dùng.
    pub value_hint: String,
    pub value_len: usize,
}

/// Quét danh sách env plain tìm giá trị trông như secret.
///
/// # Bất biến
///
/// Kết quả **không bao giờ** chứa giá trị đầy đủ. Một báo cáo bảo mật mà tự nó in secret
/// ra là đã tạo thêm một chỗ rò rỉ. `value_hint` tối đa 6 ký tự đầu.
pub fn scan_env_secrets(env: &[(String, String)]) -> Vec<EnvSecretFinding> {
    let mut out = Vec::new();

    for (name, value) in env {
        let v = value.trim();
        if is_obviously_harmless(v) {
            continue;
        }

        let hint = hint_of(v);

        // Bằng chứng mạnh: tiền tố khoá đã biết.
        if let Some((_, what)) = KEY_PREFIXES.iter().find(|(p, _)| v.starts_with(p)) {
            let live = v.starts_with("sk_live_") || v.starts_with("rk_live_");
            out.push(EnvSecretFinding {
                severity: if live { Severity::High } else { Severity::Warn },
                env_name: name.clone(),
                reason: format!(
                    "Giá trị bắt đầu bằng tiền tố của {what}, nhưng đang là env plain — \
                     ai đọc được cấu hình service/job là đọc được nó. Nên chuyển sang \
                     Secret Manager rồi tham chiếu bằng secretKeyRef."
                ),
                value_hint: hint,
                value_len: v.len(),
            });
            continue;
        }

        // Bằng chứng yếu hơn: tên biến nhạy cảm + giá trị dài, không phải đường dẫn/URL.
        let upper = name.to_ascii_uppercase();
        if SENSITIVE_NAME_PARTS.iter().any(|k| upper.contains(k)) && v.len() >= 16 {
            out.push(EnvSecretFinding {
                severity: Severity::Warn,
                env_name: name.clone(),
                reason:
                    "Tên biến gợi ý đây là bí mật và giá trị dài, nhưng đang lưu dạng plain. \
                     Kiểm tra xem có nên chuyển sang Secret Manager."
                        .to_string(),
                value_hint: hint,
                value_len: v.len(),
            });
        }
    }

    out
}

fn hint_of(v: &str) -> String {
    let take: String = v.chars().take(6).collect();
    if v.chars().count() > 6 {
        format!("{take}…")
    } else {
        take
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(f: &[Finding]) -> Vec<&str> {
        f.iter().map(|x| x.code).collect()
    }

    // --- runs_per_day ------------------------------------------------------

    #[test]
    fn dem_so_lan_chay_cua_cron_that_ngoai_thuc_te() {
        assert_eq!(runs_per_day("0 17 * * *"), Some(1), "mỗi ngày một lần");
        assert_eq!(runs_per_day("* 17 * * *"), Some(60), "đây là 5 job bị lỗi");
        assert_eq!(runs_per_day("*/5 * * * *"), Some(12 * 24));
        assert_eq!(runs_per_day("*/15 * * * *"), Some(4 * 24));
        assert_eq!(runs_per_day("*/1 * * * *"), Some(1440));
        assert_eq!(runs_per_day("30 * * * *"), Some(24));
        assert_eq!(runs_per_day("0,30 * * * *"), Some(48));
        assert_eq!(runs_per_day("0 */6 * * *"), Some(4));
        assert_eq!(runs_per_day("0 11,21 * * *"), Some(2));
        assert_eq!(runs_per_day("0 1,3,5,7,8 * * 1-5"), Some(5));
    }

    #[test]
    fn style_0_tren_15_duoc_hieu_bang_voi_sao_tren_15() {
        // `0/15` là biến thể GNU/Quartz, Cloud Scheduler nhận. Job045 trong project thật
        // dùng dạng này trong khi 8 job khác dùng `*/15`.
        assert_eq!(runs_per_day("0/15 * * * *"), runs_per_day("*/15 * * * *"));
    }

    #[test]
    fn cron_khong_phan_tich_duoc_tra_none() {
        assert_eq!(runs_per_day("bậy bạ"), None);
        assert_eq!(runs_per_day("0 17 * *"), None, "thiếu trường");
        assert_eq!(runs_per_day("*/0 * * * *"), None, "step 0 là vô nghĩa");
    }

    // --- lint_schedule -----------------------------------------------------

    #[test]
    fn bat_dung_loi_minute_wildcard_va_goi_y_sua() {
        let f = lint_schedule("* 17 * * *", None);
        assert_eq!(codes(&f), vec!["cron.minuteWildcard"]);
        assert_eq!(f[0].severity, Severity::High);
        assert_eq!(
            f[0].suggestion.as_deref(),
            Some("0 17 * * *"),
            "gợi ý phải là cron đã sửa, không phải mô tả chung"
        );
        // Message phải nói đúng rủi ro thật, không chỉ nói về tiền.
        assert!(f[0].message.contains("chồng lấn"), "{}", f[0].message);
    }

    #[test]
    fn cron_dung_khong_bi_bao_gi() {
        for ok in ["0 17 * * *", "30 20 * * *", "*/5 * * * *", "0 22 * * SUN", "1 0 15 5 *"] {
            let f = lint_schedule(ok, Some("*/"));
            assert!(
                f.iter().all(|x| x.severity != Severity::High),
                "cron hợp lệ `{ok}` bị báo High: {f:?}"
            );
        }
    }

    #[test]
    fn moi_phut_bi_bao_nhung_khong_phai_muc_high() {
        // `* * * * *` là chủ ý được (near-realtime), không nên hét lên như lỗi.
        let f = lint_schedule("* * * * *", None);
        assert!(f.iter().any(|x| x.code == "cron.everyMinute"));
        assert!(
            f.iter().all(|x| x.severity != Severity::High),
            "chạy mỗi phút có thể là chủ ý, không phải lỗi chắc chắn"
        );
    }

    #[test]
    fn cron_sai_so_truong_bi_bao_high() {
        let f = lint_schedule("0 17 * *", None);
        assert_eq!(codes(&f), vec!["cron.malformed"]);
        assert_eq!(f[0].severity, Severity::High);
        assert!(f[0].message.contains("4 trường"));
    }

    #[test]
    fn bao_lech_style_khi_da_so_dung_dang_khac() {
        let f = lint_schedule("0/15 * * * *", Some("*/"));
        assert!(f.iter().any(|x| x.code == "cron.styleMismatch"), "{f:?}");
        let s = f.iter().find(|x| x.code == "cron.styleMismatch").unwrap();
        assert_eq!(s.severity, Severity::Info, "lệch style không phải lỗi");
        assert_eq!(s.suggestion.as_deref(), Some("*/15 * * * *"));
    }

    #[test]
    fn khong_bao_lech_style_khi_dung_dang_da_so() {
        let f = lint_schedule("*/15 * * * *", Some("*/"));
        assert!(!f.iter().any(|x| x.code == "cron.styleMismatch"));
    }

    #[test]
    fn majority_style_theo_dung_so_dong_thuc_te() {
        // Trong project thật: 8 job dùng `*/15`, 1 job dùng `0/15`.
        let all = vec![
            "*/15 * * * *", "*/15 * * * *", "*/15 * * * *", "*/5 * * * *", "0/15 * * * *",
        ];
        assert_eq!(majority_step_style(all), Some("*/"));

        assert_eq!(majority_step_style(vec!["0 17 * * *"]), None, "không có step nào");
        assert_eq!(majority_step_style(vec!["0/15 * * * *", "0/5 * * * *"]), Some("0/"));
    }

    // --- scan_env_secrets --------------------------------------------------

    fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn bat_duoc_stripe_key_dang_plain() {
        // Kiểu env hay gặp ngoài thực tế: key Stripe để thẳng dạng plain trong cấu hình job.
        // Giá trị dưới đây cố tình có dấu gạch ngang để không khớp định dạng key Stripe thật —
        // nếu không, secret scanner của GitHub sẽ chặn push chính file test này.
        let f = scan_env_secrets(&env(&[(
            "STRIPE_API_KEY",
            "sk_test_KHONG-PHAI-KEY-THAT-CHI-DE-TEST-0123",
        )]));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].env_name, "STRIPE_API_KEY");
        assert!(f[0].reason.contains("Stripe"), "{}", f[0].reason);
        assert!(f[0].reason.contains("Secret Manager"));
    }

    #[test]
    fn ket_qua_khong_bao_gio_chua_gia_tri_day_du() {
        // Bất biến của module: báo cáo bảo mật không được tự tạo thêm chỗ rò rỉ.
        let secret = "sk_live_KHONG-PHAI-KEY-THAT-CHI-DE-TEST-0123";
        let f = scan_env_secrets(&env(&[("STRIPE_API_KEY", secret)]));
        let dumped = serde_json::to_string(&f).unwrap();
        assert!(!dumped.contains(secret), "toàn bộ secret bị in ra: {dumped}");
        assert!(
            !dumped.contains("PHAI-KEY-THAT"),
            "phần đuôi secret bị in ra: {dumped}"
        );
        assert_eq!(f[0].value_hint.chars().count(), 7, "6 ký tự + dấu …");
        assert_eq!(f[0].value_len, secret.len(), "độ dài vẫn báo để nhận diện");
    }

    #[test]
    fn key_live_nghiem_trong_hon_key_test() {
        let live = scan_env_secrets(&env(&[("K", "sk_live_xxxxxxxxxxxxxxxxxxxx")]));
        let test = scan_env_secrets(&env(&[("K", "sk_test_xxxxxxxxxxxxxxxxxxxx")]));
        assert_eq!(live[0].severity, Severity::High);
        assert_eq!(test[0].severity, Severity::Warn);
    }

    #[test]
    fn bat_duoc_cac_tien_to_pho_bien_khac() {
        for (v, what) in [
            ("AIzaSyDxxxxxxxxxxxxxxxxxxxxxxxx", "Google"),
            ("ghp_xxxxxxxxxxxxxxxxxxxxxxxx", "GitHub"),
            ("xoxb-1234-5678-abcdefg", "Slack"),
            ("AKIAIOSFODNN7EXAMPLE", "AWS"),
            ("SG.xxxxxxxxxxxxxxxxxxxxxx", "SendGrid"),
            ("-----BEGIN PRIVATE KEY-----", "PEM"),
        ] {
            let f = scan_env_secrets(&env(&[("SOME_VAR", v)]));
            assert_eq!(f.len(), 1, "không bắt được {what}: {v}");
        }
    }

    #[test]
    fn khong_bao_dong_gia_voi_gia_tri_ro_rang_vo_hai() {
        // Đây là các giá trị thật lấy từ job204 — nếu linter kêu ở đây thì nó thành nhiễu
        // và người dùng sẽ tắt luôn tính năng.
        let f = scan_env_secrets(&env(&[
            ("GRPC_CLIENT_ADMIN_PLAINTEXT", "false"),
            ("GRPC_CLIENT_ADMIN_PORT", "443"),
            ("FIREBASE_SERVICE_ACCOUNT", "/var/opt/firebase/firebase.json"),
            ("GOOGLE_SHEET_SERVICE_ACCOUNT", "/var/opt/google-sheet-sa.json"),
            ("CLOUD_CONFIG_URI", "https://config-123456789012.asia-northeast1.run.app"),
            (
                "MONGO_ADMIN_URL",
                "mongodb://mongo4-0.mongo4.example.svc.cluster.local:27017/manage",
            ),
            ("BARCODE_READER_DATA_SOURCE", "real"),
            ("ARGS", ""),
            ("LOGGING_LEVEL_ENV", "INFO"),
        ]));
        assert!(f.is_empty(), "báo động giả: {f:?}");
    }

    #[test]
    fn ten_bien_nhay_cam_voi_gia_tri_dai_thi_canh_bao() {
        let f = scan_env_secrets(&env(&[("DB_PASSWORD", "một-mật-khẩu-thật-dài-ở-đây")]));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, Severity::Warn);
    }

    #[test]
    fn ten_bien_nhay_cam_nhung_gia_tri_ngan_thi_bo_qua() {
        // "INFO", "true", cờ ngắn — không phải secret.
        let f = scan_env_secrets(&env(&[("AUTH_MODE", "oauth2")]));
        assert!(f.is_empty(), "{f:?}");
    }

    #[test]
    fn env_rong_khong_gay_bao_dong() {
        assert!(scan_env_secrets(&[]).is_empty());
        assert!(scan_env_secrets(&env(&[("K", "")])).is_empty());
        assert!(scan_env_secrets(&env(&[("K", "   ")])).is_empty());
    }
}
