//! Test cho tầng read-modify-write.
//!
//! Đây là bộ test quan trọng nhất của repo. Mỗi test ở đây tương ứng với một cách
//! làm sập service thật trên Cloud Run mà một implementation "hợp lý nhưng sai" sẽ mắc.

use gcp::mutate::*;
use gcp::types::{EnvChange, EnvEntry, EnvKind, ScalingUpdate};
use serde_json::{json, Value};

/// Fixture mô phỏng response GET của Cloud Run Admin API v2, dựng theo hình dạng thật
/// của một service kiểu `gateway` trong `example-project`:
///   - env trộn plain và secret-ref
///   - secret mount dạng volume
///   - có `vpcAccess`, `startupProbe`, `binaryAuthorization` — những field mà code này
///     KHÔNG hiểu và phải giữ nguyên
///   - có `template.revision` đã được chỉ định (bẫy PATCH)
fn fixture() -> Value {
    json!({
      "name": "projects/example-project/locations/asia-northeast1/services/gateway",
      "uid": "d1e2f3a4-0000-1111-2222-333344445555",
      "generation": "41",
      "labels": { "team": "platform", "managed-by": "terraform" },
      "annotations": { "run.googleapis.com/ingress": "all" },
      "createTime": "2025-01-04T02:11:00.123456Z",
      "updateTime": "2026-07-28T08:04:12.000000Z",
      "creator": "deployer@example-project.iam.gserviceaccount.com",
      "lastModifier": "you@example.com",
      "ingress": "INGRESS_TRAFFIC_ALL",
      "launchStage": "GA",
      "binaryAuthorization": { "useDefault": true },
      "template": {
        "revision": "gateway-00041-abc",
        "labels": { "run.googleapis.com/startupProbeType": "Default" },
        "scaling": { "minInstanceCount": 1, "maxInstanceCount": 10 },
        "vpcAccess": {
          "connector": "projects/example-project/locations/asia-northeast1/connectors/vpc-conn",
          "egress": "PRIVATE_RANGES_ONLY"
        },
        "timeout": "300s",
        "serviceAccount": "gateway-runtime@example-project.iam.gserviceaccount.com",
        "containers": [
          {
            "name": "app",
            "image": "asia-northeast1-docker.pkg.dev/example-project/svc/gateway:v1.8.2",
            "env": [
              { "name": "LOG_LEVEL", "value": "info" },
              { "name": "EMPTY_ON_PURPOSE" },
              {
                "name": "DB_PASSWORD",
                "valueSource": {
                  "secretKeyRef": { "secret": "gateway-db-password", "version": "latest" }
                }
              },
              {
                "name": "JWT_SIGNING_KEY",
                "valueSource": {
                  "secretKeyRef": {
                    "secret": "projects/example-project/secrets/jwt-signing-key",
                    "version": "3",
                    "someFutureFieldGoogleAdded": "keep-me"
                  }
                }
              },
              { "name": "FEATURE_FLAGS", "value": "a,b,c" }
            ],
            "resources": {
              "limits": { "cpu": "1", "memory": "512Mi" },
              "cpuIdle": true,
              "startupCpuBoost": false
            },
            "ports": [ { "name": "http1", "containerPort": 8080 } ],
            "volumeMounts": [ { "name": "tls-certs", "mountPath": "/etc/certs" } ],
            "startupProbe": {
              "timeoutSeconds": 240,
              "periodSeconds": 240,
              "failureThreshold": 1,
              "tcpSocket": { "port": 8080 }
            }
          }
        ],
        "volumes": [
          {
            "name": "tls-certs",
            "secret": {
              "secret": "gateway-tls",
              "items": [ { "path": "tls.crt", "version": "latest" } ]
            }
          }
        ],
        "maxInstanceRequestConcurrency": 80,
        "executionEnvironment": "EXECUTION_ENVIRONMENT_GEN2"
      },
      "traffic": [ { "type": "TRAFFIC_TARGET_ALLOCATION_TYPE_LATEST", "percent": 100 } ],
      "observedGeneration": "41",
      "terminalCondition": { "type": "Ready", "state": "CONDITION_SUCCEEDED" },
      "conditions": [ { "type": "RoutesReady", "state": "CONDITION_SUCCEEDED" } ],
      "latestReadyRevision": "projects/example-project/locations/asia-northeast1/revisions/gateway-00041-abc",
      "latestCreatedRevision": "projects/example-project/locations/asia-northeast1/revisions/gateway-00041-abc",
      "trafficStatuses": [ { "type": "TRAFFIC_TARGET_ALLOCATION_TYPE_LATEST", "percent": 100 } ],
      "uri": "https://gateway-a1b2c3d4e5-an.a.run.app",
      "urls": [ "https://gateway-a1b2c3d4e5-an.a.run.app" ],
      "reconciling": false,
      "etag": "\"CJmS8LwGEAE=\""
    })
}

fn env_of(payload: &Value) -> Vec<Value> {
    payload["template"]["containers"][0]["env"]
        .as_array()
        .expect("env phải là array")
        .clone()
}

fn find_env<'a>(payload: &'a Value, name: &str) -> Option<&'a Value> {
    payload["template"]["containers"][0]["env"]
        .as_array()?
        .iter()
        .find(|e| e.get("name").and_then(|n| n.as_str()) == Some(name))
}

// ===========================================================================
// sanitize_for_patch
// ===========================================================================

#[test]
fn sanitize_bo_het_field_output_only() {
    let out = sanitize_for_patch(&fixture());
    for k in [
        "uid",
        "generation",
        "createTime",
        "updateTime",
        "creator",
        "lastModifier",
        "latestReadyRevision",
        "latestCreatedRevision",
        "terminalCondition",
        "conditions",
        "observedGeneration",
        "trafficStatuses",
        "uri",
        "urls",
        "reconciling",
    ] {
        assert!(
            out.get(k).is_none(),
            "field output-only `{k}` vẫn còn trong payload PATCH"
        );
    }
}

#[test]
fn sanitize_giu_etag() {
    // Mất etag là mất lá chắn lost-update — không được lọc nhầm.
    let out = sanitize_for_patch(&fixture());
    assert_eq!(out["etag"], json!("\"CJmS8LwGEAE=\""));
    assert!(require_etag(&out).is_ok());
}

#[test]
fn sanitize_xoa_template_revision() {
    // Đây là bẫy: giữ lại revision name cũ thì PATCH bị từ chối
    // "Revision gateway-00041-abc already exists".
    let before = fixture();
    assert_eq!(before["template"]["revision"], json!("gateway-00041-abc"));

    let out = sanitize_for_patch(&before);
    assert!(
        out["template"].get("revision").is_none(),
        "template.revision phải bị xoá để Cloud Run tự sinh tên revision kế tiếp"
    );
}

#[test]
fn sanitize_giu_nguyen_moi_field_cau_hinh_ke_ca_field_la() {
    let out = sanitize_for_patch(&fixture());

    // Những field này code không hiểu nhưng xoá đi là phá cấu hình service.
    assert_eq!(
        out["template"]["vpcAccess"]["connector"],
        json!("projects/example-project/locations/asia-northeast1/connectors/vpc-conn")
    );
    assert_eq!(out["template"]["vpcAccess"]["egress"], json!("PRIVATE_RANGES_ONLY"));
    assert_eq!(out["binaryAuthorization"]["useDefault"], json!(true));
    assert_eq!(
        out["template"]["containers"][0]["startupProbe"]["timeoutSeconds"],
        json!(240)
    );
    assert_eq!(out["template"]["volumes"][0]["secret"]["secret"], json!("gateway-tls"));
    assert_eq!(out["labels"]["managed-by"], json!("terraform"));
    assert_eq!(out["launchStage"], json!("GA"));
    assert_eq!(
        out["template"]["executionEnvironment"],
        json!("EXECUTION_ENVIRONMENT_GEN2")
    );
}

// ===========================================================================
// parse_env
// ===========================================================================

#[test]
fn parse_env_phan_biet_dung_hai_dang() {
    let svc = fixture();
    let env = parse_env(&svc["template"]["containers"][0]);

    assert_eq!(env.len(), 5);

    let log = &env[0];
    assert_eq!(log.name, "LOG_LEVEL");
    assert_eq!(log.kind, EnvKind::Plain);
    assert_eq!(log.value.as_deref(), Some("info"));

    // Env không có `value` là plain rỗng, KHÔNG phải secret.
    let empty = &env[1];
    assert_eq!(empty.kind, EnvKind::Plain);
    assert_eq!(empty.value.as_deref(), Some(""));

    let db = &env[2];
    assert_eq!(db.name, "DB_PASSWORD");
    assert_eq!(db.kind, EnvKind::SecretRef);
    assert_eq!(db.secret.as_deref(), Some("gateway-db-password"));
    assert_eq!(db.version.as_deref(), Some("latest"));
    assert!(db.value.is_none(), "secret-ref không được mang value");

    // Tên secret dạng đường dẫn đầy đủ phải được rút ngắn để hiển thị.
    let jwt = &env[3];
    assert_eq!(jwt.secret.as_deref(), Some("jwt-signing-key"));
    assert_eq!(jwt.version.as_deref(), Some("3"));
}

// ===========================================================================
// apply_env — nhóm test then chốt
// ===========================================================================

#[test]
fn apply_env_giu_nguyen_secret_ref_khi_chi_sua_bien_thuong() {
    // Kịch bản thường gặp nhất: đổi LOG_LEVEL từ info sang debug.
    // Một editor coi env là Map<String,String> sẽ ghi DB_PASSWORD = "" ở đây
    // và làm service không kết nối được database.
    let svc = fixture();
    let mut desired = parse_env(&svc["template"]["containers"][0]);
    desired[0].value = Some("debug".into());

    let out = apply_env(&svc, 0, &desired).expect("apply_env phải thành công");

    let db = find_env(&out, "DB_PASSWORD").expect("DB_PASSWORD phải còn trong payload");
    assert_eq!(
        db,
        &json!({
            "name": "DB_PASSWORD",
            "valueSource": { "secretKeyRef": { "secret": "gateway-db-password", "version": "latest" } }
        }),
        "secret-ref phải được giữ y nguyên byte-for-byte"
    );
    assert!(
        db.get("value").is_none(),
        "secret-ref tuyệt đối không được biến thành field `value`"
    );

    assert_eq!(find_env(&out, "LOG_LEVEL").unwrap()["value"], json!("debug"));
}

#[test]
fn apply_env_giu_field_la_ben_trong_secretkeyref() {
    // Google có thể thêm field mới vào secretKeyRef. Dựng lại object từ đầu sẽ mất nó;
    // clone object gốc thì không.
    let svc = fixture();
    let desired = parse_env(&svc["template"]["containers"][0]);

    let out = apply_env(&svc, 0, &desired).unwrap();
    let jwt = find_env(&out, "JWT_SIGNING_KEY").unwrap();

    assert_eq!(
        jwt["valueSource"]["secretKeyRef"]["someFutureFieldGoogleAdded"],
        json!("keep-me"),
        "field lạ trong secretKeyRef bị mất — nghĩa là code đang dựng lại object thay vì clone"
    );
    // Tên secret dạng đường dẫn đầy đủ cũng phải giữ nguyên, không bị rút ngắn khi ghi.
    assert_eq!(
        jwt["valueSource"]["secretKeyRef"]["secret"],
        json!("projects/example-project/secrets/jwt-signing-key"),
        "tên secret dạng full path bị viết ngắn lại khi PATCH sẽ đổi ý nghĩa cấu hình"
    );
}

#[test]
fn apply_env_khong_lam_hong_phan_con_lai_cua_service() {
    let svc = fixture();
    let mut desired = parse_env(&svc["template"]["containers"][0]);
    desired.push(EnvEntry::plain("NEW_VAR", "hello"));

    let out = apply_env(&svc, 0, &desired).unwrap();

    assert_eq!(
        out["template"]["containers"][0]["image"],
        json!("asia-northeast1-docker.pkg.dev/example-project/svc/gateway:v1.8.2"),
        "image bị mất thì revision mới sẽ không chạy được"
    );
    assert_eq!(
        out["template"]["containers"][0]["resources"]["limits"]["memory"],
        json!("512Mi")
    );
    assert_eq!(out["template"]["scaling"]["minInstanceCount"], json!(1));
    assert_eq!(out["template"]["vpcAccess"]["egress"], json!("PRIVATE_RANGES_ONLY"));
    assert_eq!(
        out["template"]["containers"][0]["volumeMounts"][0]["mountPath"],
        json!("/etc/certs")
    );
    assert_eq!(find_env(&out, "NEW_VAR").unwrap()["value"], json!("hello"));
}

#[test]
fn apply_env_doi_duoc_version_cua_secret() {
    let svc = fixture();
    let mut desired = parse_env(&svc["template"]["containers"][0]);
    // JWT_SIGNING_KEY: 3 -> latest
    let jwt = desired
        .iter_mut()
        .find(|e| e.name == "JWT_SIGNING_KEY")
        .unwrap();
    jwt.version = Some("latest".into());

    let out = apply_env(&svc, 0, &desired).unwrap();
    let got = find_env(&out, "JWT_SIGNING_KEY").unwrap();

    assert_eq!(got["valueSource"]["secretKeyRef"]["version"], json!("latest"));
    // Phần còn lại của object vẫn nguyên.
    assert_eq!(
        got["valueSource"]["secretKeyRef"]["someFutureFieldGoogleAdded"],
        json!("keep-me")
    );
}

#[test]
fn apply_env_them_duoc_secret_ref_moi() {
    let svc = fixture();
    let mut desired = parse_env(&svc["template"]["containers"][0]);
    desired.push(EnvEntry::secret_ref("API_KEY", "third-party-api-key", "latest"));

    let out = apply_env(&svc, 0, &desired).unwrap();
    assert_eq!(
        find_env(&out, "API_KEY").unwrap(),
        &json!({
            "name": "API_KEY",
            "valueSource": { "secretKeyRef": { "secret": "third-party-api-key", "version": "latest" } }
        })
    );
}

#[test]
fn apply_env_xoa_duoc_bien() {
    let svc = fixture();
    let desired: Vec<EnvEntry> = parse_env(&svc["template"]["containers"][0])
        .into_iter()
        .filter(|e| e.name != "FEATURE_FLAGS")
        .collect();

    let out = apply_env(&svc, 0, &desired).unwrap();
    assert!(find_env(&out, "FEATURE_FLAGS").is_none());
    assert_eq!(env_of(&out).len(), 4);
}

#[test]
fn apply_env_chan_bien_trung_ten() {
    let svc = fixture();
    let desired = vec![
        EnvEntry::plain("SAME", "1"),
        EnvEntry::plain("SAME", "2"),
    ];
    let err = apply_env(&svc, 0, &desired).unwrap_err().to_string();
    assert!(err.contains("hai lần"), "message chưa nói rõ vấn đề: {err}");
}

#[test]
fn apply_env_chan_bien_dat_truoc_cua_cloud_run() {
    let svc = fixture();
    for name in ["PORT", "K_SERVICE", "K_REVISION", "K_CONFIGURATION"] {
        let desired = vec![EnvEntry::plain(name, "x")];
        let err = apply_env(&svc, 0, &desired).unwrap_err().to_string();
        assert!(
            err.contains(name),
            "phải chặn biến dành riêng `{name}`, nhận được: {err}"
        );
    }
}

#[test]
fn apply_env_chan_ten_bien_khong_hop_le() {
    let svc = fixture();
    for bad in ["1START", "has-dash", "has space", "có-dấu", ""] {
        let desired = vec![EnvEntry::plain(bad, "x")];
        assert!(
            apply_env(&svc, 0, &desired).is_err(),
            "tên `{bad}` phải bị từ chối"
        );
    }
    // Hợp lệ:
    for good in ["_UNDERSCORE_START", "A1", "LOG_LEVEL_2"] {
        let desired = vec![EnvEntry::plain(good, "x")];
        assert!(
            apply_env(&svc, 0, &desired).is_ok(),
            "tên `{good}` phải được chấp nhận"
        );
    }
}

#[test]
fn apply_env_chan_ten_co_khoang_trang_dau_cuoi() {
    // Lỗi copy-paste rất hay gặp và cực khó tìm: Cloud Run nhận cả khoảng trắng,
    // app đọc `os.Getenv("LOG_LEVEL")` sẽ không thấy gì.
    let svc = fixture();
    let desired = vec![EnvEntry::plain(" LOG_LEVEL", "info")];
    let err = apply_env(&svc, 0, &desired).unwrap_err().to_string();
    assert!(err.contains("khoảng trắng"), "{err}");
}

#[test]
fn apply_env_chan_bien_secret_bi_doi_thanh_plain() {
    let svc = fixture();
    let mut desired = parse_env(&svc["template"]["containers"][0]);
    let db = desired.iter_mut().find(|e| e.name == "DB_PASSWORD").unwrap();
    db.kind = EnvKind::Plain;
    db.value = Some("plaintext-password".into());
    db.secret = None;

    let err = apply_env(&svc, 0, &desired).unwrap_err().to_string();
    assert!(
        err.contains("Secret Manager"),
        "phải giải thích rủi ro ghi giá trị nhạy cảm vào cấu hình: {err}"
    );
}

#[test]
fn apply_env_chan_version_secret_khong_hop_le() {
    let svc = fixture();
    let mut desired = parse_env(&svc["template"]["containers"][0]);
    let db = desired.iter_mut().find(|e| e.name == "DB_PASSWORD").unwrap();
    db.version = Some("newest".into());

    let err = apply_env(&svc, 0, &desired).unwrap_err().to_string();
    assert!(err.contains("latest"), "{err}");
}

#[test]
fn apply_env_bao_loi_ro_khi_container_index_sai() {
    let svc = fixture();
    let desired = vec![EnvEntry::plain("A", "1")];
    let err = apply_env(&svc, 5, &desired).unwrap_err().to_string();
    assert!(err.contains("container"), "{err}");
}

#[test]
fn apply_env_van_xoa_template_revision() {
    // apply_env đi qua sanitize_for_patch, nên bẫy revision cũng phải được xử lý.
    let svc = fixture();
    let desired = parse_env(&svc["template"]["containers"][0]);
    let out = apply_env(&svc, 0, &desired).unwrap();
    assert!(out["template"].get("revision").is_none());
    assert!(out.get("uri").is_none());
}

// ===========================================================================
// apply_scaling
// ===========================================================================

#[test]
fn apply_scaling_ghi_dung_vi_tri_trong_json() {
    let svc = fixture();
    let upd = ScalingUpdate {
        min_instances: Some(2),
        max_instances: Some(20),
        cpu: Some("2".into()),
        memory: Some("1Gi".into()),
        concurrency: Some(100),
        timeout: Some("5m".into()),
        cpu_idle: Some(false),
        startup_cpu_boost: Some(true),
    };

    let out = apply_scaling(&svc, 0, &upd).unwrap();

    assert_eq!(out["template"]["scaling"]["minInstanceCount"], json!(2));
    assert_eq!(out["template"]["scaling"]["maxInstanceCount"], json!(20));
    assert_eq!(out["template"]["maxInstanceRequestConcurrency"], json!(100));
    assert_eq!(out["template"]["timeout"], json!("300s"), "5m phải được chuẩn hoá thành 300s");

    let res = &out["template"]["containers"][0]["resources"];
    assert_eq!(res["limits"]["cpu"], json!("2"));
    assert_eq!(res["limits"]["memory"], json!("1Gi"));
    assert_eq!(res["cpuIdle"], json!(false));
    assert_eq!(res["startupCpuBoost"], json!(true));
}

#[test]
fn apply_scaling_khong_lam_mat_env_va_secret() {
    let svc = fixture();
    let upd = ScalingUpdate {
        min_instances: Some(3),
        max_instances: None,
        cpu: None,
        memory: None,
        concurrency: None,
        timeout: None,
        cpu_idle: None,
        startup_cpu_boost: None,
    };
    let out = apply_scaling(&svc, 0, &upd).unwrap();

    assert_eq!(env_of(&out).len(), 5, "sửa scaling không được ảnh hưởng tới env");
    assert_eq!(
        find_env(&out, "DB_PASSWORD").unwrap()["valueSource"]["secretKeyRef"]["secret"],
        json!("gateway-db-password")
    );
    // max không truyền thì phải giữ giá trị cũ, không được reset về mặc định.
    assert_eq!(out["template"]["scaling"]["maxInstanceCount"], json!(10));
}

#[test]
fn apply_scaling_tao_scaling_khi_service_chua_co() {
    let mut svc = fixture();
    svc["template"].as_object_mut().unwrap().remove("scaling");

    let upd = ScalingUpdate {
        min_instances: Some(0),
        max_instances: Some(5),
        cpu: None,
        memory: None,
        concurrency: None,
        timeout: None,
        cpu_idle: None,
        startup_cpu_boost: None,
    };
    let out = apply_scaling(&svc, 0, &upd).unwrap();
    assert_eq!(out["template"]["scaling"]["minInstanceCount"], json!(0));
    assert_eq!(out["template"]["scaling"]["maxInstanceCount"], json!(5));
}

#[test]
fn validate_scaling_chan_min_lon_hon_max() {
    let upd = ScalingUpdate {
        min_instances: Some(10),
        max_instances: Some(3),
        cpu: None,
        memory: None,
        concurrency: None,
        timeout: None,
        cpu_idle: None,
        startup_cpu_boost: None,
    };
    let err = validate_scaling(&upd).unwrap_err().to_string();
    assert!(err.contains("lớn hơn"), "{err}");
}

#[test]
fn validate_cpu_memory_timeout() {
    assert!(validate_cpu("1").is_ok());
    assert!(validate_cpu("0.5").is_ok());
    assert!(validate_cpu("500m").is_ok());
    assert!(validate_cpu("80m").is_ok());
    assert!(validate_cpu("0.01").is_err());
    assert!(validate_cpu("nhiều").is_err());

    assert!(validate_memory("512Mi").is_ok());
    assert!(validate_memory("1Gi").is_ok());
    assert!(validate_memory("2G").is_ok());
    assert!(validate_memory("64Mi").is_err(), "dưới 128Mi phải bị chặn");
    assert!(validate_memory("512").is_err(), "thiếu đơn vị phải bị chặn");
    assert!(validate_memory("nhiều").is_err());

    assert_eq!(normalize_timeout("300").unwrap(), "300s");
    assert_eq!(normalize_timeout("300s").unwrap(), "300s");
    assert_eq!(normalize_timeout("5m").unwrap(), "300s");
    assert_eq!(
        normalize_timeout(" 60 s ").unwrap(),
        "60s",
        "khoảng trắng do copy-paste phải được bỏ qua thay vì báo lỗi"
    );
    assert!(normalize_timeout("0").is_err());
    assert!(normalize_timeout("2h").is_err());
    assert!(normalize_timeout("4000s").is_err(), "quá 3600s phải bị chặn");
}

// ===========================================================================
// traffic pinning
// ===========================================================================

#[test]
fn traffic_latest_100_thi_khong_ghim() {
    assert!(!is_traffic_pinned(&fixture()));
}

#[test]
fn traffic_khong_khai_bao_thi_khong_ghim() {
    let mut svc = fixture();
    svc.as_object_mut().unwrap().remove("traffic");
    assert!(!is_traffic_pinned(&svc));

    svc["traffic"] = json!([]);
    assert!(!is_traffic_pinned(&svc));
}

#[test]
fn traffic_ghim_vao_revision_thi_bi_phat_hien() {
    let mut svc = fixture();
    svc["traffic"] = json!([
        { "type": "TRAFFIC_TARGET_ALLOCATION_TYPE_REVISION", "revision": "gateway-00040-zzz", "percent": 100 }
    ]);
    assert!(
        is_traffic_pinned(&svc),
        "ghim 100% vào revision cũ mà không cảnh báo thì người dùng sẽ tưởng sửa env đã có tác dụng"
    );
}

#[test]
fn traffic_chia_doi_cung_tinh_la_ghim() {
    let mut svc = fixture();
    svc["traffic"] = json!([
        { "type": "TRAFFIC_TARGET_ALLOCATION_TYPE_LATEST", "percent": 50 },
        { "type": "TRAFFIC_TARGET_ALLOCATION_TYPE_REVISION", "revision": "gateway-00040-zzz", "percent": 50 }
    ]);
    assert!(is_traffic_pinned(&svc));
}

#[test]
fn traffic_entry_chi_co_tag_khong_tinh_la_ghim() {
    // Tag preview (percent = 0) là cấu hình bình thường, không phải ghim traffic.
    let mut svc = fixture();
    svc["traffic"] = json!([
        { "type": "TRAFFIC_TARGET_ALLOCATION_TYPE_LATEST", "percent": 100 },
        { "type": "TRAFFIC_TARGET_ALLOCATION_TYPE_REVISION", "revision": "gateway-00039-aaa", "percent": 0, "tag": "preview" }
    ]);
    assert!(!is_traffic_pinned(&svc), "tag-only không được coi là ghim traffic");
}

// ===========================================================================
// predict_next_revision
// ===========================================================================

#[test]
fn du_doan_ten_revision_ke_tiep() {
    assert_eq!(
        predict_next_revision(Some(
            "projects/example-project/locations/asia-northeast1/revisions/gateway-00041-abc"
        )),
        Some("gateway-00042-xxx".to_string())
    );
    assert_eq!(
        predict_next_revision(Some("attendance-sync-00099-w9k")),
        Some("attendance-sync-00100-xxx".to_string()),
        "tên service có dấu gạch phải xử lý đúng"
    );
    assert_eq!(predict_next_revision(None), None);
    assert_eq!(predict_next_revision(Some("khong-theo-dinh-dang")), None);
    assert_eq!(predict_next_revision(Some("only-two")), None);
}

// ===========================================================================
// diff & preview
// ===========================================================================

#[test]
fn diff_khong_bao_gio_in_gia_tri_secret() {
    let before = vec![
        EnvEntry::plain("LOG_LEVEL", "info"),
        EnvEntry::secret_ref("DB_PASSWORD", "gateway-db-password", "latest"),
    ];
    let after = vec![EnvEntry::plain("LOG_LEVEL", "debug")];

    let changes = diff_env(&before, &after);
    let removed = changes
        .iter()
        .find(|c| matches!(c, EnvChange::Removed { name, .. } if name == "DB_PASSWORD"))
        .expect("phải báo là đã xoá DB_PASSWORD");

    match removed {
        EnvChange::Removed { value, .. } => assert!(
            value.is_none(),
            "diff không được mang theo giá trị của biến secret"
        ),
        _ => unreachable!(),
    }
}

#[test]
fn diff_nhan_ra_doi_version_secret() {
    let before = vec![EnvEntry::secret_ref("JWT", "jwt-key", "3")];
    let after = vec![EnvEntry::secret_ref("JWT", "jwt-key", "latest")];

    let changes = diff_env(&before, &after);
    assert_eq!(changes.len(), 1);
    assert!(matches!(
        &changes[0],
        EnvChange::SecretVersionChanged { before, after, .. } if before == "3" && after == "latest"
    ));
}

#[test]
fn diff_rong_khi_khong_co_gi_thay_doi() {
    let env = parse_env(&fixture()["template"]["containers"][0]);
    assert!(diff_env(&env, &env).is_empty());
}

#[test]
fn preview_canh_bao_khi_traffic_bi_ghim() {
    let mut svc = fixture();
    svc["traffic"] = json!([
        { "type": "TRAFFIC_TARGET_ALLOCATION_TYPE_REVISION", "revision": "gateway-00040-zzz", "percent": 100 }
    ]);
    svc["trafficStatuses"] = svc["traffic"].clone();

    let before = parse_env(&svc["template"]["containers"][0]);
    let mut after = before.clone();
    after[0].value = Some("debug".into());

    let p = build_preview(&svc, &before, &after, vec![]);
    assert!(p.traffic_pinned);
    let joined = p.warnings.join(" | ");
    assert!(joined.contains("KHÔNG nhận traffic"), "{joined}");
    assert!(joined.contains("gateway-00040-zzz"), "phải nêu tên revision đang giữ traffic: {joined}");
}

#[test]
fn preview_canh_bao_khi_them_secret_moi() {
    let svc = fixture();
    let before = parse_env(&svc["template"]["containers"][0]);
    let mut after = before.clone();
    after.push(EnvEntry::secret_ref("NEW_SECRET", "some-secret", "latest"));

    let p = build_preview(&svc, &before, &after, vec![]);
    let joined = p.warnings.join(" | ");
    assert!(
        joined.contains("secretAccessor"),
        "phải cảnh báo runtime SA cần quyền đọc secret: {joined}"
    );
    assert!(
        joined.contains("gateway-runtime@example-project.iam.gserviceaccount.com"),
        "phải nêu đúng SA cần cấp quyền: {joined}"
    );
}

#[test]
fn preview_noi_ro_khi_khong_co_thay_doi() {
    let svc = fixture();
    let env = parse_env(&svc["template"]["containers"][0]);
    let p = build_preview(&svc, &env, &env, vec![]);
    assert!(p.env_changes.is_empty());
    assert!(p.warnings.iter().any(|w| w.contains("Không có thay đổi")));
}

#[test]
fn preview_co_goi_y_ten_revision_moi() {
    let svc = fixture();
    let before = parse_env(&svc["template"]["containers"][0]);
    let mut after = before.clone();
    after[0].value = Some("debug".into());
    let p = build_preview(&svc, &before, &after, vec![]);
    assert_eq!(p.next_revision_hint.as_deref(), Some("gateway-00042-xxx"));
}

#[test]
fn describe_scaling_chi_liet_ke_thay_doi_that() {
    let svc = fixture();
    // Truyền đúng giá trị đang có -> không có thay đổi nào để mô tả.
    let same = ScalingUpdate {
        min_instances: Some(1),
        max_instances: Some(10),
        cpu: Some("1".into()),
        memory: Some("512Mi".into()),
        concurrency: Some(80),
        timeout: Some("300s".into()),
        cpu_idle: Some(true),
        startup_cpu_boost: Some(false),
    };
    assert!(
        describe_scaling_changes(&svc, 0, &same).is_empty(),
        "gửi lại giá trị y như cũ không được tính là thay đổi"
    );

    let changed = ScalingUpdate {
        min_instances: Some(3),
        max_instances: Some(10),
        cpu: None,
        memory: Some("1Gi".into()),
        concurrency: None,
        timeout: None,
        cpu_idle: None,
        startup_cpu_boost: None,
    };
    let desc = describe_scaling_changes(&svc, 0, &changed);
    assert_eq!(desc.len(), 2, "{desc:?}");
    assert!(desc.iter().any(|d| d.contains("Min instances: 1 → 3")), "{desc:?}");
    assert!(desc.iter().any(|d| d.contains("Memory: 512Mi → 1Gi")), "{desc:?}");
}

// ===========================================================================
// secret & volume
// ===========================================================================

#[test]
fn liet_ke_du_secret_dung_qua_env_va_volume() {
    let secrets = referenced_secrets(&fixture());
    assert_eq!(
        secrets,
        vec![
            "gateway-db-password".to_string(),
            "gateway-tls".to_string(),
            "jwt-signing-key".to_string(),
        ],
        "phải gom cả secret qua env lẫn secret mount dạng volume"
    );
}

#[test]
fn parse_secret_volume_lay_dung_mount_path() {
    let vols = parse_secret_volumes(&fixture());
    assert_eq!(vols.len(), 1);
    assert_eq!(vols[0].secret, "gateway-tls");
    assert_eq!(vols[0].volume_name, "tls-certs");
    assert_eq!(
        vols[0].mount_path.as_deref(),
        Some("/etc/certs"),
        "mountPath nằm ở container.volumeMounts, phải tra chéo theo tên volume"
    );
    assert_eq!(vols[0].items, vec!["tls.crt → vlatest".to_string()]);
}

#[test]
fn rut_ngan_ten_secret() {
    assert_eq!(short_secret_name("projects/p/secrets/abc"), "abc");
    assert_eq!(short_secret_name("abc"), "abc");
}

#[test]
fn parse_containers_doc_dung_thong_tin() {
    let cs = parse_containers(&fixture()).unwrap();
    assert_eq!(cs.len(), 1);
    assert_eq!(cs[0].cpu.as_deref(), Some("1"));
    assert_eq!(cs[0].memory.as_deref(), Some("512Mi"));
    assert_eq!(cs[0].port, Some(8080));
    assert_eq!(cs[0].cpu_idle, Some(true));
    assert_eq!(cs[0].env.len(), 5);
}

#[test]
fn parse_traffic_uu_tien_traffic_statuses() {
    let mut svc = fixture();
    svc["trafficStatuses"] = json!([
        { "type": "TRAFFIC_TARGET_ALLOCATION_TYPE_LATEST", "percent": 100,
          "uri": "https://gateway-a1b2c3d4e5-an.a.run.app" }
    ]);
    let t = parse_traffic(&svc);
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].kind, "LATEST");
    assert_eq!(t[0].percent, 100);
    assert!(t[0].uri.is_some(), "uri chỉ có ở trafficStatuses, phải lấy được");
}

#[test]
fn service_thieu_containers_bao_loi_de_hieu() {
    let mut svc = fixture();
    svc["template"].as_object_mut().unwrap().remove("containers");
    let err = parse_containers(&svc).unwrap_err().to_string();
    assert!(err.contains("template.containers"), "{err}");
    assert!(err.contains("Reload"), "message nên nói người dùng làm gì tiếp: {err}");
}

#[test]
fn require_etag_bao_loi_khi_thieu() {
    let mut svc = fixture();
    svc.as_object_mut().unwrap().remove("etag");
    let out = sanitize_for_patch(&svc);
    assert!(require_etag(&out).is_err());
}

#[test]
fn string_map_doc_labels() {
    let svc = fixture();
    let labels = string_map(svc.get("labels"));
    assert_eq!(labels.get("team").map(String::as_str), Some("platform"));
    assert_eq!(labels.len(), 2);
}
