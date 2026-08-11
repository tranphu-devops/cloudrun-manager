/**
 * Mock tầng IPC để xem/sửa UI mà không cần GCP.
 *
 * Đây không phải file rác: nó cho phép làm UI, kiểm tra layout và bắt lỗi hiển thị
 * (nhãn đè nhau, tràn cột, chart trống) mà không phải đăng nhập gcloud và không đụng
 * vào project thật. `npm run preview:ui` dùng file này thay cho `@tauri-apps/api/core`.
 *
 * Dữ liệu bên dưới là hư cấu, chỉ để dựng UI — không lấy từ project thật nào.
 */

const REGION = "asia-northeast1";

/**
 * Tên hư cấu. Vài index được các hằng bên dưới nhắm tới để dựng sẵn trạng thái khó:
 * `FAILING_INDEX` (service không khởi động được), `PINNED_INDEXES` (traffic bị ghim),
 * `RECONCILING_INDEX`, `ERROR_RATE_INDEX`. Đổi độ dài mảng thì kiểm lại mấy hằng đó.
 */
const SERVICE_NAMES = [
  "api-gateway", "auth", "billing", "catalog", "inventory",
  "notifier", "reporting", "scheduler-api", "search", "web-frontend",
];

const FAILING_INDEX = 5;
const PINNED_INDEXES = [2, 9];
const RECONCILING_INDEX = 7;
const ERROR_RATE_INDEX = 3;

/** PRNG có seed: preview phải cho ra cùng một hình mỗi lần, nếu không thì không so được. */
function rng(seed: number) {
  let s = seed >>> 0 || 1;
  return () => {
    s ^= s << 13;
    s ^= s >>> 17;
    s ^= s << 5;
    s >>>= 0;
    return s / 4294967296;
  };
}

function rawService(name: string, i: number) {
  const r = rng(i + 7);
  const secretEnv =
    i % 4 === 0
      ? [
          {
            name: "DB_PASSWORD",
            valueSource: { secretKeyRef: { secret: `${name}-db-password`, version: "latest" } },
          },
          {
            name: "JWT_SIGNING_KEY",
            valueSource: {
              secretKeyRef: {
                secret: "projects/example-project/secrets/jwt-signing-key",
                version: "3",
              },
            },
          },
        ]
      : [];

  const failing = i === FAILING_INDEX;
  const pinned = PINNED_INDEXES.includes(i);

  return {
    name: `projects/example-project/locations/${REGION}/services/${name}`,
    uid: `uid-${i}`,
    generation: String(20 + i),
    labels: { team: i % 3 === 0 ? "platform" : "product", "managed-by": "terraform" },
    annotations: { "run.googleapis.com/ingress": "all" },
    createTime: "2025-02-11T03:00:00Z",
    updateTime: new Date(Date.now() - i * 3_600_000).toISOString(),
    lastModifier: i % 2 === 0 ? "you@example.com" : "deployer@example-project.iam.gserviceaccount.com",
    ingress: "INGRESS_TRAFFIC_ALL",
    launchStage: "GA",
    template: {
      revision: `${name}-000${20 + i}-abc`,
      scaling: { minInstanceCount: i % 5 === 0 ? 1 : 0, maxInstanceCount: 10 + (i % 4) * 10 },
      timeout: "300s",
      serviceAccount: `${name}-runtime@example-project.iam.gserviceaccount.com`,
      maxInstanceRequestConcurrency: 80,
      executionEnvironment: "EXECUTION_ENVIRONMENT_GEN2",
      vpcAccess: { connector: "projects/example-project/locations/asia-northeast1/connectors/vpc-conn", egress: "PRIVATE_RANGES_ONLY" },
      containers: [
        {
          name: "app",
          image: `asia-northeast1-docker.pkg.dev/example-project/svc/${name}:v1.${i % 9}.${i % 5}`,
          env: [
            { name: "LOG_LEVEL", value: i % 3 === 0 ? "debug" : "info" },
            { name: "NODE_ENV", value: "production" },
            { name: "FEATURE_FLAGS", value: "newUi,fastPath" },
            { name: "EMPTY_ON_PURPOSE" },
            ...secretEnv,
          ],
          resources: {
            limits: { cpu: i % 7 === 0 ? "2" : "1", memory: i % 7 === 0 ? "1Gi" : "512Mi" },
            cpuIdle: i % 11 !== 0,
            startupCpuBoost: i % 6 === 0,
          },
          ports: [{ name: "http1", containerPort: 8080 }],
          ...(i % 4 === 0
            ? { volumeMounts: [{ name: "tls-certs", mountPath: "/etc/certs" }] }
            : {}),
        },
      ],
      ...(i % 4 === 0
        ? {
            volumes: [
              {
                name: "tls-certs",
                secret: { secret: `${name}-tls`, items: [{ path: "tls.crt", version: "latest" }] },
              },
            ],
          }
        : {}),
    },
    traffic: pinned
      ? [{ type: "TRAFFIC_TARGET_ALLOCATION_TYPE_REVISION", revision: `${name}-000${19 + i}-zzz`, percent: 100 }]
      : [{ type: "TRAFFIC_TARGET_ALLOCATION_TYPE_LATEST", percent: 100 }],
    trafficStatuses: pinned
      ? [{ type: "TRAFFIC_TARGET_ALLOCATION_TYPE_REVISION", revision: `${name}-000${19 + i}-zzz`, percent: 100 }]
      : [{ type: "TRAFFIC_TARGET_ALLOCATION_TYPE_LATEST", percent: 100, uri: `https://${name}-a1b2c3d4e5-an.a.run.app` }],
    terminalCondition: failing
      ? { type: "Ready", state: "CONDITION_FAILED", message: "Revision không khởi động được: container failed to listen on PORT=8080" }
      : { type: "Ready", state: "CONDITION_SUCCEEDED" },
    conditions: [
      { type: "RoutesReady", state: "CONDITION_SUCCEEDED" },
      ...(failing
        ? [{ type: "ConfigurationsReady", state: "CONDITION_FAILED", reason: "HealthCheckContainerError", message: "Container không mở port 8080 trong thời gian startup probe" }]
        : []),
    ],
    latestReadyRevision: `projects/example-project/locations/${REGION}/revisions/${name}-000${20 + i}-abc`,
    latestCreatedRevision: `projects/example-project/locations/${REGION}/revisions/${name}-000${20 + i}-abc`,
    uri: `https://${name}-a1b2c3d4e5-an.a.run.app`,
    reconciling: i === RECONCILING_INDEX,
    etag: `"etag-${i}"`,
    _r: r(),
  };
}

const RAW = SERVICE_NAMES.map((n, i) => rawService(n, i));

// --- summarize (bản JS tương ứng phần Rust, chỉ dùng cho preview) ---------------

function shortName(n: string) {
  return n.split("/").pop() ?? n;
}

function parseEnv(c: Record<string, unknown>) {
  const arr = (c["env"] as Array<Record<string, unknown>>) ?? [];
  return arr.map((e) => {
    const vs = e["valueSource"] as { secretKeyRef?: { secret?: string; version?: string } } | undefined;
    if (vs?.secretKeyRef) {
      return {
        name: e["name"] as string,
        kind: "secretRef" as const,
        value: null,
        secret: shortName(vs.secretKeyRef.secret ?? ""),
        version: vs.secretKeyRef.version ?? "latest",
      };
    }
    return {
      name: e["name"] as string,
      kind: "plain" as const,
      value: (e["value"] as string) ?? "",
      secret: null,
      version: null,
    };
  });
}

function summarize(svc: ReturnType<typeof rawService>) {
  const tpl = svc.template;
  const c0 = tpl.containers[0]!;
  const env = parseEnv(c0 as unknown as Record<string, unknown>);
  const latestPct = svc.traffic
    .filter((t) => t.type.endsWith("_LATEST"))
    .reduce((a, t) => a + t.percent, 0);

  return {
    name: shortName(svc.name),
    fullName: svc.name,
    projectId: "example-project",
    region: REGION,
    uri: svc.uri,
    health:
      svc.terminalCondition.state === "CONDITION_FAILED"
        ? "notReady"
        : svc.reconciling
          ? "reconciling"
          : "ready",
    healthMessage: svc.terminalCondition.message ?? null,
    latestReadyRevision: shortName(svc.latestReadyRevision),
    latestCreatedRevision: shortName(svc.latestCreatedRevision),
    image: c0.image,
    minInstances: tpl.scaling.minInstanceCount,
    maxInstances: tpl.scaling.maxInstanceCount,
    lastModifier: svc.lastModifier,
    updateTime: svc.updateTime,
    trafficPinned: latestPct < 100,
    envCount: env.length,
    secretEnvCount: env.filter((e) => e.kind === "secretRef").length,
    containerCount: tpl.containers.length,
  };
}

function detail(svc: ReturnType<typeof rawService>) {
  const tpl = svc.template;
  const vols = (tpl as { volumes?: Array<{ name: string; secret: { secret: string; items: Array<{ path: string; version: string }> } }> }).volumes ?? [];

  return {
    summary: summarize(svc),
    etag: svc.etag,
    description: null,
    serviceAccount: tpl.serviceAccount,
    ingress: svc.ingress,
    launchStage: svc.launchStage,
    executionEnvironment: tpl.executionEnvironment,
    concurrency: tpl.maxInstanceRequestConcurrency,
    timeout: tpl.timeout,
    sessionAffinity: null,
    vpcEgress: tpl.vpcAccess.egress,
    vpcConnector: shortName(tpl.vpcAccess.connector),
    cloudsqlInstances: [],
    containers: tpl.containers.map((c, index) => ({
      index,
      name: c.name,
      image: c.image,
      cpu: c.resources.limits.cpu,
      memory: c.resources.limits.memory,
      cpuIdle: c.resources.cpuIdle,
      startupCpuBoost: c.resources.startupCpuBoost,
      port: c.ports?.[0]?.containerPort ?? null,
      env: parseEnv(c as unknown as Record<string, unknown>),
      command: [],
      args: [],
    })),
    secretVolumes: vols.map((v) => ({
      volumeName: v.name,
      secret: v.secret.secret,
      mountPath: "/etc/certs",
      items: v.secret.items.map((it) => `${it.path} → v${it.version}`),
    })),
    traffic: svc.trafficStatuses.map((t) => ({
      kind: t.type.endsWith("_REVISION") ? "REVISION" : "LATEST",
      revision: t.revision ? shortName(t.revision) : null,
      percent: t.percent,
      tag: null,
      uri: (t as { uri?: string }).uri ?? null,
    })),
    conditions: [svc.terminalCondition, ...svc.conditions].map((c) => ({
      type: c.type,
      state: c.state,
      message: (c as { message?: string }).message ?? null,
      reason: (c as { reason?: string }).reason ?? null,
      lastTransitionTime: null,
    })),
    labels: svc.labels,
    annotations: svc.annotations,
    nextRevisionHint: `${shortName(svc.name)}-000${21 + SERVICE_NAMES.indexOf(shortName(svc.name))}-xxx`,
    raw: svc,
  };
}

// --- metric ---------------------------------------------------------------------

function points(n: number, seed: number, base: number, amp: number, spike = false) {
  const r = rng(seed);
  const now = Date.now();
  const out: Array<{ t: number; v: number }> = [];
  for (let i = n - 1; i >= 0; i--) {
    const wave = Math.sin((n - i) / 6) * amp * 0.5;
    const noise = (r() - 0.5) * amp * 0.4;
    const bump = spike && i > n * 0.3 && i < n * 0.4 ? amp * 2.2 : 0;
    out.push({ t: now - i * 60_000, v: Math.max(0, base + wave + noise + bump) });
  }
  return out;
}

function chart(metric: string, unit: string, series: Array<{ label: string; points: Array<{ t: number; v: number }> }>) {
  return { metric, unit, series, unavailable: false, note: null };
}

// --- bảng lệnh ------------------------------------------------------------------

const settings = {
  readOnly: true,
  allowedProjects: ["example-project"],
  projectLock: true,
  // Cùng mặc định với `Settings::default()` bên Rust.
  language: "en" as "en" | "vi" | "ja",
  projectLabels: { "example-prod": "prod", "example-staging": "staging" } as Record<string, string>,
  recentProjects: ["example-project"],
  currentProject: "example-project",
  autoRefreshSeconds: 0,
  logPollSeconds: 3,
  revealTimeoutSeconds: 30,
  metricsWindowMinutes: 60,
};

const N = 60;

const HANDLERS: Record<string, (a: Record<string, unknown>) => unknown> = {
  auth_info: () => ({
    account: "you@example.com",
    source: "gcloudCli",
    impersonating: null,
    defaultProject: "example-project",
    gcloudPath: "C:\\Users\\you\\AppData\\Local\\Google\\Cloud SDK\\google-cloud-sdk\\bin\\gcloud.cmd",
    usingServiceAccount: false,
  }),

  list_projects: () =>
    [
      ["example-project", "example-project"],
      ["example-prod", "example-prod"],
      ["example-staging", "example-staging"],
      ["example-develop", "example-develop"],
      ["example-develop-vn", "example-develop-vn"],
      ["example-demo", "example-demo"],
      ["example-sandbox", "example-sandbox"],
    ].map(([projectId, displayName]) => ({ projectId, displayName, state: "ACTIVE" })),

  get_settings: () => settings,
  select_project: () => settings,
  set_read_only: (a) => {
    settings.readOnly = Boolean(a["value"]);
    return settings;
  },
  set_project_label: (a) => {
    settings.projectLabels[String(a["project"])] = String(a["label"]);
    return settings;
  },
  set_preferences: (a) => {
    // Chỉ `language` cần phản hồi thật: đây là cách duy nhất đổi ngôn ngữ trong preview.
    if (a["language"] === "en" || a["language"] === "vi" || a["language"] === "ja") {
      settings.language = a["language"];
    }
    return settings;
  },
  clear_cache: () => null,
  audit_path: () => "C:\\Users\\you\\AppData\\Roaming\\dev.cloudrun.cockpit\\audit.jsonl",
  audit_tail: () => [
    {
      ts: new Date().toISOString(),
      account: "you@example.com",
      effectiveIdentity: "you@example.com",
      project: "example-project",
      envLabel: "dev",
      service: "api-gateway",
      action: "updateEnv",
      changes: ["LOG_LEVEL: info → debug"],
      outcome: "ok",
      message: "Đã tạo và triển khai xong revision api-gateway-00042-abc.",
    },
  ],

  check_permissions: () => ({
    checked: true,
    note: null,
    canListServices: true,
    canReadService: true,
    canUpdateService: true,
    canReadMetrics: true,
    canReadLogs: true,
    canListSecrets: true,
    canRevealSecrets: false,
    missing: [
      "Xem giá trị secret — cần roles/secretmanager.secretAccessor (có thể cố tình không cấp trên prod)",
    ],
  }),

  list_services: () => ({
    services: RAW.map(summarize),
    ageSeconds: 4,
    regions: [REGION],
  }),
  refresh_project: () => HANDLERS["list_services"]!({}),

  get_service: (a) => {
    const svc = RAW.find((s) => shortName(s.name) === a["service"]) ?? RAW[0]!;
    return detail(svc);
  },

  list_revisions: (a) => {
    const name = String(a["service"]);
    const i = SERVICE_NAMES.indexOf(name);
    return [0, 1, 2, 3].map((k) => ({
      name: `${name}-000${20 + i - k}-${["abc", "def", "ghi", "jkl"][k]}`,
      createTime: new Date(Date.now() - k * 86_400_000).toISOString(),
      image: `asia-northeast1-docker.pkg.dev/example-project/svc/${name}:v1.${(i - k + 9) % 9}.0`,
      health: k === 0 ? "ready" : "ready",
      healthMessage: null,
      minInstances: 0,
      maxInstances: 10,
      cpu: "1",
      memory: "512Mi",
      concurrency: 80,
      logUri: null,
      trafficPercent: k === 0 ? 100 : 0,
      isLatestReady: k === 0,
    }));
  },

  project_load: () => ({
    instances: Object.fromEntries(RAW.map((s, i) => [shortName(s.name), i % 9 === 0 ? 0 : (i % 6) + 1])),
    rps: Object.fromEntries(RAW.map((s, i) => [shortName(s.name), Number((((i * 37) % 91) / 2.3).toFixed(2))])),
    errorRate: Object.fromEntries(
      RAW.map((s, i) => [
        shortName(s.name),
        i === FAILING_INDEX ? 0.132 : i === ERROR_RATE_INDEX ? 0.021 : 0,
      ]),
    ),
    missing: [],
  }),

  service_charts: () => ({
    instances: chart("run.googleapis.com/container/instance_count", "instance", [
      { label: "active", points: points(N, 1, 3, 2) },
      { label: "idle", points: points(N, 2, 1.2, 1) },
    ]),
    rps: chart("run.googleapis.com/request_count", "req/s", [
      { label: "value", points: points(N, 3, 42, 18) },
    ]),
    byClass: chart("run.googleapis.com/request_count", "req/s", [
      { label: "2xx", points: points(N, 4, 38, 12) },
      { label: "3xx", points: points(N, 5, 4, 2) },
      { label: "4xx", points: points(N, 6, 2.5, 1.5) },
      { label: "5xx", points: points(N, 7, 0.4, 0.5, true) },
    ]),
    latencyP50: chart("run.googleapis.com/request_latencies", "ms", [
      { label: "value", points: points(N, 8, 42, 12) },
    ]),
    latencyP95: chart("run.googleapis.com/request_latencies", "ms", [
      { label: "value", points: points(N, 9, 180, 60) },
    ]),
    latencyP99: chart("run.googleapis.com/request_latencies", "ms", [
      { label: "value", points: points(N, 10, 420, 180, true) },
    ]),
    cpu: chart("run.googleapis.com/container/cpu/utilizations", "%", [
      { label: "value", points: points(N, 11, 38, 14) },
    ]),
    memory: chart("run.googleapis.com/container/memory/utilizations", "%", [
      { label: "value", points: points(N, 12, 61, 8) },
    ]),
    // Cố ý để một chart ở trạng thái "không lấy được" — để kiểm tra rằng UI nói rõ
    // điều đó thay vì vẽ một đường phẳng ở 0.
    startup: {
      metric: "run.googleapis.com/container/startup_latencies",
      unit: "ms",
      series: [],
      unavailable: true,
      note: "Không lấy được dữ liệu: Không đủ quyền khi lấy metric (thiếu roles/monitoring.viewer).",
    },
    alignmentSeconds: 60,
    windowMinutes: 60,
  }),

  fetch_logs: () => ({
    entries: Array.from({ length: 45 }, (_, k) => {
      const isReq = k % 3 !== 0;
      const bad = k === 4 || k === 19;
      return {
        insertId: `ins-${k}`,
        timestamp: new Date(Date.now() - k * 4200).toISOString(),
        severity: bad ? "ERROR" : k % 7 === 0 ? "WARNING" : "INFO",
        revision: "api-gateway-00042-abc",
        message: isReq
          ? `${["GET", "POST", "PUT"][k % 3]} /api/v1/${["users", "orders", "items", "reports"][k % 4]}?page=${k} → ${bad ? 503 : 200}`
          : bad
            ? "connect ETIMEDOUT 10.0.0.10:5432 — không kết nối được database, sẽ thử lại sau 2s"
            : `handled request id=${k} in ${(k * 3.4).toFixed(1)}ms`,
        stream: isReq ? "request" : "app",
        httpStatus: isReq ? (bad ? 503 : 200) : null,
        httpMethod: isReq ? ["GET", "POST", "PUT"][k % 3] : null,
        httpPath: isReq ? `/api/v1/users?page=${k}` : null,
        latencyMs: isReq ? 12 + (k % 30) * 4.5 : null,
        raw: { insertId: `ins-${k}`, severity: bad ? "ERROR" : "INFO", note: "payload gốc từ Cloud Logging" },
      };
    }),
    nextPageToken: "next-page",
  }),

  log_explorer_url: () => "https://console.cloud.google.com/logs/query",

  list_secrets: () =>
    [
      "api-gateway-db-password", "jwt-signing-key", "api-gateway-tls", "mailer-api-key",
      "payment-secret", "search-password", "push-server-key", "secret-khong-ai-dung",
    ].map((name, i) => ({
      name,
      createTime: new Date(Date.now() - i * 30 * 86_400_000).toISOString(),
      labels: {},
      replication: "automatic",
      usedBy: name === "secret-khong-ai-dung" ? [] : ["api-gateway", "billing"].slice(0, (i % 2) + 1),
    })),

  list_secret_versions: () =>
    [3, 2, 1].map((v) => ({
      version: String(v),
      state: v === 1 ? "DISABLED" : "ENABLED",
      createTime: new Date(Date.now() - v * 10 * 86_400_000).toISOString(),
      destroyTime: null,
    })),

  reveal_secret: () => ({
    value: "postgres://appuser:MẬT-KHẨU-GIẢ-ĐỂ-XEM-UI@10.0.0.10:5432/appdb?sslmode=require",
    looksBinary: false,
    byteLen: 74,
    lineCount: 1,
    hideAfterSeconds: 30,
  }),

  verify_metrics: () => [],

  preview_env: () => ({
    envChanges: [{ kind: "changed", name: "LOG_LEVEL", before: "info", after: "debug" }],
    scalingChanges: [],
    nextRevisionHint: "api-gateway-00043-xxx",
    trafficPinned: false,
    warnings: [],
  }),
  preview_scaling: () => ({
    envChanges: [],
    scalingChanges: ["Min instances: 0 → 2"],
    nextRevisionHint: "api-gateway-00043-xxx",
    trafficPinned: false,
    warnings: [],
  }),
  apply_env: () => {
    throw {
      message: "Đang ở chế độ Read-only. Tắt Read-only ở góc trên phải nếu bạn thực sự muốn ghi.",
      detail: null,
      kind: "readOnly",
      status: null,
    };
  },
  apply_scaling: () => HANDLERS["apply_env"]!({}),

  // === v2 ====================================================================

  vault_status: () => vaultState,
  unlock_vault: (a) => {
    if (String(a["passphrase"]) !== "demo") {
      throw {
        message: "Passphrase không đúng. Nhập lại — thử \"demo\" trong bản preview.",
        detail: null,
        kind: "vaultPassphrase",
        status: null,
      };
    }
    vaultState = { ...vaultState, unlocked: true };
    return vaultState;
  },
  lock_vault: () => {
    vaultState = { ...vaultState, unlocked: false };
    return vaultState;
  },
  import_service_account: (a) => {
    const email = "cloud-run-cockpit@example-project.iam.gserviceaccount.com";
    vaultState = {
      exists: true,
      unlocked: true,
      active: { clientEmail: email, projectId: "example-project", privateKeyId: "a1b2c3d4e5f6" },
      credentialCount: (vaultState.credentialCount || 0) + 1,
      effectiveSource: "serviceAccount",
      vaultPath: vaultState.vaultPath,
    };
    void a;
    return {
      credential: vaultState.active,
      tokenOk: true,
      granted: ["run.services.list", "run.jobs.run", "monitoring.timeSeries.list"],
      missing: [],
      warnings: [],
    };
  },
  remove_credential: () => {
    const count = Math.max(0, (vaultState.credentialCount || 1) - 1);
    vaultState = {
      ...vaultState,
      credentialCount: count,
      exists: count > 0,
      active: count > 0 ? vaultState.active : null,
      effectiveSource: count > 0 ? "serviceAccount" : "gcloudCli",
      unlocked: count > 0 ? vaultState.unlocked : false,
    };
    return vaultState;
  },
  set_allowed_projects: (a) => {
    settings.allowedProjects = (a["projects"] as string[]) ?? settings.allowedProjects;
    settings.projectLock = Boolean(a["lock"]);
    return settings;
  },

  jobs_overview: () => jobsResult(),
  refresh_jobs: () => jobsResult(),
  get_job: (a) => JOBS_RAW.find((j) => shortName(j.name) === a["job"]) ?? JOBS_RAW[0],
  run_job: (a) => {
    const job = String(a["job"]);
    const row = JOBS.find((j) => j.name === job);
    if (row && row.lastExecutionStatus === "running" && !a["force"]) {
      throw {
        message: `Job \`${job}\` đang có execution chạy dở. Bật "Chạy dù đang có execution" rồi thử lại nếu chắc chắn an toàn.`,
        detail: null,
        kind: "jobRunning",
        status: null,
      };
    }
    return { operation: `operations/run-${job}-001`, execution: `${job}-xk29d`, message: `Đã tạo execution ${job}-xk29d.` };
  },
  set_schedule_paused: (a) => {
    const paused = Boolean(a["paused"]);
    return paused ? `Đã tạm dừng lịch ${a["schedulerJob"]}.` : `Đã bật lại lịch ${a["schedulerJob"]}.`;
  },

  cost_report: () => costReport(),
  recommendations: () => ({ items: RECOMMENDATIONS, apiDisabled: false, errors: [] }),
  mark_recommendation: (a) => {
    const r = RECOMMENDATIONS.find((x) => x.fullName === a["fullName"]);
    if (r) r.state = String(a["action"]).toUpperCase();
    return `Đã đánh dấu ${a["action"]}.`;
  },
};

// --- state + dữ liệu v2 ---------------------------------------------------------

let vaultState = {
  exists: false,
  unlocked: false,
  active: null as { clientEmail: string; projectId: string | null; privateKeyId: string | null } | null,
  credentialCount: 0,
  effectiveSource: "gcloudCli" as "serviceAccount" | "gcloudCli" | "adc",
  vaultPath: "C:\\Users\\you\\AppData\\Roaming\\dev.cloudrun.cockpit\\credentials.vault",
};

// --- Jobs -----------------------------------------------------------------------

type MockScheduler = {
  name: string;
  region: string;
  schedule: string;
  timeZone: string;
  state: string;
  targetJob: string | null;
  lastAttemptTime: string | null;
};

const JOB_COUNT = 140;

/** Giống thực tế: job đặt tên jobNNN, dùng chung một image, không có args phân biệt. */
function mkJob(i: number) {
  const name = `job${String(i + 1).padStart(3, "0")}`;
  const r = rng(i + 101);
  const roll = r();

  // Đa số có 1 scheduler cron bình thường; một số ít bị lỗi cron; một số không có lịch.
  const schedulers: MockScheduler[] = [];
  const findings: Array<{ severity: string; code: string; message: string; suggestion: string | null }> = [];

  const noSchedule = i % 13 === 0;
  const minuteWildcard = i === 3 || i === 47 || i === 91; // cron.minuteWildcard — chạy 60 lần/giờ
  const everyMinute = i === 22;
  const paused = i === 8 || i === 60;

  if (!noSchedule) {
    const schedule = minuteWildcard
      ? "* 3 * * *"
      : everyMinute
        ? "* * * * *"
        : `${(i * 7) % 60} ${(i % 6) + 1} * * *`;
    schedulers.push({
      name: `sched-${name}`,
      region: REGION,
      schedule,
      timeZone: i === 70 ? "UTC" : "Asia/Tokyo",
      state: paused ? "PAUSED" : "ENABLED",
      targetJob: name,
      lastAttemptTime: new Date(Date.now() - (i % 24) * 3_600_000).toISOString(),
    });
  }

  if (minuteWildcard) {
    findings.push({
      severity: "high",
      code: "cron.minuteWildcard",
      message:
        "Trường phút để trống (dùng `*`) nên job chạy mỗi phút trong khung giờ đó — 60 lần/giờ thay vì 1 lần. Gần như chắc chắn là nhầm.",
      suggestion: "0 3 * * *",
    });
  }
  if (everyMinute) {
    findings.push({
      severity: "high",
      code: "cron.everyMinute",
      message: "Lịch `* * * * *` chạy mỗi phút, 1440 lần/ngày. Kiểm tra lại xem có đúng ý không.",
      suggestion: null,
    });
  }
  if (i === 70) {
    findings.push({
      severity: "warn",
      code: "cron.mixedTimezone",
      message: "Scheduler dùng UTC trong khi phần lớn job khác dùng Asia/Tokyo — dễ nhầm giờ chạy.",
      suggestion: null,
    });
  }
  if (noSchedule) {
    findings.push({
      severity: "warn",
      code: "cron.noSchedule",
      message: "Job không có Cloud Scheduler nào trỏ tới — sẽ không tự chạy, chỉ chạy khi gọi tay.",
      suggestion: null,
    });
  }

  const envSecrets =
    i === 33
      ? [
          {
            severity: "high",
            envName: "STRIPE_SECRET_KEY",
            reason: "Giá trị bắt đầu bằng `sk_live_` — khoá bí mật Stripe để dạng plain trong cấu hình job.",
            valueHint: "sk_liv…",
            valueLen: 107,
          },
          {
            severity: "high",
            envName: "SENDGRID_API_KEY",
            reason: "Giá trị bắt đầu bằng `SG.` — API key SendGrid để dạng plain.",
            valueHint: "SG.x9K…",
            valueLen: 69,
          },
        ]
      : [];

  const runsPerDay = noSchedule ? null : minuteWildcard ? 1140 : everyMinute ? 1440 : 1;
  const statuses = ["succeeded", "succeeded", "succeeded", "failed", "succeeded", "running"] as const;
  const lastExecutionStatus = i === 12 ? "running" : i % 9 === 4 ? "failed" : statuses[i % 5];

  return {
    name,
    region: REGION,
    image: "asia-northeast1-docker.pkg.dev/example-project/batch/runner:v2.4.1",
    sourcePath: `deployments/cloud-run/base/${name}.yaml`,
    declaredSchedule: noSchedule ? null : schedulers[0]?.schedule ?? null,
    taskCount: 1,
    parallelism: 1,
    maxRetries: 3,
    timeout: "600s",
    cpu: i % 5 === 0 ? "2" : "1",
    memory: i % 5 === 0 ? "2Gi" : "512Mi",
    serviceAccount: "batch-runner@example-project.iam.gserviceaccount.com",
    executionCount: Math.floor(roll * 4000) + 10,
    lastExecution: `${name}-${Math.floor(roll * 90000).toString(36)}`,
    lastExecutionStatus,
    lastExecutionTime: new Date(Date.now() - (i % 48) * 1_800_000).toISOString(),
    health: lastExecutionStatus === "failed" ? "notReady" : "ready",
    healthMessage: lastExecutionStatus === "failed" ? "Execution cuối kết thúc với mã khác 0" : null,
    labels: { "managed-by": "terraform", team: i % 2 === 0 ? "platform" : "data" },
    schedulers,
    runsPerDay,
    findings,
    envSecrets,
    envCount: 6 + (i % 4),
    secretEnvCount: i === 33 ? 0 : 2,
  };
}

const JOBS = Array.from({ length: JOB_COUNT }, (_, i) => mkJob(i));

/** Raw để get_job trả về (dùng lại cấu trúc job double-nest thật). */
const JOBS_RAW = JOBS.slice(0, 3).map((j) => ({
  name: `projects/example-project/locations/${REGION}/jobs/${j.name}`,
  template: { template: { template: { containers: [{ image: j.image, env: [] }] } } },
}));

function jobsResult() {
  const totalRunsPerDay = JOBS.reduce((a, j) => a + (j.runsPerDay ?? 0), 0);
  return {
    jobs: JOBS,
    orphanSchedulers: [
      {
        name: "sched-job999-legacy",
        region: REGION,
        schedule: "0 2 * * *",
        timeZone: "Asia/Tokyo",
        state: "ENABLED",
        targetJob: "job999",
        lastAttemptTime: new Date(Date.now() - 3_600_000).toISOString(),
      },
      {
        name: "sched-old-report",
        region: REGION,
        schedule: "30 6 * * 1",
        timeZone: "Asia/Tokyo",
        state: "ENABLED",
        targetJob: "weekly-report-old",
        lastAttemptTime: new Date(Date.now() - 7 * 3_600_000).toISOString(),
      },
    ],
    totalRunsPerDay,
    schedulerUnavailable: false,
    schedulerNote: null,
    ageSeconds: 6,
  };
}

// --- Cost ----------------------------------------------------------------------

function costRow(name: string, i: number, kind: "service" | "job") {
  const r = rng(i + 555);
  const instanceBased = i % 11 === 0; // trùng với cpuIdle=false ở service mock
  const mode = instanceBased ? "instanceBased" : "requestBased";
  const cpu = i % 7 === 0 ? "2" : "1";
  const memory = i % 7 === 0 ? "1Gi" : "512Mi";
  const rps = kind === "service" ? Number(((i * 37) % 91) / 2.3) : 0;
  const vcpuSeconds = kind === "service" ? rps * 86400 * (instanceBased ? 1 : 0.35) : 60 * (JOBS[i]?.runsPerDay ?? 1);
  const gibSeconds = vcpuSeconds * 0.5;
  const cpuRate = instanceBased ? 0.000018 : 0.000024;
  const cpuCost = vcpuSeconds * cpuRate;
  const memoryCost = gibSeconds * 0.0000025;
  const requestCost = kind === "service" ? (rps * 86400 * 0.0000004) : 0;
  const total = cpuCost + memoryCost + requestCost;

  const drivers: string[] = [];
  if (instanceBased) drivers.push("Instance-based: tính CPU cả khi rảnh (đơn giá cao ~10 lần)");
  if ((JOBS[i]?.runsPerDay ?? 0) > 500 && kind === "job") drivers.push("Chạy rất nhiều lần/ngày");
  if (rps > 30) drivers.push("Tải request cao");
  if (i % 5 === 0 && kind === "service") drivers.push("min-instances > 0: luôn có instance chạy");

  void r;
  return {
    name,
    region: REGION,
    kind,
    cpu,
    memory,
    mode,
    modeLabel: instanceBased ? "Instance-based" : "Request-based",
    estimate: {
      mode,
      cpuCost,
      memoryCost,
      requestCost,
      total,
      vcpuSeconds,
      gibSeconds,
      estimated: true,
    },
    perDay: total,
    rps,
    minInstances: i % 5 === 0 ? 1 : 0,
    drivers,
    tier2Region: false,
  };
}

function costReport() {
  const serviceRows = SERVICE_NAMES.map((n, i) => costRow(n, i, "service"));
  const jobRows = JOBS.filter((j) => (j.runsPerDay ?? 0) > 0)
    .slice(0, 12)
    .map((j, i) => costRow(j.name, i, "job"));
  const rows = [...serviceRows, ...jobRows];
  const totalPerDay = rows.reduce((a, r) => a + r.perDay, 0);
  return {
    windowMinutes: 1440,
    rows,
    totalEstimate: totalPerDay,
    totalPerDay,
    totalPerMonth: totalPerDay * 30,
    freeTier: {
      cpuSecondsCovered: 180000,
      gibSecondsCovered: 360000,
      requestsCovered: 2_000_000,
      maxSaving: 4.32,
    },
    errorSources: [
      "Metric tải lấy theo cửa sổ ngắn rồi ngoại suy — tải đột biến ngoài cửa sổ không được tính.",
      "Đơn giá là bảng giá công khai theo region, chưa gồm committed-use discount hay hợp đồng riêng.",
      "Không tính network egress, load balancer, hay chi phí service phụ thuộc (Cloud SQL, Secret Manager…).",
      "Cloud Run Jobs ước lượng theo số lần chạy × thời lượng tối thiểu 60s — job chạy lâu hơn sẽ bị tính thiếu.",
      "Không phân biệt được chính xác vCPU-giây active vs idle ở mọi thời điểm — dùng tỉ lệ ước lượng.",
      "Startup CPU boost và các đợt scale-out ngắn không phản ánh đủ trong metric alignment 60s.",
      "Service có GPU đính kèm bị tính thiếu đáng kể — bảng đơn giá ở đây không bao gồm đơn giá GPU.",
    ],
    warnings: [
      "Bản preview chỉ đưa 12 job đầu vào ước lượng cho nhẹ. Bản thật tính tất cả.",
    ],
    usageUnavailable: false,
    note: null,
  };
}

// --- Recommendations ------------------------------------------------------------

const RECOMMENDATIONS = [
  {
    fullName: "projects/example-project/locations/asia-northeast1/recommenders/google.run.service.CostRecommender/recommendations/rec-001",
    id: "rec-001",
    recommender: "google.run.service.CostRecommender",
    location: "asia-northeast1",
    category: "COST",
    priority: "P2",
    description: "Service `search` cấp 2 vCPU nhưng dùng trung bình dưới 15% — hạ xuống 1 vCPU để giảm chi phí mà không ảnh hưởng tải hiện tại.",
    state: "ACTIVE",
    monthlyCostImpact: -38.4,
    targetResource: "//run.googleapis.com/projects/example-project/locations/asia-northeast1/services/search",
    etag: '"rec-etag-1"',
  },
  {
    fullName: "projects/example-project/locations/asia-northeast1/recommenders/google.run.service.CostRecommender/recommendations/rec-002",
    id: "rec-002",
    recommender: "google.run.service.CostRecommender",
    location: "asia-northeast1",
    category: "COST",
    priority: "P3",
    description: "Service `reporting` để instance-based (cpuIdle=false) nhưng tải rời rạc — chuyển sang request-based để chỉ tính CPU khi có request.",
    state: "ACTIVE",
    monthlyCostImpact: -21.7,
    targetResource: "//run.googleapis.com/projects/example-project/locations/asia-northeast1/services/reporting",
    etag: '"rec-etag-2"',
  },
  {
    fullName: "projects/example-project/locations/global/recommenders/google.iam.policy.Recommender/recommendations/rec-003",
    id: "rec-003",
    recommender: "google.iam.policy.Recommender",
    location: "global",
    category: "SECURITY",
    priority: "P1",
    description: "Service account `batch-runner@` có role Editor trên toàn project nhưng chỉ dùng quyền Cloud Run + Storage — thu hẹp về đúng role cần thiết.",
    state: "ACTIVE",
    monthlyCostImpact: null,
    targetResource: "//iam.googleapis.com/projects/example-project/serviceAccounts/batch-runner@example-project.iam.gserviceaccount.com",
    etag: '"rec-etag-3"',
  },
  {
    fullName: "projects/example-project/locations/global/recommenders/google.iam.policy.Recommender/recommendations/rec-004",
    id: "rec-004",
    recommender: "google.iam.policy.Recommender",
    location: "global",
    category: "SECURITY",
    priority: "P2",
    description: "Tài khoản `old-deployer@` không hoạt động 120 ngày nhưng vẫn giữ quyền deploy — cân nhắc thu hồi.",
    state: "ACTIVE",
    monthlyCostImpact: null,
    targetResource: "//iam.googleapis.com/projects/example-project/serviceAccounts/old-deployer@example-project.iam.gserviceaccount.com",
    etag: '"rec-etag-4"',
  },
  {
    fullName: "projects/example-project/locations/asia-northeast1/recommenders/google.run.service.CostRecommender/recommendations/rec-005",
    id: "rec-005",
    recommender: "google.run.service.CostRecommender",
    location: "asia-northeast1",
    category: "PERFORMANCE",
    priority: "P3",
    description: "Service `api-gateway` thường chạm trần max-instances vào giờ cao điểm — cân nhắc nâng trần để tránh 429.",
    state: "ACTIVE",
    monthlyCostImpact: 12.5,
    targetResource: "//run.googleapis.com/projects/example-project/locations/asia-northeast1/services/api-gateway",
    etag: '"rec-etag-5"',
  },
] as Array<{
  fullName: string;
  id: string;
  recommender: string;
  location: string;
  category: string;
  priority: string;
  description: string;
  state: string;
  monthlyCostImpact: number | null;
  targetResource: string | null;
  etag: string;
}>;

export async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const h = HANDLERS[cmd];
  if (!h) throw `[preview] chưa mock command \`${cmd}\``;
  // Trễ nhẹ để thấy được trạng thái loading.
  await new Promise((r) => setTimeout(r, 60));
  return h(args ?? {}) as T;
}
