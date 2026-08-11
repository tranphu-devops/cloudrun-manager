# Setup & run

**English** · [Tiếng Việt](SETUP.vi.md)

Written for Windows (the primary environment). macOS is covered at the end.

## 1. Requirements

| Thing | Notes |
|---|---|
| **Rust** (stable) | https://rustup.rs — if you already have `~/.cargo` and `~/.rustup`, `rustup update` may be all you need |
| **Node.js 20+** | |
| **Microsoft C++ Build Tools** | Tauri needs the MSVC linker. Install "Desktop development with C++" from the Visual Studio Installer |
| **WebView2 Runtime** | Already present on Windows 11 |
| **Google Cloud SDK** | `gcloud` — the app uses it to obtain access tokens |

Quick check:

```powershell
rustc --version
node --version
gcloud --version
gcloud auth list          # your account must show as ACTIVE
```

## 2. Sign in to gcloud

The app does **not** run its own OAuth flow — it inherits whichever account `gcloud` has
active.

```powershell
gcloud auth login
gcloud config set project PROJECT_ID
```

Check that a token can be minted:

```powershell
gcloud auth print-access-token
```

If that command works, the app works.

### Impersonation

If your team uses impersonation, the app inherits it automatically:

```powershell
gcloud config set auth/impersonate_service_account DEPLOYER_SA_EMAIL
```

The sub-header then shows an **Impersonating** badge with the SA name. That matters before you
click anything that writes: you need to know who you currently are.

### Service account — import directly into the app (v2)

Since v2 the app can read an SA key and mint tokens itself, with **no** gcloud involved:

1. Open **⚙ Settings → Authentication (Service Account) → Import service account**.
2. Pick the SA's JSON key file, set a **passphrase** for the vault, click *Create vault &
   import SA*.
3. The next launch shows an unlock screen for the passphrase (with a *Skip — keep using the
   gcloud CLI* button if you want to fall back).

How it works: the app signs an RS256 JWT with the key and exchanges it for an access token
(pure-Rust `rsa`+`sha2`, no C compiler needed). The private key is encrypted with your
passphrase (Argon2id + AES-256-GCM) and stored locally — it is **never** sent anywhere and
**never** lands in `settings.json`, the audit log, or the clipboard. The passphrase itself is
stored nowhere at all, not even hashed.

Token source precedence: **Service Account (vault) → gcloud CLI → ADC**. While an SA is in
use, the source badge in Settings reads *Service Account (from vault)*.

The old approach (loading the key into gcloud) still works if you would rather not use the
vault:

```powershell
gcloud auth activate-service-account --key-file=C:\path\to\key.json
gcloud config set account sa-name@project.iam.gserviceaccount.com
```

## 3. Enable APIs on the project

```powershell
gcloud services enable ^
  run.googleapis.com ^
  monitoring.googleapis.com ^
  logging.googleapis.com ^
  secretmanager.googleapis.com ^
  cloudresourcemanager.googleapis.com ^
  --project=PROJECT_ID
```

A disabled API returns **403** (not 404) — the app recognises this and prints the exact command
to run, but doing it up front saves a round trip.

## 4. Run

```powershell
npm install
npm run app:dev
```

The first cargo build takes roughly 3–8 minutes (Tauri + reqwest + rustls). Later builds take
seconds.

Build an installer:

```powershell
npm run app:build
# output: src-tauri/target/release/bundle/nsis/*.exe  and  .../msi/*.msi
```

NSIS is configured with `installMode: currentUser`, so installing does not require admin
rights.

## 5. Work on the UI without GCP

```powershell
npm run preview:ui     # http://localhost:1422
```

This mode replaces the IPC layer with the mock in `preview/mock-core.ts` (fictional data: 10
services, one that fails to start, one with pinned traffic, and one metric deliberately marked
"could not fetch"). Use it to change the UI without signing in and without touching a real
project.

## 6. Local file locations

| File | Windows path |
|---|---|
| Configuration | `%APPDATA%\dev.cloudrun.cockpit\settings.json` |
| Audit log | `%APPDATA%\dev.cloudrun.cockpit\audit.jsonl` |

See the real path inside the app: **⚙ Settings → Audit log → Show file path**.

Read the audit log with PowerShell:

```powershell
Get-Content "$env:APPDATA\dev.cloudrun.cockpit\audit.jsonl" -Tail 20 | ConvertFrom-Json |
  Format-Table ts, action, project, service, outcome
```

## 7. Do these on first run

1. **Label your projects.** The app guesses a label from the name: `prod`/`master`/`live` →
   prod, `stg`/`staging`/`uat` → staging, `dev`/`sandbox`/`test` → dev. A project with **none**
   of those keywords (say `example-project`, or `quiet-meadow-123456-a7`) stays `unknown`, and
   unknown is treated like prod — you must type the service name before writing. Label your
   scratch project as Dev to skip that step.

   The "dev" branch is deliberately narrow: guessing dev wrongly removes a safety layer from
   what might be production.

2. **Run Settings → Verify against metricDescriptors.** Confirms that the 8 metric names in the
   code match the project. A wrong metric name causes no HTTP error, just an empty series.

3. **Keep Read-only on until you actually need to write.** It defaults to on.

4. **Set the project lock (v2).** The app ships with an allowlist containing a single
   placeholder, `example-project`, so **the first run blocks everything** until you enter your
   real project ID under **⚙ Settings → Allowed projects** (the *Locked* toggle). This is a
   Rust-level guard that keeps the app off the wrong prod/staging project — even if you change
   the project dropdown or poke at devtools. Keep only the projects you genuinely intend to
   touch.

### The v2 screens

The vertical nav on the left has five screens: **Services** (as in v1), **Statistics** (a grid
over every service), **Jobs** (Cloud Run Jobs + Scheduler, with warnings for runaway crons and
plaintext env vars that look like secrets), **Cost** (an estimate — always accompanied by its 7
sources of error), and **Insights** (Recommender; marks state only, never auto-applies).

## Common problems

| Symptom | Cause & fix |
|---|---|
| "gcloud CLI not found on this machine" | On Windows `gcloud` is `gcloud.cmd`. The app probes `gcloud.cmd`/`.exe`/`.bat` across `PATH` plus the default install directories. If it still cannot find it, open a fresh terminal, run `where gcloud`, and check that path is in the *user* `PATH` (not just the current session's). |
| A black console window flashes | Should not happen — the app spawns gcloud with `CREATE_NO_WINDOW`. If you see it, report what you were doing. |
| 403 when editing env, despite `roles/run.developer` | Missing `iam.serviceAccounts.actAs` on the runtime SA. See [`IAM.md`](IAM.md) — the app prints the exact command. |
| 409 / "the service changed" | Someone else just deployed. Hit Reload and re-apply your change. The app deliberately does not auto-merge. |
| Empty chart while the service is running | Read the note under the chart. "Could not fetch metric" → missing `roles/monitoring.viewer`. "No data in this window" → Cloud Run only writes metrics when there is activity. |
| No services listed | Check the project ID. The app lists every region via `locations/-`, so it is not a region problem. |
| Build fails with `link.exe not found` | Missing MSVC Build Tools, see section 1. |

## macOS

```bash
xcode-select --install
brew install --cask google-cloud-sdk
npm install && npm run app:dev
```

`gcloud` has no `.cmd` suffix on macOS; the app handles both. Extra probed paths include
`/opt/homebrew/share/google-cloud-sdk/bin` and `~/google-cloud-sdk/bin`.
