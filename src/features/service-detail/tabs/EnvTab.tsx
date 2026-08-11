import { useEffect, useMemo, useState } from "react";

import { ApplyDialog } from "../../../components/ApplyDialog";
import {
  Badge,
  Button,
  ErrorBox,
  Input,
  Notice,
  Select,
  useToast,
} from "../../../components/ui";
import { useT, useTNode } from "../../../lib/i18n";
import { api, asCmdError } from "../../../lib/ipc";
import { useInvalidateService, useSecrets } from "../../../lib/queries";
import type {
  ApplyPreview,
  ApplyResult,
  CmdError,
  EnvEntry,
  ServiceDetail,
} from "../../../lib/types";

function sameEnv(a: EnvEntry[], b: EnvEntry[]): boolean {
  if (a.length !== b.length) return false;
  return a.every((x, i) => {
    const y = b[i];
    if (!y) return false;
    return (
      x.name === y.name &&
      x.kind === y.kind &&
      (x.value ?? "") === (y.value ?? "") &&
      (x.secret ?? "") === (y.secret ?? "") &&
      (x.version ?? "") === (y.version ?? "")
    );
  });
}

export function EnvTab({
  project,
  detail,
  containerIndex,
  readOnly,
  requiresTypedConfirm,
}: {
  project: string;
  detail: ServiceDetail;
  containerIndex: number;
  readOnly: boolean;
  requiresTypedConfirm: boolean;
}) {
  const t = useT();
  const tNode = useTNode();
  const toast = useToast();
  const invalidate = useInvalidateService();
  const container = detail.containers[containerIndex];
  const original = useMemo(() => container?.env ?? [], [container]);

  const [draft, setDraft] = useState<EnvEntry[]>(original);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [preview, setPreview] = useState<ApplyPreview | null>(null);
  const [previewError, setPreviewError] = useState<CmdError | null>(null);
  const [applyError, setApplyError] = useState<CmdError | null>(null);
  const [result, setResult] = useState<ApplyResult | null>(null);
  const [busy, setBusy] = useState(false);

  // Reload/đổi service/đổi container → bỏ draft và bám lại theo dữ liệu thật.
  // Giữ draft cũ ở đây sẽ dẫn tới việc apply thay đổi lên service khác.
  useEffect(() => {
    setDraft(original);
  }, [original, detail.etag]);

  const secrets = useSecrets(project, true);
  const dirty = !sameEnv(draft, original);
  const region = detail.summary.region;
  const service = detail.summary.name;

  const set = (i: number, patch: Partial<EnvEntry>) =>
    setDraft((d) => d.map((e, j) => (j === i ? { ...e, ...patch } : e)));

  const remove = (i: number) => setDraft((d) => d.filter((_, j) => j !== i));

  const openDialog = async () => {
    setPreview(null);
    setPreviewError(null);
    setApplyError(null);
    setResult(null);
    setDialogOpen(true);
    try {
      setPreview(
        await api.previewEnv({ project, region, service, containerIndex, env: draft }),
      );
    } catch (e) {
      setPreviewError(asCmdError(e));
    }
  };

  const doApply = async (confirmText: string | null, validateOnly: boolean) => {
    setBusy(true);
    setApplyError(null);
    setResult(null);
    try {
      const r = await api.applyEnv({
        project,
        region,
        service,
        containerIndex,
        env: draft,
        expectedEtag: detail.etag,
        confirmText,
        validateOnly,
      });
      setResult(r);
      if (!validateOnly) {
        invalidate(project, region, service);
        toast({
          tone: r.outcome.message.includes("KHÔNG khởi động được") ? "critical" : "good",
          title: r.outcome.message.includes("KHÔNG khởi động được")
            ? t("Revision mới của {service} không khởi động được", { service })
            : t("Đã cập nhật env của {service}", { service }),
          body: r.outcome.message,
        });
      }
    } catch (e) {
      setApplyError(asCmdError(e));
    } finally {
      setBusy(false);
    }
  };

  if (!container) {
    return (
      <Notice tone="critical">
        {t("Không tìm thấy container index {index}.", { index: containerIndex })}
      </Notice>
    );
  }

  return (
    <div className="flex flex-col gap-3">
      {detail.summary.trafficPinned && (
        <Notice tone="warning" icon="📌">
          {t(
            "Traffic của service này đang được ghim vào revision cụ thể. Sửa env sẽ tạo revision mới nhưng revision đó sẽ không nhận traffic — thay đổi không có tác dụng cho tới khi bạn chuyển traffic (làm trên GCP Console; app này không sửa traffic).",
          )}
        </Notice>
      )}

      <div className="flex items-center gap-2">
        <h2 className="text-[13px] font-semibold">
          {t("Biến môi trường")}
          <span className="ml-1.5 font-normal text-[var(--ink-muted)]">
            {t("{total} biến · {secret} từ secret", {
              total: draft.length,
              secret: draft.filter((e) => e.kind === "secretRef").length,
            })}
          </span>
        </h2>
        {dirty && (
          <Badge tone="warning" icon="●">
            {t("có thay đổi chưa lưu")}
          </Badge>
        )}

        <div className="ml-auto flex items-center gap-2">
          <Button
            size="sm"
            onClick={() =>
              setDraft((d) => [
                ...d,
                { name: "", kind: "plain", value: "", secret: null, version: null },
              ])
            }
          >
            {t("+ Biến thường")}
          </Button>
          <Button
            size="sm"
            disabled={!secrets.data || secrets.data.length === 0}
            title={
              secrets.data && secrets.data.length > 0
                ? t("Thêm biến lấy giá trị từ Secret Manager")
                : t("Không đọc được danh sách secret của project")
            }
            onClick={() =>
              setDraft((d) => [
                ...d,
                {
                  name: "",
                  kind: "secretRef",
                  value: null,
                  secret: secrets.data?.[0]?.name ?? "",
                  version: "latest",
                },
              ])
            }
          >
            {t("+ Biến từ Secret")}
          </Button>
          <Button size="sm" variant="ghost" disabled={!dirty} onClick={() => setDraft(original)}>
            {t("Hoàn tác")}
          </Button>
          <Button size="sm" variant="primary" disabled={!dirty} onClick={openDialog}>
            {t("Xem thay đổi & áp dụng")}
          </Button>
        </div>
      </div>

      <ErrorBox error={secrets.error} />

      <div className="overflow-hidden rounded-lg border" style={{ background: "var(--surface-1)" }}>
        <table className="w-full text-[12px]">
          <thead style={{ background: "var(--surface-2)" }}>
            <tr className="text-left">
              <th className="w-[30%] px-2 py-1.5 font-medium">{t("Tên")}</th>
              <th className="px-2 py-1.5 font-medium">{t("Giá trị")}</th>
              <th className="w-[1%] px-2 py-1.5" />
            </tr>
          </thead>
          <tbody>
            {draft.length === 0 && (
              <tr>
                <td colSpan={3} className="px-2 py-6 text-center text-[var(--ink-muted)]">
                  {t("Service này không có biến môi trường nào.")}
                </td>
              </tr>
            )}

            {draft.map((e, i) => {
              const isSecret = e.kind === "secretRef";
              const wasSecret = original.find((o) => o.name === e.name)?.kind === "secretRef";

              return (
                <tr key={`${i}-${e.name}`} className="border-t align-top">
                  <td className="px-2 py-1.5">
                    <Input
                      className="mono w-full"
                      value={e.name}
                      spellCheck={false}
                      autoComplete="off"
                      placeholder={t("TÊN_BIẾN")}
                      onChange={(ev) => set(i, { name: ev.target.value })}
                    />
                  </td>

                  <td className="px-2 py-1.5">
                    {isSecret ? (
                      <div className="flex flex-wrap items-center gap-1.5">
                        <Badge tone="info" icon="🔑">
                          Secret Manager
                        </Badge>

                        {/* Secret đã tồn tại: khoá tên secret lại, chỉ cho đổi version.
                            Đổi tên secret của một binding đang chạy là thao tác dễ gây
                            sự cố và không phải việc của v1. */}
                        {wasSecret ? (
                          <code className="mono selectable rounded border px-1.5 py-0.5">
                            {e.secret}
                          </code>
                        ) : (
                          <Select
                            className="max-w-[240px]"
                            value={e.secret ?? ""}
                            onChange={(ev) => set(i, { secret: ev.target.value })}
                          >
                            {(secrets.data ?? []).map((s) => (
                              <option key={s.name} value={s.name}>
                                {s.name}
                              </option>
                            ))}
                          </Select>
                        )}

                        <span className="text-[var(--ink-muted)]">version</span>
                        <Input
                          className="mono w-24"
                          value={e.version ?? "latest"}
                          onChange={(ev) => set(i, { version: ev.target.value })}
                          placeholder="latest"
                        />
                      </div>
                    ) : (
                      <Input
                        className="mono w-full"
                        value={e.value ?? ""}
                        spellCheck={false}
                        autoComplete="off"
                        onChange={(ev) => set(i, { value: ev.target.value })}
                      />
                    )}
                  </td>

                  <td className="px-2 py-1.5">
                    <Button
                      size="sm"
                      variant="ghost"
                      title={t("Xoá biến này")}
                      onClick={() => remove(i)}
                      aria-label={t("Xoá {name}", { name: e.name })}
                    >
                      ✕
                    </Button>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>

      <p className="text-[11px] leading-relaxed text-[var(--ink-muted)]">
        {t(
          "Biến đánh dấu 🔑 lấy giá trị từ Secret Manager. Giá trị của chúng không đi qua app này ở tab Env — muốn xem thì sang tab Secrets và bấm reveal.",
        )}{" "}
        {tNode("{vars} do Cloud Run tự quản, không đặt tay được.", {
          vars: (
            <>
              <code className="mono">PORT</code>, <code className="mono">K_SERVICE</code>,{" "}
              <code className="mono">K_REVISION</code>,{" "}
              <code className="mono">K_CONFIGURATION</code>
            </>
          ),
        })}
      </p>

      <ApplyDialog
        open={dialogOpen}
        onClose={() => setDialogOpen(false)}
        title={t("Áp dụng thay đổi env — {service}", { service })}
        serviceName={service}
        requiresTypedConfirm={requiresTypedConfirm}
        preview={preview}
        previewError={previewError}
        busy={busy}
        error={applyError}
        result={result}
        onApply={doApply}
        canWrite={!readOnly}
      />
    </div>
  );
}
