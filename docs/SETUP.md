# Cài đặt & chạy

Hướng dẫn cho máy Windows (môi trường chính). macOS ghi ở cuối.

## 1. Yêu cầu

| Thứ | Ghi chú |
|---|---|
| **Rust** (stable) | https://rustup.rs — bạn đã có `~/.cargo` và `~/.rustup` nên có thể chỉ cần `rustup update` |
| **Node.js 20+** | đã có |
| **Microsoft C++ Build Tools** | Tauri cần MSVC linker. Cài "Desktop development with C++" từ Visual Studio Installer |
| **WebView2 Runtime** | Windows 11 đã có sẵn |
| **Google Cloud SDK** | `gcloud` — app dùng nó để lấy access token |

Kiểm tra nhanh:

```powershell
rustc --version
node --version
gcloud --version
gcloud auth list          # phải thấy account của bạn ở trạng thái ACTIVE
```

## 2. Đăng nhập gcloud

App **không** tự làm OAuth flow — nó thừa hưởng account đang active của `gcloud`.

```powershell
gcloud auth login
gcloud config set project PROJECT_ID
```

Kiểm tra lấy được token:

```powershell
gcloud auth print-access-token
```

Nếu lệnh này chạy được thì app chạy được.

### Impersonation

Nếu team dùng impersonation, app tự thừa hưởng:

```powershell
gcloud config set auth/impersonate_service_account DEPLOYER_SA_EMAIL
```

Khi đó thanh phụ dưới header sẽ hiện badge **Impersonating** kèm tên SA. Điều này quan trọng
trước khi bấm sửa gì: bạn phải biết mình đang là ai.

### Service account — nhập thẳng trong app (v2)

Từ v2, app tự nhận diện SA và tự lấy token, **không cần** đi qua gcloud:

1. Mở **⚙ Cài đặt → Xác thực (Service Account) → Nhập service account**.
2. Chọn file JSON key của SA, đặt **passphrase** cho vault, bấm *Tạo vault & nhập SA*.
3. Lần mở app sau sẽ có màn nhập passphrase để mở khoá (có nút *Bỏ qua — dùng gcloud CLI như
   cũ* nếu muốn quay lại đường gcloud).

Cơ chế: app tự sign JWT RS256 bằng key rồi đổi lấy access token (pure-Rust `rsa`+`sha2`, không
cần C compiler). Key riêng được mã hoá bằng passphrase (Argon2id + AES-256-GCM) rồi lưu trên
máy — **không** gửi đi đâu, **không** nằm trong `settings.json`, audit log, hay clipboard.
Passphrase không lưu ở đâu cả, kể cả dạng hash.

Thứ tự chọn nguồn token: **Service Account (vault) → gcloud CLI → ADC**. Khi đang dùng SA,
badge nguồn trong Cài đặt hiện *Service Account (từ vault)*.

Vẫn có thể dùng cách cũ (nạp key vào gcloud) nếu không muốn dùng vault:

```powershell
gcloud auth activate-service-account --key-file=C:\path\to\key.json
gcloud config set account sa-name@project.iam.gserviceaccount.com
```

## 3. Enable API trên project

```powershell
gcloud services enable ^
  run.googleapis.com ^
  monitoring.googleapis.com ^
  logging.googleapis.com ^
  secretmanager.googleapis.com ^
  cloudresourcemanager.googleapis.com ^
  --project=PROJECT_ID
```

API chưa enable sẽ trả **403** (không phải 404) — app nhận diện và hiện đúng lệnh cần chạy,
nhưng làm trước thì đỡ một vòng.

## 4. Chạy

```powershell
npm install
npm run app:dev
```

Lần đầu cargo build mất khoảng 3–8 phút (Tauri + reqwest + rustls). Lần sau vài giây.

Build bản cài:

```powershell
npm run app:build
# đầu ra: src-tauri/target/release/bundle/nsis/*.exe  và  .../msi/*.msi
```

NSIS được cấu hình `installMode: currentUser` nên không cần quyền admin để cài.

## 5. Làm UI không cần GCP

```powershell
npm run preview:ui     # http://localhost:1422
```

Chế độ này thay tầng IPC bằng mock trong `preview/mock-core.ts` (dữ liệu hư cấu: 10 service,
có service lỗi, có service bị ghim traffic, có một metric cố tình "không lấy được"). Dùng để
sửa giao diện mà không phải đăng nhập và không đụng vào project thật.

## 6. Vị trí file trên máy

| File | Đường dẫn Windows |
|---|---|
| Cấu hình | `%APPDATA%\dev.cloudrun.cockpit\settings.json` |
| Audit log | `%APPDATA%\dev.cloudrun.cockpit\audit.jsonl` |

Xem đường dẫn thật trong app: **⚙ Cài đặt → Audit log → Hiện đường dẫn file**.

Đọc audit log bằng PowerShell:

```powershell
Get-Content "$env:APPDATA\dev.cloudrun.cockpit\audit.jsonl" -Tail 20 | ConvertFrom-Json |
  Format-Table ts, action, project, service, outcome
```

## 7. Việc cần làm ngay lần đầu chạy

1. **Gắn nhãn môi trường cho các project.** App đoán nhãn từ tên: có `prod`/`master`/`live`
   → prod, có `stg`/`staging`/`uat` → staging, có `dev`/`sandbox`/`test` → dev. Project **không
   chứa từ khoá nào** (ví dụ `example-project`, `quiet-meadow-123456-a7`) để `unknown`, và
   unknown bị xử lý như prod (phải gõ tên service mới ghi được). Gắn nhãn Dev cho project thử
   nghiệm để bỏ bước đó.

   Nhánh đoán "dev" cố tình hẹp: đoán sai thành dev là mất một lớp bảo vệ trên môi trường có
   thể là production.

2. **Chạy Cài đặt → Đối chiếu với metricDescriptors.** Xác nhận 8 tên metric trong code
   khớp với project thật. Metric sai tên không gây lỗi HTTP, chỉ trả series rỗng.

3. **Giữ Read-only cho tới khi thực sự cần sửa.** Nó mặc định bật.

4. **Đặt khoá project (v2).** App ship với allowlist chứa đúng một placeholder
   `example-project`, nên **lần đầu chạy sẽ chặn hết** cho tới khi bạn điền project ID thật
   ở **⚙ Cài đặt → Project được phép thao tác** (toggle *Đang khoá*). Đây là lớp bảo vệ ở
   tầng Rust để app không chạm nhầm prod/staging — kể cả khi đổi dropdown project hay dùng
   devtools. Chỉ để đúng những project bạn thật sự muốn thao tác.

### Các màn của v2

Thanh điều hướng dọc bên trái có 5 màn: **Service** (như v1), **Thống kê** (gridview toàn bộ
service), **Jobs** (Cloud Run Jobs + Scheduler, cảnh báo cron chạy loạn và env plain trông
như secret), **Chi phí** (ước lượng — luôn kèm 7 nguồn sai số), **Gợi ý** (Recommender, chỉ
đánh dấu trạng thái, không tự áp dụng).

## Lỗi hay gặp

| Triệu chứng | Nguyên nhân & cách sửa |
|---|---|
| "Không tìm thấy gcloud CLI trên máy" | `gcloud` trên Windows là `gcloud.cmd`. App đã dò `gcloud.cmd`/`.exe`/`.bat` trong `PATH` + các thư mục cài mặc định. Nếu vẫn không thấy, mở terminal mới rồi chạy `where gcloud` và kiểm tra đường dẫn đó có trong `PATH` của user (không chỉ của session hiện tại). |
| Nháy cửa sổ console đen | Không nên xảy ra — app spawn gcloud với `CREATE_NO_WINDOW`. Nếu thấy, báo lại kèm thao tác đang làm. |
| 403 khi sửa env, dù có `roles/run.developer` | Thiếu `iam.serviceAccounts.actAs` trên runtime SA. Xem [`IAM.md`](IAM.md) — app hiện sẵn lệnh cần chạy. |
| 409 / "Service đã bị thay đổi" | Người khác vừa deploy. Bấm Reload rồi áp lại sửa đổi. App cố ý không tự merge. |
| Chart trống nhưng service đang chạy | Xem note dưới chart. Nếu là "Không lấy được metric" → thiếu `roles/monitoring.viewer`. Nếu là "Không có dữ liệu trong khoảng này" → Cloud Run chỉ ghi metric khi có hoạt động. |
| Không thấy service nào | Kiểm tra project ID. App list mọi region qua `locations/-` nên không phải vấn đề region. |
| Build lỗi `link.exe not found` | Thiếu MSVC Build Tools, xem mục 1. |

## macOS

```bash
xcode-select --install
brew install --cask google-cloud-sdk
npm install && npm run app:dev
```

`gcloud` không có đuôi `.cmd` trên macOS; app đã xử lý cả hai. Đường dẫn dò thêm gồm
`/opt/homebrew/share/google-cloud-sdk/bin` và `~/google-cloud-sdk/bin`.
