import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";

import {
  Badge,
  Button,
  Card,
  CopyButton,
  ErrorBox,
  Loading,
  Notice,
  Select,
} from "../../../components/ui";
import { dateTime } from "../../../lib/format";
import { useT } from "../../../lib/i18n";
import { api, asCmdError, consoleSecretUrl } from "../../../lib/ipc";
import { useSecrets, useSecretVersions } from "../../../lib/queries";
import type { CmdError, RevealResult, ServiceDetail } from "../../../lib/types";

/**
 * Panel xem giá trị một secret.
 *
 * Ba lớp bảo vệ, cả ba đều cố ý:
 *   1. Phải bấm reveal — không tự hiện.
 *   2. Tự ẩn lại sau N giây, có đếm ngược để người dùng biết nó sẽ mất.
 *   3. Copy sẽ tự xoá clipboard sau 60 giây, để giá trị không nằm mãi trong clipboard
 *      rồi bị dán nhầm vào chat công việc.
 */
function RevealPanel({
  project,
  secret,
  version,
}: {
  project: string;
  secret: string;
  version: string;
}) {
  const t = useT();
  const [value, setValue] = useState<RevealResult | null>(null);
  const [error, setError] = useState<CmdError | null>(null);
  const [busy, setBusy] = useState(false);
  const [left, setLeft] = useState(0);

  // Đổi secret/version thì ẩn ngay giá trị đang hiện.
  useEffect(() => {
    setValue(null);
    setError(null);
    setLeft(0);
  }, [project, secret, version]);

  useEffect(() => {
    if (!value) return;
    setLeft(value.hideAfterSeconds);
    const id = window.setInterval(() => {
      setLeft((v) => {
        if (v <= 1) {
          setValue(null);
          window.clearInterval(id);
          return 0;
        }
        return v - 1;
      });
    }, 1000);
    return () => window.clearInterval(id);
  }, [value]);

  if (!value) {
    return (
      <div className="flex flex-col gap-2">
        <ErrorBox error={error} />
        <div className="flex items-center gap-2">
          <Button
            variant="secondary"
            loading={busy}
            onClick={async () => {
              setBusy(true);
              setError(null);
              try {
                setValue(await api.revealSecret(project, secret, version));
              } catch (e) {
                setError(asCmdError(e));
              } finally {
                setBusy(false);
              }
            }}
          >
            {t("👁 Hiện giá trị version {version}", { version })}
          </Button>
          <span className="text-[11px] text-[var(--ink-muted)]">
            {t(
              "Lần xem sẽ được ghi vào audit log trên máy (chỉ tên secret + version, không ghi giá trị).",
            )}
          </span>
        </div>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center gap-2">
        <Badge tone="warning" icon="👁">
          {t("đang hiện · tự ẩn sau {sec}s", { sec: left })}
        </Badge>
        <span className="tnum text-[11px] text-[var(--ink-muted)]">
          {t("{bytes} byte · {lines} dòng", { bytes: value.byteLen, lines: value.lineCount })}
        </span>
        <div className="ml-auto flex items-center gap-1.5">
          <CopyButton text={value.value} label="Copy" clearAfterMs={60_000} />
          <Button size="sm" variant="ghost" onClick={() => setValue(null)}>
            {t("Ẩn ngay")}
          </Button>
        </div>
      </div>

      {value.looksBinary && (
        <Notice tone="warning" icon="⚠">
          {t(
            "Nội dung này có vẻ là dữ liệu nhị phân (không phải text UTF-8). Phần hiển thị bên dưới đã bị thay ký tự nên không dùng để copy — hãy lấy trực tiếp bằng",
          )}{" "}
          <code className="mono">gcloud secrets versions access</code>.
        </Notice>
      )}

      <pre
        className="mono selectable max-h-64 overflow-auto whitespace-pre-wrap break-all rounded border p-2 text-[11px]"
        style={{ background: "var(--surface-2)" }}
      >
        {value.value}
      </pre>
    </div>
  );
}

export function SecretsTab({
  project,
  detail,
  canReveal,
}: {
  project: string;
  detail: ServiceDetail;
  canReveal: boolean;
}) {
  const t = useT();
  const all = useSecrets(project, true);
  const [selected, setSelected] = useState<string | null>(null);
  const [version, setVersion] = useState("latest");
  const versions = useSecretVersions(project, selected);

  // Secret mà chính service này đang dùng — đó là câu hỏi chính khi mở tab này.
  const usedNames = new Set<string>();
  for (const c of detail.containers) {
    for (const e of c.env) if (e.secret) usedNames.add(e.secret);
  }
  for (const v of detail.secretVolumes) usedNames.add(v.secret);

  const used = (all.data ?? []).filter((s) => usedNames.has(s.name));
  const others = (all.data ?? []).filter((s) => !usedNames.has(s.name));

  useEffect(() => {
    setVersion("latest");
  }, [selected]);

  return (
    <div className="flex flex-col gap-3">
      {!canReveal && (
        <Notice tone="info" icon="🔒">
          {t("Account hiện tại không có")}{" "}
          <code className="mono">secretmanager.versions.access</code>{" "}
          {t(
            "trên project này, nên chỉ xem được metadata. Trên project production, không cấp quyền này là một lựa chọn hợp lý.",
          )}
        </Notice>
      )}

      <ErrorBox error={all.error} onRetry={() => void all.refetch()} />
      {all.isLoading && <Loading label={t("Đang lấy danh sách secret…")} />}

      <Card
        title={t("Secret mà {service} đang dùng ({count})", {
          service: detail.summary.name,
          count: usedNames.size,
        })}
      >
        {usedNames.size === 0 ? (
          <p className="text-[12px] text-[var(--ink-muted)]">
            {t("Service này không tham chiếu secret nào — không qua env, không qua volume mount.")}
          </p>
        ) : (
          <div className="flex flex-col gap-3">
            {/* Env secret-ref */}
            {detail.containers.map((c) =>
              c.env
                .filter((e) => e.kind === "secretRef")
                .map((e) => (
                  <div
                    key={`${c.index}-${e.name}`}
                    className="flex flex-wrap items-center gap-2 border-b pb-2 text-[12px] last:border-b-0 last:pb-0"
                  >
                    <Badge tone="info" icon="🔑">
                      env
                    </Badge>
                    <code className="mono font-semibold">{e.name}</code>
                    <span className="text-[var(--ink-muted)]">←</span>
                    <code className="mono">{e.secret}</code>
                    <Badge>version {e.version}</Badge>
                    {e.version === "latest" && (
                      <span className="text-[11px] text-[var(--ink-muted)]">
                        {t("dùng")} <code className="mono">latest</code>
                        {t(
                          ": revision mới sẽ tự lấy version mới nhất, revision đang chạy thì không",
                        )}
                      </span>
                    )}
                    <div className="ml-auto flex gap-1.5">
                      <Button size="sm" variant="ghost" onClick={() => setSelected(e.secret ?? null)}>
                        Xem
                      </Button>
                      <Button
                        size="sm"
                        variant="ghost"
                        onClick={() => void openUrl(consoleSecretUrl(project, e.secret ?? ""))}
                      >
                        Console ↗
                      </Button>
                    </div>
                  </div>
                )),
            )}

            {/* Secret mount dạng volume — dễ bị bỏ sót nếu chỉ nhìn env. */}
            {detail.secretVolumes.map((v) => (
              <div
                key={v.volumeName}
                className="flex flex-wrap items-center gap-2 border-b pb-2 text-[12px] last:border-b-0 last:pb-0"
              >
                <Badge tone="serious" icon="📁">
                  volume
                </Badge>
                <code className="mono font-semibold">{v.mountPath ?? v.volumeName}</code>
                <span className="text-[var(--ink-muted)]">←</span>
                <code className="mono">{v.secret}</code>
                {v.items.map((it) => (
                  <Badge key={it}>{it}</Badge>
                ))}
                <div className="ml-auto">
                  <Button size="sm" variant="ghost" onClick={() => setSelected(v.secret)}>
                    Xem
                  </Button>
                </div>
              </div>
            ))}
          </div>
        )}
      </Card>

      {selected && (
        <Card
          title={`Secret: ${selected}`}
          actions={
            <div className="flex items-center gap-2">
              <Select value={version} onChange={(e) => setVersion(e.target.value)} className="h-7 text-[11px]">
                <option value="latest">latest</option>
                {(versions.data ?? []).map((v) => (
                  <option key={v.version} value={v.version} disabled={v.state !== "ENABLED"}>
                    v{v.version} {v.state !== "ENABLED" ? `(${v.state})` : ""}
                  </option>
                ))}
              </Select>
              <Button size="sm" variant="ghost" onClick={() => setSelected(null)}>
                {t("Đóng")}
              </Button>
            </div>
          }
        >
          <div className="flex flex-col gap-3">
            <ErrorBox error={versions.error} />
            {versions.data && versions.data.length > 0 && (
              <div className="overflow-hidden rounded border">
                <table className="w-full text-[11px]">
                  <thead style={{ background: "var(--surface-2)" }}>
                    <tr className="text-left">
                      <th className="px-2 py-1 font-medium">{t("Version")}</th>
                      <th className="px-2 py-1 font-medium">{t("Trạng thái")}</th>
                      <th className="px-2 py-1 font-medium">{t("Tạo lúc")}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {versions.data.slice(0, 10).map((v) => (
                      <tr key={v.version} className="border-t">
                        <td className="mono px-2 py-1">v{v.version}</td>
                        <td className="px-2 py-1">
                          <Badge
                            tone={
                              v.state === "ENABLED" ? "good" : v.state === "DISABLED" ? "warning" : "critical"
                            }
                          >
                            {v.state}
                          </Badge>
                        </td>
                        <td className="px-2 py-1">{dateTime(v.createTime)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}

            {canReveal ? (
              <RevealPanel project={project} secret={selected} version={version} />
            ) : (
              <Notice tone="info">
                {t("Không có quyền xem giá trị secret trên project này.")}
              </Notice>
            )}
          </div>
        </Card>
      )}

      {others.length > 0 && (
        <Card title={t("Secret khác trong project ({count})", { count: others.length })}>
          <div className="max-h-72 overflow-auto">
            <table className="w-full text-[11px]">
              <thead className="sticky top-0" style={{ background: "var(--surface-2)" }}>
                <tr className="text-left">
                  <th className="px-2 py-1 font-medium">{t("Tên")}</th>
                  <th className="px-2 py-1 font-medium">{t("Service đang dùng")}</th>
                  <th className="px-2 py-1 font-medium">{t("Tạo lúc")}</th>
                  <th className="px-2 py-1" />
                </tr>
              </thead>
              <tbody>
                {others.map((s) => (
                  <tr key={s.name} className="border-t">
                    <td className="mono px-2 py-1">{s.name}</td>
                    <td className="px-2 py-1">
                      {s.usedBy.length === 0 ? (
                        // Secret không service nào dùng là tín hiệu để dọn dẹp — nói rõ.
                        <span className="text-[var(--ink-muted)]">{t("không service nào")}</span>
                      ) : (
                        s.usedBy.join(", ")
                      )}
                    </td>
                    <td className="px-2 py-1">{dateTime(s.createTime)}</td>
                    <td className="px-2 py-1 text-right">
                      <Button size="sm" variant="ghost" onClick={() => setSelected(s.name)}>
                        Xem
                      </Button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </Card>
      )}

      {used.length !== usedNames.size && all.data && (
        <Notice tone="info" icon="ℹ">
          {t(
            "Có secret service này tham chiếu nhưng không xuất hiện trong danh sách Secret Manager của project — thường là secret nằm ở project khác (cross-project reference), hoặc đã bị xoá.",
          )}
        </Notice>
      )}
    </div>
  );
}
