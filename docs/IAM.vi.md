# Quyền GCP cần có

[English](IAM.md) · **Tiếng Việt**

## API phải enable

```bash
gcloud services enable \
  run.googleapis.com \
  monitoring.googleapis.com \
  logging.googleapis.com \
  secretmanager.googleapis.com \
  cloudresourcemanager.googleapis.com \
  cloudscheduler.googleapis.com \
  recommender.googleapis.com \
  --project=PROJECT_ID
```

Hai API cuối (`cloudscheduler`, `recommender`) là cho v2 (màn Jobs và màn Gợi ý). Nếu chưa
enable, app không chết — màn Jobs vẫn hiện job nhưng cột lịch báo "thiếu dữ liệu Scheduler",
màn Gợi ý hiện thông báo API chưa bật kèm link enable.

API chưa enable trả về **403**, không phải 404 — app nhận diện trường hợp này và hiện đúng
lệnh cần chạy.

## Role theo từng tính năng

| Tính năng trong app | Permission | Role gợi ý |
|---|---|---|
| Danh sách project ở dropdown | `resourcemanager.projects.list` | `roles/browser` (cấp ở organization/folder) |
| Sidebar + tab Tổng quan/Revisions | `run.services.list`, `run.services.get`, `run.revisions.list` | `roles/run.viewer` |
| Sửa env, sửa scaling | `run.services.update` **+ `iam.serviceAccounts.actAs`** | `roles/run.developer` + xem mục dưới |
| Tab Tải (biểu đồ) | `monitoring.timeSeries.list`, `monitoring.metricDescriptors.list` | `roles/monitoring.viewer` |
| Tab Log | `logging.logEntries.list` | `roles/logging.viewer` |
| Danh sách secret + version | `secretmanager.secrets.list`, `secretmanager.versions.list` | `roles/secretmanager.viewer` |
| Xem **giá trị** secret | `secretmanager.versions.access` | `roles/secretmanager.secretAccessor` |
| Màn Jobs — xem job + lịch (v2) | `run.jobs.list`, `run.jobs.get`, `run.executions.list`, `cloudscheduler.jobs.list` | `roles/run.viewer` + `roles/cloudscheduler.viewer` |
| Chạy tay job (v2) | `run.jobs.run` **+ `iam.serviceAccounts.actAs`** trên SA của job | `roles/run.developer` + xem mục actAs |
| Pause/resume lịch (v2) | `cloudscheduler.jobs.pause`, `cloudscheduler.jobs.resume` | `roles/cloudscheduler.admin` |
| Màn Chi phí (v2) | như tab Tải — dùng metric tải để ước lượng | `roles/monitoring.viewer` |
| Màn Gợi ý — xem + đánh dấu (v2) | `recommender.*.list`, `recommender.*.update` | `roles/recommender.viewer` (xem) / `roles/recommender.*Admin` (đánh dấu) |

App tự kiểm tra bằng `projects:testIamPermissions` khi bạn chọn project, và hiện danh sách
thiếu ở thanh phụ dưới header. Nếu bản thân lệnh kiểm tra cũng bị chặn, app chuyển sang chế
độ lạc quan (không khoá tính năng nào) — vì đoán "không có quyền" sẽ làm app thành vô dụng
với người thật sự có quyền, còn đoán "có quyền" thì tệ nhất là nhận một lỗi 403 đã được diễn
giải rõ ràng.

## `iam.serviceAccounts.actAs` — lỗi 403 hay gặp nhất

**`roles/run.developer` một mình KHÔNG đủ để tạo revision mới.** Bạn còn phải được phép "đóng
vai" service account mà service đang chạy dưới danh nghĩa. Message gốc của Google cho lỗi này
khá mơ hồ, nên app diễn giải sẵn và in ra đúng lệnh cần chạy.

Tìm runtime SA: tab **Tổng quan → Service account**. Rồi cấp:

```bash
gcloud iam service-accounts add-iam-policy-binding RUNTIME_SA_EMAIL \
  --member="user:YOUR_EMAIL" \
  --role="roles/iam.serviceAccountUser" \
  --project=PROJECT_ID
```

Ví dụ với service `api-gateway` trong project `example-project`:

```bash
gcloud iam service-accounts add-iam-policy-binding \
  api-gateway-runtime@example-project.iam.gserviceaccount.com \
  --member="user:you@example.com" \
  --role="roles/iam.serviceAccountUser" \
  --project=example-project
```

Nếu nhiều service dùng cùng một SE thì cấp một lần là xong; nếu mỗi service một SA riêng thì
phải cấp cho từng SA — hoặc cấp `roles/iam.serviceAccountUser` ở cấp project (rộng hơn, cân
nhắc theo chính sách).

## Cấu hình khuyến nghị: tách quyền theo môi trường

Dùng hai gcloud config, để việc sửa prod là một hành động có chủ đích chứ không phải một cú
bấm nhầm:

```bash
# dev/stg — có quyền ghi
gcloud config configurations create crc-dev
gcloud config set account YOUR_EMAIL
gcloud config set project DEV_PROJECT_ID

# prod — chỉ đọc
gcloud config configurations create crc-prod-ro
gcloud config set account YOUR_EMAIL
gcloud config set project PROD_PROJECT_ID
```

Đổi qua lại:

```bash
gcloud config configurations activate crc-dev
```

App đọc account/project đang active của gcloud, nên đổi config rồi bấm **Reload** trong app
là đủ.

### Cân nhắc không cấp `secretAccessor` trên prod

Nếu trên project production account của bạn không có `roles/secretmanager.secretAccessor`,
app vẫn dùng bình thường: tab Secrets hiện đủ metadata (secret nào, version nào, service nào
dùng) nhưng nút reveal bị khoá kèm giải thích. Đây là một lựa chọn hợp lý — phần lớn công
việc vận hành cần biết *service đang trỏ vào secret nào*, không cần biết *giá trị là gì*.

## Ma trận tối thiểu theo vai

| Vai | Dev/Staging | Production |
|---|---|---|
| Vận hành hằng ngày (sửa env, scaling) | `run.developer` + `actAs`, `monitoring.viewer`, `logging.viewer`, `secretmanager.viewer` + `secretAccessor` | `run.viewer`, `monitoring.viewer`, `logging.viewer`, `secretmanager.viewer` |
| Trực sự cố (chỉ điều tra) | `run.viewer`, `monitoring.viewer`, `logging.viewer` | như trên |
| Được phép sửa prod | thêm `run.developer` + `actAs` trên runtime SA của đúng những service được phép | |

Cộng thêm `roles/browser` ở cấp organization hoặc folder để dropdown project hiện đủ.

## Kiểm tra nhanh trước khi báo lỗi

```bash
# App có list được service không?
gcloud run services list --project=PROJECT_ID --format="value(metadata.name)" | head

# Có đọc được metric không?
gcloud auth print-access-token >/dev/null && echo "token OK"

# Có quyền cụ thể nào?
gcloud projects test-iam-permissions PROJECT_ID \
  --permissions=run.services.update,monitoring.timeSeries.list,logging.logEntries.list,secretmanager.versions.access
```

Lệnh cuối trả về đúng những permission bạn *có* — permission không xuất hiện trong output là
permission đang thiếu.
