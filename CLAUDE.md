# CLAUDE.md

**English** · [Tiếng Việt](CLAUDE.vi.md)

Guidance for Claude Code when working in this repo.

## What this repo is

A Tauri 2 + React desktop app for operating Cloud Run on GCP: browse services, edit env, edit
scaling, inspect secrets/logs/load/instances, switch projects. An internal operations tool,
run primarily on Windows (macOS best-effort). Open source under the MIT license — see
`LICENSE`.

**Language policy:**

- **Docs** exist in English (`*.md`) and Vietnamese (`*.vi.md`); keep both in sync.
- **Code comments** are Vietnamese. Variable, function and file names are English.
- **UI strings** are authored in Vietnamese and translated at render time. Every
  user-visible string goes through `t()` from `src/lib/i18n.tsx`, whose **key is the
  Vietnamese sentence itself** (gettext style). Add the English text to
  `src/lib/locales/en.ts`. A missing key falls back to Vietnamese rather than rendering
  empty — see invariant #19.
- **Error messages produced by Rust** (`gcp::error`, cron lint, `CostReport.errorSources`)
  are still Vietnamese only. They must say *what to do next*, not merely *what went wrong* —
  see `crates/gcp/src/error.rs` for the standard.

## Verify loop — run before reporting done

```bash
cd crates/gcp && cargo test && cargo clippy --all-targets   # 200 tests, must be 0 warnings
cd ../src-tauri && cargo test && cargo clippy --all-targets #  50 tests, must be 0 warnings
cd .. && npm run typecheck                                  # tsc --noEmit
npm run preview:build                                       # does it still bundle
```

`cargo test` in `crates/gcp` is the gate that matters most. That crate deliberately does
**not** depend on Tauri, so all the risky logic is testable without standing up a webview.

To see the UI without GCP: `npm run preview:ui` → http://localhost:1422. It swaps the IPC layer
for `preview/mock-core.ts` (fictional data: 10 services, one failing, one with pinned traffic,
one metric deliberately marked "could not fetch"). Use this when changing UI; do not go sign
in to gcloud.

## Code map

```
crates/gcp/                 pure-Rust GCP client — do NOT import tauri here
  src/mutate.rs             ★ read-modify-write of a service. The riskiest file.
  src/run.rs                Cloud Run Admin v2: list/get/patch, revisions, awaiting operations
  src/monitoring.rs         Monitoring v3: charts + load snapshot + usage-by-service for billing
  src/logging.rs            Logging v2: build filter, parse entry, dedupe
  src/secretmanager.rs      Secret Manager v1
  src/resourcemanager.rs    project list + testIamPermissions
  src/auth.rs               TokenProvider: Service Account → gcloud CLI → ADC (in that order)
  src/sa.rs                 ★ (v2) parse SA key + sign RS256 JWT → exchange for a token. Pure-Rust rsa+sha2.
  src/cronlint.rs           (v2) cron lint (minute-wildcard…) + scan plaintext env that looks like secrets
  src/jobs.rs               ★ (v2) Cloud Run Jobs + Scheduler. `build_overview` is a pure fn, heavily tested.
  src/billing.rs            ★ (v2) cost estimation. `estimate` is a pure fn. Unit prices + free tier.
  src/recommender.rs        (v2) Recommender API: list + mark state (NO auto-apply)
  src/secret.rs             Secret newtype: redacting Debug + zeroizing Drop
  src/error.rs              ★ map GCP errors → actionable Vietnamese guidance
  src/types.ts ↔ types.rs   DTOs. Change one side, you must change the other.
  tests/mutate_test.rs      ★ 46 tests, each one a distinct way to break a real service

src-tauri/src/
  state.rs                  AppState::guard_write (+ guard_project) — the single gate for writes
  config.rs                 Settings, EnvLabel, suggest_label, project allowlist (project_lock)
  vault.rs                  ★ (v2) encrypted vault: Argon2id KDF + AES-256-GCM, header-as-AAD
  audit.rs                  JSONL audit log (plus RunJob, SetSchedulePaused, MarkRecommendation)
  commands/mutate.rs        ★ apply flow: guard → fresh GET → compare etag → patch → audit
  commands/auth.rs          (v2) import SA into the vault, unlock/lock, project allowlist
  commands/jobs.rs          (v2) jobs_overview, run_job (blocks while running), set_schedule_paused
  commands/insights.rs      (v2) cost_report, recommendations, mark_recommendation
  capabilities/default.json ★ the frontend's permissions. Do not add shell/fs/http here.

src/                        React
  lib/types.ts              ★ mirror of crates/gcp/src/types.rs
  lib/ipc.ts                invoke wrapper (api + apiV2). Commands snake_case, params camelCase.
  lib/i18n.tsx              ★ t() + I18nProvider. Key = the Vietnamese sentence (gettext style).
  lib/locales/en.ts         English dictionary. Missing key → falls back to Vietnamese.
  lib/format.ts             number/date formatting; holds the locale at module scope (see its header)
  components/LanguageGate.tsx  feeds Settings.language into the tree, above ToastHost
  components/NavRail.tsx    (v2) vertical navigation across the 5 screens
  components/charts.tsx     charts + StatTile, per the dataviz skill
  features/service-detail/tabs/  7 tabs
  features/statistics/      (v2) grid view over every service
  features/jobs/            (v2) Jobs screen (cron lint, env-secret scan)
  features/billing/         (v2) cost screen (always "estimated", 7 error sources shown in the UI)
  features/recommendations/ (v2) insights screen (mark-only)
  features/vault/           (v2) UnlockScreen (unlock at launch) + CredentialPanel (import SA)
```

## Invariants you must not break

This is the most important section of this file. Each item corresponds to a real way of
breaking a live Cloud Run service, and each has a test guarding it.

### 1. The write path works on `serde_json::Value`, not Rust structs

Cloud Run v2 is a **declarative** API: `PATCH` means "this is the state I want". If you
deserialize a Service into a struct and serialize it back, every field you did not declare
(`vpcAccess`, `binaryAuthorization`, `livenessProbe`, whatever Google added last month) will
**vanish from the payload and be deleted from the real service**.

So: clone the JSON you GET, touch exactly the path you need to change, leave the rest alone.
**Do not "clean up" `mutate.rs` into tight structs** — that is not technical debt, it is the
defence.

### 2. `env[]` mixes two shapes — never model env as `Map<String,String>`

```jsonc
{ "name": "LOG_LEVEL", "value": "debug" }
{ "name": "DB_PASSWORD", "valueSource": { "secretKeyRef": { "secret": "...", "version": "latest" } } }
```

An editor typed as `Record<string,string>` turns `DB_PASSWORD` into an empty string and takes
the service's database connection with it. `apply_env` clones the secret-ref's original object
verbatim and touches only `version`, so unknown fields inside `secretKeyRef` survive. Test:
`apply_env_giu_field_la_ben_trong_secretkeyref`.

### 3. `sanitize_for_patch` must strip `template.revision`

If a service was ever deployed with an explicit revision name, keeping that field in the PATCH
gets rejected by Cloud Run: *"Revision X already exists"*. Test:
`sanitize_xoa_template_revision`.

### 4. `etag` is mandatory, and a 409 must never be auto-retried

`patch_service` calls `require_etag` before sending. `commands/mutate.rs::fresh_and_check`
re-GETs a fresh copy (bypassing the cache) and compares its etag with the one the user is
looking at — if they differ it stops and reports a conflict. Retrying a 409 means clobbering
someone else's change. `client.rs` also does not retry `PATCH` on network errors (the request
may have reached the server and created a revision before the connection dropped).

### 5. Writes must go through `AppState::guard_write`

The read-only check and the "type the service name" check live in the **Rust layer**, not just
in a disabled button. A disabled button alone means one frontend state bug — or devtools — is
enough to bypass the entire protection.

Read-only is **ON by default**. A config file that fails to parse falls back to
`Settings::default()`, which turns it back on. Do not change this default.

### 6. Pinned traffic must be surfaced

If `traffic` points at a specific revision instead of LATEST, the new revision **receives no
traffic** — the user sees "success" while nothing actually changed. `is_traffic_pinned` detects
it; the UI warns on the Env tab, the Overview tab, and with a sidebar badge. Do not drop any of
those.

### 7. Secret values are never cached, never logged

`secretmanager::access_version` uses `client.get` (no cache) — caching here is a security bug,
not an optimisation. Values are wrapped in `Secret` (redacting Debug + zeroizing Drop). The
audit log records *who viewed which secret*, never the content. The env diff does not print
secret-ref values (`EnvChange::Removed.value = None`).

### 8. The frontend gets no shell/fs/http

`src-tauri/capabilities/default.json` contains only `core:default` plus `opener:allow-open-url`
restricted to `https://*`. Every file and network operation goes through a `#[tauri::command]`.
That is what stops an XSS hole in the webview from escalating into running commands or reading
the disk. A new capability needs an explicit justification.

### 9. "Metric unavailable" ≠ "metric is zero"

The Monitoring API **does not report an error for a wrong metric name** — it returns an empty
series. Drawing a flat line at 0 then reads as "this service has no traffic", which is more
dangerous than showing no chart. `ChartData.unavailable` separates the two cases; `TimeChart`
renders two distinct states. Do not collapse them.

Same in the sidebar: the badge shows `–` when there is no metric data yet, not `0`.

### 10. One query for the whole project, not one query per service

A project can easily hold ~100 services. `fetch_project_load` groups by
`resource.label.service_name`, so 3 queries cover the entire sidebar. Do not turn this into a
loop over services — it will hit the Monitoring API quota immediately.

### 11. The app is locked to one project (Rust-level allowlist) — v2

`config::project_allowed()` + `AppState::guard_project()` block every operation on a project
outside `allowed_projects`. The default locks to exactly **one** project
(`DEFAULT_ALLOWED_PROJECT`, currently the placeholder `example-project`) so the app can
**never** wander into the wrong prod/staging environment. `guard_project` is called at the top
of `guard_write` — **before** the read-only check (a test pins this ordering). This is a
Rust-level guard, not a hidden dropdown. Do not loosen the default into "allow everything".

> Common trap: the environment label (an `env=` label, a scheduler name prefix,
> `SPRING_PROFILES_ACTIVE`) usually does **not** equal the project ID. The allowlist matches
> project IDs, not labels.

### 12. An SA private key never exists in plaintext — v2

The key lives in an encrypted vault (`vault.rs`: Argon2id 64MiB/3/1 + AES-256-GCM, with the
header as AAD to block parameter-downgrade attacks). It is wrapped in `Secret` (redacting Debug
+ zeroizing Drop). Never let the key reach `settings.json`, the audit log, a cache, or an IPC
response. `CredentialInfo` carries only the email + key id for display. The frontend reads the
SA file with `FileReader` (a web API) and sends the JSON over IPC once — Tauri is never granted
fs permission for this.

### 13. The passphrase is stored nowhere, not even hashed — v2

It exists in RAM only during unlock/import. Forgetting it means re-importing every SA.
`UnlockedVault` holds the derived key in session memory; locking (`lock_vault`) wipes it from
RAM.

### 14. `jobs:run` is NOT idempotent — v2

Calling it twice creates two executions, and a batch job may then process the same data twice.
So: (a) `run_job` blocks if the job already has a `Running` execution, unless `force=true`;
(b) no auto-retry — `client.rs::PostWrite`/`post_no_retry` (the request may have reached the
server before the connection dropped); (c) it still goes through `guard_write` (requiring the
job name to be typed on prod/unknown projects). See invariant #4 — same reasoning as the 409 on
PATCH.

### 15. The cron column always carries a timezone; an empty cron ≠ no schedule — v2

A cron without a timezone is misinformation (`SchedulerJob.timeZone` is always displayed). And
an empty cron column may mean **Scheduler data is missing** (`schedulerUnavailable`) rather than
the job having no schedule — the UI must distinguish these two states, exactly as invariant #9
does for metrics.

### 16. Cost figures are ALWAYS estimates — v2

`CostEstimate.estimated` is always `true` (it is in the payload so the UI cannot forget). Cloud
Run bills on actual vCPU-seconds/GiB-seconds; this app only has load metrics × public list
prices. **The seven sources of error must be shown directly on the Billing screen**
(`CostReport.errorSources`), not buried in docs. A test asserts each source is > 40 characters
(long enough to be useful). Billing mode matters: request-based (`cpuIdle=true`) is far cheaper
than instance-based (~10× the CPU rate) — the "Billing mode" column spells this out because it
is the single largest optimisation lever.

### 17. Recommendations only mark state, they are NOT auto-applied — v2

`recommender::mark` only changes state (dismissed/claimed/…) on the Recommender API. Actually
applying a recommendation (changing scaling, editing IAM) affects traffic and security, so the
user should do it deliberately in the Console. The UI says so. Do not add an "apply" button
without designing its own confirmation layer.

### 18. Cloud Run Jobs use a doubly nested template — v2

`job.template.template.containers` (an ExecutionTemplate wrapping a TaskTemplate) — **unlike**
services, which have a single `template.containers`. `jobs::task_container` walks the correct
nesting; getting it wrong by one level yields empty and makes you believe the job has no
container.

### 19. A missing translation falls back to Vietnamese, never to empty

`t()` keys are the Vietnamese sentences themselves, so an untranslated string renders as
Vietnamese instead of a blank or `undefined`. For an operations tool that matters: a warning
shown in the wrong language is recoverable, a warning that renders as nothing is not.

Consequence to keep in mind: **editing a Vietnamese string silently orphans its translation.**
When you reword one, update the matching key in `src/lib/locales/en.ts` in the same commit.

Two mechanical traps, both already hit once:

- Inside a `.map()` whose callback parameter is named `t`, `t()` resolves to the item, not the
  translator. Name such parameters `toast`, `tr`, … (see `ToastHost`, `OverviewTab`).
- `useT()` is a hook, so it must be called **before** any early `return` — `ErrorBox`,
  `TooltipCard` and `JobDetailDialog` all return early.

## Conventions

**Serde:** every DTO has `#[serde(rename_all = "camelCase")]`. `Option<T>` serializes to
`null` rather than omitting the field — so TS uses `T | null`, not `?:`.

**IPC:** command names stay snake_case (matching the Rust function name), parameters are passed
**camelCase**. Passing snake_case yields a "missing field" error.

**TS types:** `src/lib/types.ts` is a hand-written mirror of `crates/gcp/src/types.rs`. No
generator (specta 2.0 is still rc). Changing one side **requires** changing the other —
TypeScript cannot catch the drift because IPC data is `any` at the boundary.

**Errors reaching the frontend:** `CmdError { message, detail, kind, status }`. `kind` is a
stable string for the frontend to branch on (`conflict` → force a reload, `auth` → guide toward
`gcloud auth login`); it is not for display.

**Charts:** follow the `dataviz` skill. The palette in `src/styles.css` has been run through
`validate_palette.js` (PASS in light + dark, first 4 slots, adjacent pairlist). **Slot order is
a colour-blindness safety mechanism, not decoration** — to change it, change the whole set and
re-run the validator; do not tweak individual hex values. Never use two y-axes. Colour binds to
the entity (looked up by series name), never to the index of the array being rendered.

**Tailwind:** v4, configured in CSS. `src/styles.css` contains `@source "./";` — required
because `vite.preview.config.ts` moves the root to `preview/`, and Tailwind would otherwise miss
`src/` and emit near-empty CSS **with no error at all**. Do not delete that line.

## Windows environment

- On Windows `gcloud` is **`gcloud.cmd`**, not `gcloud`. `auth.rs::gcloud_candidates` probes in
  the order `.cmd` → `.exe` → `.bat` → no extension. This is the number one reason an app like
  this dies at the first step on Windows.
- Spawning gcloud requires `CREATE_NO_WINDOW` (`0x08000000`), otherwise a black console window
  flashes on every token refresh.
- `main.rs` sets `windows_subsystem = "windows"` for release builds.

## Out of scope (deliberately)

Deploying a new image, shifting traffic, rolling back a revision, editing secret values,
editing IAM/VPC/Cloud SQL. These affect live traffic or security directly, so they belong in
the GCP Console where Google already provides confirmation and audit.

> Cloud Run Jobs came **into scope in v2**, but only for: viewing the overview, running
> manually (with the idempotency guard — invariant #14), and pausing/resuming Scheduler. Job
> definitions are not editable here. Recommendations are marked only, never applied
> (invariant #17).

If asked to add one of the out-of-scope features: ask first, because each needs its own
confirmation layer rather than just one more command.

## GCP projects used in examples

This repo is public, so **no real project ID appears anywhere**. Every example, test and mock
uses the placeholder set below — change `DEFAULT_ALLOWED_PROJECT`
(`src-tauri/src/config.rs`) or the allowlist in Settings to your own project before real use.

| Project ID (placeholder) | Role in examples |
|---|---|
| `example-project` | the default entry in the allowlist. The name contains no dev/prod keyword, so `suggest_label` returns `unknown` → the app treats it as prod. |
| `example-prod` | production |
| `example-staging` | staging |
| `example-develop`, `example-develop-vn`, `example-sandbox`, `example-demo` | dev |

The "dev" branch in `suggest_label` is deliberately narrow: guessing dev wrongly removes a
safety layer from what might be production. Guessing prod wrongly only makes the app ask one
more question.

**When adding tests or examples: never paste a real project ID, email, service account, or
service name into the repo.** Use the placeholder set above. Note that GitHub push protection
also rejects strings that merely *look* like live credentials — see the comment in
`cronlint.rs` about why the Stripe test fixtures contain dashes.
