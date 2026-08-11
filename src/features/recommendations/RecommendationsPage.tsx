import { useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { StatTile } from "../../components/charts";
import { Badge, Button, EmptyState, ErrorBox, Input, Loading, Notice, Select, useToast } from "../../components/ui";
import { useT } from "../../lib/i18n";
import { apiV2, asCmdError } from "../../lib/ipc";
import type { CmdError, MarkAction, Recommendation, RecommendationsResult } from "../../lib/types";

/**
 * Màn Recommendations.
 *
 * Ràng buộc cốt lõi: **app không bao giờ tự áp dụng recommendation.** Recommender API cho
 * biết "nên giảm CPU service X" hay "role Y quá rộng", nhưng áp dụng thật (đổi scaling, sửa
 * IAM) là thao tác ảnh hưởng traffic/bảo mật, phải làm có chủ đích trên Console. Ở đây chỉ:
 *  - Xem danh sách gợi ý, sắp theo mức tiết kiệm / độ ưu tiên.
 *  - Đánh dấu trạng thái (Bỏ qua / Nhận xử lý) để lần sau khỏi thấy lại — CHỈ đổi trạng thái
 *    trên Recommender, không đụng tới tài nguyên.
 */

const CATEGORY_VI: Record<string, string> = {
  COST: "Chi phí",
  SECURITY: "Bảo mật",
  PERFORMANCE: "Hiệu năng",
  MANAGEABILITY: "Vận hành",
  RELIABILITY: "Độ tin cậy",
  SUSTAINABILITY: "Bền vững",
};

const PRIORITY_META: Record<string, { tone: "critical" | "warning" | "info" | "neutral"; text: string }> = {
  P1: { tone: "critical", text: "P1 · cao nhất" },
  P2: { tone: "warning", text: "P2 · cao" },
  P3: { tone: "info", text: "P3 · vừa" },
  P4: { tone: "neutral", text: "P4 · thấp" },
};

const CATEGORY_TONE: Record<string, "info" | "critical" | "warning" | "good" | "neutral"> = {
  COST: "good",
  SECURITY: "critical",
  PERFORMANCE: "info",
  RELIABILITY: "warning",
};

const USD = new Intl.NumberFormat("en-US", { style: "currency", currency: "USD", maximumFractionDigits: 2 });

export function RecommendationsPage({
  project,
  readOnly,
}: {
  project: string;
  readOnly: boolean;
}) {
  const t = useT();
  const qc = useQueryClient();
  const toast = useToast();
  const [filter, setFilter] = useState("");
  const [category, setCategory] = useState<string>("all");

  const q = useQuery<RecommendationsResult, CmdError>({
    queryKey: ["recommendations", project],
    queryFn: () => apiV2.recommendations(project),
    staleTime: 5 * 60_000,
    retry: false,
  });

  const items = q.data?.items ?? [];

  const stats = useMemo(() => {
    const saving = items
      .filter((r) => (r.monthlyCostImpact ?? 0) < 0)
      .reduce((a, r) => a + Math.abs(r.monthlyCostImpact ?? 0), 0);
    return {
      total: items.length,
      cost: items.filter((r) => r.category === "COST").length,
      security: items.filter((r) => r.category === "SECURITY").length,
      saving,
    };
  }, [items]);

  const rows = useMemo(() => {
    const f = filter.trim().toLowerCase();
    let list = items.filter((r) => {
      if (category !== "all" && r.category !== category) return false;
      if (!f) return true;
      return (
        r.description.toLowerCase().includes(f) ||
        (r.targetResource ?? "").toLowerCase().includes(f) ||
        r.recommender.toLowerCase().includes(f)
      );
    });
    // Tiết kiệm nhiều nhất lên đầu; sau đó tới độ ưu tiên.
    list = [...list].sort(
      (a, b) =>
        (a.monthlyCostImpact ?? 0) - (b.monthlyCostImpact ?? 0) ||
        (a.priority || "P9").localeCompare(b.priority || "P9"),
    );
    return list;
  }, [items, filter, category]);

  const refresh = () => void qc.invalidateQueries({ queryKey: ["recommendations", project] });

  if (q.isLoading) return <Loading label={t("Đang lấy recommendation từ các Recommender…")} />;

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto p-3">
      <ErrorBox error={q.error} onRetry={() => void q.refetch()} />

      <Notice tone="info" icon="ℹ">
        {t(
          "App chỉ đọc gợi ý và đánh dấu trạng thái. Áp dụng thật (đổi scaling, sửa IAM…) không làm ở đây — mở tài nguyên tương ứng trên Console để thao tác có kiểm soát.",
        )}
      </Notice>

      {q.data?.apiDisabled && (
        <Notice tone="warning" icon="⚠">
          {t(
            "Recommender API chưa được bật trên project này nên danh sách có thể thiếu. Bật tại",
          )}{" "}
          <a
            className="underline"
            href={`https://console.cloud.google.com/apis/library/recommender.googleapis.com?project=${encodeURIComponent(project)}`}
            target="_blank"
            rel="noreferrer"
          >
            API Library
          </a>
          .
        </Notice>
      )}

      {q.data && q.data.errors.length > 0 && (
        <Notice tone="warning" icon="⚠">
          {t("Một số recommender không lấy được:")}
          {"\n"}
          {q.data.errors.map((e) => `• ${e}`).join("\n")}
        </Notice>
      )}

      <div className="grid grid-cols-2 gap-2 lg:grid-cols-4">
        <StatTile label={t("Tổng gợi ý")} value={stats.total} />
        <StatTile
          label={t("Về chi phí")}
          value={stats.cost}
          tone={stats.cost > 0 ? "good" : "neutral"}
          icon="₫"
        />
        <StatTile
          label={t("Về bảo mật")}
          value={stats.security}
          tone={stats.security > 0 ? "critical" : "good"}
          icon={stats.security > 0 ? "⚠" : "✓"}
        />
        <StatTile
          label={t("Tiết kiệm được ≈")}
          value={USD.format(stats.saving)}
          tone="good"
          icon="≈"
          sub={t("mỗi tháng, theo GCP")}
        />
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <Input
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          placeholder={t("tìm theo mô tả, tài nguyên…")}
          className="w-72"
        />
        <Select value={category} onChange={(e) => setCategory(e.target.value)}>
          <option value="all">{t("Tất cả nhóm")}</option>
          <option value="COST">{t("Chi phí")}</option>
          <option value="SECURITY">{t("Bảo mật")}</option>
          <option value="PERFORMANCE">{t("Hiệu năng")}</option>
          <option value="RELIABILITY">{t("Độ tin cậy")}</option>
          <option value="MANAGEABILITY">{t("Vận hành")}</option>
        </Select>
        <span className="text-[11px] text-[var(--ink-muted)]">
          {t("{shown}/{total} gợi ý", { shown: rows.length, total: stats.total })}
        </span>
        <Button size="sm" variant="ghost" className="ml-auto" loading={q.isFetching} onClick={() => void q.refetch()}>
          ⟳ Reload
        </Button>
      </div>

      {rows.length === 0 ? (
        <EmptyState
          icon="✓"
          title={t("Không có gợi ý nào")}
          hint={t(
            "Hoặc project đang tối ưu tốt, hoặc Recommender chưa đủ dữ liệu (cần vài ngày quan sát).",
          )}
        />
      ) : (
        <div className="flex flex-col gap-2">
          {rows.map((r) => (
            <RecCard key={r.fullName} rec={r} project={project} readOnly={readOnly} onDone={refresh} toast={toast} />
          ))}
        </div>
      )}
    </div>
  );
}

function RecCard({
  rec,
  project,
  readOnly,
  onDone,
  toast,
}: {
  rec: Recommendation;
  project: string;
  readOnly: boolean;
  onDone: () => void;
  toast: (t: { tone: "good" | "critical" | "warning" | "info"; title: string; body?: string }) => void;
}) {
  const t = useT();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<CmdError | null>(null);

  const saving = (rec.monthlyCostImpact ?? 0) < 0 ? Math.abs(rec.monthlyCostImpact ?? 0) : null;
  const extra = (rec.monthlyCostImpact ?? 0) > 0 ? rec.monthlyCostImpact : null;
  const prio = PRIORITY_META[rec.priority] ?? { tone: "neutral" as const, text: rec.priority || "–" };
  const alreadyMarked = rec.state !== "ACTIVE";

  const mark = async (action: MarkAction, title: string) => {
    setBusy(true);
    setError(null);
    try {
      await apiV2.markRecommendation({ project, fullName: rec.fullName, etag: rec.etag, action });
      toast({ tone: "good", title });
      onDone();
    } catch (e) {
      setError(asCmdError(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="rounded-lg border p-3" style={{ background: "var(--surface-1)" }}>
      <div className="mb-1.5 flex flex-wrap items-center gap-2">
        <Badge tone={CATEGORY_TONE[rec.category] ?? "neutral"}>
          {t(CATEGORY_VI[rec.category] ?? rec.category)}
        </Badge>
        <Badge tone={prio.tone}>{t(prio.text)}</Badge>
        {saving !== null && (
          <Badge tone="good" icon="↓">
            {t("tiết kiệm ≈ {amount}/tháng", { amount: USD.format(saving) })}
          </Badge>
        )}
        {extra !== null && (
          <Badge tone="warning" icon="↑">
            {t("tăng ≈ {amount}/tháng", { amount: USD.format(extra) })}
          </Badge>
        )}
        {alreadyMarked && (
          <Badge tone="neutral">{t("đã đánh dấu: {state}", { state: rec.state })}</Badge>
        )}
        <span className="ml-auto text-[10px] text-[var(--ink-muted)]">{rec.location}</span>
      </div>

      <p className="selectable text-[13px] leading-relaxed">{rec.description}</p>

      <div className="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-[var(--ink-muted)]">
        {rec.targetResource && (
          <span className="mono selectable" title={t("Tài nguyên bị ảnh hưởng")}>
            🎯 {rec.targetResource}
          </span>
        )}
        <span className="mono">{rec.recommender}</span>
      </div>

      {error && (
        <div className="mt-2">
          <ErrorBox error={error} />
        </div>
      )}

      <div className="mt-2 flex items-center gap-2">
        {readOnly && (
          <span className="text-[11px] text-[var(--ink-muted)]">
            {t("Chế độ chỉ đọc — bật “Cho ghi” để đánh dấu trạng thái.")}
          </span>
        )}
        <div className="ml-auto flex items-center gap-2">
          <Button
            size="sm"
            variant="ghost"
            disabled={readOnly || busy}
            title={t(
              "Đánh dấu đã nhận xử lý (claimed) — chỉ đổi trạng thái, không áp dụng gì",
            )}
            onClick={() => void mark("claimed", t("Đã đánh dấu nhận xử lý"))}
          >
            {t("Nhận xử lý")}
          </Button>
          <Button
            size="sm"
            variant="secondary"
            disabled={readOnly || busy}
            loading={busy}
            title={t("Bỏ qua gợi ý này — lần sau không hiện lại")}
            onClick={() => void mark("dismissed", t("Đã bỏ qua gợi ý"))}
          >
            {t("Bỏ qua")}
          </Button>
        </div>
      </div>
    </div>
  );
}
