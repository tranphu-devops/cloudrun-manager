import { useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { Badge, Button, Dialog, ErrorBox, Input, Loading, Notice, Select, useToast } from "../../components/ui";
import { StatTile } from "../../components/charts";
import { ago, agoSeconds, dateTime, num } from "../../lib/format";
import { apiV2, asCmdError } from "../../lib/ipc";
import type { CmdError, ExecStatus, Finding, JobRow, JobsResult, Severity } from "../../lib/types";

const SEV_TONE: Record<Severity, "critical" | "warning" | "info"> = {
  high: "critical",
  warn: "warning",
  info: "info",
};
const SEV_ICON: Record<Severity, string> = { high: "✕", warn: "⚠", info: "ℹ" };

const EXEC_TONE: Record<ExecStatus, { tone: "good" | "critical" | "warning" | "neutral"; icon: string; text: string }> = {
  succeeded: { tone: "good", icon: "✓", text: "thành công" },
  failed: { tone: "critical", icon: "✕", text: "thất bại" },
  cancelled: { tone: "warning", icon: "⊘", text: "bị huỷ" },
  running: { tone: "warning", icon: "◐", text: "đang chạy" },
  unknown: { tone: "neutral", icon: "○", text: "chưa chạy" },
};

function worst(findings: Finding[]): Severity | null {
  if (findings.some((f) => f.severity === "high")) return "high";
  if (findings.some((f) => f.severity === "warn")) return "warn";
  if (findings.some((f) => f.severity === "info")) return "info";
  return null;
}

/**
 * Màn Jobs.
 *
 * Ràng buộc trung tâm: 196 job tên `job001`…`job233`, dùng chung một image, không có args.
 * Tên và image đều không phân biệt được job nào là job nào — nên grid ưu tiên **cron +
 * timezone + đường dẫn source + trạng thái lần chạy cuối**, và có ô tìm kiếm chạy trên cả
 * source path.
 */
export function JobsPage({
  project,
  readOnly,
  requiresTypedConfirm,
}: {
  project: string;
  readOnly: boolean;
  requiresTypedConfirm: boolean;
}) {
  const qc = useQueryClient();
  const toast = useToast();
  const [filter, setFilter] = useState("");
  const [onlyIssues, setOnlyIssues] = useState(false);
  const [sortBy, setSortBy] = useState<"name" | "runs" | "lastRun" | "issues">("issues");
  const [detail, setDetail] = useState<JobRow | null>(null);

  const q = useQuery<JobsResult, CmdError>({
    queryKey: ["jobs", project],
    queryFn: () => apiV2.jobsOverview(project),
    staleTime: 30_000,
    retry: false,
  });

  const rows = useMemo(() => {
    const f = filter.trim().toLowerCase();
    let list = (q.data?.jobs ?? []).filter((j) => {
      if (onlyIssues && j.findings.length === 0 && j.envSecrets.length === 0) return false;
      if (!f) return true;
      // Tìm cả trên source path và cron: tên job không mang thông tin nên tìm theo tên
      // một mình gần như vô dụng ở đây.
      return (
        j.name.toLowerCase().includes(f) ||
        (j.sourcePath ?? "").toLowerCase().includes(f) ||
        (j.image ?? "").toLowerCase().includes(f) ||
        j.schedulers.some((s) => s.schedule.includes(f) || s.name.toLowerCase().includes(f))
      );
    });

    const sevRank = (j: JobRow) => {
      const w = worst(j.findings);
      const s = j.envSecrets.some((e) => e.severity === "high") ? "high" : null;
      const top = w === "high" || s === "high" ? 0 : w === "warn" ? 1 : w === "info" ? 2 : 3;
      return top;
    };

    list = [...list].sort((a, b) => {
      switch (sortBy) {
        case "runs":
          return (b.runsPerDay ?? -1) - (a.runsPerDay ?? -1) || a.name.localeCompare(b.name);
        case "lastRun":
          return (b.lastExecutionTime ?? "").localeCompare(a.lastExecutionTime ?? "") || a.name.localeCompare(b.name);
        case "issues":
          return sevRank(a) - sevRank(b) || (b.runsPerDay ?? 0) - (a.runsPerDay ?? 0) || a.name.localeCompare(b.name);
        default:
          return a.name.localeCompare(b.name);
      }
    });
    return list;
  }, [q.data, filter, onlyIssues, sortBy]);

  const stats = useMemo(() => {
    const all = q.data?.jobs ?? [];
    return {
      total: all.length,
      high: all.filter((j) => worst(j.findings) === "high").length,
      noSchedule: all.filter((j) => j.schedulers.length === 0).length,
      failing: all.filter((j) => j.lastExecutionStatus === "failed").length,
      envSecrets: all.filter((j) => j.envSecrets.length > 0).length,
    };
  }, [q.data]);

  const refresh = async () => {
    await apiV2.refreshJobs(project);
    await qc.invalidateQueries({ queryKey: ["jobs", project] });
  };

  if (q.isLoading) return <Loading label="Đang lấy Jobs và Scheduler…" />;

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto p-3">
      <ErrorBox error={q.error} onRetry={() => void q.refetch()} />

      {q.data?.schedulerUnavailable && (
        <Notice tone="warning" icon="⚠">
          {q.data.schedulerNote}
        </Notice>
      )}
      {!q.data?.schedulerUnavailable && q.data?.schedulerNote && (
        <Notice tone="info" icon="ℹ">
          {q.data.schedulerNote}
        </Notice>
      )}

      <div className="grid grid-cols-5 gap-2">
        <StatTile label="Tổng số job" value={stats.total} />
        <StatTile
          label="Cron cần sửa"
          value={stats.high}
          tone={stats.high > 0 ? "critical" : "good"}
          icon={stats.high > 0 ? "⚠" : "✓"}
          sub="trường phút để trống"
        />
        <StatTile
          label="Không có lịch"
          value={stats.noSchedule}
          tone={stats.noSchedule > 0 ? "warning" : "neutral"}
          icon="⚠"
          sub="không tự chạy"
        />
        <StatTile
          label="Lần chạy cuối lỗi"
          value={stats.failing}
          tone={stats.failing > 0 ? "critical" : "good"}
          icon={stats.failing > 0 ? "✕" : "✓"}
        />
        <StatTile
          label="Tổng lần chạy / ngày"
          value={num(q.data?.totalRunsPerDay ?? 0)}
          sub="suy từ cron của Scheduler"
        />
      </div>

      {stats.envSecrets > 0 && (
        <Notice tone="critical" icon="🔑">
          <strong>{stats.envSecrets} job</strong> có biến môi trường dạng plain trông như secret
          (Stripe key, token, mật khẩu…). Ai đọc được cấu hình job là đọc được giá trị đó — nên
          chuyển sang Secret Manager rồi tham chiếu bằng <code className="mono">secretKeyRef</code>.
          Bấm vào job để xem biến nào.
        </Notice>
      )}

      {(q.data?.orphanSchedulers.length ?? 0) > 0 && (
        <Notice tone="warning" icon="⚠">
          <strong>{q.data?.orphanSchedulers.length} Cloud Scheduler</strong> đang trỏ tới job không
          còn tồn tại. Mỗi lần fire là một lỗi im lặng:{"\n"}
          {q.data?.orphanSchedulers
            .map((s) => `• ${s.name} → ${s.targetJob} (${s.schedule})`)
            .join("\n")}
        </Notice>
      )}

      <div className="flex flex-wrap items-center gap-2">
        <Input
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          placeholder="tìm theo tên, source path, image, hoặc cron…"
          className="w-80"
        />
        <label className="flex items-center gap-1.5 text-[12px]">
          <input type="checkbox" checked={onlyIssues} onChange={(e) => setOnlyIssues(e.target.checked)} />
          Chỉ hiện job có vấn đề
        </label>
        <Select value={sortBy} onChange={(e) => setSortBy(e.target.value as typeof sortBy)}>
          <option value="issues">Sắp xếp: mức độ vấn đề</option>
          <option value="runs">Sắp xếp: số lần chạy/ngày</option>
          <option value="lastRun">Sắp xếp: lần chạy gần nhất</option>
          <option value="name">Sắp xếp: tên</option>
        </Select>
        <span className="text-[11px] text-[var(--ink-muted)]">
          {rows.length}/{stats.total} job
          {q.data ? ` · dữ liệu ${agoSeconds(q.data.ageSeconds)}` : ""}
        </span>
        <Button size="sm" variant="ghost" className="ml-auto" loading={q.isFetching} onClick={() => void refresh()}>
          ⟳ Reload
        </Button>
      </div>

      <div className="overflow-x-auto rounded-lg border" style={{ background: "var(--surface-1)" }}>
        <table className="w-full text-[11px]">
          <thead style={{ background: "var(--surface-2)" }}>
            <tr className="text-left">
              <th className="px-2 py-1.5 font-medium">Job</th>
              <th className="px-2 py-1.5 font-medium">Cron · timezone</th>
              <th className="tnum px-2 py-1.5 text-right font-medium">Lần/ngày</th>
              <th className="px-2 py-1.5 font-medium">Lần chạy cuối</th>
              <th className="px-2 py-1.5 font-medium">Resource</th>
              <th className="px-2 py-1.5 font-medium">Nguồn (repo)</th>
              <th className="px-2 py-1.5 font-medium">Cần để ý</th>
            </tr>
          </thead>
          <tbody>
            {rows.length === 0 && (
              <tr>
                <td colSpan={7} className="px-2 py-6 text-center text-[var(--ink-muted)]">
                  Không có job nào khớp.
                </td>
              </tr>
            )}
            {rows.map((j) => {
              const w = worst(j.findings);
              const ex = EXEC_TONE[j.lastExecutionStatus];
              return (
                <tr
                  key={`${j.region}/${j.name}`}
                  className="cursor-pointer border-t hover:bg-[var(--surface-2)]"
                  onClick={() => setDetail(j)}
                  style={
                    w === "high"
                      ? { background: "color-mix(in oklab, var(--status-critical) 7%, transparent)" }
                      : undefined
                  }
                >
                  <td className="mono px-2 py-1.5 whitespace-nowrap font-medium">{j.name}</td>
                  <td className="mono px-2 py-1.5 whitespace-nowrap">
                    {j.schedulers.length === 0 ? (
                      <span className="text-[var(--ink-muted)]">— không có lịch</span>
                    ) : (
                      j.schedulers.map((s) => (
                        <div key={s.name} className="flex items-center gap-1.5">
                          <span>{s.schedule}</span>
                          {/* Cron không có timezone là thông tin sai. */}
                          <span className="text-[var(--ink-muted)]">{s.timeZone}</span>
                          {s.state !== "ENABLED" && <Badge tone="warning">{s.state}</Badge>}
                        </div>
                      ))
                    )}
                  </td>
                  <td className="tnum px-2 py-1.5 text-right">
                    {j.runsPerDay === null ? "–" : num(j.runsPerDay)}
                  </td>
                  <td className="px-2 py-1.5 whitespace-nowrap">
                    <span className="flex items-center gap-1.5">
                      <span style={{ color: `var(--status-${ex.tone === "neutral" ? "good" : ex.tone})` }}>
                        {ex.icon}
                      </span>
                      {j.lastExecutionTime ? ago(j.lastExecutionTime) : ex.text}
                    </span>
                  </td>
                  <td className="px-2 py-1.5 whitespace-nowrap">
                    {j.cpu ?? "–"} / {j.memory ?? "–"}
                  </td>
                  <td className="mono px-2 py-1.5 text-[var(--ink-muted)]">
                    {j.sourcePath?.replace("deployments/cloud-run/base/", "…/") ?? "–"}
                  </td>
                  <td className="px-2 py-1.5">
                    <span className="flex flex-wrap items-center gap-1">
                      {w && (
                        <Badge tone={SEV_TONE[w]} icon={SEV_ICON[w]}>
                          {j.findings.length}
                        </Badge>
                      )}
                      {j.envSecrets.length > 0 && (
                        <Badge tone="critical" icon="🔑" title="env plain trông như secret">
                          {j.envSecrets.length}
                        </Badge>
                      )}
                    </span>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>

      <JobDetailDialog
        job={detail}
        project={project}
        readOnly={readOnly}
        requiresTypedConfirm={requiresTypedConfirm}
        onClose={() => setDetail(null)}
        onChanged={() => void refresh()}
        toast={toast}
      />
    </div>
  );
}

function JobDetailDialog({
  job,
  project,
  readOnly,
  requiresTypedConfirm,
  onClose,
  onChanged,
  toast,
}: {
  job: JobRow | null;
  project: string;
  readOnly: boolean;
  requiresTypedConfirm: boolean;
  onClose: () => void;
  onChanged: () => void;
  toast: (t: { tone: "good" | "critical" | "warning" | "info"; title: string; body?: string }) => void;
}) {
  const [confirm, setConfirm] = useState("");
  const [force, setForce] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<CmdError | null>(null);

  if (!job) return null;
  const confirmOk = !requiresTypedConfirm || confirm.trim() === job.name;

  const act = async (fn: () => Promise<unknown>, title: string) => {
    setBusy(true);
    setError(null);
    try {
      await fn();
      toast({ tone: "good", title });
      onChanged();
    } catch (e) {
      setError(asCmdError(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open onClose={onClose} title={`Job ${job.name}`} width={780}>
      <div className="flex flex-col gap-3">
        {job.envSecrets.length > 0 && (
          <Notice tone="critical" icon="🔑">
            {job.envSecrets.length} biến môi trường dạng plain trông như secret:{"\n"}
            {job.envSecrets
              .map((e) => `• ${e.envName} (${e.valueHint}, ${e.valueLen} byte) — ${e.reason}`)
              .join("\n")}
          </Notice>
        )}

        {job.findings.map((f, i) => (
          <Notice key={i} tone={SEV_TONE[f.severity]} icon={SEV_ICON[f.severity]}>
            {f.message}
            {f.suggestion && (
              <>
                {"\n\n"}Gợi ý: <code className="mono">{f.suggestion}</code>
              </>
            )}
          </Notice>
        ))}

        <div className="grid grid-cols-2 gap-x-6 gap-y-1.5 text-[12px]">
          {(
            [
              ["Region", job.region],
              ["Image", job.image ?? "–"],
              ["Nguồn trong repo", job.sourcePath ?? "–"],
              ["Service account", job.serviceAccount ?? "–"],
              ["Task / parallelism", `${job.taskCount ?? "–"} / ${job.parallelism ?? "–"}`],
              ["Max retries", String(job.maxRetries ?? "–")],
              ["Timeout", job.timeout ?? "–"],
              ["CPU / Memory", `${job.cpu ?? "–"} / ${job.memory ?? "–"}`],
              ["Tổng số lần đã chạy", num(job.executionCount ?? 0)],
              ["Execution cuối", job.lastExecution ?? "–"],
              ["Thời điểm", job.lastExecutionTime ? dateTime(job.lastExecutionTime) : "–"],
              ["Env", `${job.envCount} biến · ${job.secretEnvCount} từ secret`],
            ] as Array<[string, string]>
          ).map(([k, v]) => (
            <div key={k} className="flex gap-2">
              <span className="w-[150px] shrink-0 text-[var(--ink-muted)]">{k}</span>
              <span className="mono selectable min-w-0 break-all">{v}</span>
            </div>
          ))}
        </div>

        {job.schedulers.length > 0 && (
          <div>
            <h3 className="mb-1.5 text-[12px] font-semibold">Lịch chạy</h3>
            <div className="flex flex-col gap-1.5">
              {job.schedulers.map((s) => (
                <div key={s.name} className="flex flex-wrap items-center gap-2 rounded border px-2 py-1.5 text-[11px]">
                  <span className="mono font-medium">{s.name}</span>
                  <code className="mono rounded border px-1">{s.schedule}</code>
                  <span className="text-[var(--ink-muted)]">{s.timeZone}</span>
                  <Badge tone={s.state === "ENABLED" ? "good" : "warning"}>{s.state}</Badge>
                  {s.lastAttemptTime && (
                    <span className="text-[var(--ink-muted)]">fire cuối {ago(s.lastAttemptTime)}</span>
                  )}
                  <Button
                    size="sm"
                    variant="ghost"
                    className="ml-auto"
                    disabled={readOnly || busy || !confirmOk}
                    title={
                      s.state === "ENABLED"
                        ? "Tạm dừng lịch — đảo lại được, dùng khi cron chạy loạn"
                        : "Bật lại lịch"
                    }
                    onClick={() =>
                      void act(
                        () =>
                          apiV2.setSchedulePaused({
                            project,
                            region: s.region,
                            schedulerJob: s.name,
                            paused: s.state === "ENABLED",
                            confirmText: confirmOk ? confirm || null : null,
                          }),
                        s.state === "ENABLED" ? `Đã tạm dừng ${s.name}` : `Đã bật lại ${s.name}`,
                      )
                    }
                  >
                    {s.state === "ENABLED" ? "⏸ Tạm dừng" : "▶ Bật lại"}
                  </Button>
                </div>
              ))}
            </div>
          </div>
        )}

        <Notice tone="warning" icon="⚠">
          <strong>Chạy tay job không idempotent.</strong> Khác với sửa env (gửi hai lần cho cùng kết
          quả), chạy job hai lần tạo hai execution và job batch có thể xử lý trùng dữ liệu.
          {job.lastExecutionTime && (
            <>
              {"\n\n"}Lần chạy gần nhất: {ago(job.lastExecutionTime)} — nếu vừa chạy xong thì rất có
              thể bạn không cần chạy tay.
            </>
          )}
        </Notice>

        {requiresTypedConfirm && (
          <div className="rounded-md border p-2.5" style={{ borderColor: "var(--status-critical)" }}>
            <p className="mb-1.5 text-[12px]">
              Gõ đúng tên job <code className="mono">{job.name}</code> để mở các thao tác ghi:
            </p>
            <Input
              value={confirm}
              onChange={(e) => setConfirm(e.target.value)}
              placeholder="gõ lại tên job"
              invalid={confirm.length > 0 && !confirmOk}
              autoComplete="off"
            />
          </div>
        )}

        <label className="flex items-center gap-2 text-[12px]">
          <input type="checkbox" checked={force} onChange={(e) => setForce(e.target.checked)} />
          Chạy dù đang có execution dở (bỏ lớp chặn chồng lấn)
        </label>

        <ErrorBox error={error} />

        <div className="flex items-center gap-2">
          {readOnly && (
            <span className="text-[11px] text-[var(--ink-muted)]">
              Đang ở chế độ chỉ đọc — bật “Cho ghi” ở thanh trên để chạy job.
            </span>
          )}
          <Button
            variant="danger"
            className="ml-auto"
            disabled={readOnly || busy || !confirmOk}
            loading={busy}
            onClick={() =>
              void act(
                () =>
                  apiV2.runJob({
                    project,
                    region: job.region,
                    job: job.name,
                    confirmText: confirmOk ? confirm || null : null,
                    force,
                  }),
                `Đã tạo execution cho ${job.name}`,
              )
            }
          >
            ▶ Chạy job ngay
          </Button>
        </div>
      </div>
    </Dialog>
  );
}

