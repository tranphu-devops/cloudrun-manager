//! Ước lượng chi phí Cloud Run từ metric × giá SKU.
//!
//! # Đây là ƯỚC LƯỢNG, không phải chi phí thật
//!
//! Không có API nào trả về chi phí thực tế theo service; nguồn chính xác duy nhất là
//! BigQuery billing export. Module này nhân lượng tài nguyên đo được với đơn giá công bố.
//! Mọi type/hàm ở đây mang chữ `estimate` để không ai đọc nhầm — xem `ERROR_SOURCES`.
//!
//! # Bẫy lớn nhất: Cloud Run có HAI mô hình tính tiền, lệch nhau ~10× ở CPU
//!
//! Mô hình nào được áp dụng phụ thuộc cờ `resources.cpuIdle` của container:
//!
//! | | request-based (`cpuIdle = true`, mặc định) | instance-based (`cpuIdle = false`) |
//! |---|---|---|
//! | CPU active | $0.000024 / vCPU-s | $0.000018 / vCPU-s (cả vòng đời) |
//! | CPU idle | $0.0000025 / vCPU-s | — |
//! | Memory | $0.0000025 / GiB-s | $0.000002 / GiB-s |
//! | Request | $0.40 / triệu | **không tính** |
//!
//! Dùng một mô hình cho cả hai là sai từ 10% đến gần 10× tuỳ hình dạng tải. Đây là lý do
//! `estimate` **bắt buộc** nhận `cpu_idle`.

use serde::{Deserialize, Serialize};

/// Đơn giá tier 1 (bao gồm `asia-northeast1`), USD.
///
/// Hardcode làm fallback; `fetch_price_book` lấy giá thật từ Cloud Billing Catalog API để
/// không phải sửa code khi Google đổi giá.
pub mod rates {
    /// request-based
    pub const CPU_ACTIVE: f64 = 0.000024;
    pub const CPU_IDLE: f64 = 0.0000025;
    pub const MEM_ACTIVE: f64 = 0.0000025;
    pub const MEM_IDLE: f64 = 0.0000025;
    pub const PER_MILLION_REQUESTS: f64 = 0.40;

    /// instance-based (và Cloud Run Jobs)
    pub const CPU_INSTANCE: f64 = 0.000018;
    pub const MEM_INSTANCE: f64 = 0.000002;

    /// Free tier mỗi tháng, mỗi billing account (KHÔNG chia được theo service).
    pub const FREE_CPU_S_REQUEST_BASED: f64 = 180_000.0;
    pub const FREE_MEM_S_REQUEST_BASED: f64 = 360_000.0;
    pub const FREE_REQUESTS: f64 = 2_000_000.0;
    pub const FREE_CPU_S_INSTANCE_BASED: f64 = 240_000.0;
    pub const FREE_MEM_S_INSTANCE_BASED: f64 = 450_000.0;
}

/// Region tier 2 có giá cao hơn. Toàn bộ hạ tầng hiện tại ở `asia-northeast1` (tier 1),
/// nhưng phải chặn để không lặng lẽ báo số thấp hơn thực tế nếu có service ở region khác.
pub const TIER_2_REGIONS: &[&str] = &[
    "asia-east2", "asia-northeast3", "asia-southeast1", "asia-southeast2", "asia-south2",
    "australia-southeast1", "australia-southeast2", "europe-central2", "europe-west10",
    "europe-west12", "europe-west2", "europe-west3", "europe-west6", "me-central1",
    "me-central2", "northamerica-northeast1", "northamerica-northeast2", "southamerica-east1",
    "southamerica-west1", "us-west2", "us-west3", "us-west4",
];

pub fn is_tier2(region: &str) -> bool {
    TIER_2_REGIONS.contains(&region)
}

/// Bảy nguồn sai số, hiện thẳng trên UI chứ không cất trong doc.
///
/// Một con số chi phí không kèm biên độ sai sẽ bị dùng để đối chiếu hoá đơn, rồi khi lệch
/// thì mất niềm tin vào cả app.
pub const ERROR_SOURCES: &[&str] = &[
    "Free tier không trừ được theo từng service — nó tính trên cả billing account mỗi tháng và dùng chung giữa các service. Số hiện ở đây là chi phí gộp (gross).",
    "Committed use discount và giá thương lượng riêng không được áp.",
    "Không tính egress mạng, dung lượng Artifact Registry, Cloud Build, phí ingest log.",
    "Instance-based làm tròn tối thiểu 1 phút mỗi instance — job chạy 5 giây vẫn bị tính 1 phút, nên ước lượng sẽ thấp hơn thực tế.",
    "Thời gian startup và shutdown vẫn bị tính tiền nhưng không phản ánh đủ trong metric instance_count.",
    "GPU không được mô hình hoá — service nào gắn GPU sẽ bị ước lượng THẤP hơn thực tế rất nhiều, vì riêng GPU đã đắt hơn CPU và memory cộng lại.",
    "Region tier 2 có đơn giá cao hơn; app cảnh báo riêng khi gặp.",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BillingMode {
    /// `cpuIdle = true` — mặc định của Cloud Run.
    RequestBased,
    /// `cpuIdle = false` ("CPU always allocated"), và mọi Cloud Run Job.
    InstanceBased,
}

impl BillingMode {
    /// Suy từ cờ `cpuIdle` của container. `None` = mặc định Cloud Run = request-based.
    pub fn from_cpu_idle(cpu_idle: Option<bool>) -> Self {
        match cpu_idle {
            Some(false) => BillingMode::InstanceBased,
            _ => BillingMode::RequestBased,
        }
    }

    pub fn label_vi(self) -> &'static str {
        match self {
            BillingMode::RequestBased => "theo request (CPU chỉ khi xử lý)",
            BillingMode::InstanceBased => "theo instance (CPU luôn cấp)",
        }
    }
}

/// Lượng tài nguyên đo được trong một cửa sổ thời gian.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    /// instance-giây ở trạng thái active.
    pub instance_seconds_active: f64,
    /// instance-giây ở trạng thái idle (min-instance giữ ấm).
    pub instance_seconds_idle: f64,
    pub requests: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostEstimate {
    pub mode: BillingMode,
    pub cpu_cost: f64,
    pub memory_cost: f64,
    pub request_cost: f64,
    pub total: f64,
    pub vcpu_seconds: f64,
    pub gib_seconds: f64,
    /// Luôn `true`. Có mặt trong payload để frontend không thể quên rằng đây là ước lượng.
    pub estimated: bool,
}

/// Tính chi phí ước lượng. Hàm thuần — đây là chỗ được test bằng số tính tay.
///
/// `cpu` = số vCPU (1.0, 0.5…), `mem_gib` = GiB.
pub fn estimate(usage: Usage, cpu: f64, mem_gib: f64, mode: BillingMode) -> CostEstimate {
    let inst_total = usage.instance_seconds_active + usage.instance_seconds_idle;

    let (cpu_cost, memory_cost, request_cost) = match mode {
        BillingMode::RequestBased => (
            usage.instance_seconds_active * cpu * rates::CPU_ACTIVE
                + usage.instance_seconds_idle * cpu * rates::CPU_IDLE,
            usage.instance_seconds_active * mem_gib * rates::MEM_ACTIVE
                + usage.instance_seconds_idle * mem_gib * rates::MEM_IDLE,
            usage.requests / 1_000_000.0 * rates::PER_MILLION_REQUESTS,
        ),
        BillingMode::InstanceBased => (
            inst_total * cpu * rates::CPU_INSTANCE,
            inst_total * mem_gib * rates::MEM_INSTANCE,
            // Instance-based không tính phí theo request.
            0.0,
        ),
    };

    CostEstimate {
        mode,
        cpu_cost,
        memory_cost,
        request_cost,
        total: cpu_cost + memory_cost + request_cost,
        vcpu_seconds: inst_total * cpu,
        gib_seconds: inst_total * mem_gib,
        estimated: true,
    }
}

/// `"1"`, `"0.5"`, `"500m"` → số vCPU.
pub fn parse_cpu(cpu: Option<&str>) -> f64 {
    let Some(s) = cpu.map(str::trim) else {
        // Cloud Run mặc định 1 vCPU khi không khai báo.
        return 1.0;
    };
    if let Some(m) = s.strip_suffix('m') {
        return m.parse::<f64>().unwrap_or(1000.0) / 1000.0;
    }
    s.parse().unwrap_or(1.0)
}

/// `"512Mi"`, `"1Gi"`, `"2G"` → GiB.
pub fn parse_memory_gib(mem: Option<&str>) -> f64 {
    let Some(s) = mem.map(str::trim) else {
        // Mặc định của Cloud Run.
        return 0.5;
    };
    for (unit, factor) in [
        ("Gi", 1.0),
        ("Mi", 1.0 / 1024.0),
        ("Ki", 1.0 / (1024.0 * 1024.0)),
        // G/M/K thập phân: quy về GiB cho đúng đơn vị tính tiền.
        ("G", 1_000_000_000.0 / 1_073_741_824.0),
        ("M", 1_000_000.0 / 1_073_741_824.0),
        ("K", 1_000.0 / 1_073_741_824.0),
    ] {
        if let Some(num) = s.strip_suffix(unit) {
            if let Ok(v) = num.parse::<f64>() {
                return v * factor;
            }
        }
    }
    // Số trần = byte.
    s.parse::<f64>()
        .map(|b| b / 1_073_741_824.0)
        .unwrap_or(0.5)
}

/// Free tier có thể bù được bao nhiêu, tính ở cấp project.
///
/// Trả về **cận trên** của phần được miễn: free tier dùng chung giữa các service nên không
/// chia được, và app cố tình không chia — chia bừa sẽ tạo ra con số trông chính xác mà sai.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FreeTierOffset {
    pub cpu_seconds_covered: f64,
    pub gib_seconds_covered: f64,
    pub requests_covered: f64,
    pub max_saving: f64,
}

pub fn free_tier_offset(
    total_vcpu_s: f64,
    total_gib_s: f64,
    total_requests: f64,
    mode: BillingMode,
) -> FreeTierOffset {
    let (fcpu, fmem, cpu_rate, mem_rate) = match mode {
        BillingMode::RequestBased => (
            rates::FREE_CPU_S_REQUEST_BASED,
            rates::FREE_MEM_S_REQUEST_BASED,
            rates::CPU_ACTIVE,
            rates::MEM_ACTIVE,
        ),
        BillingMode::InstanceBased => (
            rates::FREE_CPU_S_INSTANCE_BASED,
            rates::FREE_MEM_S_INSTANCE_BASED,
            rates::CPU_INSTANCE,
            rates::MEM_INSTANCE,
        ),
    };
    let cpu_cov = total_vcpu_s.min(fcpu);
    let mem_cov = total_gib_s.min(fmem);
    let req_cov = match mode {
        BillingMode::RequestBased => total_requests.min(rates::FREE_REQUESTS),
        BillingMode::InstanceBased => 0.0,
    };

    FreeTierOffset {
        cpu_seconds_covered: cpu_cov,
        gib_seconds_covered: mem_cov,
        requests_covered: req_cov,
        max_saving: cpu_cov * cpu_rate
            + mem_cov * mem_rate
            + req_cov / 1_000_000.0 * rates::PER_MILLION_REQUESTS,
    }
}

/// Lý do một service tốn tiền, suy ra từ chính cấu hình + tải.
///
/// Con số tổng chỉ nói "tốn bao nhiêu". Cái người vận hành cần là "vì sao" và "sửa được không".
pub fn cost_drivers(
    min_instances: Option<i64>,
    rps: f64,
    mode: BillingMode,
    cpu: f64,
    est_total: f64,
) -> Vec<String> {
    let mut out = Vec::new();

    if min_instances.unwrap_or(0) > 0 && rps < 0.05 {
        out.push(format!(
            "min-instances = {} nhưng gần như không có request. Đang trả tiền 24/7 để giữ ấm một \
             service không ai gọi — đặt về 0 nếu chấp nhận được cold start.",
            min_instances.unwrap_or(0)
        ));
    } else if min_instances.unwrap_or(0) > 0 {
        out.push(format!(
            "min-instances = {} nên luôn có instance chạy và bị tính tiền cả khi rảnh.",
            min_instances.unwrap_or(0)
        ));
    }

    if mode == BillingMode::InstanceBased {
        if rps < 1.0 {
            out.push(
                "Đang bật \"CPU luôn cấp\" (instance-based) nhưng tải thấp. Chế độ này tính tiền \
                 toàn bộ vòng đời instance; nếu app không có việc chạy nền ngoài request thì tắt \
                 đi sẽ rẻ hơn."
                    .to_string(),
            );
        } else {
            out.push(
                "Đang bật \"CPU luôn cấp\" (instance-based): tính tiền cả vòng đời instance, \
                 không chỉ lúc xử lý request."
                    .to_string(),
            );
        }
    }

    if cpu >= 2.0 {
        out.push(format!(
            "Cấp {cpu} vCPU mỗi instance — chi phí CPU tỉ lệ thẳng với con số này."
        ));
    }

    if rps >= 20.0 {
        out.push(format!("Tải cao ({rps:.0} req/s) nên cần nhiều instance."));
    }

    if out.is_empty() && est_total > 0.0 {
        out.push("Không có dấu hiệu cấu hình bất thường — chi phí đến từ lượng tải bình thường.".to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    // --- chọn mô hình -----------------------------------------------------

    #[test]
    fn cpu_idle_quyet_dinh_mo_hinh_tinh_tien() {
        assert_eq!(BillingMode::from_cpu_idle(Some(true)), BillingMode::RequestBased);
        assert_eq!(BillingMode::from_cpu_idle(Some(false)), BillingMode::InstanceBased);
        assert_eq!(
            BillingMode::from_cpu_idle(None),
            BillingMode::RequestBased,
            "không khai báo = mặc định Cloud Run = request-based"
        );
    }

    // --- công thức, đối chiếu bằng số tính tay ----------------------------

    #[test]
    fn request_based_tinh_dung_theo_so_tay() {
        // 1000 s active + 500 s idle, 1 vCPU, 0.5 GiB, 100k request.
        let u = Usage {
            instance_seconds_active: 1000.0,
            instance_seconds_idle: 500.0,
            requests: 100_000.0,
        };
        let e = estimate(u, 1.0, 0.5, BillingMode::RequestBased);

        // CPU  = 1000×1×0.000024 + 500×1×0.0000025 = 0.024 + 0.00125 = 0.02525
        assert!(close(e.cpu_cost, 0.02525), "cpu = {}", e.cpu_cost);
        // MEM  = 1000×0.5×0.0000025 + 500×0.5×0.0000025 = 0.00125 + 0.000625 = 0.001875
        assert!(close(e.memory_cost, 0.001875), "mem = {}", e.memory_cost);
        // REQ  = 100000/1e6 × 0.40 = 0.04
        assert!(close(e.request_cost, 0.04), "req = {}", e.request_cost);
        assert!(close(e.total, 0.02525 + 0.001875 + 0.04));
        assert!(e.estimated);
    }

    #[test]
    fn instance_based_tinh_dung_theo_so_tay_va_khong_tinh_request() {
        let u = Usage {
            instance_seconds_active: 1000.0,
            instance_seconds_idle: 500.0,
            requests: 100_000.0,
        };
        let e = estimate(u, 1.0, 0.5, BillingMode::InstanceBased);

        // CPU = 1500×1×0.000018 = 0.027
        assert!(close(e.cpu_cost, 0.027), "cpu = {}", e.cpu_cost);
        // MEM = 1500×0.5×0.000002 = 0.0015
        assert!(close(e.memory_cost, 0.0015), "mem = {}", e.memory_cost);
        assert!(
            close(e.request_cost, 0.0),
            "instance-based KHÔNG tính phí request, nhận {}",
            e.request_cost
        );
    }

    #[test]
    fn hai_mo_hinh_lech_nhau_dang_ke_o_cpu_idle() {
        // Đây là lý do phải rẽ nhánh: cùng một lượng tài nguyên, khác mô hình → khác xa.
        let u = Usage {
            instance_seconds_active: 0.0,
            instance_seconds_idle: 100_000.0,
            requests: 0.0,
        };
        let req = estimate(u, 1.0, 0.5, BillingMode::RequestBased);
        let inst = estimate(u, 1.0, 0.5, BillingMode::InstanceBased);

        // idle: request-based 0.0000025 vs instance-based 0.000018 → 7.2×
        let ratio = inst.cpu_cost / req.cpu_cost;
        assert!(
            (ratio - 7.2).abs() < 0.01,
            "tỉ lệ CPU idle giữa hai mô hình phải là 7.2×, nhận {ratio}"
        );
    }

    #[test]
    fn khong_co_tai_thi_chi_phi_bang_khong() {
        let e = estimate(Usage::default(), 1.0, 0.5, BillingMode::RequestBased);
        assert!(close(e.total, 0.0));
        assert!(close(e.vcpu_seconds, 0.0));
    }

    #[test]
    fn cpu_nhieu_hon_thi_chi_phi_cpu_ti_le_thuan() {
        let u = Usage {
            instance_seconds_active: 1000.0,
            instance_seconds_idle: 0.0,
            requests: 0.0,
        };
        let a = estimate(u, 1.0, 0.5, BillingMode::RequestBased);
        let b = estimate(u, 4.0, 0.5, BillingMode::RequestBased);
        assert!(close(b.cpu_cost, a.cpu_cost * 4.0));
        assert!(close(b.memory_cost, a.memory_cost), "memory không đổi theo cpu");
    }

    // --- parse cpu / memory ----------------------------------------------

    #[test]
    fn parse_cpu_cac_dang_cloud_run_nhan() {
        assert!(close(parse_cpu(Some("1")), 1.0));
        assert!(close(parse_cpu(Some("2")), 2.0));
        assert!(close(parse_cpu(Some("0.5")), 0.5));
        assert!(close(parse_cpu(Some("500m")), 0.5));
        assert!(close(parse_cpu(Some("80m")), 0.08));
        assert!(close(parse_cpu(Some(" 1 ")), 1.0));
    }

    #[test]
    fn parse_cpu_thieu_hoac_rac_thi_ve_mac_dinh_1() {
        assert!(close(parse_cpu(None), 1.0));
        assert!(close(parse_cpu(Some("bậy")), 1.0));
    }

    #[test]
    fn parse_memory_cac_dang_cloud_run_nhan() {
        assert!(close(parse_memory_gib(Some("1Gi")), 1.0));
        assert!(close(parse_memory_gib(Some("512Mi")), 0.5));
        assert!(close(parse_memory_gib(Some("2Gi")), 2.0));
        assert!(close(parse_memory_gib(Some("128Mi")), 0.125));
        // Job204 thật dùng 2Gi.
        assert!(close(parse_memory_gib(Some("2Gi")), 2.0));
    }

    #[test]
    fn parse_memory_don_vi_thap_phan_khac_don_vi_nhi_phan() {
        // 1G = 10^9 byte ≈ 0.931 GiB, KHÔNG phải 1 GiB. Nhầm chỗ này là sai 7%.
        let g = parse_memory_gib(Some("1G"));
        assert!((g - 0.9313).abs() < 0.001, "1G phải ≈0.931 GiB, nhận {g}");
        assert!(close(parse_memory_gib(Some("1Gi")), 1.0));
    }

    #[test]
    fn parse_memory_thieu_thi_ve_mac_dinh_512mi() {
        assert!(close(parse_memory_gib(None), 0.5));
        assert!(close(parse_memory_gib(Some("bậy")), 0.5));
    }

    // --- tier region ------------------------------------------------------

    #[test]
    fn asia_northeast1_la_tier1_nen_dung_gia_mac_dinh() {
        assert!(!is_tier2("asia-northeast1"), "toàn bộ hạ tầng hiện tại ở đây");
        assert!(!is_tier2("us-central1"));
        assert!(is_tier2("asia-southeast1"), "Singapore là tier 2");
        assert!(is_tier2("asia-northeast3"));
    }

    // --- free tier --------------------------------------------------------

    #[test]
    fn free_tier_bi_kep_o_han_muc_khong_vuot_qua() {
        let f = free_tier_offset(1_000_000.0, 1_000_000.0, 10_000_000.0, BillingMode::RequestBased);
        assert!(close(f.cpu_seconds_covered, rates::FREE_CPU_S_REQUEST_BASED));
        assert!(close(f.gib_seconds_covered, rates::FREE_MEM_S_REQUEST_BASED));
        assert!(close(f.requests_covered, rates::FREE_REQUESTS));
    }

    #[test]
    fn dung_it_hon_free_tier_thi_chi_bu_dung_phan_da_dung() {
        let f = free_tier_offset(1000.0, 2000.0, 5000.0, BillingMode::RequestBased);
        assert!(close(f.cpu_seconds_covered, 1000.0));
        assert!(close(f.gib_seconds_covered, 2000.0));
        assert!(close(f.requests_covered, 5000.0));
    }

    #[test]
    fn instance_based_khong_co_free_tier_cho_request() {
        let f = free_tier_offset(1000.0, 1000.0, 1_000_000.0, BillingMode::InstanceBased);
        assert!(close(f.requests_covered, 0.0));
    }

    #[test]
    fn free_tier_han_muc_khac_nhau_giua_hai_mo_hinh() {
        let r = free_tier_offset(1e9, 1e9, 0.0, BillingMode::RequestBased);
        let i = free_tier_offset(1e9, 1e9, 0.0, BillingMode::InstanceBased);
        assert!(
            i.cpu_seconds_covered > r.cpu_seconds_covered,
            "instance-based có free tier CPU cao hơn (240k vs 180k)"
        );
    }

    // --- cost drivers -----------------------------------------------------

    #[test]
    fn chi_ra_min_instance_giu_am_service_khong_ai_goi() {
        let d = cost_drivers(Some(2), 0.0, BillingMode::RequestBased, 1.0, 5.0);
        let joined = d.join(" | ");
        assert!(joined.contains("không ai gọi"), "{joined}");
        assert!(joined.contains("cold start"), "phải nêu cái đánh đổi: {joined}");
    }

    #[test]
    fn min_instance_co_tai_thi_khong_noi_la_vo_ich() {
        let d = cost_drivers(Some(2), 50.0, BillingMode::RequestBased, 1.0, 5.0);
        assert!(!d.join(" ").contains("không ai gọi"));
    }

    #[test]
    fn chi_ra_cpu_always_on_khi_tai_thap() {
        let d = cost_drivers(None, 0.1, BillingMode::InstanceBased, 1.0, 5.0);
        assert!(d.join(" ").contains("tắt"), "{d:?}");
    }

    #[test]
    fn khong_co_dau_hieu_bat_thuong_thi_noi_ro_la_binh_thuong() {
        let d = cost_drivers(Some(0), 5.0, BillingMode::RequestBased, 1.0, 3.0);
        assert_eq!(d.len(), 1);
        assert!(d[0].contains("bình thường"), "{d:?}");
    }

    // --- bất biến của module ---------------------------------------------

    #[test]
    fn co_du_bay_nguon_sai_so_va_free_tier_dung_dau() {
        assert_eq!(ERROR_SOURCES.len(), 7);
        assert!(
            ERROR_SOURCES[0].contains("Free tier"),
            "nguồn sai số lớn nhất phải đứng đầu"
        );
        // Không nguồn nào được để trống — đây là nội dung hiện trên UI.
        assert!(ERROR_SOURCES.iter().all(|s| s.len() > 40));
    }

    #[test]
    fn moi_ket_qua_deu_danh_dau_la_uoc_luong() {
        for mode in [BillingMode::RequestBased, BillingMode::InstanceBased] {
            let e = estimate(Usage::default(), 1.0, 0.5, mode);
            assert!(e.estimated, "cờ estimated phải luôn bật");
        }
    }
}
