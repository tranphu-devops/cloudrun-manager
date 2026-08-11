//! Read-modify-write cho Cloud Run Service. Đây là module rủi ro nhất của cả app.
//!
//! # Vì sao không dùng struct Rust chặt cho đường ghi
//!
//! Cloud Run v2 là API **declarative**: `PATCH` nghĩa là "đây là trạng thái tôi muốn",
//! không phải "sửa riêng field này". Nếu deserialize Service vào một struct Rust rồi
//! serialize lại, mọi field mình chưa khai báo (`vpcAccess`, `binaryAuthorization`,
//! `volumes`, `livenessProbe`, field Google mới thêm tuần trước…) sẽ **biến mất khỏi
//! payload và bị xoá khỏi service thật**.
//!
//! Nên toàn bộ đường ghi làm việc trên `serde_json::Value`: clone nguyên JSON đã GET,
//! chạm đúng path cần sửa, giữ nguyên phần còn lại.
//!
//! # Ba cái bẫy đã xử lý ở đây
//!
//! 1. **Env secret-ref**: `template.containers[].env[]` trộn hai dạng — `{name, value}`
//!    và `{name, valueSource.secretKeyRef}`. Một editor coi env là `Map<String,String>`
//!    sẽ biến `DB_PASSWORD` thành chuỗi rỗng và làm sập service. Xem `apply_env`.
//! 2. **`template.revision`**: nếu service từng deploy với revision name chỉ định sẵn,
//!    giữ lại field đó khi PATCH sẽ bị từ chối vì "revision đã tồn tại". Phải xoá để
//!    Cloud Run tự sinh tên kế tiếp. Xem `sanitize_for_patch`.
//! 3. **Field output-only**: `conditions`, `latestReadyRevision`, `uri`… gửi lên có thể
//!    bị API từ chối. Lọc bỏ trước khi PATCH.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use crate::error::{GcpError, Result};
use crate::types::{
    ApplyPreview, ContainerView, EnvChange, EnvEntry, EnvKind, ScalingUpdate, SecretVolumeMount,
    TrafficEntry,
};

/// Field do server sinh ra, không được gửi lại khi PATCH.
const OUTPUT_ONLY: &[&str] = &[
    "uid",
    "generation",
    "createTime",
    "updateTime",
    "deleteTime",
    "expireTime",
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
    "satisfiesPzs",
    "reconciling",
];

/// Env var Cloud Run tự quản; set tay sẽ bị API từ chối.
const RESERVED_ENV: &[&str] = &["PORT", "K_SERVICE", "K_REVISION", "K_CONFIGURATION"];

// ---------------------------------------------------------------------------
// Chuẩn bị payload PATCH
// ---------------------------------------------------------------------------

/// Dọn JSON Service để dùng làm body của PATCH.
///
/// Giữ `etag` (cần cho optimistic concurrency) và mọi field cấu hình khác kể cả
/// field module này không hiểu.
pub fn sanitize_for_patch(svc: &Value) -> Value {
    let mut out = svc.clone();

    if let Some(obj) = out.as_object_mut() {
        for k in OUTPUT_ONLY {
            obj.remove(*k);
        }
    }

    // `template.revision` là tên revision do người deploy chỉ định. Giữ nguyên khi
    // PATCH thì Cloud Run báo "Revision <tên> already exists" và thao tác thất bại.
    // Xoá đi để Cloud Run tự đánh số tiếp (service-00042-abc).
    if let Some(tpl) = out.get_mut("template").and_then(|t| t.as_object_mut()) {
        tpl.remove("revision");
    }

    out
}

/// Tên revision dự kiến sẽ được tạo, suy ra từ `latestCreatedRevision`.
///
/// Chỉ để hiển thị trước khi apply ("sẽ tạo revision gateway-00042-xxx"), không dùng
/// cho logic nào — hậu tố 4 ký tự do Cloud Run sinh ngẫu nhiên nên không đoán được.
pub fn predict_next_revision(latest_created: Option<&str>) -> Option<String> {
    let full = latest_created?;
    let short = full.rsplit('/').next()?;

    let parts: Vec<&str> = short.split('-').collect();
    if parts.len() < 3 {
        return None;
    }
    // Định dạng: {service}-{00041}-{abc}. Số thứ tự là segment kế cuối.
    let idx = parts.len() - 2;
    let num = parts[idx];
    if num.len() != 5 || !num.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let next: u32 = num.parse::<u32>().ok()? + 1;

    let mut rebuilt = parts.clone();
    let next_str = format!("{next:05}");
    rebuilt[idx] = &next_str;
    let last = rebuilt.len() - 1;
    rebuilt[last] = "xxx";
    Some(rebuilt.join("-"))
}

/// Traffic có bị ghim vào revision cụ thể không.
///
/// Trả `true` khi tổng % chảy vào `LATEST` nhỏ hơn 100 — nghĩa là revision mới tạo ra
/// sẽ không nhận đủ traffic. Entry chỉ có tag (percent = 0) không tính là ghim.
pub fn is_traffic_pinned(svc: &Value) -> bool {
    let Some(traffic) = svc.get("traffic").and_then(|t| t.as_array()) else {
        // Không khai báo traffic = 100% về LATEST (mặc định của Cloud Run).
        return false;
    };
    if traffic.is_empty() {
        return false;
    }

    let latest_percent: i64 = traffic
        .iter()
        .filter(|t| {
            let ty = t.get("type").and_then(|v| v.as_str()).unwrap_or("");
            // Thiếu `type` thì Cloud Run coi như LATEST.
            ty.is_empty() || ty.ends_with("_LATEST")
        })
        .map(|t| t.get("percent").and_then(|v| v.as_i64()).unwrap_or(0))
        .sum();

    latest_percent < 100
}

// ---------------------------------------------------------------------------
// Đọc env
// ---------------------------------------------------------------------------

fn containers(svc: &Value) -> Result<&Vec<Value>> {
    svc.get("template")
        .and_then(|t| t.get("containers"))
        .and_then(|c| c.as_array())
        .ok_or_else(|| {
            GcpError::Invalid(
                "Service không có `template.containers`. Dữ liệu trả về bất thường — thử Reload; \
                 nếu vẫn vậy thì service này có thể đang ở trạng thái lỗi, hãy xem trên GCP Console."
                    .to_string(),
            )
        })
}

/// Tên secret có thể là `projects/p/secrets/name` hoặc chỉ `name`. Rút về `name` để hiển thị.
pub fn short_secret_name(s: &str) -> String {
    s.rsplit('/').next().unwrap_or(s).to_string()
}

/// Đọc danh sách env của một container, phân biệt plain và secret-ref.
pub fn parse_env(container: &Value) -> Vec<EnvEntry> {
    let Some(arr) = container.get("env").and_then(|e| e.as_array()) else {
        return Vec::new();
    };

    arr.iter()
        .filter_map(|e| {
            let name = e.get("name")?.as_str()?.to_string();

            if let Some(kr) = e
                .get("valueSource")
                .and_then(|vs| vs.get("secretKeyRef"))
            {
                let secret = kr
                    .get("secret")
                    .and_then(|s| s.as_str())
                    .map(short_secret_name)
                    .unwrap_or_default();
                let version = kr
                    .get("version")
                    .and_then(|v| v.as_str())
                    .unwrap_or("latest")
                    .to_string();
                return Some(EnvEntry::secret_ref(name, secret, version));
            }

            // Env plain. Thiếu `value` nghĩa là chuỗi rỗng — vẫn là plain, không phải secret.
            let value = e
                .get("value")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            Some(EnvEntry::plain(name, value))
        })
        .collect()
}

pub fn parse_containers(svc: &Value) -> Result<Vec<ContainerView>> {
    let list = containers(svc)?;
    Ok(list
        .iter()
        .enumerate()
        .map(|(index, c)| {
            let limits = c.get("resources").and_then(|r| r.get("limits"));
            ContainerView {
                index,
                name: c.get("name").and_then(|v| v.as_str()).map(String::from),
                image: c.get("image").and_then(|v| v.as_str()).map(String::from),
                cpu: limits
                    .and_then(|l| l.get("cpu"))
                    .and_then(|v| v.as_str())
                    .map(String::from),
                memory: limits
                    .and_then(|l| l.get("memory"))
                    .and_then(|v| v.as_str())
                    .map(String::from),
                cpu_idle: c
                    .get("resources")
                    .and_then(|r| r.get("cpuIdle"))
                    .and_then(|v| v.as_bool()),
                startup_cpu_boost: c
                    .get("resources")
                    .and_then(|r| r.get("startupCpuBoost"))
                    .and_then(|v| v.as_bool()),
                port: c
                    .get("ports")
                    .and_then(|p| p.as_array())
                    .and_then(|a| a.first())
                    .and_then(|p| p.get("containerPort"))
                    .and_then(|v| v.as_i64()),
                env: parse_env(c),
                command: string_list(c.get("command")),
                args: string_list(c.get("args")),
            }
        })
        .collect())
}

pub fn parse_traffic(svc: &Value) -> Vec<TrafficEntry> {
    // Ưu tiên `trafficStatuses` (trạng thái thực tế) vì nó có thêm `uri` của tag,
    // fallback về `traffic` (mong muốn) khi service chưa reconcile xong.
    let src = svc
        .get("trafficStatuses")
        .and_then(|t| t.as_array())
        .filter(|a| !a.is_empty())
        .or_else(|| svc.get("traffic").and_then(|t| t.as_array()));

    let Some(arr) = src else { return Vec::new() };

    arr.iter()
        .map(|t| {
            let ty = t.get("type").and_then(|v| v.as_str()).unwrap_or("");
            TrafficEntry {
                kind: if ty.ends_with("_REVISION") {
                    "REVISION".to_string()
                } else {
                    "LATEST".to_string()
                },
                revision: t
                    .get("revision")
                    .and_then(|v| v.as_str())
                    .map(short_secret_name),
                percent: t.get("percent").and_then(|v| v.as_i64()).unwrap_or(0),
                tag: t.get("tag").and_then(|v| v.as_str()).map(String::from),
                uri: t.get("uri").and_then(|v| v.as_str()).map(String::from),
            }
        })
        .collect()
}

/// Secret được mount dưới dạng volume (khác với env secret-ref).
/// Cần liệt kê để tab Secrets nói đủ "service này dùng những secret nào".
pub fn parse_secret_volumes(svc: &Value) -> Vec<SecretVolumeMount> {
    let Some(volumes) = svc
        .get("template")
        .and_then(|t| t.get("volumes"))
        .and_then(|v| v.as_array())
    else {
        return Vec::new();
    };

    // mountPath nằm ở container.volumeMounts, tra chéo theo tên volume.
    let mut mount_paths: BTreeMap<String, String> = BTreeMap::new();
    if let Ok(cs) = containers(svc) {
        for c in cs {
            if let Some(mounts) = c.get("volumeMounts").and_then(|m| m.as_array()) {
                for m in mounts {
                    if let (Some(n), Some(p)) = (
                        m.get("name").and_then(|v| v.as_str()),
                        m.get("mountPath").and_then(|v| v.as_str()),
                    ) {
                        mount_paths.insert(n.to_string(), p.to_string());
                    }
                }
            }
        }
    }

    volumes
        .iter()
        .filter_map(|v| {
            let sec = v.get("secret")?;
            let volume_name = v.get("name")?.as_str()?.to_string();
            let secret = sec
                .get("secret")
                .and_then(|s| s.as_str())
                .map(short_secret_name)
                .unwrap_or_default();
            let items = sec
                .get("items")
                .and_then(|i| i.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|it| {
                            let path = it.get("path").and_then(|p| p.as_str())?;
                            let ver = it.get("version").and_then(|p| p.as_str()).unwrap_or("latest");
                            Some(format!("{path} → v{ver}"))
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(SecretVolumeMount {
                mount_path: mount_paths.get(&volume_name).cloned(),
                volume_name,
                secret,
                items,
            })
        })
        .collect()
}

/// Mọi tên secret service đang dùng, cả qua env lẫn qua volume.
pub fn referenced_secrets(svc: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();

    if let Ok(cs) = containers(svc) {
        for c in cs {
            for e in parse_env(c) {
                if let Some(s) = e.secret {
                    out.push(s);
                }
            }
        }
    }
    for v in parse_secret_volumes(svc) {
        out.push(v.secret);
    }

    out.sort();
    out.dedup();
    out
}

fn string_list(v: Option<&Value>) -> Vec<String> {
    v.and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Ghi env
// ---------------------------------------------------------------------------

/// Dựng payload PATCH với danh sách env mới cho container `container_idx`.
///
/// Quy tắc an toàn với secret-ref: KHÔNG dựng lại object `valueSource` từ đầu nếu
/// entry đó đã tồn tại — clone nguyên object gốc rồi chỉ chạm vào `version` nếu người
/// dùng đổi version. Nhờ vậy các field bên trong `secretKeyRef` mà module này chưa
/// biết vẫn được giữ nguyên.
pub fn apply_env(svc: &Value, container_idx: usize, desired: &[EnvEntry]) -> Result<Value> {
    validate_env_list(desired)?;

    let originals = original_env_map(svc, container_idx)?;

    let mut new_env: Vec<Value> = Vec::with_capacity(desired.len());
    for e in desired {
        match e.kind {
            EnvKind::Plain => {
                // Nếu trước đó là secret-ref mà giờ thành plain thì đó là hành động
                // xoá binding secret — có chủ đích, nhưng phải là chủ đích rõ ràng.
                // Chặn ở đây vì UI v1 render secret-ref ở dạng khoá, không cho đổi
                // sang plain; nếu vào được nhánh này thì là bug ở tầng trên.
                if let Some(orig) = originals.get(&e.name) {
                    if orig.get("valueSource").is_some() {
                        return Err(GcpError::Invalid(format!(
                            "`{}` đang là biến lấy từ Secret Manager. Đổi nó thành giá trị thường \
                             sẽ ghi giá trị nhạy cảm trực tiếp vào cấu hình service (ai xem được \
                             service là xem được giá trị). Nếu thực sự muốn, hãy xoá biến rồi thêm lại.",
                            e.name
                        )));
                    }
                }
                new_env.push(json!({
                    "name": e.name,
                    "value": e.value.clone().unwrap_or_default(),
                }));
            }

            EnvKind::SecretRef => {
                let secret = e.secret.as_deref().unwrap_or("").trim();
                if secret.is_empty() {
                    return Err(GcpError::Invalid(format!(
                        "Biến `{}` khai là lấy từ Secret nhưng không có tên secret.",
                        e.name
                    )));
                }
                let version = e.version.as_deref().unwrap_or("latest").trim();
                validate_secret_version(&e.name, version)?;

                match originals.get(&e.name) {
                    // Đã tồn tại: giữ nguyên object gốc, chỉ sửa version.
                    Some(orig) if orig.get("valueSource").is_some() => {
                        let mut o = orig.clone();
                        o["valueSource"]["secretKeyRef"]["version"] = json!(version);
                        new_env.push(o);
                    }
                    // Mới thêm: dựng tối thiểu, đúng schema.
                    _ => {
                        new_env.push(json!({
                            "name": e.name,
                            "valueSource": {
                                "secretKeyRef": { "secret": secret, "version": version }
                            }
                        }));
                    }
                }
            }
        }
    }

    let mut out = sanitize_for_patch(svc);
    set_container_field(&mut out, container_idx, "env", Value::Array(new_env))?;
    Ok(out)
}

/// Map name -> object env gốc, để `apply_env` clone lại thay vì dựng mới.
fn original_env_map(svc: &Value, container_idx: usize) -> Result<BTreeMap<String, Value>> {
    let cs = containers(svc)?;
    let c = cs.get(container_idx).ok_or_else(|| {
        GcpError::Invalid(format!(
            "Service chỉ có {} container, không có container thứ {}.",
            cs.len(),
            container_idx + 1
        ))
    })?;

    let mut map = BTreeMap::new();
    if let Some(arr) = c.get("env").and_then(|e| e.as_array()) {
        for e in arr {
            if let Some(n) = e.get("name").and_then(|v| v.as_str()) {
                map.insert(n.to_string(), e.clone());
            }
        }
    }
    Ok(map)
}

fn set_container_field(
    svc: &mut Value,
    container_idx: usize,
    field: &str,
    value: Value,
) -> Result<()> {
    let cs = svc
        .get_mut("template")
        .and_then(|t| t.get_mut("containers"))
        .and_then(|c| c.as_array_mut())
        .ok_or_else(|| GcpError::Invalid("Payload thiếu template.containers.".to_string()))?;

    let c = cs
        .get_mut(container_idx)
        .ok_or_else(|| GcpError::Invalid(format!("Không có container index {container_idx}.")))?;
    let obj = c
        .as_object_mut()
        .ok_or_else(|| GcpError::Invalid("Container không phải object JSON.".to_string()))?;
    obj.insert(field.to_string(), value);
    Ok(())
}

// ---------------------------------------------------------------------------
// Ghi scaling / resource
// ---------------------------------------------------------------------------

pub fn apply_scaling(svc: &Value, container_idx: usize, upd: &ScalingUpdate) -> Result<Value> {
    validate_scaling(upd)?;

    let mut out = sanitize_for_patch(svc);

    // min/max instance nằm ở template.scaling (RevisionScaling).
    if upd.min_instances.is_some() || upd.max_instances.is_some() {
        let tpl = out
            .get_mut("template")
            .and_then(|t| t.as_object_mut())
            .ok_or_else(|| GcpError::Invalid("Payload thiếu template.".to_string()))?;
        let scaling = tpl
            .entry("scaling".to_string())
            .or_insert_with(|| json!({}));
        let sc = scaling
            .as_object_mut()
            .ok_or_else(|| GcpError::Invalid("template.scaling không phải object.".to_string()))?;
        if let Some(v) = upd.min_instances {
            sc.insert("minInstanceCount".into(), json!(v));
        }
        if let Some(v) = upd.max_instances {
            sc.insert("maxInstanceCount".into(), json!(v));
        }
    }

    if let Some(v) = upd.concurrency {
        out["template"]["maxInstanceRequestConcurrency"] = json!(v);
    }
    if let Some(t) = &upd.timeout {
        let t = normalize_timeout(t)?;
        out["template"]["timeout"] = json!(t);
    }

    // cpu/memory nằm trong resources.limits của container.
    if upd.cpu.is_some()
        || upd.memory.is_some()
        || upd.cpu_idle.is_some()
        || upd.startup_cpu_boost.is_some()
    {
        let cs = out
            .get_mut("template")
            .and_then(|t| t.get_mut("containers"))
            .and_then(|c| c.as_array_mut())
            .ok_or_else(|| GcpError::Invalid("Payload thiếu template.containers.".to_string()))?;
        let c = cs.get_mut(container_idx).ok_or_else(|| {
            GcpError::Invalid(format!("Không có container index {container_idx}."))
        })?;
        let cobj = c
            .as_object_mut()
            .ok_or_else(|| GcpError::Invalid("Container không phải object.".to_string()))?;
        let res = cobj
            .entry("resources".to_string())
            .or_insert_with(|| json!({}));
        let robj = res
            .as_object_mut()
            .ok_or_else(|| GcpError::Invalid("resources không phải object.".to_string()))?;

        if upd.cpu.is_some() || upd.memory.is_some() {
            let limits = robj.entry("limits".to_string()).or_insert_with(|| json!({}));
            let lobj = limits
                .as_object_mut()
                .ok_or_else(|| GcpError::Invalid("resources.limits không phải object.".to_string()))?;
            if let Some(cpu) = &upd.cpu {
                lobj.insert("cpu".into(), json!(cpu.trim()));
            }
            if let Some(mem) = &upd.memory {
                lobj.insert("memory".into(), json!(mem.trim()));
            }
        }
        if let Some(b) = upd.cpu_idle {
            robj.insert("cpuIdle".into(), json!(b));
        }
        if let Some(b) = upd.startup_cpu_boost {
            robj.insert("startupCpuBoost".into(), json!(b));
        }
    }

    Ok(out)
}

// ---------------------------------------------------------------------------
// Validate
// ---------------------------------------------------------------------------

pub fn validate_env_list(list: &[EnvEntry]) -> Result<()> {
    let mut seen: BTreeMap<&str, ()> = BTreeMap::new();
    for e in list {
        let name = e.name.trim();
        if name.is_empty() {
            return Err(GcpError::Invalid(
                "Có biến môi trường chưa đặt tên.".to_string(),
            ));
        }
        if name != e.name {
            return Err(GcpError::Invalid(format!(
                "Tên biến `{}` có khoảng trắng ở đầu/cuối. Cloud Run sẽ nhận cả khoảng trắng đó và \
                 app của bạn sẽ không đọc được biến — hãy xoá khoảng trắng.",
                e.name
            )));
        }
        if !is_valid_env_name(name) {
            return Err(GcpError::Invalid(format!(
                "Tên biến `{name}` không hợp lệ. Chỉ dùng chữ cái, số và `_`, và không bắt đầu bằng số."
            )));
        }
        if RESERVED_ENV.contains(&name) {
            return Err(GcpError::Invalid(format!(
                "`{name}` là biến do Cloud Run tự quản, không đặt tay được. \
                 (`PORT` được Cloud Run cấp cho container; `K_SERVICE`/`K_REVISION`/`K_CONFIGURATION` \
                 là metadata runtime.)"
            )));
        }
        if seen.insert(name, ()).is_some() {
            return Err(GcpError::Invalid(format!(
                "Biến `{name}` bị khai hai lần. Cloud Run không báo lỗi trong trường hợp này mà lấy \
                 giá trị cuối — rất dễ gây bug khó tìm, nên app chặn luôn."
            )));
        }
    }
    Ok(())
}

fn is_valid_env_name(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn validate_secret_version(env_name: &str, version: &str) -> Result<()> {
    if version == "latest" {
        return Ok(());
    }
    if !version.is_empty() && version.bytes().all(|b| b.is_ascii_digit()) {
        return Ok(());
    }
    Err(GcpError::Invalid(format!(
        "Version của secret cho biến `{env_name}` phải là `latest` hoặc một số (ví dụ `3`), đang là `{version}`."
    )))
}

pub fn validate_scaling(upd: &ScalingUpdate) -> Result<()> {
    if let Some(min) = upd.min_instances {
        if min < 0 {
            return Err(GcpError::Invalid(
                "Min instances không được âm.".to_string(),
            ));
        }
    }
    if let Some(max) = upd.max_instances {
        if max < 1 {
            return Err(GcpError::Invalid(
                "Max instances phải từ 1 trở lên.".to_string(),
            ));
        }
    }
    if let (Some(min), Some(max)) = (upd.min_instances, upd.max_instances) {
        if min > max {
            return Err(GcpError::Invalid(format!(
                "Min instances ({min}) đang lớn hơn max instances ({max})."
            )));
        }
    }
    if let Some(c) = upd.concurrency {
        if !(1..=1000).contains(&c) {
            return Err(GcpError::Invalid(format!(
                "Concurrency phải trong khoảng 1–1000, đang là {c}."
            )));
        }
    }
    if let Some(cpu) = &upd.cpu {
        validate_cpu(cpu)?;
    }
    if let Some(mem) = &upd.memory {
        validate_memory(mem)?;
    }
    if let Some(t) = &upd.timeout {
        normalize_timeout(t)?;
    }
    Ok(())
}

pub fn validate_cpu(cpu: &str) -> Result<()> {
    let s = cpu.trim();
    let ok = if let Some(m) = s.strip_suffix('m') {
        m.parse::<u32>().map(|v| v >= 80).unwrap_or(false)
    } else {
        s.parse::<f64>().map(|v| v >= 0.08).unwrap_or(false)
    };
    if ok {
        Ok(())
    } else {
        Err(GcpError::Invalid(format!(
            "CPU `{cpu}` không hợp lệ. Dùng dạng số (`1`, `2`, `0.5`) hoặc millicore (`500m`), tối thiểu 0.08 (`80m`)."
        )))
    }
}

pub fn validate_memory(mem: &str) -> Result<()> {
    let s = mem.trim();
    for unit in ["Gi", "Mi", "Ki", "G", "M", "K"] {
        if let Some(num) = s.strip_suffix(unit) {
            if let Ok(v) = num.parse::<f64>() {
                if v <= 0.0 {
                    break;
                }
                let mib = match unit {
                    "Gi" | "G" => v * 1024.0,
                    "Mi" | "M" => v,
                    "Ki" | "K" => v / 1024.0,
                    _ => v,
                };
                if mib < 128.0 {
                    return Err(GcpError::Invalid(format!(
                        "Memory `{mem}` nhỏ hơn mức tối thiểu 128Mi của Cloud Run."
                    )));
                }
                return Ok(());
            }
            break;
        }
    }
    Err(GcpError::Invalid(format!(
        "Memory `{mem}` không hợp lệ. Dùng dạng `512Mi`, `1Gi`, `2Gi`."
    )))
}

/// Cloud Run nhận timeout dạng Duration protobuf (`300s`). Người dùng hay gõ `300`
/// hoặc `5m` nên chuẩn hoá lại thay vì bắt họ nhớ định dạng.
pub fn normalize_timeout(t: &str) -> Result<String> {
    let s = t.trim();
    let secs: i64 = if let Some(n) = s.strip_suffix('s') {
        n.trim().parse().map_err(|_| bad_timeout(t))?
    } else if let Some(n) = s.strip_suffix('m') {
        n.trim()
            .parse::<i64>()
            .map_err(|_| bad_timeout(t))?
            .checked_mul(60)
            .ok_or_else(|| bad_timeout(t))?
    } else {
        s.parse().map_err(|_| bad_timeout(t))?
    };

    if !(1..=3600).contains(&secs) {
        return Err(GcpError::Invalid(format!(
            "Timeout phải trong khoảng 1s–3600s (60 phút), đang là {secs}s."
        )));
    }
    Ok(format!("{secs}s"))
}

fn bad_timeout(t: &str) -> GcpError {
    GcpError::Invalid(format!(
        "Timeout `{t}` không hợp lệ. Dùng `300s`, `5m`, hoặc chỉ số giây `300`."
    ))
}

// ---------------------------------------------------------------------------
// Diff & preview
// ---------------------------------------------------------------------------

/// So sánh env trước/sau. Không bao giờ in giá trị của secret-ref.
pub fn diff_env(before: &[EnvEntry], after: &[EnvEntry]) -> Vec<EnvChange> {
    let idx_before: BTreeMap<&str, &EnvEntry> =
        before.iter().map(|e| (e.name.as_str(), e)).collect();
    let idx_after: BTreeMap<&str, &EnvEntry> = after.iter().map(|e| (e.name.as_str(), e)).collect();

    let mut out = Vec::new();

    // Giữ thứ tự theo `after` để diff đọc giống thứ tự trên UI, rồi mới tới phần bị xoá.
    for a in after {
        match idx_before.get(a.name.as_str()) {
            None => out.push(EnvChange::Added {
                name: a.name.clone(),
                value: match a.kind {
                    EnvKind::Plain => a.value.clone().unwrap_or_default(),
                    EnvKind::SecretRef => format!(
                        "🔑 {}:{}",
                        a.secret.clone().unwrap_or_default(),
                        a.version.clone().unwrap_or_else(|| "latest".into())
                    ),
                },
            }),
            Some(b) => match (b.kind, a.kind) {
                (EnvKind::Plain, EnvKind::Plain) => {
                    let bv = b.value.clone().unwrap_or_default();
                    let av = a.value.clone().unwrap_or_default();
                    if bv != av {
                        out.push(EnvChange::Changed {
                            name: a.name.clone(),
                            before: bv,
                            after: av,
                        });
                    }
                }
                (EnvKind::SecretRef, EnvKind::SecretRef) => {
                    let bv = b.version.clone().unwrap_or_else(|| "latest".into());
                    let av = a.version.clone().unwrap_or_else(|| "latest".into());
                    if bv != av {
                        out.push(EnvChange::SecretVersionChanged {
                            name: a.name.clone(),
                            secret: a.secret.clone().unwrap_or_default(),
                            before: bv,
                            after: av,
                        });
                    }
                }
                // Đổi kiểu: coi như xoá + thêm để diff nói rõ chuyện gì xảy ra.
                (EnvKind::SecretRef, EnvKind::Plain) => {
                    out.push(EnvChange::Removed {
                        name: a.name.clone(),
                        value: None,
                    });
                    out.push(EnvChange::Added {
                        name: a.name.clone(),
                        value: a.value.clone().unwrap_or_default(),
                    });
                }
                (EnvKind::Plain, EnvKind::SecretRef) => {
                    out.push(EnvChange::Removed {
                        name: a.name.clone(),
                        value: b.value.clone(),
                    });
                    out.push(EnvChange::Added {
                        name: a.name.clone(),
                        value: format!(
                            "🔑 {}:{}",
                            a.secret.clone().unwrap_or_default(),
                            a.version.clone().unwrap_or_else(|| "latest".into())
                        ),
                    });
                }
            },
        }
    }

    for b in before {
        if !idx_after.contains_key(b.name.as_str()) {
            out.push(EnvChange::Removed {
                name: b.name.clone(),
                // Không in giá trị secret ra diff.
                value: match b.kind {
                    EnvKind::Plain => b.value.clone(),
                    EnvKind::SecretRef => None,
                },
            });
        }
    }

    out
}

/// Dựng preview để hiện trước khi apply: diff + cảnh báo.
pub fn build_preview(
    svc: &Value,
    before_env: &[EnvEntry],
    after_env: &[EnvEntry],
    scaling_changes: Vec<String>,
) -> ApplyPreview {
    let env_changes = diff_env(before_env, after_env);
    let traffic_pinned = is_traffic_pinned(svc);

    let mut warnings = Vec::new();

    if traffic_pinned {
        let pinned: Vec<String> = parse_traffic(svc)
            .into_iter()
            .filter(|t| t.kind == "REVISION" && t.percent > 0)
            .map(|t| {
                format!(
                    "{} ({}%)",
                    t.revision.unwrap_or_else(|| "?".into()),
                    t.percent
                )
            })
            .collect();
        warnings.push(format!(
            "Traffic của service đang được ghim vào revision cụ thể: {}. \
             Revision mới tạo ra sẽ KHÔNG nhận traffic, nên thay đổi này sẽ không có tác dụng \
             cho tới khi bạn chuyển traffic sang revision mới (làm trên GCP Console ở v1).",
            pinned.join(", ")
        ));
    }

    // Thêm secret env mới: revision sẽ fail khởi động nếu runtime SA chưa có quyền đọc secret.
    let new_secrets: Vec<&EnvChange> = env_changes
        .iter()
        .filter(|c| matches!(c, EnvChange::Added { value, .. } if value.starts_with("🔑 ")))
        .collect();
    if !new_secrets.is_empty() {
        let sa = svc
            .get("template")
            .and_then(|t| t.get("serviceAccount"))
            .and_then(|v| v.as_str())
            .unwrap_or("runtime service account của service");
        warnings.push(format!(
            "Bạn đang thêm biến lấy từ Secret Manager. Revision mới sẽ không khởi động được nếu \
             `{sa}` chưa có `roles/secretmanager.secretAccessor` trên secret đó."
        ));
    }

    if env_changes.is_empty() && scaling_changes.is_empty() {
        warnings.push(
            "Không có thay đổi nào so với cấu hình hiện tại — apply sẽ không tạo revision mới."
                .to_string(),
        );
    }

    ApplyPreview {
        env_changes,
        scaling_changes,
        next_revision_hint: predict_next_revision(
            svc.get("latestCreatedRevision").and_then(|v| v.as_str()),
        ),
        traffic_pinned,
        warnings,
    }
}

/// Mô tả thay đổi scaling dạng câu, để đưa vào preview và audit log.
pub fn describe_scaling_changes(svc: &Value, container_idx: usize, upd: &ScalingUpdate) -> Vec<String> {
    let tpl = svc.get("template");
    let cur_min = tpl
        .and_then(|t| t.get("scaling"))
        .and_then(|s| s.get("minInstanceCount"))
        .and_then(|v| v.as_i64());
    let cur_max = tpl
        .and_then(|t| t.get("scaling"))
        .and_then(|s| s.get("maxInstanceCount"))
        .and_then(|v| v.as_i64());
    let cur_conc = tpl
        .and_then(|t| t.get("maxInstanceRequestConcurrency"))
        .and_then(|v| v.as_i64());
    let cur_timeout = tpl
        .and_then(|t| t.get("timeout"))
        .and_then(|v| v.as_str())
        .map(String::from);

    let container = containers(svc).ok().and_then(|cs| cs.get(container_idx).cloned());
    let limits = container
        .as_ref()
        .and_then(|c| c.get("resources").and_then(|r| r.get("limits")).cloned());
    let cur_cpu = limits
        .as_ref()
        .and_then(|l| l.get("cpu"))
        .and_then(|v| v.as_str())
        .map(String::from);
    let cur_mem = limits
        .as_ref()
        .and_then(|l| l.get("memory"))
        .and_then(|v| v.as_str())
        .map(String::from);

    let mut out = Vec::new();
    let show_opt_i64 = |v: Option<i64>| v.map(|x| x.to_string()).unwrap_or_else(|| "mặc định".into());
    let show_opt_str =
        |v: &Option<String>| v.clone().unwrap_or_else(|| "mặc định".into());

    if let Some(v) = upd.min_instances {
        if Some(v) != cur_min {
            out.push(format!("Min instances: {} → {}", show_opt_i64(cur_min), v));
        }
    }
    if let Some(v) = upd.max_instances {
        if Some(v) != cur_max {
            out.push(format!("Max instances: {} → {}", show_opt_i64(cur_max), v));
        }
    }
    if let Some(v) = &upd.cpu {
        if Some(v.trim().to_string()) != cur_cpu {
            out.push(format!("CPU: {} → {}", show_opt_str(&cur_cpu), v.trim()));
        }
    }
    if let Some(v) = &upd.memory {
        if Some(v.trim().to_string()) != cur_mem {
            out.push(format!("Memory: {} → {}", show_opt_str(&cur_mem), v.trim()));
        }
    }
    if let Some(v) = upd.concurrency {
        if Some(v) != cur_conc {
            out.push(format!("Concurrency: {} → {}", show_opt_i64(cur_conc), v));
        }
    }
    if let Some(v) = &upd.timeout {
        if let Ok(norm) = normalize_timeout(v) {
            if Some(norm.clone()) != cur_timeout {
                out.push(format!("Timeout: {} → {}", show_opt_str(&cur_timeout), norm));
            }
        }
    }
    if let Some(v) = upd.cpu_idle {
        let cur = container
            .as_ref()
            .and_then(|c| c.get("resources"))
            .and_then(|r| r.get("cpuIdle"))
            .and_then(|x| x.as_bool());
        if Some(v) != cur {
            out.push(format!(
                "CPU allocation: {} → {}",
                if cur.unwrap_or(true) { "chỉ khi xử lý request" } else { "luôn cấp" },
                if v { "chỉ khi xử lý request" } else { "luôn cấp" }
            ));
        }
    }
    if let Some(v) = upd.startup_cpu_boost {
        let cur = container
            .as_ref()
            .and_then(|c| c.get("resources"))
            .and_then(|r| r.get("startupCpuBoost"))
            .and_then(|x| x.as_bool());
        if Some(v) != cur {
            out.push(format!(
                "Startup CPU boost: {} → {}",
                if cur.unwrap_or(false) { "bật" } else { "tắt" },
                if v { "bật" } else { "tắt" }
            ));
        }
    }

    out
}

/// Bảo đảm payload gửi lên có etag. Thiếu etag = mất lá chắn lost-update.
pub fn require_etag(payload: &Value) -> Result<&str> {
    payload
        .get("etag")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            GcpError::Invalid(
                "Payload không có etag nên không thể bảo đảm bạn đang sửa trên bản mới nhất. \
                 Bấm Reload rồi thử lại."
                    .to_string(),
            )
        })
}

/// Lấy `Map` labels/annotations về `BTreeMap<String,String>`.
pub fn string_map(v: Option<&Value>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if let Some(Value::Object(o)) = v {
        for (k, val) in o {
            if let Some(s) = val.as_str() {
                out.insert(k.clone(), s.to_string());
            }
        }
    }
    out
}

/// Helper cho test và cho tầng trên: object rỗng an toàn.
pub fn empty_object() -> Map<String, Value> {
    Map::new()
}
