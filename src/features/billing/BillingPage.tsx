import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { StatTile } from "../../components/charts";
import { Badge, Button, Card, EmptyState, ErrorBox, Input, Loading, Notice, Select } from "../../components/ui";
import { num, regionLabel } from "../../lib/format";
import { useT } from "../../lib/i18n";
import { apiV2 } from "../../lib/ipc";
import type { CmdError, CostReport, CostRow } from "../../lib/types";

/**
 * Màn Billing.
 *
 * Con số ở đây **luôn là ước lượng** — Cloud Run tính tiền theo vCPU-giây và GiB-giây thực
 * tế, còn app chỉ có metric tải cộng đơn giá công khai, không có hoá đơn thật. Vì thế:
 *  - Mọi tổng đều gắn nhãn "ước lượng".
 *  - Bảy nguồn sai số hiện thẳng trên màn, không giấu trong doc — người đọc phải biết con
 *    số này sai lệch ở đâu trước khi mang đi báo cáo.
 *  - Có link sang Cloud Billing để đối chiếu số thật.
 *
 * Ràng buộc tính tiền: request-based (cpuIdle=true) rẻ hơn instance-based (~10 lần đơn giá
 * CPU). Cột "Kiểu tính tiền" nói rõ mỗi service đang ở kiểu nào — vì đây là đòn bẩy tối ưu
 * chi phí lớn nhất.
 */

const USD = new Intl.NumberFormat("en-US", { style: "currency", currency: "USD", maximumFractionDigits: 2 });
const USD4 = new Intl.NumberFormat("en-US", { style: "currency", currency: "USD", maximumFractionDigits: 4 });

/** Tiền nhỏ cần nhiều số lẻ hơn để không bị làm tròn về $0.00. */
function money(v: number): string {
  if (v === 0) return "$0";
  if (Math.abs(v) < 1) return USD4.format(v);
  return USD.format(v);
}

type SortKey = "cost" | "cpu" | "name";

export function BillingPage({ project }: { project: string }) {
  const t = useT();
  const [sortBy, setSortBy] = useState<SortKey>("cost");
  const [filter, setFilter] = useState("");
  const [kindFilter, setKindFilter] = useState<"all" | "service" | "job">("all");

  const q = useQuery<CostReport, CmdError>({
    queryKey: ["cost", project],
    queryFn: () => apiV2.costReport(project),
    staleTime: 60_000,
    retry: false,
  });

  const rows = useMemo(() => {
    const f = filter.trim().toLowerCase();
    let list = (q.data?.rows ?? []).filter((r) => {
      if (kindFilter !== "all" && r.kind !== kindFilter) return false;
      if (!f) return true;
      return r.name.toLowerCase().includes(f) || r.region.toLowerCase().includes(f);
    });
    list = [...list].sort((a, b) => {
      switch (sortBy) {
        case "cpu":
          return b.estimate.vcpuSeconds - a.estimate.vcpuSeconds || a.name.localeCompare(b.name);
        case "name":
          return a.name.localeCompare(b.name);
        default:
          return b.estimate.total - a.estimate.total || a.name.localeCompare(b.name);
      }
    });
    return list;
  }, [q.data, filter, sortBy, kindFilter]);

  const report = q.data;

  if (q.isLoading) return <Loading label={t("Đang ước lượng chi phí từ metric tải…")} />;

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto p-3">
      <ErrorBox error={q.error} onRetry={() => void q.refetch()} />

      {/* Nhãn "ước lượng" đặt cao nhất, trước cả số — để không ai đọc con số mà quên nó là ước lượng. */}
      <Notice tone="warning" icon="≈">
        <strong>{t("Đây là ước lượng, không phải hoá đơn.")}</strong>{" "}
        {t(
          "Con số suy từ metric tải × đơn giá công khai của Cloud Run, chưa gồm committed-use discount, network egress, hay chi phí service khác (Cloud SQL, Secret Manager…). Đối chiếu số thật ở",
        )}{" "}
        <a
          className="underline"
          href={`https://console.cloud.google.com/billing/linkedaccount?project=${encodeURIComponent(project)}`}
          target="_blank"
          rel="noreferrer"
        >
          Cloud Billing
        </a>
        .
      </Notice>

      {report?.usageUnavailable && (
        <Notice tone="critical" icon="⚠">
          {t(
            "Không lấy được metric tải cho project này (thiếu quyền Monitoring hoặc chưa bật API). Con số bên dưới suy từ cấu hình min-instances và giả định tải mặc định — sai lệch lớn hơn bình thường.",
          )}{" "}
          {report.note}
        </Notice>
      )}

      <div className="grid grid-cols-2 gap-2 lg:grid-cols-4">
        <StatTile
          label={t("Ước lượng / ngày")}
          value={money(report?.totalPerDay ?? 0)}
          tone="warning"
          icon="≈"
        />
        <StatTile
          label={t("Ước lượng / tháng")}
          value={money(report?.totalPerMonth ?? 0)}
          tone="warning"
          icon="≈"
          sub={t("× 30 ngày")}
        />
        <StatTile
          label={t("Free tier bù được")}
          value={report ? `−${money(report.freeTier.maxSaving).replace("$", "$")}` : "–"}
          tone="good"
          icon="✓"
          sub={t("đã trừ khỏi số trên")}
        />
        <StatTile
          label={t("Dòng chi phí")}
          value={num(report?.rows.length ?? 0)}
          sub={t("service + job")}
        />
      </div>

      {report && report.warnings.length > 0 && (
        <Notice tone="warning" icon="⚠">
          {report.warnings.join("\n")}
        </Notice>
      )}

      <div className="flex flex-wrap items-center gap-2">
        <Input
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          placeholder={t("tìm theo tên, region…")}
          className="w-64"
        />
        <Select value={kindFilter} onChange={(e) => setKindFilter(e.target.value as typeof kindFilter)}>
          <option value="all">{t("Tất cả")}</option>
          <option value="service">{t("Chỉ service")}</option>
          <option value="job">{t("Chỉ job")}</option>
        </Select>
        <Select value={sortBy} onChange={(e) => setSortBy(e.target.value as SortKey)}>
          <option value="cost">{t("Sắp xếp: chi phí ước lượng")}</option>
          <option value="cpu">{t("Sắp xếp: vCPU-giây")}</option>
          <option value="name">{t("Sắp xếp: tên")}</option>
        </Select>
        <span className="text-[11px] text-[var(--ink-muted)]">
          {t("{n} dòng · cửa sổ {win} phút", {
            n: rows.length,
            win: report?.windowMinutes ?? "–",
          })}
        </span>
        <Button size="sm" variant="ghost" className="ml-auto" loading={q.isFetching} onClick={() => void q.refetch()}>
          ⟳ Reload
        </Button>
      </div>

      {rows.length === 0 ? (
        <EmptyState icon="₫" title={t("Không có dòng chi phí nào khớp")} />
      ) : (
        <div className="overflow-x-auto rounded-lg border" style={{ background: "var(--surface-1)" }}>
          <table className="w-full text-[11px]">
            <thead style={{ background: "var(--surface-2)" }}>
              <tr className="text-left">
                <th className="px-2 py-1.5 font-medium">{t("Tên")}</th>
                <th className="px-2 py-1.5 font-medium">{t("Kiểu tính tiền")}</th>
                <th className="px-2 py-1.5 font-medium">{t("Resource")}</th>
                <th className="tnum px-2 py-1.5 text-right font-medium">CPU (≈)</th>
                <th className="tnum px-2 py-1.5 text-right font-medium">RAM (≈)</th>
                <th className="tnum px-2 py-1.5 text-right font-medium">Request (≈)</th>
                <th className="tnum px-2 py-1.5 text-right font-medium">{t("Tổng ≈/ngày")}</th>
                <th className="px-2 py-1.5 font-medium">{t("Vì sao tốn")}</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => (
                <CostRowView key={`${r.kind}/${r.region}/${r.name}`} r={r} />
              ))}
            </tbody>
          </table>
        </div>
      )}

      {/* Bảy nguồn sai số — bắt buộc hiện trên UI, không cất trong doc. */}
      {report && (
        <Card
          title={t("Bảy nguồn sai số của ước lượng này ({count})", {
            count: report.errorSources.length,
          })}
        >
          <ol className="flex list-decimal flex-col gap-1.5 pl-5 text-[12px] leading-relaxed">
            {report.errorSources.map((s, i) => (
              <li key={i} className="selectable">
                {s}
              </li>
            ))}
          </ol>
        </Card>
      )}
    </div>
  );
}

function CostRowView({ r }: { r: CostRow }) {
  const t = useT();
  const instanceBased = r.mode === "instanceBased";
  return (
    <tr className="border-t align-top hover:bg-[var(--surface-2)]">
      <td className="px-2 py-1.5">
        <div className="mono font-medium">{r.name}</div>
        <div className="text-[10px] text-[var(--ink-muted)]">
          {r.kind === "job" ? "job" : "service"} · {regionLabel(r.region)}
          {r.tier2Region && " · tier-2"}
        </div>
      </td>
      <td className="px-2 py-1.5 whitespace-nowrap">
        <Badge
          tone={instanceBased ? "warning" : "good"}
          title={
            instanceBased
              ? t("Instance-based: tính CPU cả khi rảnh, đơn giá CPU cao hơn ~10 lần")
              : t("Request-based: chỉ tính CPU khi xử lý request (cpuIdle=true)")
          }
        >
          {r.modeLabel}
        </Badge>
      </td>
      <td className="mono px-2 py-1.5 whitespace-nowrap text-[var(--ink-muted)]">
        {r.cpu ?? "–"} / {r.memory ?? "–"}
      </td>
      <td className="tnum px-2 py-1.5 text-right">{money(r.estimate.cpuCost)}</td>
      <td className="tnum px-2 py-1.5 text-right">{money(r.estimate.memoryCost)}</td>
      <td className="tnum px-2 py-1.5 text-right">{money(r.estimate.requestCost)}</td>
      <td className="tnum px-2 py-1.5 text-right font-semibold">{money(r.perDay)}</td>
      <td className="px-2 py-1.5 text-[var(--ink-secondary)]">
        {r.drivers.length === 0 ? (
          <span className="text-[var(--ink-muted)]">–</span>
        ) : (
          <ul className="flex flex-col gap-0.5">
            {r.drivers.map((d, i) => (
              <li key={i}>• {d}</li>
            ))}
          </ul>
        )}
      </td>
    </tr>
  );
}
