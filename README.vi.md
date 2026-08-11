# Cloud Run Cockpit

[English](README.md) · **Tiếng Việt**

App desktop (Tauri 2 + React) để vận hành Cloud Run trên GCP mà không phải mở Console:
xem nhanh service, sửa env, xem secret, xem log, xem tải và số instance, đổi được project.

```
┌──────────────────────────────────────────────────────────────────────┐
│ Cloud Run Cockpit  [example-project ▾] ?CHƯA GẮN NHÃN   🔒 Chỉ đọc  ⟳ │
├──────────────────┬───────────────────────────────────────────────────┤
│ 🔍 tìm service   │ ● api-gateway        asia-northeast1 · Tokyo      │
│                  │ ┌─────┬────┬───────┬───────┬────┬───┬──────────┐  │
│ ● api-gateway    │ │Tổng │Env │Scaling│Secrets│Tải │Log│Revisions │  │
│   6.0 inst 31rps │ └─────┴────┴───────┴───────┴────┴───┴──────────┘  │
│ ✕ notifier       │  Instance 6.0   RPS 31   5xx 0.00%   conc 80      │
│   ⚠ 13.2% lỗi    │  ┌── chart số instance ──┐ ┌── chart rps ──┐      │
│ ● billing   📌   │  └───────────────────────┘ └───────────────┘      │
└──────────────────┴───────────────────────────────────────────────────┘
```

## Bắt đầu

```bash
npm install
npm run app:dev      # chạy app (cần Rust + gcloud, xem docs/SETUP.md)
npm run preview:ui   # xem/sửa UI với dữ liệu giả, KHÔNG cần gcloud và không đụng GCP
```

Chi tiết cài đặt: [`docs/SETUP.vi.md`](docs/SETUP.vi.md) · Quyền GCP cần có:
[`docs/IAM.vi.md`](docs/IAM.vi.md)

## Kiến trúc

```
WebView (React + TS + Tailwind + TanStack Query + Recharts)
    │  Tauri IPC — chỉ đúng tập #[tauri::command] được khai báo
Rust core (src-tauri)  — auth guard, audit log, cấu hình
    │
crates/gcp             — client GCP thuần Rust, KHÔNG phụ thuộc Tauri
    ├─ Cloud Run Admin API v2      services, revisions, patch
    ├─ Cloud Monitoring API v3     biểu đồ tải, số instance
    ├─ Cloud Logging API v2        log (polling)
    ├─ Secret Manager API v1       metadata + reveal
    └─ Resource Manager API v3     danh sách project, kiểm tra quyền
```

Hai quyết định định hình cả repo:

**1. Toàn bộ credential và network call nằm trong Rust.** Frontend không được cấp plugin
`shell`, `fs`, hay `http` (xem `src-tauri/capabilities/default.json`), nên một lỗ XSS ở
webview không leo thang thành chạy lệnh hay đọc ổ đĩa.

**2. `crates/gcp` cố tình không phụ thuộc Tauri.** Nhờ vậy toàn bộ logic rủi ro
(read-modify-write service, parse env, diff, validate) chạy được dưới `cargo test` trên
bất kỳ máy nào, không cần dựng webview. Đây là nơi có ~200 test.

## Ba cạm bẫy mà code này xử lý sẵn

Đọc `crates/gcp/src/mutate.rs` và `crates/gcp/tests/mutate_test.rs` trước khi sửa đường ghi.

| Bẫy | Hậu quả nếu làm sai | Cách xử lý |
|---|---|---|
| `env[]` trộn `{name,value}` và `{name,valueSource.secretKeyRef}` | Editor kiểu `Map<String,String>` biến `DB_PASSWORD` thành chuỗi rỗng → **service mất kết nối DB** | Clone nguyên object gốc của secret-ref, chỉ chạm `version`. UI render secret-ref ở dạng khoá. |
| `template.revision` còn trong payload PATCH | Cloud Run từ chối: "Revision X already exists" | `sanitize_for_patch` xoá field này để Cloud Run tự đánh số tiếp |
| `traffic` ghim vào revision cụ thể | Sửa env "thành công" nhưng revision mới **không nhận traffic** → thay đổi im lặng vô hiệu | `is_traffic_pinned` phát hiện, UI cảnh báo vàng ở tab Env và Overview |

Ngoài ra: `PATCH` luôn gửi `etag` để chặn ghi đè mất thay đổi của người khác (409 thì báo
lỗi, **không** tự retry), và luôn GET lại bản tươi trước khi ghi thay vì dùng cache.

## Lớp an toàn khi ghi

1. **Read-only mặc định BẬT.** Phải tắt thủ công. File cấu hình hỏng → về mặc định, tức là
   read-only bật lại.
2. **Project gắn nhãn `prod` hoặc chưa gắn nhãn** → phải gõ đúng tên service mới ghi được.
   Kiểm ở tầng Rust (`AppState::guard_write`), không chỉ khoá nút ở UI.
3. **Diff bắt buộc** trước khi apply, kèm tên revision dự kiến.
4. **Dry-run** qua `validateOnly=true`: Cloud Run xác nhận cấu hình mà không tạo revision.
5. **Audit log JSONL** trên máy — mọi thao tác ghi và mọi lần xem secret, kèm diff, kể cả
   khi thất bại. Không bao giờ ghi giá trị secret.

## Secret

Mặc định chỉ hiện metadata. Reveal cần bấm nút, tự ẩn sau 30 giây (có đếm ngược), copy thì
tự xoá clipboard sau 60 giây. Giá trị secret không đi qua cache và bọc trong type `Secret`
có `Debug` redact + `Drop` zeroize, nên không rò qua log hay panic message.

## Về metric

Monitoring API **không báo lỗi khi tên metric sai** — nó trả về series rỗng. Vẽ đường phẳng
ở 0 khi đó sẽ bị đọc thành "service không có tải", sai lệch nguy hiểm hơn là không có chart.
Nên:

- `ChartData.unavailable` phân biệt rõ "không lấy được" với "có dữ liệu và bằng 0"
- Cài đặt → **Đối chiếu với metricDescriptors** kiểm tra catalog với project thật
- Sidebar dùng **một** truy vấn gộp theo `service_name` cho cả project, không phải một
  truy vấn mỗi service (project cỡ ~100 service sẽ đụng quota ngay nếu làm kiểu vòng lặp)

## Kiểm thử

```bash
cd crates/gcp && cargo test && cargo clippy --all-targets   # 200 test, phải 0 warning
cd ../src-tauri && cargo test && cargo clippy --all-targets #  50 test, phải 0 warning
cd .. && npm run typecheck
npm run preview:ui   # xem UI với dữ liệu giả
```

## Phạm vi

Có: xem service/revision/traffic/condition, sửa env, sửa scaling & resource, xem secret,
xem log, xem tải, đổi project, gắn nhãn môi trường, audit log. Từ v2 có thêm: Cloud Run Jobs
(xem tổng quan, chạy tay, pause/resume Scheduler), màn Thống kê, ước lượng chi phí, và
Recommender.

Chưa có (cố ý): deploy image mới, chuyển traffic, rollback revision, sửa giá trị secret,
sửa IAM/VPC/Cloud SQL. Mấy cái này ảnh hưởng trực tiếp tới traffic đang chạy hoặc bảo mật
nên để trên Console, nơi có sẵn xác nhận và audit của Google. Định nghĩa job cũng không sửa
được ở đây, và recommendation chỉ đánh dấu chứ không tự áp dụng.

## Cấu hình trước khi dùng thật

Repo không chứa project ID, email hay service account của bất kỳ hạ tầng thật nào — mọi ví dụ
dùng placeholder `example-project`, `example-prod`, `example-staging`… Trước khi chạy:

1. Sửa `DEFAULT_ALLOWED_PROJECT` trong `src-tauri/src/config.rs`, **hoặc** điền project ID
   thật ở **⚙ Cài đặt → Project được phép thao tác**. Để nguyên placeholder thì app chặn mọi
   thao tác (fail an toàn).
2. Đổi `identifier` trong `src-tauri/tauri.conf.json` nếu bạn build bản cài riêng.

## Giấy phép

[MIT](LICENSE).
