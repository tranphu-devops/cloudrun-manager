import { Fragment, useCallback, useEffect, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";

import { Badge, Button, ErrorBox, Input, Loading, Select, Toggle } from "../../../components/ui";
import { cn, dateTime, ms, SEVERITY_ORDER } from "../../../lib/format";
import { api, asCmdError } from "../../../lib/ipc";
import type { CmdError, LogEntry, RevisionInfo } from "../../../lib/types";

/** Số dòng giữ trong bộ nhớ. Log là dòng chảy vô hạn, phải có chặn trên. */
const MAX_LINES = 3000;

const SEVERITY_TONE: Record<string, { color: string; icon: string }> = {
  DEBUG: { color: "var(--ink-muted)", icon: "·" },
  INFO: { color: "var(--series-1)", icon: "·" },
  NOTICE: { color: "var(--series-1)", icon: "·" },
  WARNING: { color: "var(--status-warning)", icon: "⚠" },
  ERROR: { color: "var(--status-critical)", icon: "✕" },
  CRITICAL: { color: "var(--status-critical)", icon: "✕" },
  ALERT: { color: "var(--status-critical)", icon: "✕" },
  EMERGENCY: { color: "var(--status-critical)", icon: "✕" },
};

function mergeDedupe(existing: LogEntry[], incoming: LogEntry[]): LogEntry[] {
  // Polling luôn chồng lấn: cùng một entry sẽ về nhiều lần vì log có thể được ghi trễ
  // so với timestamp của nó. Không dedupe thì danh sách sẽ nhân bản.
  const seen = new Set(existing.map((e) => e.insertId).filter(Boolean));
  const fresh = incoming.filter((e) => !e.insertId || !seen.has(e.insertId));
  return [...fresh, ...existing]
    .sort((a, b) => b.timestamp.localeCompare(a.timestamp))
    .slice(0, MAX_LINES);
}

export function LogsTab({
  project,
  region,
  service,
  revisions,
  pollSeconds,
}: {
  project: string;
  region: string;
  service: string;
  revisions: RevisionInfo[];
  pollSeconds: number;
}) {
  const [entries, setEntries] = useState<LogEntry[]>([]);
  const [error, setError] = useState<CmdError | null>(null);
  const [loading, setLoading] = useState(false);
  const [live, setLive] = useState(false);
  const [expanded, setExpanded] = useState<string | null>(null);

  const [severity, setSeverity] = useState("DEFAULT");
  const [stream, setStream] = useState("all");
  const [revision, setRevision] = useState("");
  const [minutes, setMinutes] = useState(60);
  const [search, setSearch] = useState("");
  const [searchApplied, setSearchApplied] = useState("");

  const [nextToken, setNextToken] = useState<string | null>(null);
  const newestRef = useRef<string | null>(null);

  const load = useCallback(
    async (mode: "replace" | "tail" | "more") => {
      setLoading(true);
      if (mode === "replace") setError(null);
      try {
        const page = await api.fetchLogs({
          project,
          region,
          service,
          revision: revision || null,
          minSeverity: severity,
          search: searchApplied || null,
          stream,
          minutes,
          // Tail chỉ hỏi log mới hơn dòng mới nhất đã thấy — nếu không, mỗi nhịp poll
          // lại tải về cả cửa sổ 60 phút.
          since: mode === "tail" ? newestRef.current : null,
          pageSize: mode === "tail" ? 100 : 200,
          pageToken: mode === "more" ? nextToken : null,
        });

        setEntries((prev) => {
          const merged = mode === "replace" ? page.entries : mergeDedupe(prev, page.entries);
          newestRef.current = merged[0]?.timestamp ?? newestRef.current;
          return merged;
        });
        if (mode !== "tail") setNextToken(page.nextPageToken);
        setError(null);
      } catch (e) {
        setError(asCmdError(e));
        // Lỗi khi đang tail thì dừng tail, đừng đập API mỗi 3 giây với cùng một lỗi.
        if (mode === "tail") setLive(false);
      } finally {
        setLoading(false);
      }
    },
    [project, region, service, revision, severity, searchApplied, stream, minutes, nextToken],
  );

  // Đổi service / đổi filter → tải lại từ đầu.
  useEffect(() => {
    setEntries([]);
    setNextToken(null);
    newestRef.current = null;
    void load("replace");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [project, region, service, revision, severity, searchApplied, stream, minutes]);

  useEffect(() => {
    if (!live) return;
    const id = window.setInterval(() => void load("tail"), Math.max(pollSeconds, 2) * 1000);
    return () => window.clearInterval(id);
  }, [live, pollSeconds, load]);

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-2">
      <div className="flex flex-wrap items-center gap-2">
        <Select value={severity} onChange={(e) => setSeverity(e.target.value)} aria-label="Mức độ">
          {SEVERITY_ORDER.map((s) => (
            <option key={s} value={s}>
              {s === "DEFAULT" ? "Mọi mức độ" : `≥ ${s}`}
            </option>
          ))}
        </Select>

        <Select value={stream} onChange={(e) => setStream(e.target.value)} aria-label="Loại log">
          <option value="all">Tất cả log</option>
          <option value="request">Chỉ request log</option>
          <option value="app">Chỉ app log (stdout/stderr)</option>
        </Select>

        <Select value={revision} onChange={(e) => setRevision(e.target.value)} aria-label="Revision">
          <option value="">Mọi revision</option>
          {revisions.map((r) => (
            <option key={r.name} value={r.name}>
              {r.name}
              {r.trafficPercent > 0 ? ` (${r.trafficPercent}%)` : ""}
            </option>
          ))}
        </Select>

        <Select value={String(minutes)} onChange={(e) => setMinutes(Number(e.target.value))} aria-label="Khoảng thời gian">
          <option value="15">15 phút</option>
          <option value="60">1 giờ</option>
          <option value="360">6 giờ</option>
          <option value="1440">24 giờ</option>
          <option value="10080">7 ngày</option>
        </Select>

        <form
          className="flex items-center gap-1"
          onSubmit={(e) => {
            e.preventDefault();
            setSearchApplied(search);
          }}
        >
          <Input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="tìm trong log…"
            className="w-48"
          />
          <Button size="sm" type="submit">
            Tìm
          </Button>
        </form>

        <div className="ml-auto flex items-center gap-2">
          <Toggle
            checked={live}
            onChange={setLive}
            label={live ? `⏸ Đang theo dõi (${pollSeconds}s)` : "▶ Theo dõi"}
            hint={`REST API của Cloud Logging không có streaming thật — app hỏi lại mỗi ${pollSeconds} giây.`}
          />
          <Button size="sm" variant="ghost" loading={loading} onClick={() => void load("replace")}>
            ⟳
          </Button>
          <Button
            size="sm"
            variant="ghost"
            onClick={async () => openUrl(await api.logExplorerUrl(project, region, service))}
            title="Mở Log Explorer với đúng filter service/region"
          >
            Log Explorer ↗
          </Button>
        </div>
      </div>

      <ErrorBox error={error} onRetry={() => void load("replace")} />

      <div
        className="min-h-0 flex-1 overflow-auto rounded-lg border"
        style={{ background: "var(--surface-1)" }}
      >
        {loading && entries.length === 0 && <Loading label="Đang lấy log…" />}
        {!loading && entries.length === 0 && !error && (
          <p className="p-4 text-center text-[12px] text-[var(--ink-muted)]">
            Không có log nào khớp trong khoảng thời gian này.
          </p>
        )}

        <table className="w-full text-[11px]">
          <tbody>
            {entries.map((e) => {
              const tone = SEVERITY_TONE[e.severity] ?? SEVERITY_TONE["INFO"]!;
              const isOpen = expanded === e.insertId;
              const bad = e.httpStatus !== null && e.httpStatus >= 500;

              const rowKey = e.insertId || `${e.timestamp}-${e.message.slice(0, 24)}`;

              return (
                <Fragment key={rowKey}>
                  <tr
                    className="cursor-pointer border-b align-top hover:bg-[var(--surface-2)]"
                    onClick={() => setExpanded(isOpen ? null : e.insertId)}
                  >
                    <td className="mono whitespace-nowrap px-2 py-1 text-[var(--ink-muted)]">
                      {e.timestamp.slice(11, 23)}
                    </td>
                    <td className="w-[1%] px-1 py-1" style={{ color: tone.color }} title={e.severity}>
                      {tone.icon}
                    </td>
                    <td className="w-[1%] px-1 py-1">
                      {e.stream === "request" ? (
                        <span
                          className="tnum"
                          style={{ color: bad ? "var(--status-critical)" : "var(--ink-muted)" }}
                        >
                          {e.httpStatus ?? "–"}
                        </span>
                      ) : (
                        <span className="text-[var(--ink-muted)]">app</span>
                      )}
                    </td>
                    <td className="mono selectable px-2 py-1">
                      <span className={cn("break-all", !isOpen && "line-clamp-2")}>{e.message}</span>
                    </td>
                    <td className="tnum whitespace-nowrap px-2 py-1 text-right text-[var(--ink-muted)]">
                      {e.latencyMs !== null ? ms(e.latencyMs) : ""}
                    </td>
                  </tr>
                  {isOpen && (
                    <tr className="border-b">
                      <td colSpan={5} className="px-2 py-2" style={{ background: "var(--surface-2)" }}>
                        <div className="mb-1 flex flex-wrap items-center gap-2">
                          <Badge>{e.severity}</Badge>
                          {e.revision && <span className="mono">{e.revision}</span>}
                          <span className="text-[var(--ink-muted)]">{dateTime(e.timestamp)}</span>
                          {e.httpMethod && (
                            <span className="mono">
                              {e.httpMethod} {e.httpPath}
                            </span>
                          )}
                        </div>
                        <pre className="mono selectable max-h-72 overflow-auto whitespace-pre-wrap rounded border p-2">
                          {JSON.stringify(e.raw, null, 2)}
                        </pre>
                      </td>
                    </tr>
                  )}
                </Fragment>
              );
            })}
          </tbody>
        </table>

        {nextToken && (
          <div className="flex justify-center p-2">
            <Button size="sm" loading={loading} onClick={() => void load("more")}>
              Tải thêm
            </Button>
          </div>
        )}
      </div>

      <p className="text-[10px] text-[var(--ink-muted)]">
        Giữ tối đa {MAX_LINES.toLocaleString("vi-VN")} dòng trong bộ nhớ · bấm một dòng để xem JSON gốc
        {entries.length > 0 && ` · đang hiển thị ${entries.length.toLocaleString("vi-VN")} dòng`}
      </p>
    </div>
  );
}
