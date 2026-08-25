# Cloud Run Cockpit

**English** · [Tiếng Việt](README.vi.md)

Desktop app (Tauri 2 + React) for operating Cloud Run on GCP without opening the Console:
browse services, edit env and scaling, inspect secrets, tail logs, watch load, run Jobs,
estimate cost, switch projects.

```
┌──────────────────────────────────────────────────────────────────────┐
│ Cloud Run Cockpit  [example-project ▾] ?UNLABELED      🔒 Read-only ⟳ │
├──────────────────┬───────────────────────────────────────────────────┤
│ 🔍 find service  │ ● api-gateway        asia-northeast1 · Tokyo      │
│                  │ ┌────────┬───┬───────┬───────┬────┬───┬────────┐  │
│ ● api-gateway    │ │Overview│Env│Scaling│Secrets│Load│Log│Revision│  │
│   6.0 inst 31rps │ └────────┴───┴───────┴───────┴────┴───┴────────┘  │
│ ✕ notifier       │  Instances 6.0  RPS 31  5xx 0.00%   conc 80       │
│   ⚠ 13.2% errors │  ┌── instance count ────┐ ┌── rps ────────┐       │
│ ● billing   📌   │  └───────────────────────┘ └───────────────┘      │
└──────────────────┴───────────────────────────────────────────────────┘
```

## Getting started

```bash
npm install
npm run app:dev      # run the app (needs Rust + gcloud, see docs/SETUP.md)
npm run preview:ui   # browse the UI with fake data — no gcloud, never touches GCP
```

Setup: [`docs/SETUP.md`](docs/SETUP.md) · GCP permissions: [`docs/IAM.md`](docs/IAM.md)

**Before real use:** the repo contains no real project ID, email or service account — every
example is a placeholder (`example-project`, `example-prod`…). Set `DEFAULT_ALLOWED_PROJECT`
in `src-tauri/src/config.rs`, or enter your project under **⚙ Settings → Allowed projects**.
Left at the placeholder, the app blocks every operation — that is the intended fail-safe.

**Language:** docs in English and Vietnamese; UI in English (default), Vietnamese, Japanese
(**⚙ Settings → Language**). Code comments and Rust-generated error messages are Vietnamese
only — they are written to tell an operator *what to do next*, which beats uniformity here.

## Architecture

```
WebView (React + TS + Tailwind + TanStack Query + Recharts)
    │  Tauri IPC — only the declared set of #[tauri::command]
Rust core (src-tauri)  — auth guard, audit log, configuration
    │
crates/gcp             — pure-Rust GCP client, NO Tauri dependency
    ├─ Cloud Run Admin API v2      services, revisions, patch, jobs
    ├─ Cloud Monitoring API v3     load charts, instance count
    ├─ Cloud Logging API v2        logs (polling)
    ├─ Secret Manager API v1       metadata + reveal
    └─ Resource Manager API v3     project list, permission checks
```

Two decisions shape the repo:

1. **Every credential and network call lives in Rust.** The frontend gets no `shell`, `fs` or
   `http` plugin (`src-tauri/capabilities/default.json`), so an XSS hole in the webview cannot
   escalate into running commands or reading the disk.
2. **`crates/gcp` does not depend on Tauri**, so all the risky logic (read-modify-write, env
   parsing, diffing, validation) runs under `cargo test` with no webview. ~200 tests live there.

## Three traps this code already handles

Read `crates/gcp/src/mutate.rs` and `tests/mutate_test.rs` before touching the write path.

| Trap | What goes wrong | How it is handled |
|---|---|---|
| `env[]` mixes `{name,value}` and `{name,valueSource.secretKeyRef}` | An editor modelled as `Map<String,String>` turns `DB_PASSWORD` into an empty string → **the service loses its database connection** | Clone the secret-ref object verbatim, touch only `version` |
| `template.revision` left in the PATCH payload | Cloud Run rejects it: "Revision X already exists" | `sanitize_for_patch` strips the field |
| `traffic` pinned to a specific revision | Editing env "succeeds" but the new revision **receives no traffic** → the change is silently void | `is_traffic_pinned` detects it; the UI warns on Env and Overview |

`PATCH` always sends an `etag`, a 409 is **never** auto-retried, and the app re-GETs a fresh
copy before writing instead of trusting the cache.

## Safety layers on the write path

1. **Read-only is ON by default** — a corrupt config falls back to defaults, which turns it
   back on.
2. **Projects labelled `prod`, or unlabelled** → you must type the service name. Enforced in
   Rust (`AppState::guard_write`), not by disabling a button.
3. **A mandatory diff** before applying, with the expected revision name.
4. **Dry-run** via `validateOnly=true`.
5. **A local JSONL audit log** of every write and every secret reveal, failures included. It
   never records secret values.

Secrets show metadata only by default: revealing takes a deliberate click, auto-hides after 30s,
and copying clears the clipboard after 60s. Values skip the cache and are wrapped in a `Secret`
type with a redacting `Debug` and a zeroizing `Drop`.

## About metrics

The Monitoring API **does not report an error for a wrong metric name** — it returns an empty
series, and a flat line at 0 reads as "no traffic", which is worse than no chart. So
`ChartData.unavailable` separates "could not fetch" from "fetched, and it is zero", Settings →
**Verify against metricDescriptors** checks the catalog against a real project, and the sidebar
uses **one** query grouped by `service_name` for the whole project (one query per service would
hit the quota at ~100 services).

## Tests

```bash
cd crates/gcp && cargo test && cargo clippy --all-targets   # 200 tests, must be 0 warnings
cd ../src-tauri && cargo test && cargo clippy --all-targets #  50 tests, must be 0 warnings
cd .. && npm run typecheck
```

## Scope

**In:** view services/revisions/traffic/conditions, edit env, edit scaling & resources, view
secrets, tail logs, watch load, switch projects, label environments, audit log. Since v2:
Cloud Run Jobs overview + manual run + Scheduler pause/resume, a statistics grid, cost
estimation, Recommender insights.

**Deliberately out:** deploying an image, shifting traffic, rolling back a revision, editing
secret values, editing IAM/VPC/Cloud SQL. Those hit live traffic or security directly, so they
belong in the Console where Google already provides confirmation and audit. Job definitions are
not editable here, and recommendations are only marked, never auto-applied.

## License

[MIT](LICENSE).
