import { openUrl } from "@tauri-apps/plugin-opener";

import { Badge, Button, Card, KeyValue, Notice } from "../../../components/ui";
import { StatTile } from "../../../components/charts";
import { compact, dateTime, humanTimeout, percent, regionLabel, shortImage } from "../../../lib/format";
import { useT } from "../../../lib/i18n";
import { consoleServiceUrl } from "../../../lib/ipc";
import type { ProjectLoadSnapshot, ServiceDetail } from "../../../lib/types";
import { HealthDot } from "../../service-list/Sidebar";

/** Giá trị là key dịch, không phải chuỗi hiển thị — bọc `t()` ở chỗ render. */
const INGRESS_TEXT: Record<string, string> = {
  INGRESS_TRAFFIC_ALL: "Public — ai cũng gọi được (all)",
  INGRESS_TRAFFIC_INTERNAL_ONLY: "Chỉ nội bộ VPC / Cloud Run khác",
  INGRESS_TRAFFIC_INTERNAL_LOAD_BALANCER: "Nội bộ + Load Balancer",
};

export function OverviewTab({
  project,
  detail,
  load,
  containerIndex,
}: {
  project: string;
  detail: ServiceDetail;
  load: ProjectLoadSnapshot | undefined;
  containerIndex: number;
}) {
  const t = useT();
  const s = detail.summary;
  const c = detail.containers[containerIndex];

  const inst = load?.instances[s.name];
  const rps = load?.rps[s.name];
  const err = load?.errorRate[s.name];

  const errTone = err === undefined ? "neutral" : err >= 0.05 ? "critical" : err >= 0.01 ? "warning" : "good";

  return (
    <div className="flex flex-col gap-3">
      {s.health === "notReady" && (
        <Notice tone="critical" icon="✕">
          {t("Service đang ở trạng thái không ready.")}
          {s.healthMessage ? ` ${t("Cloud Run báo:")} ${s.healthMessage}` : ""}
          {"\n"}
          {t("Xem tab Logs và Revisions để tìm nguyên nhân.")}
        </Notice>
      )}

      {s.trafficPinned && (
        <Notice tone="warning" icon="📌">
          {t(
            "Traffic đang được ghim vào revision cụ thể thay vì LATEST. Mọi revision mới tạo ra (kể cả khi bạn sửa env) sẽ không nhận traffic cho tới khi traffic được chuyển sang.",
          )}
        </Notice>
      )}

      {/* Con số "bây giờ đang thế nào" là câu hỏi một giá trị, không phải một đường —
          nên đứng đầu dưới dạng tile, chart diễn biến để ở tab Metrics. */}
      <div className="grid grid-cols-5 gap-2">
        <StatTile
          label={t("Instance đang chạy")}
          value={inst === undefined ? "–" : compact(inst)}
          sub={
            s.minInstances !== null || s.maxInstances !== null
              ? t("scaling {min}–{max}", {
                  min: s.minInstances ?? 0,
                  max: s.maxInstances ?? "∞",
                })
              : t("scaling mặc định")
          }
        />
        <StatTile
          label={t("Request / giây")}
          value={rps === undefined ? "–" : compact(rps)}
          sub={t("30 phút gần nhất")}
        />
        <StatTile
          label={t("Tỉ lệ 5xx")}
          value={err === undefined ? "–" : percent(err, 2)}
          tone={errTone}
          icon={errTone === "good" ? "✓" : "⚠"}
          sub={t("30 phút gần nhất")}
        />
        <StatTile
          label={t("Concurrency")}
          value={detail.concurrency ?? "–"}
          sub={t("request / instance")}
        />
        <StatTile
          label={t("Timeout")}
          value={humanTimeout(detail.timeout)}
          sub={c?.cpu && c?.memory ? `${c.cpu} vCPU · ${c.memory}` : undefined}
        />
      </div>

      <div className="grid grid-cols-2 gap-3">
        <Card
          title={t("Service")}
          actions={
            <div className="flex items-center gap-1.5">
              {s.uri && (
                <Button size="sm" variant="ghost" onClick={() => void openUrl(s.uri as string)}>
                  {t("Mở URL ↗")}
                </Button>
              )}
              <Button
                size="sm"
                variant="ghost"
                onClick={() => void openUrl(consoleServiceUrl(project, s.region, s.name))}
              >
                GCP Console ↗
              </Button>
            </div>
          }
        >
          <KeyValue
            items={[
              [
                t("Trạng thái"),
                <span className="flex items-center gap-1.5">
                  <HealthDot health={s.health} message={s.healthMessage} />
                  {s.health === "ready"
                    ? t("Ready")
                    : s.health === "reconciling"
                      ? t("Đang triển khai")
                      : s.health === "notReady"
                        ? t("Không ready")
                        : t("Không rõ")}
                </span>,
              ],
              ["URL", s.uri ? <span className="mono break-all">{s.uri}</span> : "–"],
              [t("Region"), regionLabel(s.region)],
              [
                t("Revision đang chạy"),
                <span className="mono">{s.latestReadyRevision ?? "–"}</span>,
              ],
              [
                t("Image"),
                <span className="mono break-all" title={c?.image ?? undefined}>
                  {shortImage(c?.image ?? null)}
                </span>,
              ],
              [
                t("Service account"),
                <span className="mono break-all">
                  {detail.serviceAccount ?? t("(mặc định Compute)")}
                </span>,
              ],
              [
                t("Ingress"),
                detail.ingress
                  ? t(INGRESS_TEXT[detail.ingress] ?? detail.ingress)
                  : "–",
              ],
              [t("Port container"), c?.port ?? "–"],
              [
                t("Sửa lần cuối"),
                `${dateTime(s.updateTime)}${s.lastModifier ? ` · ${s.lastModifier}` : ""}`,
              ],
            ]}
          />
        </Card>

        <div className="flex flex-col gap-3">
          <Card title="Traffic">
            {detail.traffic.length === 0 ? (
              <p className="text-[12px] text-[var(--ink-muted)]">
                {t("Không khai báo traffic — 100% về revision mới nhất (mặc định của Cloud Run).")}
              </p>
            ) : (
              <ul className="flex flex-col gap-1.5">
                {/* Tên `tr`, không phải `t` — `t` đã là hàm dịch trong scope này. */}
                {detail.traffic.map((tr, i) => (
                  <li key={i} className="flex items-center gap-2 text-[12px]">
                    <span className="tnum w-10 shrink-0 text-right font-semibold">{tr.percent}%</span>
                    {/* Track có bề rộng cố định: thanh 100% mà cho co giãn theo flex sẽ
                        đẩy tên revision ra khỏi khung. */}
                    <span
                      className="h-2 w-20 shrink-0 overflow-hidden rounded-sm"
                      style={{ background: "var(--surface-2)" }}
                    >
                      <span
                        className="block h-full rounded-sm"
                        style={{
                          width: `${Math.max(tr.percent, 2)}%`,
                          background: tr.kind === "LATEST" ? "var(--series-1)" : "var(--series-2)",
                        }}
                      />
                    </span>
                    <span className="mono min-w-0 flex-1 truncate" title={tr.revision ?? "LATEST"}>
                      {tr.kind === "LATEST" ? "LATEST" : (tr.revision ?? "?")}
                    </span>
                    {tr.tag && <Badge tone="info">tag {tr.tag}</Badge>}
                  </li>
                ))}
              </ul>
            )}
          </Card>

          <Card title={t("Mạng & tích hợp")}>
            <KeyValue
              items={[
                [t("VPC connector"), detail.vpcConnector ?? "–"],
                [t("VPC egress"), detail.vpcEgress ?? "–"],
                [
                  t("Cloud SQL"),
                  detail.cloudsqlInstances.length > 0 ? (
                    <ul className="mono flex flex-col">
                      {detail.cloudsqlInstances.map((x) => (
                        <li key={x} className="break-all">
                          {x}
                        </li>
                      ))}
                    </ul>
                  ) : (
                    "–"
                  ),
                ],
                [t("Execution environment"), detail.executionEnvironment ?? "–"],
                [t("Launch stage"), detail.launchStage ?? "–"],
                [
                  t("Session affinity"),
                  detail.sessionAffinity === null
                    ? "–"
                    : detail.sessionAffinity
                      ? t("bật")
                      : t("tắt"),
                ],
              ]}
            />
          </Card>
        </div>
      </div>

      {detail.conditions.length > 0 && (
        <Card title={t("Condition từ Cloud Run")}>
          <ul className="flex flex-col gap-1">
            {detail.conditions.map((c2, i) => {
              const ok = c2.state === "CONDITION_SUCCEEDED";
              const pending = c2.state.includes("PENDING") || c2.state.includes("RECONCILING");
              return (
                <li key={i} className="flex flex-wrap items-baseline gap-2 text-[11px]">
                  <Badge tone={ok ? "good" : pending ? "warning" : "critical"} icon={ok ? "✓" : pending ? "◐" : "✕"}>
                    {c2.type}
                  </Badge>
                  {c2.reason && <span className="mono text-[var(--ink-muted)]">{c2.reason}</span>}
                  {c2.message && <span className="selectable flex-1">{c2.message}</span>}
                </li>
              );
            })}
          </ul>
        </Card>
      )}

      {(Object.keys(detail.labels).length > 0 || Object.keys(detail.annotations).length > 0) && (
        <Card title="Labels & annotations">
          <div className="grid grid-cols-2 gap-4">
            <div>
              <h3 className="mb-1 text-[11px] font-semibold text-[var(--ink-secondary)]">Labels</h3>
              <KeyValue
                items={
                  Object.keys(detail.labels).length === 0
                    ? [["", "–"]]
                    : Object.entries(detail.labels).map(([k, v]) => [
                        <span className="mono">{k}</span>,
                        <span className="mono">{v}</span>,
                      ])
                }
              />
            </div>
            <div>
              <h3 className="mb-1 text-[11px] font-semibold text-[var(--ink-secondary)]">Annotations</h3>
              <KeyValue
                items={
                  Object.keys(detail.annotations).length === 0
                    ? [["", "–"]]
                    : Object.entries(detail.annotations).map(([k, v]) => [
                        <span className="mono break-all">{k}</span>,
                        <span className="mono break-all">{v}</span>,
                      ])
                }
              />
            </div>
          </div>
        </Card>
      )}
    </div>
  );
}
