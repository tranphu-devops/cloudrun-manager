import { Badge, ErrorBox, Loading, Notice } from "../../../components/ui";
import { HealthDot } from "../../service-list/Sidebar";
import { ago, dateTime, shortImage, shortSha } from "../../../lib/format";
import { useRevisions } from "../../../lib/queries";

export function RevisionsTab({
  project,
  region,
  service,
}: {
  project: string;
  region: string;
  service: string;
}) {
  const q = useRevisions(project, region, service);

  return (
    <div className="flex flex-col gap-3">
      <Notice tone="info" icon="ℹ">
        Tab này chỉ để xem. App v1 không chuyển traffic và không rollback — hai thao tác đó ảnh hưởng
        trực tiếp tới traffic đang chạy nên để trên GCP Console, nơi có sẵn xác nhận và audit của
        Google.
      </Notice>

      <ErrorBox error={q.error} onRetry={() => void q.refetch()} />
      {q.isLoading && <Loading label="Đang lấy revision…" />}

      {q.data && q.data.length === 0 && (
        <p className="text-[12px] text-[var(--ink-muted)]">Không có revision nào.</p>
      )}

      {q.data && q.data.length > 0 && (
        <div className="overflow-hidden rounded-lg border" style={{ background: "var(--surface-1)" }}>
          <table className="w-full text-[11px]">
            <thead style={{ background: "var(--surface-2)" }}>
              <tr className="text-left">
                <th className="px-2 py-1.5 font-medium">Revision</th>
                <th className="px-2 py-1.5 font-medium">Traffic</th>
                <th className="px-2 py-1.5 font-medium">Tạo lúc</th>
                <th className="px-2 py-1.5 font-medium">Image</th>
                <th className="px-2 py-1.5 font-medium">Scaling</th>
                <th className="px-2 py-1.5 font-medium">Resource</th>
              </tr>
            </thead>
            <tbody>
              {q.data.map((r) => {
                const serving = r.trafficPercent > 0;
                return (
                  <tr
                    key={r.name}
                    className="border-t"
                    style={
                      serving
                        ? { background: "color-mix(in oklab, var(--series-1) 7%, transparent)" }
                        : undefined
                    }
                  >
                    <td className="px-2 py-1.5">
                      <span className="flex items-center gap-1.5">
                        <HealthDot health={r.health} message={r.healthMessage} />
                        <span className="mono">{r.name}</span>
                        {r.isLatestReady && <Badge tone="good">latest ready</Badge>}
                      </span>
                      {r.healthMessage && r.health === "notReady" && (
                        <p
                          className="selectable mt-0.5 pl-[15px]"
                          style={{ color: "var(--status-critical)" }}
                        >
                          {r.healthMessage}
                        </p>
                      )}
                    </td>
                    <td className="tnum px-2 py-1.5">
                      {serving ? (
                        <span className="flex items-center gap-1.5">
                          <span
                            className="inline-block h-1.5 rounded-sm"
                            style={{
                              width: Math.max(r.trafficPercent * 0.5, 3),
                              background: "var(--series-1)",
                            }}
                          />
                          {r.trafficPercent}%
                        </span>
                      ) : (
                        <span className="text-[var(--ink-muted)]">0%</span>
                      )}
                    </td>
                    <td className="whitespace-nowrap px-2 py-1.5" title={dateTime(r.createTime)}>
                      {ago(r.createTime)}
                    </td>
                    <td className="mono px-2 py-1.5" title={r.image ?? undefined}>
                      {shortImage(r.image)}
                      {shortSha(r.image) && (
                        <span className="ml-1 text-[var(--ink-muted)]">@{shortSha(r.image)}</span>
                      )}
                    </td>
                    <td className="tnum whitespace-nowrap px-2 py-1.5">
                      {r.minInstances ?? 0}–{r.maxInstances ?? "∞"}
                      {r.concurrency !== null && (
                        <span className="ml-1 text-[var(--ink-muted)]">· conc {r.concurrency}</span>
                      )}
                    </td>
                    <td className="whitespace-nowrap px-2 py-1.5">
                      {r.cpu ?? "–"} / {r.memory ?? "–"}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
