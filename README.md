# Cloud Run Cockpit

**English** · [Tiếng Việt](README.vi.md)

A desktop app (Tauri 2 + React) for operating Cloud Run on GCP without opening the Console:
browse services, edit env vars, inspect secrets, tail logs, watch load and instance counts,
switch projects.

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

> **Note on language.** Documentation is available in English and Vietnamese. Code comments,
> error messages and UI strings are written in **Vietnamese** — this is an internal operations
> tool for a Vietnamese-speaking team, and the error messages are tuned to tell an operator
> *what to do next*, which is worth more than uniformity.

## Getting started

```bash
npm install
npm run app:dev      # run the app (needs Rust + gcloud, see docs/SETUP.md)
npm run preview:ui   # browse/edit the UI with fake data — no gcloud, never touches GCP
```

Setup details: [`docs/SETUP.md`](docs/SETUP.md) · Required GCP permissions: [`docs/IAM.md`](docs/IAM.md)

## Architecture

```
WebView (React + TS + Tailwind + TanStack Query + Recharts)
    │  Tauri IPC — only the declared set of #[tauri::command]
Rust core (src-tauri)  — auth guard, audit log, configuration
    │
crates/gcp             — pure-Rust GCP client, NO Tauri dependency
    ├─ Cloud Run Admin API v2      services, revisions, patch
    ├─ Cloud Monitoring API v3     load charts, instance count
    ├─ Cloud Logging API v2        logs (polling)
    ├─ Secret Manager API v1       metadata + reveal
    └─ Resource Manager API v3     project list, permission checks
```

Two decisions shape the whole repo:

**1. Every credential and network call lives in Rust.** The frontend is not granted the
`shell`, `fs`, or `http` plugins (see `src-tauri/capabilities/default.json`), so an XSS hole
in the webview cannot escalate into running commands or reading the disk.

**2. `crates/gcp` deliberately does not depend on Tauri.** That way all the risky logic
(read-modify-write of a service, env parsing, diffing, validation) runs under `cargo test` on
any machine, with no webview to stand up. That is where ~200 of the tests live.

## Three traps this code already handles

Read `crates/gcp/src/mutate.rs` and `crates/gcp/tests/mutate_test.rs` before touching the
write path.

| Trap | What goes wrong | How it is handled |
|---|---|---|
| `env[]` mixes `{name,value}` and `{name,valueSource.secretKeyRef}` | An editor modelled as `Map<String,String>` turns `DB_PASSWORD` into an empty string → **the service loses its database connection** | Clone the secret-ref object verbatim, touch only `version`. The UI renders secret-refs as locked. |
| `template.revision` left in the PATCH payload | Cloud Run rejects it: "Revision X already exists" | `sanitize_for_patch` strips the field so Cloud Run keeps numbering |
| `traffic` pinned to a specific revision | Editing env "succeeds" but the new revision **receives no traffic** → the change is silently void | `is_traffic_pinned` detects it; the UI shows an amber warning on the Env and Overview tabs |

Beyond that: `PATCH` always sends an `etag` so you cannot clobber someone else's change (a
409 surfaces as an error and is **never** auto-retried), and the app always re-GETs a fresh
copy before writing instead of trusting the cache.

## Safety layers on the write path

1. **Read-only is ON by default.** You have to turn it off deliberately. A corrupt config file
   falls back to defaults — which means read-only turns back on.
2. **Projects labelled `prod`, or not labelled at all** → you must type the service name to
   confirm. Enforced in Rust (`AppState::guard_write`), not just by disabling a button.
3. **A diff is mandatory** before applying, together with the expected revision name.
4. **Dry-run** via `validateOnly=true`: Cloud Run validates the config without creating a
   revision.
5. **A local JSONL audit log** — every write and every secret reveal, with the diff, including
   the failures. It never records secret values.

## Secrets

Only metadata is shown by default. Revealing takes a deliberate click, auto-hides after 30
seconds (with a countdown), and copying clears the clipboard after 60 seconds. Secret values
skip the cache entirely and are wrapped in a `Secret` type with a redacting `Debug` and a
zeroizing `Drop`, so they do not leak through logs or panic messages.

## About metrics

The Monitoring API **does not report an error for a wrong metric name** — it returns an empty
series. Drawing a flat line at 0 in that case reads as "this service has no traffic", which is
more dangerous than showing no chart at all. So:

- `ChartData.unavailable` distinguishes "could not fetch" from "fetched, and it is zero"
- Settings → **Verify against metricDescriptors** checks the catalog against a real project
- The sidebar uses **one** query grouped by `service_name` for the whole project, not one query
  per service (a project with ~100 services would hit the Monitoring API quota immediately)

## Tests

```bash
cd crates/gcp && cargo test && cargo clippy --all-targets   # 200 tests, must be 0 warnings
cd ../src-tauri && cargo test && cargo clippy --all-targets #  50 tests, must be 0 warnings
cd .. && npm run typecheck
npm run preview:ui   # browse the UI with fake data
```

## Scope

In scope: view services/revisions/traffic/conditions, edit env, edit scaling & resources, view
secrets, tail logs, watch load, switch projects, label environments, audit log. Since v2 also:
Cloud Run Jobs overview + manual run + Scheduler pause/resume, a statistics grid, cost
estimation, and Recommender insights.

Deliberately out of scope: deploying a new image, shifting traffic, rolling back a revision,
editing secret values, editing IAM/VPC/Cloud SQL. Those affect live traffic or security
directly, so they belong in the Console where Google already provides confirmation and audit.
Job definitions are not editable here either, and recommendations are only marked, never
auto-applied.

## Configure before real use

The repo contains no project ID, email or service account belonging to any real
infrastructure — every example uses placeholders such as `example-project`, `example-prod`,
`example-staging`. Before you run it:

1. Change `DEFAULT_ALLOWED_PROJECT` in `src-tauri/src/config.rs`, **or** enter your real
   project ID under **⚙ Settings → Allowed projects**. Left at the placeholder, the app blocks
   every operation — that is the intended fail-safe.
2. Change `identifier` in `src-tauri/tauri.conf.json` if you build your own installer.

## License

[MIT](LICENSE).
