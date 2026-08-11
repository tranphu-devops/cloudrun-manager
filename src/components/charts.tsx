/**
 * Chart component.
 *
 * Bảng màu và quy tắc mark lấy từ skill dataviz; bộ 4 slot đầu đã chạy qua
 * `validate_palette.js` và PASS ở cả light lẫn dark trên pairlist adjacent (line, stack).
 * Ba điểm bắt buộc được cài sẵn ở đây, đừng bỏ khi sửa:
 *
 *  1. **Màu gắn với thực thể, không gắn với thứ hạng.** `seriesColor()` tra theo tên
 *     series, nên bật/tắt một series không làm những series còn lại đổi màu.
 *  2. **Không bao giờ có hai trục y.** Hai đại lượng khác đơn vị thì tách thành hai chart.
 *  3. **Có nhãn/bảng số kèm theo.** Light mode có 2 slot dưới 3:1 contrast, nên legend
 *     luôn hiện và mỗi chart có nút "Bảng số" — màu không bao giờ là kênh duy nhất.
 */

import { useMemo, useState, type ReactNode } from "react";
import {
  Area,
  AreaChart,
  CartesianGrid,
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";

import { cn, compact, num, timeAxis, timeTooltip } from "../lib/format";
import type { ChartData, SeriesData } from "../lib/types";
import { useT } from "../lib/i18n";
import { Badge, Button, EmptyState } from "./ui";

/** Thứ tự slot là cơ chế bảo đảm an toàn cho người mù màu — không đảo, không sinh thêm. */
const SLOTS = [
  "var(--series-1)",
  "var(--series-2)",
  "var(--series-3)",
  "var(--series-4)",
  "var(--series-5)",
  "var(--series-6)",
  "var(--series-7)",
  "var(--series-8)",
] as const;

/**
 * Màu theo tên series, cố định theo thứ tự xuất hiện lần đầu trong `order`.
 *
 * Nếu chấm màu theo index của mảng đang render thì lọc bỏ một series sẽ khiến những
 * series còn lại bị sơn lại màu khác — người đọc mất mốc so sánh giữa hai lần xem.
 */
function seriesColor(label: string, order: string[]): string {
  const i = order.indexOf(label);
  return SLOTS[i >= 0 ? Math.min(i, SLOTS.length - 1) : 0] as string;
}

/**
 * Nhãn dễ đọc cho những label kỹ thuật hay gặp.
 *
 * Giá trị ở đây là **key dịch**, không phải chuỗi hiển thị — chỗ nào render thì bọc `t()`.
 */
const LABEL_VI: Record<string, string> = {
  active: "đang xử lý",
  idle: "rảnh (idle)",
  value: "giá trị",
  "2xx": "2xx",
  "3xx": "3xx",
  "4xx": "4xx",
  "5xx": "5xx",
  unknown: "không rõ",
};

function labelOf(s: string) {
  return LABEL_VI[s] ?? s;
}

type Row = { t: number } & Record<string, number>;

function toRows(series: SeriesData[]): { rows: Row[]; order: string[] } {
  const byTime = new Map<number, Row>();
  const order: string[] = [];

  for (const s of series) {
    if (!order.includes(s.label)) order.push(s.label);
    for (const p of s.points) {
      let row = byTime.get(p.t);
      if (!row) {
        row = { t: p.t } as Row;
        byTime.set(p.t, row);
      }
      row[s.label] = p.v;
    }
  }

  return {
    rows: [...byTime.values()].sort((a, b) => a.t - b.t),
    order,
  };
}

// ---------------------------------------------------------------------------
// Khung chart: tiêu đề, legend, trạng thái rỗng, bảng số
// ---------------------------------------------------------------------------

function Legend({ order, unit }: { order: string[]; unit: string }) {
  const t = useT();
  // Một series thì tiêu đề đã nói rõ là gì — thêm legend chỉ là nhiễu.
  if (order.length < 2) return null;
  return (
    <ul className="flex flex-wrap items-center gap-x-3 gap-y-1">
      {order.map((label) => (
        <li key={label} className="flex items-center gap-1.5 text-[11px] text-[var(--ink-secondary)]">
          <span
            aria-hidden
            className="inline-block h-2 w-2 rounded-sm"
            style={{ background: seriesColor(label, order) }}
          />
          {t(labelOf(label))}
          {unit && <span className="text-[var(--ink-muted)]">({unit})</span>}
        </li>
      ))}
    </ul>
  );
}

function DataTable({
  rows,
  order,
  unit,
  windowMinutes,
}: {
  rows: Row[];
  order: string[];
  unit: string;
  windowMinutes: number;
}) {
  const t = useT();
  // Mới nhất lên đầu: khi đọc bảng người ta hỏi "bây giờ đang bao nhiêu" trước.
  const recent = [...rows].reverse().slice(0, 60);
  return (
    <div className="max-h-64 overflow-auto rounded border">
      <table className="w-full text-[11px]">
        <thead className="sticky top-0" style={{ background: "var(--surface-2)" }}>
          <tr>
            <th className="px-2 py-1 text-left font-medium">{t("Thời điểm")}</th>
            {order.map((o) => (
              <th key={o} className="px-2 py-1 text-right font-medium">
                {t(labelOf(o))} {unit && <span className="text-[var(--ink-muted)]">({unit})</span>}
              </th>
            ))}
          </tr>
        </thead>
        <tbody className="tnum">
          {recent.map((r) => (
            <tr key={r.t} className="border-t">
              <td className="px-2 py-1 whitespace-nowrap">{timeAxis(r.t, windowMinutes)}</td>
              {order.map((o) => (
                <td key={o} className="px-2 py-1 text-right">
                  {r[o] === undefined ? "–" : num(r[o], unit === "%" ? 1 : 2)}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function TooltipCard({
  active,
  label,
  payload,
  unit,
  order,
}: {
  active?: boolean;
  label?: number | string;
  payload?: Array<{ name?: string; value?: number }>;
  unit: string;
  order: string[];
}) {
  // Hook phải đứng trước return sớm.
  const t = useT();
  if (!active || !payload || payload.length === 0) return null;
  return (
    <div
      className="rounded-md border px-2.5 py-2 text-[11px] shadow-lg"
      style={{ background: "var(--surface-1)" }}
    >
      <p className="mb-1 font-medium text-[var(--ink-secondary)]">
        {typeof label === "number" ? timeTooltip(label) : String(label ?? "")}
      </p>
      <ul className="tnum flex flex-col gap-0.5">
        {payload.map((p, i) => (
          <li key={i} className="flex items-center justify-between gap-3">
            <span className="flex items-center gap-1.5">
              <span
                aria-hidden
                className="inline-block h-2 w-2 rounded-sm"
                style={{ background: seriesColor(p.name ?? "", order) }}
              />
              {t(labelOf(p.name ?? ""))}
            </span>
            <span className="font-medium">
              {num(p.value ?? null, unit === "%" ? 1 : 2)}
              {unit && <span className="ml-0.5 text-[var(--ink-muted)]">{unit}</span>}
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}

const AXIS_STYLE = { fontSize: 10, fill: "var(--ink-muted)" } as const;

export function TimeChart({
  title,
  hint,
  data,
  windowMinutes,
  stacked = false,
  height = 170,
  yDomainMax,
}: {
  title: string;
  hint?: ReactNode;
  data: ChartData;
  windowMinutes: number;
  stacked?: boolean;
  height?: number;
  yDomainMax?: number;
}) {
  const t = useT();
  const [showTable, setShowTable] = useState(false);
  const { rows, order } = useMemo(() => toRows(data.series), [data.series]);

  const header = (
    <div className="mb-2 flex items-start justify-between gap-2">
      <div className="min-w-0">
        <h3 className="text-[12px] font-semibold">{title}</h3>
        {hint && <p className="text-[11px] text-[var(--ink-muted)]">{hint}</p>}
      </div>
      <div className="flex shrink-0 items-center gap-2">
        <Legend order={order} unit={data.unit} />
        {rows.length > 0 && (
          <Button size="sm" variant="ghost" onClick={() => setShowTable((v) => !v)}>
            {showTable ? t("Xem chart") : t("Bảng số")}
          </Button>
        )}
      </div>
    </div>
  );

  // Không lấy được dữ liệu KHÁC với dữ liệu bằng 0. Vẽ đường phẳng ở 0 cho trường hợp
  // đầu sẽ bị đọc thành "service không có tải" — sai lệch nguy hiểm khi đang xử lý sự cố.
  if (data.unavailable) {
    return (
      <div>
        {header}
        <div
          className="flex flex-col items-center justify-center gap-2 rounded border border-dashed px-3 text-center"
          style={{ height }}
        >
          <Badge tone="warning" icon="⚠">
            {t("Không lấy được metric")}
          </Badge>
          <p className="max-w-md text-[11px] leading-relaxed text-[var(--ink-muted)]">
            {data.note ?? t("Monitoring API không trả về dữ liệu cho metric này.")}
          </p>
          <p className="mono text-[10px] text-[var(--ink-muted)]">{data.metric}</p>
        </div>
      </div>
    );
  }

  if (rows.length === 0) {
    return (
      <div>
        {header}
        <div className="rounded border border-dashed" style={{ height }}>
          <EmptyState
            icon="—"
            title={t("Không có dữ liệu trong khoảng này")}
            hint={
              data.note ??
              t(
                "Cloud Run chỉ ghi metric khi có hoạt động, nên service đang không nhận request sẽ trống.",
              )
            }
          />
        </div>
      </div>
    );
  }

  if (showTable) {
    return (
      <div>
        {header}
        <DataTable rows={rows} order={order} unit={data.unit} windowMinutes={windowMinutes} />
      </div>
    );
  }

  const commonAxes = (
    <>
      {/* Grid và trục phải lùi về sau, dữ liệu mới là thứ được nhìn. */}
      <CartesianGrid stroke="var(--grid)" strokeWidth={1} vertical={false} />
      <XAxis
        dataKey="t"
        type="number"
        scale="time"
        domain={["dataMin", "dataMax"]}
        tickFormatter={(t: number) => timeAxis(t, windowMinutes)}
        tick={AXIS_STYLE}
        stroke="var(--axis)"
        strokeWidth={1}
        minTickGap={44}
      />
      <YAxis
        tick={AXIS_STYLE}
        stroke="var(--axis)"
        strokeWidth={1}
        width={46}
        tickFormatter={(v: number) => compact(v)}
        domain={[0, yDomainMax ?? "auto"]}
      />
      <Tooltip
        // Crosshair dọc: chart thời gian luôn cần biết "cột này là lúc nào".
        cursor={{ stroke: "var(--axis)", strokeWidth: 1 }}
        content={<TooltipCard unit={data.unit} order={order} />}
      />
    </>
  );

  return (
    <div>
      {header}
      <div style={{ height }}>
        <ResponsiveContainer width="100%" height="100%">
          {stacked ? (
            <AreaChart data={rows} margin={{ top: 4, right: 8, bottom: 0, left: 0 }}>
              {commonAxes}
              {order.map((label) => (
                <Area
                  key={label}
                  type="monotone"
                  dataKey={label}
                  name={label}
                  stackId="1"
                  fill={seriesColor(label, order)}
                  fillOpacity={0.85}
                  // Viền màu nền tạo khe 2px giữa các lớp — ranh giới đọc được mà
                  // không cần thêm màu nào.
                  stroke="var(--surface-1)"
                  strokeWidth={2}
                  isAnimationActive={false}
                />
              ))}
            </AreaChart>
          ) : (
            <LineChart data={rows} margin={{ top: 4, right: 8, bottom: 0, left: 0 }}>
              {commonAxes}
              {order.map((label) => (
                <Line
                  key={label}
                  type="monotone"
                  dataKey={label}
                  name={label}
                  stroke={seriesColor(label, order)}
                  strokeWidth={2}
                  dot={false}
                  // Điểm hover ≥8px để dễ nhắm bằng chuột.
                  activeDot={{ r: 4, strokeWidth: 2, stroke: "var(--surface-1)" }}
                  isAnimationActive={false}
                  connectNulls
                />
              ))}
            </LineChart>
          )}
        </ResponsiveContainer>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Stat tile
// ---------------------------------------------------------------------------

/**
 * Ô số liệu.
 *
 * Có những câu hỏi không cần chart: "đang có mấy instance" là một con số, không phải
 * một đường. Tile trả lời trực tiếp; chart bên dưới để xem diễn biến.
 */
export function StatTile({
  label,
  value,
  unit,
  sub,
  tone = "neutral",
  icon,
}: {
  label: string;
  value: ReactNode;
  unit?: string;
  sub?: ReactNode;
  tone?: "neutral" | "good" | "warning" | "critical";
  icon?: string;
}) {
  const color =
    tone === "good"
      ? "var(--status-good)"
      : tone === "warning"
        ? "var(--status-warning)"
        : tone === "critical"
          ? "var(--status-critical)"
          : "var(--ink-primary)";

  return (
    <div className="rounded-lg border px-3 py-2" style={{ background: "var(--surface-1)" }}>
      <p className="text-[11px] text-[var(--ink-muted)]">{label}</p>
      <p className="tnum mt-0.5 flex items-baseline gap-1 text-[20px] font-semibold leading-none">
        {/* Màu trạng thái luôn đi kèm icon + chữ, không bao giờ là kênh duy nhất. */}
        {icon && tone !== "neutral" && (
          <span aria-hidden style={{ color, fontSize: 13 }}>
            {icon}
          </span>
        )}
        <span style={{ color }}>{value}</span>
        {unit && <span className="text-[11px] font-normal text-[var(--ink-muted)]">{unit}</span>}
      </p>
      {sub && <p className="mt-1 text-[11px] text-[var(--ink-secondary)]">{sub}</p>}
    </div>
  );
}

/** Sparkline nhỏ cho sidebar — không trục, không tooltip, chỉ hình dáng. */
export function Sparkline({
  points,
  width = 44,
  height = 14,
  color = "var(--series-1)",
}: {
  points: number[];
  width?: number;
  height?: number;
  color?: string;
}) {
  if (points.length < 2) return <span className="inline-block" style={{ width, height }} />;

  const max = Math.max(...points, 0.0001);
  const step = width / (points.length - 1);
  const d = points
    .map((v, i) => `${i === 0 ? "M" : "L"}${(i * step).toFixed(1)},${(height - (v / max) * height).toFixed(1)}`)
    .join(" ");

  return (
    <svg width={width} height={height} aria-hidden className={cn("shrink-0 overflow-visible")}>
      <path d={d} fill="none" stroke={color} strokeWidth={1.5} strokeLinejoin="round" />
    </svg>
  );
}
