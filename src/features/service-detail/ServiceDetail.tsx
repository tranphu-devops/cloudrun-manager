import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";

import { Badge, Button, ErrorBox, Loading, Select, Tabs } from "../../components/ui";
import { regionLabel } from "../../lib/format";
import { useRevisions, useService } from "../../lib/queries";
import type { CapabilitiesResult, ProjectLoadSnapshot } from "../../lib/types";
import { HealthDot } from "../service-list/Sidebar";
import { EnvTab } from "./tabs/EnvTab";
import { LogsTab } from "./tabs/LogsTab";
import { MetricsTab } from "./tabs/MetricsTab";
import { OverviewTab } from "./tabs/OverviewTab";
import { RevisionsTab } from "./tabs/RevisionsTab";
import { ScalingTab } from "./tabs/ScalingTab";
import { SecretsTab } from "./tabs/SecretsTab";

type TabId = "overview" | "env" | "scaling" | "secrets" | "metrics" | "logs" | "revisions";

export function ServiceDetailPane({
  project,
  region,
  service,
  load,
  caps,
  readOnly,
  requiresTypedConfirm,
  metricsMinutes,
  onMetricsMinutesChange,
  logPollSeconds,
  autoRefreshMs,
}: {
  project: string;
  region: string;
  service: string;
  load: ProjectLoadSnapshot | undefined;
  caps: CapabilitiesResult | undefined;
  readOnly: boolean;
  requiresTypedConfirm: boolean;
  metricsMinutes: number;
  onMetricsMinutesChange: (m: number) => void;
  logPollSeconds: number;
  autoRefreshMs: number;
}) {
  const [tab, setTab] = useState<TabId>("overview");
  const [containerIndex, setContainerIndex] = useState(0);

  const q = useService(project, region, service);
  // Log và Metrics đều cần danh sách revision để lọc; lấy sẵn khi mở service.
  const revisions = useRevisions(project, region, service);

  // Đổi service: về tab Overview và container đầu. Giữ nguyên tab Env của service cũ
  // sẽ khiến người dùng nhìn form sửa của một service khác.
  useEffect(() => {
    setTab("overview");
    setContainerIndex(0);
  }, [project, region, service]);

  if (q.isLoading) return <Loading label={`Đang lấy ${service}…`} />;

  if (q.error) {
    return (
      <div className="p-4">
        <ErrorBox error={q.error} onRetry={() => void q.refetch()} />
      </div>
    );
  }
  if (!q.data) return null;

  const d = q.data;
  const s = d.summary;
  const multiContainer = d.containers.length > 1;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <header className="flex shrink-0 items-start gap-3 border-b px-3 py-2">
        <div className="min-w-0">
          <h1 className="flex items-center gap-2 text-[15px] font-semibold">
            <HealthDot health={s.health} message={s.healthMessage} />
            <span className="truncate">{s.name}</span>
            {s.trafficPinned && (
              <Badge tone="warning" icon="📌" title="Traffic ghim vào revision cụ thể">
                traffic ghim
              </Badge>
            )}
            {q.isFetching && <span className="text-[11px] text-[var(--ink-muted)]">cập nhật…</span>}
          </h1>
          <p className="mt-0.5 flex flex-wrap items-center gap-x-2 text-[11px] text-[var(--ink-muted)]">
            <span>{regionLabel(s.region)}</span>
            <span>·</span>
            <span className="mono">{s.latestReadyRevision ?? "chưa có revision ready"}</span>
            {s.uri && (
              <>
                <span>·</span>
                <button
                  className="mono max-w-[380px] truncate underline decoration-dotted hover:no-underline"
                  onClick={() => void openUrl(s.uri as string)}
                  title={s.uri}
                >
                  {s.uri.replace(/^https:\/\//, "")}
                </button>
              </>
            )}
          </p>
        </div>

        <div className="ml-auto flex shrink-0 items-center gap-2">
          {multiContainer && (
            <Select
              value={String(containerIndex)}
              onChange={(e) => setContainerIndex(Number(e.target.value))}
              aria-label="Container"
              title="Service này có nhiều container (sidecar)"
            >
              {d.containers.map((c) => (
                <option key={c.index} value={c.index}>
                  {c.name ?? `container ${c.index + 1}`}
                </option>
              ))}
            </Select>
          )}
          <Button size="sm" variant="ghost" onClick={() => void q.refetch()} loading={q.isFetching}>
            ⟳
          </Button>
        </div>
      </header>

      <Tabs<TabId>
        value={tab}
        onChange={setTab}
        tabs={[
          { id: "overview", label: "Tổng quan" },
          {
            id: "env",
            label: "Env",
            badge: (
              <span className="tnum text-[10px] text-[var(--ink-muted)]">
                {s.envCount}
                {s.secretEnvCount > 0 && ` · 🔑${s.secretEnvCount}`}
              </span>
            ),
          },
          { id: "scaling", label: "Scaling" },
          { id: "secrets", label: "Secrets" },
          { id: "metrics", label: "Tải" },
          { id: "logs", label: "Log" },
          { id: "revisions", label: "Revisions" },
        ]}
      />

      <div className="min-h-0 flex-1 overflow-y-auto p-3">
        {tab === "overview" && (
          <OverviewTab project={project} detail={d} load={load} containerIndex={containerIndex} />
        )}
        {tab === "env" && (
          <EnvTab
            project={project}
            detail={d}
            containerIndex={containerIndex}
            readOnly={readOnly}
            requiresTypedConfirm={requiresTypedConfirm}
          />
        )}
        {tab === "scaling" && (
          <ScalingTab
            project={project}
            detail={d}
            containerIndex={containerIndex}
            readOnly={readOnly}
            requiresTypedConfirm={requiresTypedConfirm}
          />
        )}
        {tab === "secrets" && (
          <SecretsTab
            project={project}
            detail={d}
            canReveal={caps?.canRevealSecrets ?? true}
          />
        )}
        {tab === "metrics" && (
          <MetricsTab
            project={project}
            region={region}
            service={service}
            minutes={metricsMinutes}
            onMinutesChange={onMetricsMinutesChange}
            autoRefreshMs={autoRefreshMs}
          />
        )}
        {tab === "logs" && (
          <LogsTab
            project={project}
            region={region}
            service={service}
            revisions={revisions.data ?? []}
            pollSeconds={logPollSeconds}
          />
        )}
        {tab === "revisions" && (
          <RevisionsTab project={project} region={region} service={service} />
        )}
      </div>
    </div>
  );
}
