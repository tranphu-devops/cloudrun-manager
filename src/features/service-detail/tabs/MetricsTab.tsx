import { TimeChart } from "../../../components/charts";
import { Card, ErrorBox, Loading, Notice, Select } from "../../../components/ui";
import { useT, useTNode } from "../../../lib/i18n";
import { useCharts } from "../../../lib/queries";
import type { ChartData } from "../../../lib/types";

const WINDOWS: Array<{ minutes: number; label: string }> = [
  { minutes: 60, label: "1 giờ" },
  { minutes: 360, label: "6 giờ" },
  { minutes: 1440, label: "24 giờ" },
  { minutes: 4320, label: "3 ngày" },
  { minutes: 10080, label: "7 ngày" },
];

export function MetricsTab({
  project,
  region,
  service,
  minutes,
  onMinutesChange,
  autoRefreshMs,
}: {
  project: string;
  region: string;
  service: string;
  minutes: number;
  onMinutesChange: (m: number) => void;
  autoRefreshMs: number;
}) {
  const t = useT();
  const tNode = useTNode();
  const q = useCharts(project, region, service, minutes, true, autoRefreshMs);

  const align = q.data?.alignmentSeconds ?? 60;
  const alignLabel =
    align >= 3600
      ? t("{n} giờ", { n: align / 3600 })
      : align >= 60
        ? t("{n} phút", { n: align / 60 })
        : `${align}s`;

  return (
    <div className="flex flex-col gap-3">
      {/* Filter đứng một hàng ngay trên chart, không rải rác. */}
      <div className="flex items-center gap-2">
        <Select
          value={String(minutes)}
          onChange={(e) => onMinutesChange(Number(e.target.value))}
          aria-label={t("Khoảng thời gian")}
        >
          {WINDOWS.map((w) => (
            <option key={w.minutes} value={w.minutes}>
              {t(w.label)}
            </option>
          ))}
        </Select>
        <span className="text-[11px] text-[var(--ink-muted)]">
          {t("mỗi điểm = {align}", { align: alignLabel })}
          {q.isFetching && t(" · đang cập nhật…")}
        </span>
      </div>

      <ErrorBox error={q.error} onRetry={() => void q.refetch()} />
      {q.isLoading && <Loading label={t("Đang lấy metric…")} />}

      {q.data && (
        <>
          {[q.data.instances, q.data.rps, q.data.cpu].every((c) => c.unavailable) && (
            <Notice tone="warning" icon="⚠">
              {tNode(
                "Không lấy được metric nào. Thường là thiếu {role} trên project, hoặc Monitoring API chưa được enable. Vào Cài đặt → “Đối chiếu với metricDescriptors” để kiểm tra chính xác.",
                { role: <strong>roles/monitoring.viewer</strong> },
              )}
            </Notice>
          )}

          <div className="grid grid-cols-2 gap-3">
            <Card>
              <TimeChart
                title={t("Số instance")}
                hint={t(
                  "Tách theo trạng thái: instance idle vẫn tính tiền nếu bật CPU always-allocated.",
                )}
                data={q.data.instances}
                windowMinutes={minutes}
              />
            </Card>

            <Card>
              <TimeChart
                title={t("Request / giây")}
                data={q.data.rps}
                windowMinutes={minutes}
              />
            </Card>

            <Card>
              <TimeChart
                title={t("Request theo nhóm response code")}
                hint={t("Xếp lớp: tổng chiều cao là tổng request.")}
                data={q.data.byClass}
                windowMinutes={minutes}
                stacked
              />
            </Card>

            <Card>
              {/* Ba percentile cùng đơn vị ms → một chart, một trục. Không bao giờ hai trục y. */}
              <TimeChart
                title={t("Latency p50 / p95 / p99")}
                hint={t("Đơn vị ms.")}
                data={mergeLatency(q.data.latencyP50, q.data.latencyP95, q.data.latencyP99)}
                windowMinutes={minutes}
              />
            </Card>

            <Card>
              <TimeChart
                title={t("CPU & Memory utilization (p99)")}
                hint={t("Cùng đơn vị % nên dùng chung một trục.")}
                data={mergeUtil(q.data.cpu, q.data.memory)}
                windowMinutes={minutes}
                yDomainMax={100}
              />
            </Card>

            <Card>
              <TimeChart
                title={t("Startup latency (p95)")}
                hint={t("Thời gian container khởi động — cold start. Đơn vị ms.")}
                data={q.data.startup}
                windowMinutes={minutes}
              />
            </Card>
          </div>
        </>
      )}
    </div>
  );
}

/** Gộp ba chart percentile thành một chart ba series (cùng đơn vị ms). */
function mergeLatency(p50: ChartData, p95: ChartData, p99: ChartData): ChartData {
  const all = [p50, p95, p99];
  return {
    metric: "run.googleapis.com/request_latencies",
    unit: "ms",
    // Chỉ coi là unavailable khi cả ba đều lỗi — một percentile thiếu không nên
    // xoá cả chart.
    unavailable: all.every((c) => c.unavailable),
    note: all.find((c) => c.note)?.note ?? null,
    series: [
      ...(p50.series[0] ? [{ label: "p50", points: p50.series[0].points }] : []),
      ...(p95.series[0] ? [{ label: "p95", points: p95.series[0].points }] : []),
      ...(p99.series[0] ? [{ label: "p99", points: p99.series[0].points }] : []),
    ],
  };
}

function mergeUtil(cpu: ChartData, mem: ChartData): ChartData {
  return {
    metric: "run.googleapis.com/container/{cpu,memory}/utilizations",
    unit: "%",
    unavailable: cpu.unavailable && mem.unavailable,
    note: cpu.note ?? mem.note ?? null,
    series: [
      ...(cpu.series[0] ? [{ label: "CPU", points: cpu.series[0].points }] : []),
      ...(mem.series[0] ? [{ label: "Memory", points: mem.series[0].points }] : []),
    ],
  };
}
