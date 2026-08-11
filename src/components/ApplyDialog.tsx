import { useEffect, useState } from "react";

import { useT } from "../lib/i18n";
import type { ApplyPreview, ApplyResult, CmdError, EnvChange } from "../lib/types";
import { Badge, Button, Dialog, ErrorBox, Input, Notice } from "./ui";

/**
 * Hiển thị diff env.
 *
 * Giá trị của biến secret không bao giờ xuất hiện ở đây — backend đã cắt từ gốc
 * (`EnvChange::Removed.value = None` cho secret-ref), phần này chỉ hiện đúng những gì
 * nhận được. Đừng thêm fallback kiểu `value ?? something` cho nhánh secret.
 */
function DiffRow({ c }: { c: EnvChange }) {
  const t = useT();
  const base = "mono flex items-start gap-2 rounded px-1.5 py-1 text-[11px] leading-relaxed";

  if (c.kind === "added") {
    return (
      <div className={base} style={{ background: "color-mix(in oklab, var(--status-good) 10%, transparent)" }}>
        <span className="shrink-0 font-semibold" style={{ color: "var(--status-good)" }}>
          {t("+ thêm")}
        </span>
        <span className="selectable min-w-0 break-all">
          <strong>{c.name}</strong> = {c.value}
        </span>
      </div>
    );
  }

  if (c.kind === "removed") {
    return (
      <div className={base} style={{ background: "color-mix(in oklab, var(--status-critical) 10%, transparent)" }}>
        <span className="shrink-0 font-semibold" style={{ color: "var(--status-critical)" }}>
          {t("− xoá")}
        </span>
        <span className="selectable min-w-0 break-all">
          <strong>{c.name}</strong>
          {c.value === null ? (
            <em className="ml-1 not-italic text-[var(--ink-muted)]">
              {t("(biến lấy từ Secret Manager)")}
            </em>
          ) : (
            <> = {c.value}</>
          )}
        </span>
      </div>
    );
  }

  if (c.kind === "secretVersionChanged") {
    return (
      <div className={base} style={{ background: "color-mix(in oklab, var(--status-warning) 12%, transparent)" }}>
        <span className="shrink-0 font-semibold" style={{ color: "var(--status-warning)" }}>
          🔑 version
        </span>
        <span className="selectable min-w-0 break-all">
          <strong>{c.name}</strong> · secret {c.secret}: v{c.before} → v{c.after}
        </span>
      </div>
    );
  }

  return (
    <div className={base} style={{ background: "color-mix(in oklab, var(--series-1) 10%, transparent)" }}>
      <span className="shrink-0 font-semibold" style={{ color: "var(--series-1)" }}>
        {t("~ sửa")}
      </span>
      <span className="selectable min-w-0 break-all">
        <strong>{c.name}</strong>{" "}
        <span className="line-through opacity-60">{c.before || t("(rỗng)")}</span> →{" "}
        <strong>{c.after || t("(rỗng)")}</strong>
      </span>
    </div>
  );
}

export function ApplyDialog({
  open,
  onClose,
  title,
  serviceName,
  requiresTypedConfirm,
  preview,
  previewError,
  busy,
  error,
  result,
  onApply,
  canWrite,
}: {
  open: boolean;
  onClose: () => void;
  title: string;
  serviceName: string;
  requiresTypedConfirm: boolean;
  preview: ApplyPreview | null;
  previewError?: CmdError | null;
  busy: boolean;
  error: CmdError | null;
  result: ApplyResult | null;
  onApply: (confirmText: string | null, validateOnly: boolean) => void;
  canWrite: boolean;
}) {
  const t = useT();
  const [confirm, setConfirm] = useState("");

  // Mỗi lần mở lại phải gõ lại tên. Giữ lại giá trị cũ sẽ biến lớp xác nhận thành
  // một cú bấm — đúng cái mà nó tồn tại để ngăn.
  useEffect(() => {
    if (open) setConfirm("");
  }, [open]);

  const confirmOk = !requiresTypedConfirm || confirm.trim() === serviceName;
  const nothingToDo =
    preview !== null && preview.envChanges.length === 0 && preview.scalingChanges.length === 0;

  const done = result !== null && !result.validatedOnly;
  const outcomeFailed = result?.outcome.message.includes("KHÔNG khởi động được") ?? false;

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title={title}
      width={720}
      footer={
        done ? (
          <Button variant="primary" onClick={onClose}>
            {t("Đóng")}
          </Button>
        ) : (
          <>
            <Button variant="ghost" onClick={onClose} disabled={busy}>
              {t("Huỷ")}
            </Button>
            <Button
              onClick={() => onApply(confirmOk ? confirm || null : null, true)}
              disabled={busy || !canWrite || !confirmOk}
              loading={busy}
              title={t(
                "Gửi lên Cloud Run với validateOnly=true: kiểm tra cấu hình mà không tạo revision",
              )}
            >
              {t("Kiểm tra trước")}
            </Button>
            <Button
              variant={requiresTypedConfirm ? "danger" : "primary"}
              onClick={() => onApply(confirmOk ? confirm || null : null, false)}
              disabled={busy || !canWrite || !confirmOk || nothingToDo}
              loading={busy}
            >
              {requiresTypedConfirm ? t("Áp dụng (tạo revision mới)") : t("Áp dụng")}
            </Button>
          </>
        )
      }
    >
      <div className="flex flex-col gap-3">
        <ErrorBox error={previewError} />

        {!canWrite && (
          <Notice tone="warning" icon="🔒">
            {t("Đang ở chế độ chỉ đọc. Bật “Cho ghi” ở thanh trên mới áp dụng được.")}
          </Notice>
        )}

        {preview && (
          <>
            {preview.warnings.map((w, i) => (
              <Notice key={i} tone="warning" icon="⚠">
                {w}
              </Notice>
            ))}

            {preview.envChanges.length > 0 && (
              <section>
                <h3 className="mb-1.5 text-[12px] font-semibold">
                  {t("Thay đổi biến môi trường ({n})", { n: preview.envChanges.length })}
                </h3>
                <div className="flex flex-col gap-1">
                  {preview.envChanges.map((c, i) => (
                    <DiffRow key={i} c={c} />
                  ))}
                </div>
              </section>
            )}

            {preview.scalingChanges.length > 0 && (
              <section>
                <h3 className="mb-1.5 text-[12px] font-semibold">
                  {t("Thay đổi scaling / resource ({n})", { n: preview.scalingChanges.length })}
                </h3>
                <ul className="mono flex flex-col gap-1 text-[11px]">
                  {preview.scalingChanges.map((c, i) => (
                    <li
                      key={i}
                      className="selectable rounded px-1.5 py-1"
                      style={{ background: "color-mix(in oklab, var(--series-1) 10%, transparent)" }}
                    >
                      {c}
                    </li>
                  ))}
                </ul>
              </section>
            )}

            {!done && !nothingToDo && (
              <Notice tone="info" icon="ℹ">
                {t("Thao tác này tạo một revision mới")}
                {preview.nextRevisionHint ? (
                  <>
                    {" "}
                    ({t("dự kiến")} <code className="mono">{preview.nextRevisionHint}</code>)
                  </>
                ) : null}
                {". "}
                {t(
                  "Cloud Run chỉ chuyển traffic sang revision mới sau khi nó khởi động thành công — nếu nó lỗi, service vẫn tiếp tục chạy revision hiện tại.",
                )}
              </Notice>
            )}
          </>
        )}

        {requiresTypedConfirm && !done && (
          <div
            className="rounded-md border p-3"
            style={{ borderColor: "var(--status-critical)" }}
          >
            <p className="mb-2 text-[12px] leading-relaxed">
              {t(
                "Project này được gắn nhãn production hoặc chưa gắn nhãn. Gõ đúng tên service để xác nhận:",
              )}
            </p>
            <div className="flex items-center gap-2">
              <code className="mono select-none rounded border px-1.5 py-0.5 text-[12px]">
                {serviceName}
              </code>
              <Input
                value={confirm}
                onChange={(e) => setConfirm(e.target.value)}
                placeholder={t("gõ lại tên service")}
                autoComplete="off"
                spellCheck={false}
                invalid={confirm.length > 0 && !confirmOk}
                className="flex-1"
              />
              {confirmOk && confirm.length > 0 && (
                <Badge tone="good" icon="✓">
                  {t("khớp")}
                </Badge>
              )}
            </div>
          </div>
        )}

        <ErrorBox error={error} />

        {result && (
          <Notice
            tone={outcomeFailed ? "critical" : result.validatedOnly ? "info" : "good"}
            icon={outcomeFailed ? "✕" : result.validatedOnly ? "🔎" : "✓"}
          >
            {result.outcome.message}
            {result.outcome.newRevision && (
              <>
                {"\n\n"}
                {t("Revision mới:")} <code className="mono">{result.outcome.newRevision}</code>
              </>
            )}
          </Notice>
        )}
      </div>
    </Dialog>
  );
}
