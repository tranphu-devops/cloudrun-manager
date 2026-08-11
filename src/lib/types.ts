/**
 * Kiểu dữ liệu tương ứng 1-1 với struct trong Rust.
 *
 * Rust dùng `#[serde(rename_all = "camelCase")]` ở mọi DTO nên tên field khớp trực tiếp.
 * `Option<T>` ở Rust ra `T | null` (serde ghi `null`, không bỏ field), nên đừng dùng
 * `?:` cho những field đó — sẽ che mất trường hợp giá trị thật là null.
 *
 * File này phải được sửa cùng lúc với `crates/gcp/src/types.rs`. Nếu lệch, TypeScript
 * sẽ không báo gì (dữ liệu đến từ IPC là `any` ở ranh giới) nên đây là điểm cần cẩn thận.
 */

export type Health = "ready" | "notReady" | "reconciling" | "unknown";
export type EnvKind = "plain" | "secretRef";
export type EnvLabel = "dev" | "staging" | "prod" | "unknown";
/** Ngôn ngữ UI. Chỉ ảnh hưởng tầng React — message lỗi từ Rust vẫn là tiếng Việt. */
export type Language = "en" | "vi";
export type TokenSource = "serviceAccount" | "gcloudCli" | "adc";

export interface AuthInfo {
  account: string;
  source: TokenSource;
  impersonating: string | null;
  defaultProject: string | null;
  gcloudPath: string | null;
  usingServiceAccount: boolean;
}

export interface ProjectInfo {
  projectId: string;
  displayName: string;
  state: string;
}

export interface ServiceSummary {
  name: string;
  fullName: string;
  projectId: string;
  region: string;
  uri: string | null;
  health: Health;
  healthMessage: string | null;
  latestReadyRevision: string | null;
  latestCreatedRevision: string | null;
  image: string | null;
  minInstances: number | null;
  maxInstances: number | null;
  lastModifier: string | null;
  updateTime: string | null;
  /** Traffic bị ghim vào revision cụ thể → revision mới sẽ không nhận traffic. */
  trafficPinned: boolean;
  envCount: number;
  secretEnvCount: number;
  containerCount: number;
}

export interface EnvEntry {
  name: string;
  kind: EnvKind;
  value: string | null;
  secret: string | null;
  version: string | null;
}

export interface SecretVolumeMount {
  volumeName: string;
  secret: string;
  mountPath: string | null;
  items: string[];
}

export interface TrafficEntry {
  kind: "LATEST" | "REVISION";
  revision: string | null;
  percent: number;
  tag: string | null;
  uri: string | null;
}

export interface ConditionView {
  type: string;
  state: string;
  message: string | null;
  reason: string | null;
  lastTransitionTime: string | null;
}

export interface ContainerView {
  index: number;
  name: string | null;
  image: string | null;
  cpu: string | null;
  memory: string | null;
  cpuIdle: boolean | null;
  startupCpuBoost: boolean | null;
  port: number | null;
  env: EnvEntry[];
  command: string[];
  args: string[];
}

export interface ServiceDetail {
  summary: ServiceSummary;
  /** Bắt buộc gửi lại khi apply để chặn ghi đè thay đổi của người khác. */
  etag: string;
  description: string | null;
  serviceAccount: string | null;
  ingress: string | null;
  launchStage: string | null;
  executionEnvironment: string | null;
  concurrency: number | null;
  timeout: string | null;
  sessionAffinity: boolean | null;
  vpcEgress: string | null;
  vpcConnector: string | null;
  cloudsqlInstances: string[];
  containers: ContainerView[];
  secretVolumes: SecretVolumeMount[];
  traffic: TrafficEntry[];
  conditions: ConditionView[];
  labels: Record<string, string>;
  annotations: Record<string, string>;
  nextRevisionHint: string | null;
  /** JSON thô — chỉ để xem, không gửi ngược lại (Rust tự GET bản tươi khi apply). */
  raw: unknown;
}

export interface RevisionInfo {
  name: string;
  createTime: string | null;
  image: string | null;
  health: Health;
  healthMessage: string | null;
  minInstances: number | null;
  maxInstances: number | null;
  cpu: string | null;
  memory: string | null;
  concurrency: number | null;
  logUri: string | null;
  trafficPercent: number;
  isLatestReady: boolean;
}

export interface SecretInfo {
  name: string;
  createTime: string | null;
  labels: Record<string, string>;
  replication: string | null;
  usedBy: string[];
}

export interface SecretVersionInfo {
  version: string;
  state: string;
  createTime: string | null;
  destroyTime: string | null;
}

export interface LogEntry {
  insertId: string;
  timestamp: string;
  severity: string;
  revision: string | null;
  message: string;
  stream: "request" | "app";
  httpStatus: number | null;
  httpMethod: string | null;
  httpPath: string | null;
  latencyMs: number | null;
  raw: unknown;
}

export interface LogPage {
  entries: LogEntry[];
  nextPageToken: string | null;
}

export interface TimeSeriesPoint {
  t: number;
  v: number;
}

export interface SeriesData {
  label: string;
  points: TimeSeriesPoint[];
}

export interface ChartData {
  metric: string;
  unit: string;
  series: SeriesData[];
  /** Không lấy được dữ liệu (metric sai tên / thiếu quyền) — KHÁC với "có dữ liệu = 0". */
  unavailable: boolean;
  note: string | null;
}

export interface ServiceCharts {
  instances: ChartData;
  rps: ChartData;
  byClass: ChartData;
  latencyP50: ChartData;
  latencyP95: ChartData;
  latencyP99: ChartData;
  cpu: ChartData;
  memory: ChartData;
  startup: ChartData;
  alignmentSeconds: number;
  windowMinutes: number;
}

export interface ProjectLoadSnapshot {
  instances: Record<string, number>;
  rps: Record<string, number>;
  errorRate: Record<string, number>;
  missing: string[];
}

export interface ScalingUpdate {
  minInstances: number | null;
  maxInstances: number | null;
  cpu: string | null;
  memory: string | null;
  concurrency: number | null;
  timeout: string | null;
  cpuIdle: boolean | null;
  startupCpuBoost: boolean | null;
}

export type EnvChange =
  | { kind: "added"; name: string; value: string }
  | { kind: "removed"; name: string; value: string | null }
  | { kind: "changed"; name: string; before: string; after: string }
  | {
      kind: "secretVersionChanged";
      name: string;
      secret: string;
      before: string;
      after: string;
    };

export interface ApplyPreview {
  envChanges: EnvChange[];
  scalingChanges: string[];
  nextRevisionHint: string | null;
  trafficPinned: boolean;
  warnings: string[];
}

export interface PatchOutcome {
  operation: string | null;
  done: boolean;
  newRevision: string | null;
  message: string;
}

export interface ApplyResult {
  preview: ApplyPreview;
  outcome: PatchOutcome;
  newEtag: string | null;
  validatedOnly: boolean;
}

export interface Settings {
  readOnly: boolean;
  /** Project app được phép thao tác. Chặn ở tầng Rust, không chỉ ẩn dropdown. */
  allowedProjects: string[];
  projectLock: boolean;
  language: Language;
  projectLabels: Record<string, EnvLabel>;
  recentProjects: string[];
  currentProject: string | null;
  autoRefreshSeconds: number;
  logPollSeconds: number;
  revealTimeoutSeconds: number;
  metricsWindowMinutes: number;
}

export interface CapabilitiesResult {
  checked: boolean;
  note: string | null;
  canListServices: boolean;
  canReadService: boolean;
  canUpdateService: boolean;
  canReadMetrics: boolean;
  canReadLogs: boolean;
  canListSecrets: boolean;
  canRevealSecrets: boolean;
  missing: string[];
}

export interface ServiceListResult {
  services: ServiceSummary[];
  /** Dữ liệu này cũ bao nhiêu giây — hiện lên UI để không ai ra quyết định trên số cũ. */
  ageSeconds: number;
  regions: string[];
}

export interface MetricCheck {
  metric: string;
  exists: boolean;
  metricKind: string | null;
  valueType: string | null;
}

export interface RevealResult {
  value: string;
  looksBinary: boolean;
  byteLen: number;
  lineCount: number;
  hideAfterSeconds: number;
}

/** Lỗi từ Rust. `kind` là chuỗi ổn định để phân nhánh xử lý, không phải để hiển thị. */
export interface CmdError {
  message: string;
  detail: string | null;
  kind:
    | "auth"
    | "permission"
    | "conflict"
    | "readOnly"
    | "needsConfirm"
    | "network"
    | "invalid"
    | "notFound"
    | "rateLimit"
    | "projectLocked"
    | "vaultPassphrase"
    | "vaultMissing"
    | "vaultCorrupt"
    | "vaultLocked"
    | "jobRunning"
    | "other";
  status: number | null;
}

// ===========================================================================
// v2
// ===========================================================================

export type Severity = "high" | "warn" | "info";
export type BillingMode = "requestBased" | "instanceBased";
export type ExecStatus = "succeeded" | "failed" | "cancelled" | "running" | "unknown";
export type MarkAction = "dismissed" | "claimed" | "succeeded" | "failed";

export interface Finding {
  severity: Severity;
  /** Mã ổn định để nhóm/filter, không phải để hiển thị. */
  code: string;
  message: string;
  suggestion: string | null;
}

export interface EnvSecretFinding {
  severity: Severity;
  envName: string;
  reason: string;
  /** Tối đa 6 ký tự đầu — backend không bao giờ trả giá trị đầy đủ. */
  valueHint: string;
  valueLen: number;
}

export interface SchedulerJob {
  name: string;
  region: string;
  schedule: string;
  /** Cron không có timezone là thông tin sai — luôn hiện kèm. */
  timeZone: string;
  state: string;
  targetJob: string | null;
  lastAttemptTime: string | null;
}

export interface JobRow {
  name: string;
  region: string;
  image: string | null;
  /** Từ annotation `batch/source` — nhận diện tốt nhất khi tên job là `jobNNN`. */
  sourcePath: string | null;
  declaredSchedule: string | null;
  taskCount: number | null;
  parallelism: number | null;
  maxRetries: number | null;
  timeout: string | null;
  cpu: string | null;
  memory: string | null;
  serviceAccount: string | null;
  executionCount: number | null;
  lastExecution: string | null;
  lastExecutionStatus: ExecStatus;
  lastExecutionTime: string | null;
  health: Health;
  healthMessage: string | null;
  labels: Record<string, string>;
  schedulers: SchedulerJob[];
  runsPerDay: number | null;
  findings: Finding[];
  envSecrets: EnvSecretFinding[];
  envCount: number;
  secretEnvCount: number;
}

export interface JobsResult {
  jobs: JobRow[];
  /** Scheduler trỏ tới job không tồn tại — mỗi lần fire là một lỗi im lặng. */
  orphanSchedulers: SchedulerJob[];
  totalRunsPerDay: number;
  /** Cột cron trống vì THIẾU DỮ LIỆU, không phải vì job không có lịch. */
  schedulerUnavailable: boolean;
  schedulerNote: string | null;
  ageSeconds: number;
}

export interface RunJobOutcome {
  operation: string | null;
  execution: string | null;
  message: string;
}

export interface CostEstimate {
  mode: BillingMode;
  cpuCost: number;
  memoryCost: number;
  requestCost: number;
  total: number;
  vcpuSeconds: number;
  gibSeconds: number;
  /** Luôn true. Có trong payload để UI không thể quên đây là ước lượng. */
  estimated: boolean;
}

export interface CostRow {
  name: string;
  region: string;
  kind: "service" | "job";
  cpu: string | null;
  memory: string | null;
  mode: BillingMode;
  modeLabel: string;
  estimate: CostEstimate;
  perDay: number;
  rps: number;
  minInstances: number | null;
  /** Vì sao tốn — không chỉ con số. */
  drivers: string[];
  tier2Region: boolean;
}

export interface FreeTierOffset {
  cpuSecondsCovered: number;
  gibSecondsCovered: number;
  requestsCovered: number;
  maxSaving: number;
}

export interface CostReport {
  windowMinutes: number;
  rows: CostRow[];
  totalEstimate: number;
  totalPerDay: number;
  totalPerMonth: number;
  freeTier: FreeTierOffset;
  /** Bảy nguồn sai số — hiện thẳng trên UI, không cất trong doc. */
  errorSources: string[];
  warnings: string[];
  usageUnavailable: boolean;
  note: string | null;
}

export interface Recommendation {
  fullName: string;
  id: string;
  recommender: string;
  location: string;
  category: string;
  priority: string;
  description: string;
  state: string;
  /** Dấu âm = tiết kiệm. */
  monthlyCostImpact: number | null;
  targetResource: string | null;
  etag: string;
}

export interface RecommendationsResult {
  items: Recommendation[];
  apiDisabled: boolean;
  errors: string[];
}

export interface CredentialInfo {
  clientEmail: string;
  projectId: string | null;
  privateKeyId: string | null;
}

export interface VaultStatus {
  exists: boolean;
  unlocked: boolean;
  active: CredentialInfo | null;
  credentialCount: number;
  effectiveSource: "serviceAccount" | "gcloudCli" | "adc";
  vaultPath: string;
}

export interface ImportResult {
  credential: CredentialInfo;
  tokenOk: boolean;
  granted: string[] | null;
  missing: string[];
  warnings: string[];
}
