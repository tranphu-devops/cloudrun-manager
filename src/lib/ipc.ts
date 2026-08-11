/**
 * Wrapper có kiểu cho mọi `invoke`.
 *
 * Quy ước Tauri 2: tên command giữ snake_case (đúng tên hàm Rust), còn tham số truyền
 * bằng **camelCase** (Tauri tự đổi sang snake_case khi deserialize). Truyền snake_case
 * ở đây sẽ lỗi "missing field".
 */

import { invoke } from "@tauri-apps/api/core";

import type {
  ApplyPreview,
  ApplyResult,
  AuthInfo,
  CapabilitiesResult,
  CmdError,
  EnvEntry,
  EnvLabel,
  Language,
  LogPage,
  MetricCheck,
  ProjectInfo,
  ProjectLoadSnapshot,
  RevealResult,
  RevisionInfo,
  ScalingUpdate,
  SecretInfo,
  SecretVersionInfo,
  ServiceCharts,
  ServiceDetail,
  ServiceListResult,
  Settings,
} from "./types";

/**
 * Chuẩn hoá lỗi.
 *
 * Rust trả về `CmdError` dạng object. Nhưng lỗi hạ tầng của Tauri (command không tồn
 * tại, IPC hỏng) lại là string — không xử lý thì `err.message` thành `undefined` và UI
 * hiện ô lỗi rỗng, tệ hơn cả lỗi gốc.
 */
export function asCmdError(e: unknown): CmdError {
  if (e && typeof e === "object" && "message" in e && "kind" in e) {
    return e as CmdError;
  }
  return {
    message: typeof e === "string" ? e : "Lỗi không xác định khi gọi tới tầng Rust.",
    detail: e && typeof e === "object" ? JSON.stringify(e) : null,
    kind: "other",
    status: null,
  };
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw asCmdError(e);
  }
}

// --- auth / project / cấu hình -------------------------------------------------

export const api = {
  authInfo: () => call<AuthInfo>("auth_info"),
  listProjects: () => call<ProjectInfo[]>("list_projects"),
  getSettings: () => call<Settings>("get_settings"),
  setReadOnly: (value: boolean) => call<Settings>("set_read_only", { value }),
  setProjectLabel: (project: string, label: EnvLabel) =>
    call<Settings>("set_project_label", { project, label }),
  setPreferences: (p: {
    autoRefreshSeconds?: number;
    logPollSeconds?: number;
    revealTimeoutSeconds?: number;
    metricsWindowMinutes?: number;
    language?: Language;
  }) => call<Settings>("set_preferences", p),
  checkPermissions: (project: string) =>
    call<CapabilitiesResult>("check_permissions", { project }),
  selectProject: (project: string) => call<Settings>("select_project", { project }),
  verifyMetrics: (project: string) => call<MetricCheck[]>("verify_metrics", { project }),
  auditTail: (limit?: number) => call<unknown[]>("audit_tail", { limit: limit ?? 200 }),
  auditPath: () => call<string>("audit_path"),
  clearCache: () => call<void>("clear_cache"),

  // --- service ----------------------------------------------------------------

  listServices: (project: string) => call<ServiceListResult>("list_services", { project }),
  refreshProject: (project: string) =>
    call<ServiceListResult>("refresh_project", { project }),
  getService: (project: string, region: string, service: string) =>
    call<ServiceDetail>("get_service", { project, region, service }),
  listRevisions: (project: string, region: string, service: string) =>
    call<RevisionInfo[]>("list_revisions", { project, region, service }),
  projectLoad: (project: string, minutes?: number) =>
    call<ProjectLoadSnapshot>("project_load", { project, minutes: minutes ?? 30 }),

  // --- ghi --------------------------------------------------------------------

  previewEnv: (a: {
    project: string;
    region: string;
    service: string;
    containerIndex: number;
    env: EnvEntry[];
  }) => call<ApplyPreview>("preview_env", a),

  applyEnv: (a: {
    project: string;
    region: string;
    service: string;
    containerIndex: number;
    env: EnvEntry[];
    expectedEtag: string;
    confirmText: string | null;
    validateOnly: boolean;
  }) => call<ApplyResult>("apply_env", a),

  previewScaling: (a: {
    project: string;
    region: string;
    service: string;
    containerIndex: number;
    update: ScalingUpdate;
  }) => call<ApplyPreview>("preview_scaling", a),

  applyScaling: (a: {
    project: string;
    region: string;
    service: string;
    containerIndex: number;
    update: ScalingUpdate;
    expectedEtag: string;
    confirmText: string | null;
    validateOnly: boolean;
  }) => call<ApplyResult>("apply_scaling", a),

  // --- metric / log / secret --------------------------------------------------

  serviceCharts: (project: string, region: string, service: string, minutes?: number) =>
    call<ServiceCharts>("service_charts", { project, region, service, minutes }),

  fetchLogs: (a: {
    project: string;
    region: string;
    service: string;
    revision?: string | null;
    minSeverity?: string | null;
    search?: string | null;
    stream?: string | null;
    minutes?: number | null;
    since?: string | null;
    pageSize?: number | null;
    pageToken?: string | null;
  }) => call<LogPage>("fetch_logs", a),

  logExplorerUrl: (project: string, region: string, service: string) =>
    call<string>("log_explorer_url", { project, region, service }),

  listSecrets: (project: string) => call<SecretInfo[]>("list_secrets", { project }),
  listSecretVersions: (project: string, secret: string) =>
    call<SecretVersionInfo[]>("list_secret_versions", { project, secret }),
  revealSecret: (project: string, secret: string, version?: string) =>
    call<RevealResult>("reveal_secret", { project, secret, version: version ?? "latest" }),
};

/** Link tới trang service trên GCP Console — đường thoát khi app không đủ. */
export function consoleServiceUrl(project: string, region: string, service: string) {
  return `https://console.cloud.google.com/run/detail/${encodeURIComponent(
    region,
  )}/${encodeURIComponent(service)}/metrics?project=${encodeURIComponent(project)}`;
}

export function consoleSecretUrl(project: string, secret: string) {
  return `https://console.cloud.google.com/security/secret-manager/secret/${encodeURIComponent(
    secret,
  )}/versions?project=${encodeURIComponent(project)}`;
}

// ===========================================================================
// v2
// ===========================================================================

import type {
  CostReport,
  ImportResult,
  JobsResult,
  MarkAction,
  RecommendationsResult,
  RunJobOutcome,
  Settings as SettingsT,
  VaultStatus,
} from "./types";

export const apiV2 = {
  // --- credential vault ---
  vaultStatus: () => call<VaultStatus>("vault_status"),
  importServiceAccount: (keyJson: string, passphrase: string) =>
    call<ImportResult>("import_service_account", { keyJson, passphrase }),
  unlockVault: (passphrase: string) => call<VaultStatus>("unlock_vault", { passphrase }),
  lockVault: () => call<VaultStatus>("lock_vault"),
  removeCredential: (index: number) => call<VaultStatus>("remove_credential", { index }),
  setAllowedProjects: (projects: string[], lock: boolean) =>
    call<SettingsT>("set_allowed_projects", { projects, lock }),

  // --- jobs ---
  jobsOverview: (project: string) => call<JobsResult>("jobs_overview", { project }),
  refreshJobs: (project: string) => call<JobsResult>("refresh_jobs", { project }),
  runJob: (a: {
    project: string;
    region: string;
    job: string;
    confirmText: string | null;
    force: boolean;
  }) => call<RunJobOutcome>("run_job", a),
  setSchedulePaused: (a: {
    project: string;
    region: string;
    schedulerJob: string;
    paused: boolean;
    confirmText: string | null;
  }) => call<string>("set_schedule_paused", a),

  // --- chi phí + recommendation ---
  costReport: (project: string, minutes?: number) =>
    call<CostReport>("cost_report", { project, minutes }),
  recommendations: (project: string) =>
    call<RecommendationsResult>("recommendations", { project }),
  markRecommendation: (a: {
    project: string;
    fullName: string;
    etag: string;
    action: MarkAction;
  }) => call<string>("mark_recommendation", a),
};
