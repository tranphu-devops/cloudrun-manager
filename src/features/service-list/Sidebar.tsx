import { useMemo, useState } from "react";

import { Badge, ErrorBox, Input, Loading } from "../../components/ui";
import { cn, compact, percent, regionLabel } from "../../lib/format";
import type { CmdError, Health, ProjectLoadSnapshot, ServiceSummary } from "../../lib/types";

export function HealthDot({ health, message }: { health: Health; message?: string | null }) {
  const map: Record<Health, { color: string; icon: string; text: string }> = {
    ready: { color: "var(--status-good)", icon: "●", text: "Ready" },
    notReady: { color: "var(--status-critical)", icon: "✕", text: "Không ready" },
    reconciling: { color: "var(--status-warning)", icon: "◐", text: "Đang triển khai" },
    unknown: { color: "var(--ink-muted)", icon: "○", text: "Không rõ" },
  };
  const m = map[health];
  return (
    <span
      aria-label={m.text}
      title={message ? `${m.text} — ${message}` : m.text}
      className="shrink-0 text-[10px] leading-none"
      style={{ color: m.color }}
    >
      {m.icon}
    </span>
  );
}

/** Ngưỡng error rate coi là đáng chú ý / đáng báo động. */
const ERR_WARN = 0.01;
const ERR_CRIT = 0.05;

export function Sidebar({
  services,
  load,
  loading,
  error,
  selected,
  onSelect,
}: {
  services: ServiceSummary[];
  load: ProjectLoadSnapshot | undefined;
  loading: boolean;
  error: CmdError | null | undefined;
  selected: { region: string; name: string } | null;
  onSelect: (s: ServiceSummary) => void;
}) {
  const [filter, setFilter] = useState("");
  const [onlyProblems, setOnlyProblems] = useState(false);

  const groups = useMemo(() => {
    const f = filter.trim().toLowerCase();
    const rate = (n: string) => load?.errorRate[n] ?? 0;

    const filtered = services.filter((s) => {
      if (f && !s.name.toLowerCase().includes(f) && !(s.image ?? "").toLowerCase().includes(f)) {
        return false;
      }
      if (onlyProblems) {
        return s.health !== "ready" || s.trafficPinned || rate(s.name) >= ERR_WARN;
      }
      return true;
    });

    const byRegion = new Map<string, ServiceSummary[]>();
    for (const s of filtered) {
      const arr = byRegion.get(s.region);
      if (arr) arr.push(s);
      else byRegion.set(s.region, [s]);
    }
    return [...byRegion.entries()].sort((a, b) => a[0].localeCompare(b[0]));
  }, [services, filter, onlyProblems, load]);

  const problemCount = services.filter(
    (s) => s.health !== "ready" || s.trafficPinned || (load?.errorRate[s.name] ?? 0) >= ERR_WARN,
  ).length;

  return (
    <aside
      className="flex w-[292px] shrink-0 flex-col border-r"
      style={{ background: "var(--surface-1)" }}
    >
      <div className="flex flex-col gap-1.5 border-b p-2">
        <Input
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          placeholder={`Tìm trong ${services.length} service…`}
          aria-label="Tìm service"
        />
        <label className="flex items-center gap-1.5 text-[11px] text-[var(--ink-secondary)]">
          <input
            type="checkbox"
            checked={onlyProblems}
            onChange={(e) => setOnlyProblems(e.target.checked)}
          />
          Chỉ hiện service cần để ý
          {problemCount > 0 && (
            <span className="tnum" style={{ color: "var(--status-warning)" }}>
              ({problemCount})
            </span>
          )}
        </label>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
        {loading && services.length === 0 && <Loading label="Đang lấy danh sách service…" />}
        {error && (
          <div className="p-2">
            <ErrorBox error={error} />
          </div>
        )}
        {!loading && services.length > 0 && groups.length === 0 && (
          <p className="p-3 text-[12px] text-[var(--ink-muted)]">Không có service nào khớp.</p>
        )}

        {groups.map(([region, items]) => (
          <div key={region}>
            <h3
              className="sticky top-0 z-10 border-b px-2 py-1 text-[10px] font-semibold uppercase tracking-wide text-[var(--ink-muted)]"
              style={{ background: "var(--surface-2)" }}
            >
              {regionLabel(region)} · {items.length}
            </h3>
            <ul>
              {items.map((s) => {
                const active = selected?.name === s.name && selected?.region === s.region;
                const inst = load?.instances[s.name];
                const rps = load?.rps[s.name];
                const err = load?.errorRate[s.name] ?? 0;

                return (
                  <li key={`${s.region}/${s.name}`}>
                    <button
                      onClick={() => onSelect(s)}
                      className={cn(
                        "flex w-full flex-col gap-0.5 border-b px-2 py-1.5 text-left transition-colors",
                        active ? "font-medium" : "hover:bg-[var(--surface-2)]",
                      )}
                      style={
                        active
                          ? {
                              background: "color-mix(in oklab, var(--series-1) 12%, var(--surface-1))",
                              boxShadow: "inset 2px 0 0 var(--series-1)",
                            }
                          : undefined
                      }
                    >
                      <span className="flex min-w-0 items-center gap-1.5">
                        <HealthDot health={s.health} message={s.healthMessage} />
                        <span className="min-w-0 flex-1 truncate text-[12px]">{s.name}</span>
                        {s.trafficPinned && (
                          <span
                            title="Traffic bị ghim vào revision cụ thể — revision mới sẽ không nhận traffic"
                            className="shrink-0 text-[10px]"
                            style={{ color: "var(--status-warning)" }}
                          >
                            📌
                          </span>
                        )}
                        {s.secretEnvCount > 0 && (
                          <span
                            className="shrink-0 text-[10px] opacity-60"
                            title={`${s.secretEnvCount} biến lấy từ Secret Manager`}
                          >
                            🔑
                          </span>
                        )}
                      </span>

                      <span className="tnum flex items-center gap-2 pl-[15px] text-[10px] text-[var(--ink-muted)]">
                        {/* `undefined` = chưa có dữ liệu metric. Hiện "–" thay vì 0 để
                            không ai đọc thành "service không chạy instance nào". */}
                        <span title="Số instance">
                          {inst === undefined ? "– inst" : `${compact(inst)} inst`}
                        </span>
                        <span title="Request mỗi giây">
                          {rps === undefined ? "– rps" : `${compact(rps)} rps`}
                        </span>
                        {err >= ERR_WARN && (
                          <Badge
                            tone={err >= ERR_CRIT ? "critical" : "warning"}
                            icon="⚠"
                            title="Tỉ lệ response 5xx trong 30 phút gần nhất"
                          >
                            {percent(err, 1)} lỗi
                          </Badge>
                        )}
                      </span>
                    </button>
                  </li>
                );
              })}
            </ul>
          </div>
        ))}
      </div>

      {load && load.missing.length > 0 && (
        <div className="border-t px-2 py-1.5 text-[10px] text-[var(--ink-muted)]">
          Không lấy được: {load.missing.join(", ")} — badge tương ứng hiện “–”.
        </div>
      )}
    </aside>
  );
}
