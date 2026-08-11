import { useMemo, useState } from "react";

import { StatTile } from "../../components/charts";
import { Badge, Button, EmptyState, ErrorBox, Input, Loading, Notice, Select } from "../../components/ui";
import { ago, compact, num, percent, regionLabel, shortImage } from "../../lib/format";
import { useProjectLoad, useServices } from "../../lib/queries";
import type { Health, ProjectLoadSnapshot, ServiceSummary } from "../../lib/types";

/**
 * Màn Statistics.
 *
 * Câu hỏi màn này trả lời: "toàn bộ Cloud Run của project đang thế nào?" — một lưới dày,
 * xem được ~95 service trong một màn hình, không phải bấm vào từng cái. Gộp trạng thái
 * (health), tải (instance/rps/error) và cấu hình (min/max, secret, traffic ghim) vào cùng
 * một hàng để thấy ngay chỗ bất thường.
 *
 * Dùng chung query với màn Services (`useServices` + `useProjectLoad`) nên không tốn thêm
 * round-trip: hai màn đọc cùng một cache.
 */

const HEALTH_META: Record<Health, { tone: "good" | "critical" | "warning" | "neutral"; icon: string; text: string }> = {
  ready: { tone: "good", icon: "✓", text: "sẵn sàng" },
  notReady: { tone: "critical", icon: "✕", text: "lỗi" },
  reconciling: { tone: "warning", icon: "◐", text: "đang cập nhật" },
  unknown: { tone: "neutral", icon: "○", text: "không rõ" },
};

type SortKey = "name" | "instances" | "rps" | "errorRate" | "health";

function load(snap: ProjectLoadSnapshot | undefined, name: string) {
  return {
    instances: snap?.instances[name] ?? null,
    rps: snap?.rps[name] ?? null,
    errorRate: snap?.errorRate[name] ?? null,
  };
}

export function StatisticsPage({
  project,
  autoRefreshMs,
  onOpenService,
}: {
  project: string;
  autoRefreshMs: number;
  onOpenService: (s: { region: string; name: string }) => void;
}) {
  const servicesQ = useServices(project, autoRefreshMs);
  const loadQ = useProjectLoad(project, autoRefreshMs);

  const [filter, setFilter] = useState("");
  const [sortBy, setSortBy] = useState<SortKey>("health");
  const [onlyIssues, setOnlyIssues] = useState(false);

  const all = servicesQ.data?.services ?? [];
  const snap = loadQ.data;

  const stats = useMemo(() => {
    const totalInstances = Object.values(snap?.instances ?? {}).reduce((a, v) => a + v, 0);
    const totalRps = Object.values(snap?.rps ?? {}).reduce((a, v) => a + v, 0);
    const erroring = Object.entries(snap?.errorRate ?? {}).filter(([, v]) => v > 0).length;
    return {
      total: all.length,
      unhealthy: all.filter((s) => s.health === "notReady").length,
      reconciling: all.filter((s) => s.health === "reconciling").length,
      pinned: all.filter((s) => s.trafficPinned).length,
      withSecrets: all.filter((s) => s.secretEnvCount > 0).length,
      alwaysOn: all.filter((s) => (s.minInstances ?? 0) > 0).length,
      totalInstances,
      totalRps,
      erroring,
    };
  }, [all, snap]);

  const rows = useMemo(() => {
    const f = filter.trim().toLowerCase();
    const healthRank: Record<Health, number> = { notReady: 0, reconciling: 1, unknown: 2, ready: 3 };

    let list = all.filter((s) => {
      const l = load(snap, s.name);
      const hasIssue = s.health === "notReady" || s.trafficPinned || (l.errorRate ?? 0) > 0;
      if (onlyIssues && !hasIssue) return false;
      if (!f) return true;
      return (
        s.name.toLowerCase().includes(f) ||
        (s.image ?? "").toLowerCase().includes(f) ||
        s.region.toLowerCase().includes(f)
      );
    });

    list = [...list].sort((a, b) => {
      const la = load(snap, a.name);
      const lb = load(snap, b.name);
      switch (sortBy) {
        case "instances":
          return (lb.instances ?? -1) - (la.instances ?? -1) || a.name.localeCompare(b.name);
        case "rps":
          return (lb.rps ?? -1) - (la.rps ?? -1) || a.name.localeCompare(b.name);
        case "errorRate":
          return (lb.errorRate ?? -1) - (la.errorRate ?? -1) || a.name.localeCompare(b.name);
        case "health":
          return healthRank[a.health] - healthRank[b.health] || a.name.localeCompare(b.name);
        default:
          return a.name.localeCompare(b.name);
      }
    });
    return list;
  }, [all, snap, filter, sortBy, onlyIssues]);

  if (servicesQ.isLoading) return <Loading label="Đang lấy toàn bộ service…" />;

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto p-3">
      <ErrorBox error={servicesQ.error} onRetry={() => void servicesQ.refetch()} />

      {snap && snap.missing.length > 0 && (
        <Notice tone="warning" icon="⚠">
          {snap.missing.length} service không lấy được số tải (instance/rps). Cột tải của chúng hiện{" "}
          <span className="mono">–</span>, không phải 0 — đừng đọc thành “không có tải”.
        </Notice>
      )}

      <div className="grid grid-cols-4 gap-2 lg:grid-cols-8">
        <StatTile label="Tổng service" value={stats.total} />
        <StatTile
          label="Đang lỗi"
          value={stats.unhealthy}
          tone={stats.unhealthy > 0 ? "critical" : "good"}
          icon={stats.unhealthy > 0 ? "✕" : "✓"}
        />
        <StatTile
          label="Đang cập nhật"
          value={stats.reconciling}
          tone={stats.reconciling > 0 ? "warning" : "neutral"}
          icon="◐"
        />
        <StatTile
          label="Có request lỗi"
          value={stats.erroring}
          tone={stats.erroring > 0 ? "warning" : "good"}
          icon={stats.erroring > 0 ? "⚠" : "✓"}
          sub="trong 30 phút"
        />
        <StatTile
          label="Traffic ghim"
          value={stats.pinned}
          tone={stats.pinned > 0 ? "warning" : "neutral"}
          icon="⚠"
          sub="revision mới không nhận traffic"
        />
        <StatTile label="Luôn bật (min>0)" value={stats.alwaysOn} sub="tính tiền cả khi rảnh" />
        <StatTile label="Tổng instance" value={compact(stats.totalInstances)} sub="đang chạy" />
        <StatTile label="Tổng RPS" value={compact(stats.totalRps)} unit="req/s" />
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <Input
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          placeholder="tìm theo tên, image, region…"
          className="w-72"
        />
        <label className="flex items-center gap-1.5 text-[12px]">
          <input type="checkbox" checked={onlyIssues} onChange={(e) => setOnlyIssues(e.target.checked)} />
          Chỉ hiện service có vấn đề
        </label>
        <Select value={sortBy} onChange={(e) => setSortBy(e.target.value as SortKey)}>
          <option value="health">Sắp xếp: trạng thái</option>
          <option value="instances">Sắp xếp: số instance</option>
          <option value="rps">Sắp xếp: RPS</option>
          <option value="errorRate">Sắp xếp: tỉ lệ lỗi</option>
          <option value="name">Sắp xếp: tên</option>
        </Select>
        <span className="text-[11px] text-[var(--ink-muted)]">
          {rows.length}/{stats.total} service
          {servicesQ.data ? ` · dữ liệu ${ago(new Date(Date.now() - servicesQ.data.ageSeconds * 1000).toISOString())}` : ""}
        </span>
        <Button
          size="sm"
          variant="ghost"
          className="ml-auto"
          loading={servicesQ.isFetching || loadQ.isFetching}
          onClick={() => {
            void servicesQ.refetch();
            void loadQ.refetch();
          }}
        >
          ⟳ Reload
        </Button>
      </div>

      {rows.length === 0 ? (
        <EmptyState icon="◧" title="Không có service nào khớp" />
      ) : (
        <div className="overflow-x-auto rounded-lg border" style={{ background: "var(--surface-1)" }}>
          <table className="w-full text-[11px]">
            <thead style={{ background: "var(--surface-2)" }}>
              <tr className="text-left">
                <th className="px-2 py-1.5 font-medium">Service</th>
                <th className="px-2 py-1.5 font-medium">Trạng thái</th>
                <th className="tnum px-2 py-1.5 text-right font-medium">Instance</th>
                <th className="tnum px-2 py-1.5 text-right font-medium">RPS</th>
                <th className="tnum px-2 py-1.5 text-right font-medium">Lỗi</th>
                <th className="tnum px-2 py-1.5 text-right font-medium">Min/Max</th>
                <th className="px-2 py-1.5 font-medium">Env</th>
                <th className="px-2 py-1.5 font-medium">Image</th>
                <th className="px-2 py-1.5 font-medium">Sửa lần cuối</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((s) => (
                <StatRow key={`${s.region}/${s.name}`} s={s} snap={snap} onOpen={() => onOpenService({ region: s.region, name: s.name })} />
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

function StatRow({
  s,
  snap,
  onOpen,
}: {
  s: ServiceSummary;
  snap: ProjectLoadSnapshot | undefined;
  onOpen: () => void;
}) {
  const h = HEALTH_META[s.health];
  const l = load(snap, s.name);
  const err = l.errorRate ?? 0;

  return (
    <tr
      className="cursor-pointer border-t hover:bg-[var(--surface-2)]"
      onClick={onOpen}
      style={
        s.health === "notReady"
          ? { background: "color-mix(in oklab, var(--status-critical) 7%, transparent)" }
          : undefined
      }
    >
      <td className="px-2 py-1.5">
        <div className="mono font-medium">{s.name}</div>
        <div className="text-[10px] text-[var(--ink-muted)]">{regionLabel(s.region)}</div>
      </td>
      <td className="px-2 py-1.5 whitespace-nowrap">
        <Badge tone={h.tone} icon={h.icon}>
          {h.text}
        </Badge>
      </td>
      <td className="tnum px-2 py-1.5 text-right">{l.instances === null ? "–" : num(l.instances)}</td>
      <td className="tnum px-2 py-1.5 text-right">{l.rps === null ? "–" : num(l.rps, 1)}</td>
      <td
        className="tnum px-2 py-1.5 text-right"
        style={err > 0 ? { color: "var(--status-critical)", fontWeight: 600 } : undefined}
      >
        {l.errorRate === null ? "–" : err === 0 ? "0" : percent(err)}
      </td>
      <td className="tnum px-2 py-1.5 text-right whitespace-nowrap">
        {s.minInstances ?? 0}
        <span className="text-[var(--ink-muted)]"> / {s.maxInstances ?? "∞"}</span>
      </td>
      <td className="px-2 py-1.5 whitespace-nowrap">
        <span className="flex items-center gap-1">
          <span>{s.envCount}</span>
          {s.secretEnvCount > 0 && (
            <Badge tone="info" icon="🔑" title="biến từ Secret Manager">
              {s.secretEnvCount}
            </Badge>
          )}
          {s.trafficPinned && (
            <Badge tone="warning" icon="📌" title="Traffic ghim — revision mới không nhận traffic">
              ghim
            </Badge>
          )}
        </span>
      </td>
      <td className="mono px-2 py-1.5 text-[var(--ink-muted)]" title={s.image ?? undefined}>
        {shortImage(s.image)}
      </td>
      <td className="px-2 py-1.5 whitespace-nowrap text-[var(--ink-muted)]">
        {ago(s.updateTime)}
        {s.lastModifier && <div className="text-[10px]">{s.lastModifier.split("@")[0]}</div>}
      </td>
    </tr>
  );
}
