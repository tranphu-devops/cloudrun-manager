# Required GCP permissions

**English** · [Tiếng Việt](IAM.vi.md)

## APIs that must be enabled

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

The last two (`cloudscheduler`, `recommender`) are for v2 (the Jobs and Insights screens).
Without them the app still works — the Jobs screen lists jobs but the schedule column reports
"Scheduler data unavailable", and the Insights screen shows an API-disabled notice with a link
to enable it.

A disabled API returns **403**, not 404 — the app recognises this case and prints the exact
command to run.

## Roles per feature

| Feature in the app | Permission | Suggested role |
|---|---|---|
| Project dropdown | `resourcemanager.projects.list` | `roles/browser` (granted at organization/folder level) |
| Sidebar + Overview/Revisions tabs | `run.services.list`, `run.services.get`, `run.revisions.list` | `roles/run.viewer` |
| Editing env and scaling | `run.services.update` **+ `iam.serviceAccounts.actAs`** | `roles/run.developer` + see below |
| Load tab (charts) | `monitoring.timeSeries.list`, `monitoring.metricDescriptors.list` | `roles/monitoring.viewer` |
| Logs tab | `logging.logEntries.list` | `roles/logging.viewer` |
| Secret list + versions | `secretmanager.secrets.list`, `secretmanager.versions.list` | `roles/secretmanager.viewer` |
| Revealing a secret **value** | `secretmanager.versions.access` | `roles/secretmanager.secretAccessor` |
| Jobs screen — jobs + schedules (v2) | `run.jobs.list`, `run.jobs.get`, `run.executions.list`, `cloudscheduler.jobs.list` | `roles/run.viewer` + `roles/cloudscheduler.viewer` |
| Running a job manually (v2) | `run.jobs.run` **+ `iam.serviceAccounts.actAs`** on the job's SA | `roles/run.developer` + see the actAs section |
| Pause/resume a schedule (v2) | `cloudscheduler.jobs.pause`, `cloudscheduler.jobs.resume` | `roles/cloudscheduler.admin` |
| Cost screen (v2) | same as the Load tab — it estimates from load metrics | `roles/monitoring.viewer` |
| Insights screen — view + mark (v2) | `recommender.*.list`, `recommender.*.update` | `roles/recommender.viewer` (view) / `roles/recommender.*Admin` (mark) |

The app checks these with `projects:testIamPermissions` when you select a project and lists
what is missing in the sub-header. If the check itself is blocked, the app switches to an
optimistic mode and disables nothing — guessing "no permission" would make the app useless for
someone who does have permission, whereas guessing "has permission" costs at worst a 403 that
is already explained clearly.

## `iam.serviceAccounts.actAs` — the most common 403

**`roles/run.developer` alone is NOT enough to create a revision.** You also need permission to
act as the service account the service runs under. Google's own message for this is vague, so
the app translates it and prints the command you need.

Find the runtime SA on the **Overview → Service account** tab, then grant:

```bash
gcloud iam service-accounts add-iam-policy-binding RUNTIME_SA_EMAIL \
  --member="user:YOUR_EMAIL" \
  --role="roles/iam.serviceAccountUser" \
  --project=PROJECT_ID
```

For example, with the service `api-gateway` in the project `example-project`:

```bash
gcloud iam service-accounts add-iam-policy-binding \
  api-gateway-runtime@example-project.iam.gserviceaccount.com \
  --member="user:you@example.com" \
  --role="roles/iam.serviceAccountUser" \
  --project=example-project
```

If many services share one SA, granting it once is enough; if each service has its own SA you
must grant it per SA — or grant `roles/iam.serviceAccountUser` at the project level (broader,
weigh it against your policy).

## Recommended setup: split permissions by environment

Use two gcloud configurations so that editing prod is a deliberate act rather than a misclick:

```bash
# dev/stg — write access
gcloud config configurations create crc-dev
gcloud config set account YOUR_EMAIL
gcloud config set project DEV_PROJECT_ID

# prod — read-only
gcloud config configurations create crc-prod-ro
gcloud config set account YOUR_EMAIL
gcloud config set project PROD_PROJECT_ID
```

Switch between them:

```bash
gcloud config configurations activate crc-dev
```

The app reads whichever account/project gcloud has active, so switching configuration and
hitting **Reload** in the app is enough.

### Consider withholding `secretAccessor` on prod

If your account lacks `roles/secretmanager.secretAccessor` on a production project, the app
still works normally: the Secrets tab shows full metadata (which secret, which version, which
service uses it) while the reveal button is disabled with an explanation. That is a reasonable
trade — most operational work needs to know *which secret a service points at*, not *what the
value is*.

## Minimum matrix by role

| Role | Dev/Staging | Production |
|---|---|---|
| Day-to-day operations (edit env, scaling) | `run.developer` + `actAs`, `monitoring.viewer`, `logging.viewer`, `secretmanager.viewer` + `secretAccessor` | `run.viewer`, `monitoring.viewer`, `logging.viewer`, `secretmanager.viewer` |
| On-call (investigation only) | `run.viewer`, `monitoring.viewer`, `logging.viewer` | same |
| Allowed to edit prod | plus `run.developer` + `actAs` on the runtime SA of exactly the permitted services | |

Add `roles/browser` at the organization or folder level so the project dropdown is complete.

## Quick checks before filing a bug

```bash
# Can the app list services?
gcloud run services list --project=PROJECT_ID --format="value(metadata.name)" | head

# Can it read metrics?
gcloud auth print-access-token >/dev/null && echo "token OK"

# Which specific permissions do you hold?
gcloud projects test-iam-permissions PROJECT_ID \
  --permissions=run.services.update,monitoring.timeSeries.list,logging.logEntries.list,secretmanager.versions.access
```

That last command returns exactly the permissions you *do* have — anything absent from the
output is what you are missing.
