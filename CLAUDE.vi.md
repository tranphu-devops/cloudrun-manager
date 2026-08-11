# CLAUDE.md (tiếng Việt)

[English](CLAUDE.md) · **Tiếng Việt**

Hướng dẫn cho Claude Code khi làm việc trong repo này. Bản tiếng Anh `CLAUDE.md` là bản
Claude Code đọc mặc định — sửa bên nào cũng phải đồng bộ bên kia.

## Repo này là gì

App desktop Tauri 2 + React để vận hành Cloud Run trên GCP: xem service, sửa env, sửa
scaling, xem secret/log/tải/instance, đổi project. Công cụ vận hành nội bộ, chạy trên Windows
là chính (macOS best-effort). Mã nguồn mở theo giấy phép MIT — xem `LICENSE`.

**Ngôn ngữ:**

- **Doc** có bản tiếng Anh (`*.md`) và tiếng Việt (`*.vi.md`); sửa một bên phải đồng bộ bên kia.
- **Comment trong code** viết tiếng Việt. Tên biến, tên hàm, tên file dùng tiếng Anh.
- **Text UI** viết bằng tiếng Việt rồi dịch lúc render. Mọi chuỗi người dùng thấy đều đi qua
  `t()` trong `src/lib/i18n.tsx`, mà **key chính là câu tiếng Việt** (kiểu gettext). Thêm bản
  dịch vào `src/lib/locales/{en,ja}.ts`. Thiếu key thì rơi về tiếng Việt chứ không ra chuỗi
  rỗng — xem bất biến #19. Thêm ngôn ngữ thứ tư chỉ cần sửa `Language` ở
  `src-tauri/src/config.rs` và `src/lib/types.ts`, thêm file `locales/xx.ts` mới, và một dòng
  mỗi chỗ trong `DICTS`/`LANGUAGE_NAMES` của `i18n.tsx` — không phải đụng file UI nào. Câu cần
  chèn markup ở giữa (link, `<code>`, số in đậm) phải dùng `tNode()`, không cắt `t()` ra nhiều
  mảnh — xem bất biến #20.
- **Message lỗi sinh từ Rust** (`gcp::error`, cron lint, `CostReport.errorSources`) vẫn chỉ có
  tiếng Việt. Chúng phải nói được *phải làm gì tiếp*, không chỉ nói *cái gì sai* — xem
  `crates/gcp/src/error.rs` để thấy chuẩn.

## Vòng verify — chạy trước khi báo xong

```bash
cd crates/gcp && cargo test && cargo clippy --all-targets   # 200 test, phải 0 warning
cd ../src-tauri && cargo test && cargo clippy --all-targets #  50 test, phải 0 warning
cd .. && npm run typecheck                                  # tsc --noEmit
npm run preview:build                                       # bundle được không
```

`cargo test` ở `crates/gcp` là cửa ải quan trọng nhất. Crate đó cố tình **không** phụ thuộc
Tauri nên toàn bộ logic rủi ro test được mà không cần dựng webview.

Xem UI mà không cần GCP: `npm run preview:ui` → http://localhost:1422. Nó thay tầng IPC bằng
`preview/mock-core.ts` (dữ liệu hư cấu: 10 service, có service lỗi, có service ghim traffic,
có một metric cố tình "không lấy được"). Sửa UI thì dùng cái này, đừng đi đăng nhập gcloud.

## Bản đồ code

```
crates/gcp/                 client GCP thuần Rust — KHÔNG import tauri ở đây
  src/mutate.rs             ★ read-modify-write service. File rủi ro nhất.
  src/run.rs                Cloud Run Admin v2: list/get/patch, revisions, chờ operation
  src/monitoring.rs         Monitoring v3: chart + snapshot tải + usage-by-service cho billing
  src/logging.rs            Logging v2: build filter, parse entry, dedupe
  src/secretmanager.rs      Secret Manager v1
  src/resourcemanager.rs    list project + testIamPermissions
  src/auth.rs               TokenProvider: Service Account → gcloud CLI → ADC (theo thứ tự)
  src/sa.rs                 ★ (v2) parse SA key + tự sign JWT RS256 → đổi lấy token. Pure-Rust rsa+sha2.
  src/cronlint.rs           (v2) lint cron (minute-wildcard…) + quét env plain trông như secret
  src/jobs.rs               ★ (v2) Cloud Run Jobs + Scheduler. `build_overview` là fn thuần, test nặng.
  src/billing.rs            ★ (v2) ước lượng chi phí. `estimate` là fn thuần. Đơn giá + free tier.
  src/recommender.rs        (v2) Recommender API: list + mark trạng thái (KHÔNG auto-apply)
  src/secret.rs             newtype Secret: Debug redact + Drop zeroize
  src/error.rs              ★ map lỗi GCP → hướng dẫn hành động tiếng Việt
  src/types.ts ↔ types.rs   DTO. Sửa một bên phải sửa bên kia.
  tests/mutate_test.rs      ★ 46 test, mỗi cái là một cách làm sập service thật

src-tauri/src/
  state.rs                  AppState::guard_write (+ guard_project) — cổng duy nhất cho ghi
  config.rs                 Settings, EnvLabel, suggest_label, allowlist project (project_lock)
  vault.rs                  ★ (v2) vault mã hoá: Argon2id KDF + AES-256-GCM, header-as-AAD
  audit.rs                  audit log JSONL (thêm RunJob, SetSchedulePaused, MarkRecommendation)
  commands/mutate.rs        ★ luồng apply: guard → GET tươi → so etag → patch → audit
  commands/auth.rs          (v2) import SA vào vault, unlock/lock, allowlist project
  commands/jobs.rs          (v2) jobs_overview, run_job (chặn nếu đang chạy), set_schedule_paused
  commands/insights.rs      (v2) cost_report, recommendations, mark_recommendation
  capabilities/default.json ★ quyền của frontend. Đừng thêm shell/fs/http vào đây.

src/                        React
  lib/types.ts              ★ mirror của crates/gcp/src/types.rs
  lib/ipc.ts                wrapper invoke (api + apiV2). Command snake_case, tham số camelCase.
  lib/i18n.tsx              ★ t()/tNode() + I18nProvider. Key = câu tiếng Việt (kiểu gettext).
  lib/locales/{en,ja}.ts    từ điển từng ngôn ngữ. Thiếu key → rơi về tiếng Việt.
  lib/format.ts             format số/ngày; giữ locale ở tầm module (xem header của file)
  components/LanguageGate.tsx  đưa Settings.language xuống cây, nằm trên ToastHost
  components/NavRail.tsx    (v2) điều hướng dọc giữa 5 màn
  components/charts.tsx     chart + StatTile theo skill dataviz
  features/service-detail/tabs/  7 tab
  features/statistics/      (v2) gridview toàn bộ service
  features/jobs/            (v2) màn Jobs (cron lint, quét env-secret)
  features/billing/         (v2) màn chi phí (luôn "ước lượng", 7 nguồn sai số hiện trên UI)
  features/recommendations/ (v2) màn gợi ý (mark-only)
  features/vault/           (v2) UnlockScreen (mở khoá khi mở app) + CredentialPanel (import SA)
```

## Bất biến không được phá

Đây là phần quan trọng nhất của file này. Mỗi mục đều tương ứng một cách làm sập service thật
trên Cloud Run và đều có test bảo vệ.

### 1. Đường ghi làm việc trên `serde_json::Value`, không phải struct Rust

Cloud Run v2 là API **declarative**: `PATCH` nghĩa là "đây là trạng thái tôi muốn". Nếu
deserialize Service vào struct rồi serialize lại, mọi field chưa khai báo (`vpcAccess`,
`binaryAuthorization`, `livenessProbe`, field Google mới thêm) sẽ **biến mất khỏi payload và
bị xoá khỏi service thật**.

Nên: clone JSON đã GET, chạm đúng path cần sửa, giữ nguyên phần còn lại. **Đừng "dọn dẹp"
`mutate.rs` thành struct chặt** — nó không phải nợ kỹ thuật, nó là biện pháp phòng vệ.

### 2. `env[]` trộn hai dạng — không bao giờ coi env là `Map<String,String>`

```jsonc
{ "name": "LOG_LEVEL", "value": "debug" }
{ "name": "DB_PASSWORD", "valueSource": { "secretKeyRef": { "secret": "...", "version": "latest" } } }
```

Editor kiểu `Record<string,string>` sẽ biến `DB_PASSWORD` thành chuỗi rỗng và làm service mất
kết nối database. `apply_env` clone nguyên object gốc của secret-ref và chỉ chạm `version`,
nên field lạ bên trong `secretKeyRef` vẫn còn. Test:
`apply_env_giu_field_la_ben_trong_secretkeyref`.

### 3. `sanitize_for_patch` phải xoá `template.revision`

Nếu service từng deploy với revision name chỉ định sẵn, giữ field đó khi PATCH sẽ bị Cloud Run
từ chối: *"Revision X already exists"*. Test: `sanitize_xoa_template_revision`.

### 4. `etag` là bắt buộc, và 409 không được auto-retry

`patch_service` gọi `require_etag` trước khi gửi. `commands/mutate.rs::fresh_and_check` GET
lại bản tươi (bỏ cache) rồi so etag với bản người dùng đang xem — khác nhau thì dừng và báo
conflict. Retry 409 = ghi đè mất thay đổi của người khác. `client.rs` cũng không retry
`PATCH` khi lỗi mạng (request có thể đã tới server và tạo revision rồi mới đứt).

### 5. Ghi phải đi qua `AppState::guard_write`

Kiểm read-only và kiểm "gõ đúng tên service" ở **tầng Rust**, không chỉ khoá nút ở UI. Khoá
nút một mình thì một lỗi state ở frontend hoặc devtools là đủ để bỏ qua cả lớp bảo vệ.

Read-only **mặc định BẬT**. File cấu hình đọc lỗi → về `Settings::default()`, tức là bật lại.
Đừng đổi mặc định này.

### 6. Traffic ghim phải được cảnh báo

Nếu `traffic` trỏ cứng vào revision cụ thể thay vì LATEST, revision mới sinh ra **không nhận
traffic** — người dùng thấy "thành công" mà thực tế không có gì thay đổi. `is_traffic_pinned`
phát hiện; UI cảnh báo ở tab Env, tab Tổng quan, và badge ở sidebar. Đừng bỏ mấy chỗ đó.

### 7. Giá trị secret không cache, không log

`secretmanager::access_version` dùng `client.get` (không cache) — có cache ở đây là lỗi bảo
mật, không phải tối ưu. Giá trị bọc trong `Secret` (Debug redact + Drop zeroize). Audit log
ghi *ai xem secret nào*, tuyệt đối không ghi nội dung. Diff env không in giá trị của
secret-ref (`EnvChange::Removed.value = None`).

### 8. Frontend không được cấp shell/fs/http

`src-tauri/capabilities/default.json` chỉ có `core:default` + `opener:allow-open-url` giới hạn
`https://*`. Mọi thao tác file và mạng đi qua `#[tauri::command]`. Nhờ vậy một lỗ XSS ở
webview không leo thang thành chạy lệnh hay đọc ổ đĩa. Cần capability mới thì phải có lý do rõ
ràng.

### 9. Metric không lấy được ≠ metric bằng 0

Monitoring API **không báo lỗi khi tên metric sai** — nó trả series rỗng. Vẽ đường phẳng ở 0
khi đó sẽ bị đọc thành "service không có tải", sai lệch nguy hiểm hơn là không có chart.
`ChartData.unavailable` phân biệt hai trường hợp; `TimeChart` render hai trạng thái khác nhau.
Đừng gộp lại.

Tương tự ở sidebar: badge hiện `–` khi chưa có dữ liệu metric, không hiện `0`.

### 10. Một truy vấn cho cả project, không phải một truy vấn mỗi service

Một project dễ dàng có ~100 service. `fetch_project_load` gộp theo
`resource.label.service_name` nên 3 truy vấn là đủ cho toàn bộ sidebar. Đừng đổi thành vòng
lặp gọi từng service — sẽ đụng quota Monitoring API ngay.

### 11. App bị khoá vào một project (allowlist ở tầng Rust) — v2

`config::project_allowed()` + `AppState::guard_project()` chặn mọi thao tác trên project ngoài
`allowed_projects`. Mặc định khoá vào đúng **một** project (`DEFAULT_ALLOWED_PROJECT`, hiện là
placeholder `example-project`) để app **không bao giờ** đụng nhầm prod/staging. `guard_project`
gọi ở đầu `guard_write` — **trước** cả check read-only (test bắt đúng thứ tự này). Đây là guard
tầng Rust, không phải chỉ ẩn dropdown. Đừng nới mặc định thành "cho qua hết".

> Cạm bẫy hay gặp: nhãn môi trường (label `env=`, prefix của scheduler, `SPRING_PROFILES_ACTIVE`)
> thường **không** trùng project ID. Allowlist so khớp project ID, không so nhãn.

### 12. Private key của SA không bao giờ ra dạng plaintext — v2

Key nằm trong vault mã hoá (`vault.rs`: Argon2id 64MiB/3/1 + AES-256-GCM, header làm AAD để
chặn downgrade tham số). Bọc trong `Secret` (Debug redact + Drop zeroize). Tuyệt đối không để
key vào `settings.json`, audit log, cache, hay trả ra IPC. `CredentialInfo` chỉ mang email +
key id để hiển thị. Frontend đọc file SA bằng `FileReader` (web API) rồi gửi JSON qua IPC một
lần — không cấp quyền fs cho Tauri để làm việc này.

### 13. Passphrase không lưu ở đâu, kể cả hash — v2

Chỉ nằm trong RAM lúc unlock/import. Quên passphrase = phải nhập lại từng SA. `UnlockedVault`
giữ key đã derive trong bộ nhớ phiên; khoá lại (`lock_vault`) là xoá khỏi RAM.

### 14. `jobs:run` KHÔNG idempotent — v2

Gọi hai lần tạo hai execution, job batch có thể xử lý trùng dữ liệu. Nên: (a) `run_job` chặn
nếu job đang có execution `Running`, trừ khi `force=true`; (b) không auto-retry —
`client.rs::PostWrite`/`post_no_retry` (request có thể đã tới server rồi mới đứt mạng); (c) vẫn
qua `guard_write` (đòi gõ tên job trên project prod/unknown). Xem invariant #4 — cùng lý do với
409 của PATCH.

### 15. Cột cron luôn kèm timezone; cron trống ≠ không có lịch — v2

Cron không có timezone là thông tin sai (`SchedulerJob.timeZone` luôn hiện). Và cột cron trống
có thể vì **thiếu dữ liệu Scheduler** (`schedulerUnavailable`) chứ không phải job không có lịch
— hai trạng thái này phải phân biệt trên UI, giống invariant #9 với metric.

### 16. Số chi phí LUÔN là ước lượng — v2

`CostEstimate.estimated` luôn `true` (có trong payload để UI không quên). Cloud Run tính theo
vCPU-giây/GiB-giây thực; app chỉ có metric tải × đơn giá công khai. **Bảy nguồn sai số bắt buộc
hiện thẳng trên màn Billing** (`CostReport.errorSources`), không cất trong doc. Test bắt mỗi
nguồn > 40 ký tự (đủ để hữu ích). Kiểu tính tiền: request-based (`cpuIdle=true`) rẻ hơn
instance-based (~10 lần đơn giá CPU) — cột "Kiểu tính tiền" nói rõ vì đây là đòn bẩy tối ưu lớn
nhất.

### 17. Recommendation chỉ đánh dấu trạng thái, KHÔNG auto-apply — v2

`recommender::mark` chỉ đổi state (dismissed/claimed/…) trên Recommender API. Áp dụng thật (đổi
scaling, sửa IAM) ảnh hưởng traffic/bảo mật nên để người dùng làm có chủ đích trên Console. UI
nói rõ điều này. Đừng thêm nút "áp dụng" mà không thiết kế lớp xác nhận riêng.

### 18. Cloud Run Jobs dùng template lồng hai lớp — v2

`job.template.template.containers` (ExecutionTemplate bọc TaskTemplate) — KHÁC service chỉ có
một lớp `template.containers`. `jobs::task_container` đọc đúng đường lồng này; đọc sai một lớp
sẽ ra rỗng và tưởng job không có container.

### 19. Thiếu bản dịch thì rơi về tiếng Việt, không bao giờ ra chuỗi rỗng

Key của `t()` chính là câu tiếng Việt, nên chuỗi chưa dịch sẽ hiện tiếng Việt thay vì để trống
hay `undefined`. Với công cụ vận hành thì điều này quan trọng: một cảnh báo hiện sai ngôn ngữ
còn cứu được, một cảnh báo hiện ra khoảng trắng thì không.

Hệ quả phải nhớ: **sửa câu tiếng Việt là làm mồ côi bản dịch của nó.** Đổi chữ thì sửa luôn key
tương ứng trong `src/lib/locales/en.ts` trong cùng commit.

Hai cạm bẫy máy móc, đều đã đụng một lần:

- Trong `.map()` mà tham số callback tên là `t`, gọi `t()` sẽ trúng phần tử chứ không phải hàm
  dịch. Đặt tên khác: `toast`, `tr`… (xem `ToastHost`, `OverviewTab`).
- `useT()` là hook nên phải gọi **trước** mọi `return` sớm — `ErrorBox`, `TooltipCard`,
  `JobDetailDialog` đều return sớm.
- **Rà key kiểu chỉ grep lời gọi `t("…")` trực tiếp sẽ bỏ sót mọi lời gọi tra bảng** —
  `t(it.label)`, `t(LABEL_TEXT[v])`, `t(h.text)`, `t(labelOf(x))`. Đó là cách `NavRail`,
  `LABEL_TEXT`/`ACTION_TEXT` của `TopBar`, `HEALTH_META` của `StatisticsPage`, `EXEC_TONE` của
  `JobsPage`, `CATEGORY_VI`/`PRIORITY_META` của `RecommendationsPage`, `SOURCE_TEXT` của
  `CredentialPanel`, `INGRESS_TEXT` của `OverviewTab`, và nhãn `WINDOWS` của `MetricsTab` đều
  render — regex chỉ khớp dấu `"` ngay sau `t(` đi qua hết mấy chỗ này và báo nhầm "đã phủ đủ
  key". Đây chính xác là lý do "Thống kê", "Gợi ý", "3 ngày" và "lỗi" (chữ thường) lọt ra
  ngoài không có bản dịch dù đã chạy kiểm tra phủ key. Muốn rà cho đúng, grep thêm
  `\bt(Node)?\(\s*[^"'` )]` (bất kỳ gì không phải chuỗi trực tiếp) rồi lần tay từng bảng
  `Record<…, string>` / `{ …, text: "…" }` / `{ …, label: "…" }` nuôi lời gọi đó, liệt kê hết
  mọi giá trị bảng có thể sinh ra — không chỉ những cái script tình cờ khớp được.

### 20. Câu cần chèn markup ở giữa thì dùng `tNode()`, không bao giờ cắt `t()` ra nhiều mảnh

Bản đầu tiên của UI này cắt câu quanh `<code>`/`<strong>`/`<a>` kiểu:

```tsx
{t("Account hiện tại không có")} <code>secretmanager.versions.access</code> {t("trên project này…")}
```

Cách đó chạy được chỉ vì tình cờ: tiếng Việt, tiếng Anh và (phần lớn) tiếng Nhật xếp mệnh đề
theo thứ tự gần giống nhau, nên mảnh markup rơi vào đúng chỗ hợp lý. Nó vỡ ngay khi một ngôn
ngữ đặt động từ ở vị trí khác — tiếng Nhật là SOV, nên vị ngữ từng nằm giữa câu tiếng Việt
phải dời ra cuối câu, mà người dịch không dời được vì vị trí markup do JSX quyết định, không
do bản dịch. Lỗi này bị phát hiện — và sửa ở cả chín chỗ — khi thêm tiếng Nhật.

Cách sửa: giữ nguyên cả câu làm một key, đặt placeholder `{name}` ngay chỗ cần chèn markup,
rồi truyền `ReactNode` qua `tNode()`:

```tsx
{tNode("Account hiện tại không có {perm} trên project này, nên chỉ xem được metadata.", {
  perm: <code className="mono">secretmanager.versions.access</code>,
})}
```

Mỗi ngôn ngữ tự do đặt `{perm}` ở đúng vị trí ngữ pháp của mình. `useTNode()` nằm cạnh
`useT()` trong `src/lib/i18n.tsx` — đọc chú thích đầu file đó trước khi thêm chỗ dùng mới.

## Quy ước

**Serde:** mọi DTO có `#[serde(rename_all = "camelCase")]`. `Option<T>` ra `null`, không bỏ
field — nên TS dùng `T | null`, không dùng `?:`.

**IPC:** tên command giữ snake_case (đúng tên hàm Rust), tham số truyền **camelCase**. Truyền
snake_case sẽ lỗi "missing field".

**Type TS:** `src/lib/types.ts` là bản mirror viết tay của `crates/gcp/src/types.rs`. Không
dùng generator (specta 2.0 còn rc). Sửa một bên **phải** sửa bên kia — TypeScript không bắt
được lệch này vì dữ liệu qua IPC là `any` ở ranh giới.

**Lỗi ra frontend:** `CmdError { message, detail, kind, status }`. `kind` là chuỗi ổn định để
frontend phân nhánh (`conflict` → bắt reload, `auth` → hướng dẫn `gcloud auth login`), không
phải để hiển thị.

**Chart:** theo skill `dataviz`. Bảng màu ở `src/styles.css` đã chạy qua
`validate_palette.js` (PASS light + dark, 4 slot đầu, pairlist adjacent). **Thứ tự slot là cơ
chế an toàn cho người mù màu, không phải thẩm mỹ** — muốn đổi thì đổi cả bộ rồi chạy lại
validator, đừng sửa từng hex. Không bao giờ dùng hai trục y. Màu gắn với thực thể (tra theo
tên series), không gắn với index của mảng đang render.

**Tailwind:** v4, cấu hình bằng CSS. `src/styles.css` có `@source "./";` — cần vì
`vite.preview.config.ts` đổi root sang `preview/` và Tailwind sẽ bỏ sót `src/`, sinh ra CSS
gần như rỗng mà **không báo lỗi gì**. Đừng xoá dòng đó.

## Môi trường Windows

- `gcloud` trên Windows là **`gcloud.cmd`**, không phải `gcloud`. `auth.rs::gcloud_candidates`
  dò theo thứ tự `.cmd` → `.exe` → `.bat` → không đuôi. Đây là lỗi số một khiến app kiểu này
  chết ngay bước đầu trên Windows.
- Spawn gcloud phải có `CREATE_NO_WINDOW` (`0x08000000`), không thì nháy cửa sổ console đen
  mỗi lần refresh token.
- `main.rs` có `windows_subsystem = "windows"` cho bản release.

## Ngoài phạm vi (cố ý)

Deploy image mới, chuyển traffic, rollback revision, sửa giá trị secret, sửa IAM/VPC/Cloud SQL.
Mấy cái này ảnh hưởng trực tiếp tới traffic đang chạy hoặc bảo mật nên để trên GCP Console, nơi
có sẵn xác nhận và audit của Google.

> Cloud Run Jobs đã **vào phạm vi từ v2** nhưng chỉ ở mức: xem tổng quan, chạy tay (có lớp
> chặn idempotent — invariant #14), và pause/resume Scheduler. Không sửa định nghĩa job ở đây.
> Recommendation cũng chỉ đánh dấu, không áp dụng (invariant #17).

Nếu được yêu cầu thêm mấy tính năng còn ngoài phạm vi: hỏi lại trước, vì chúng cần thiết kế lớp
xác nhận riêng chứ không phải chỉ thêm một command.

## Project GCP dùng trong ví dụ

Repo này công khai nên **không** ghi project ID thật ở bất kỳ đâu. Mọi ví dụ, test và mock
dùng bộ tên placeholder dưới đây — sửa `DEFAULT_ALLOWED_PROJECT` (`src-tauri/src/config.rs`)
hoặc allowlist trong Cài đặt thành project của bạn trước khi chạy thật.

| Project ID (placeholder) | Vai trong ví dụ |
|---|---|
| `example-project` | project mặc định trong allowlist. Tên không có từ khoá dev/prod nên `suggest_label` trả `unknown` → app xử lý như prod. |
| `example-prod` | production |
| `example-staging` | staging |
| `example-develop`, `example-develop-vn`, `example-sandbox`, `example-demo` | dev |

Nhánh đoán "dev" trong `suggest_label` cố tình hẹp: đoán sai thành dev là mất một lớp bảo vệ
trên môi trường có thể là production. Đoán sai thành prod chỉ làm app hỏi kỹ hơn.

**Khi thêm test/ví dụ mới: đừng dán project ID, email, service account, hay tên service của
hạ tầng thật vào repo.** Dùng bộ placeholder trên.
