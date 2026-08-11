import { useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { Badge, Button, ErrorBox, Field, Input, Loading, Notice } from "../../components/ui";
import { useT, useTNode } from "../../lib/i18n";
import { apiV2, asCmdError } from "../../lib/ipc";
import type { CmdError, ImportResult, VaultStatus } from "../../lib/types";

/**
 * Panel quản lý credential — nhúng trong màn Cài đặt.
 *
 * Bất biến bảo mật giữ ở đây:
 *  - Private key của SA đọc từ file ngay trong renderer bằng FileReader (web API, không cần
 *    cấp quyền fs cho Tauri), gửi qua IPC một lần rồi **không giữ lại trong state**.
 *  - Passphrase chỉ nằm trong state trong lúc thao tác, xoá ngay sau khi import xong.
 *  - Backend không bao giờ trả private key ra — chỉ trả email + key id để hiển thị.
 */

/** Giá trị là key dịch — bọc `t()` ở chỗ render. */
const SOURCE_TEXT: Record<VaultStatus["effectiveSource"], string> = {
  serviceAccount: "Service Account (từ vault)",
  gcloudCli: "gcloud CLI (tài khoản máy)",
  adc: "Application Default Credentials",
};

export function CredentialPanel({
  allowedProjects,
  onVaultChanged,
}: {
  allowedProjects: string[];
  onVaultChanged?: (s: VaultStatus) => void;
}) {
  const t = useT();
  const tNode = useTNode();
  const qc = useQueryClient();
  const fileRef = useRef<HTMLInputElement>(null);
  const [keyJson, setKeyJson] = useState("");
  const [fileName, setFileName] = useState<string | null>(null);
  const [passphrase, setPassphrase] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<CmdError | null>(null);
  const [result, setResult] = useState<ImportResult | null>(null);

  const statusQ = useQuery<VaultStatus, CmdError>({
    queryKey: ["vaultStatus"],
    queryFn: () => apiV2.vaultStatus(),
    staleTime: 10_000,
    retry: false,
  });
  const status = statusQ.data;

  const refresh = async () => {
    const s = await qc.invalidateQueries({ queryKey: ["vaultStatus"] });
    void s;
    const fresh = await apiV2.vaultStatus();
    qc.setQueryData(["vaultStatus"], fresh);
    onVaultChanged?.(fresh);
    return fresh;
  };

  const onPickFile = (file: File) => {
    const reader = new FileReader();
    reader.onload = () => {
      setKeyJson(String(reader.result ?? ""));
      setFileName(file.name);
      setError(null);
    };
    reader.onerror = () =>
      setError({
        message: t("Không đọc được file. Chọn lại file JSON service account."),
        detail: null,
        kind: "invalid",
        status: null,
      });
    reader.readAsText(file);
  };

  const doImport = async () => {
    if (!keyJson.trim() || !passphrase) return;
    setBusy(true);
    setError(null);
    setResult(null);
    try {
      const res = await apiV2.importServiceAccount(keyJson, passphrase);
      setResult(res);
      // Xoá dữ liệu nhạy cảm khỏi state ngay khi không cần nữa.
      setKeyJson("");
      setFileName("");
      setPassphrase("");
      if (fileRef.current) fileRef.current.value = "";
      await refresh();
    } catch (e) {
      setError(asCmdError(e));
    } finally {
      setBusy(false);
    }
  };

  const removeAt = async (index: number) => {
    setBusy(true);
    setError(null);
    try {
      await apiV2.removeCredential(index);
      await refresh();
    } catch (e) {
      setError(asCmdError(e));
    } finally {
      setBusy(false);
    }
  };

  const lock = async () => {
    setBusy(true);
    try {
      await apiV2.lockVault();
      await refresh();
    } catch (e) {
      setError(asCmdError(e));
    } finally {
      setBusy(false);
    }
  };

  if (statusQ.isLoading) return <Loading label={t("Đang kiểm tra vault…")} />;

  const importable = keyJson.trim().length > 0 && passphrase.length > 0 && !busy;

  return (
    <div className="flex flex-col gap-3">
      <ErrorBox error={statusQ.error ?? error} onRetry={statusQ.error ? () => void statusQ.refetch() : undefined} />

      {/* Trạng thái hiện tại */}
      <div className="rounded-md border p-3" style={{ background: "var(--surface-2)" }}>
        <div className="flex flex-wrap items-center gap-2 text-[12px]">
          <span className="text-[var(--ink-muted)]">{t("Đang xác thực bằng:")}</span>
          <Badge tone={status?.effectiveSource === "serviceAccount" ? "info" : "neutral"}>
            {status ? t(SOURCE_TEXT[status.effectiveSource]) : "–"}
          </Badge>
          {status?.exists && (
            <Badge tone={status.unlocked ? "good" : "warning"} icon={status.unlocked ? "🔓" : "🔒"}>
              {status.unlocked ? t("đã mở khoá") : t("đang khoá")}
            </Badge>
          )}
        </div>
        {status?.active && (
          <div className="mono selectable mt-1.5 text-[11px] break-all">{status.active.clientEmail}</div>
        )}
        {status?.exists && status.unlocked && (
          <div className="mt-2">
            <Button size="sm" variant="ghost" onClick={() => void lock()} disabled={busy}>
              {t("🔒 Khoá lại")}
            </Button>
          </div>
        )}
      </div>

      {/* Danh sách credential đã lưu — chỉ khi đã mở khoá */}
      {status?.unlocked && status.credentialCount > 0 && (
        <div>
          <h4 className="mb-1.5 text-[12px] font-semibold">
            {t("Credential đã lưu ({count})", { count: status.credentialCount })}
          </h4>
          <div className="flex flex-col gap-1.5">
            {Array.from({ length: status.credentialCount }).map((_, i) => {
              const isActive = status.active && i === activeIndexGuess(status, i);
              return (
                <div key={i} className="flex items-center gap-2 rounded border px-2 py-1.5 text-[11px]">
                  <span className="mono flex-1 truncate">
                    {isActive ? status.active?.clientEmail : `Credential #${i + 1}`}
                  </span>
                  {isActive && <Badge tone="good">{t("đang dùng")}</Badge>}
                  <Button
                    size="sm"
                    variant="ghost"
                    disabled={busy}
                    onClick={() => void removeAt(i)}
                    title={t("Xoá credential này khỏi vault")}
                  >
                    {t("Xoá")}
                  </Button>
                </div>
              );
            })}
          </div>
        </div>
      )}

      {/* Kết quả import gần nhất */}
      {result && (
        <Notice tone={result.tokenOk ? "good" : "warning"} icon={result.tokenOk ? "✓" : "⚠"}>
          {result.tokenOk
            ? t("Đã lấy được token với {email}.", { email: result.credential.clientEmail })
            : t(
                "Đã lưu {email} nhưng chưa lấy được token — kiểm tra lại SA hoặc đồng hồ máy.",
                { email: result.credential.clientEmail },
              )}
          {result.missing.length > 0 && (
            <>
              {"\n\n"}
              {t("Thiếu quyền trên project cần dùng:")}
              {"\n"}
              {result.missing.map((m) => `• ${m}`).join("\n")}
            </>
          )}
          {result.warnings.length > 0 && (
            <>
              {"\n\n"}
              {result.warnings.join("\n")}
            </>
          )}
        </Notice>
      )}

      {/* Import SA mới */}
      <div className="rounded-md border p-3">
        <h4 className="mb-2 text-[12px] font-semibold">
          {status?.exists ? t("Thêm service account") : t("Nhập service account (tạo vault)")}
        </h4>
        <p className="mb-2 text-[11px] leading-relaxed text-[var(--ink-muted)]">
          {t(
            "Chọn file JSON key của service account. File được đọc ngay trong app, key riêng được mã hoá bằng passphrase rồi lưu trên máy — không gửi đi đâu, không nằm trong settings hay log.",
          )}
          {allowedProjects.length > 0 && (
            <>
              {" "}
              {tNode("App sẽ kiểm quyền của SA trên {projects}.", {
                projects: <span className="mono">{allowedProjects.join(", ")}</span>,
              })}
            </>
          )}
        </p>

        <div className="flex flex-col gap-2">
          <input
            ref={fileRef}
            type="file"
            accept=".json,application/json"
            className="text-[12px] file:mr-2 file:rounded file:border file:bg-[var(--surface-2)] file:px-2 file:py-1 file:text-[12px]"
            onChange={(e) => {
              const f = e.target.files?.[0];
              if (f) onPickFile(f);
            }}
          />
          {fileName && (
            <span className="text-[11px] text-[var(--ink-muted)]">
              {t("Đã nạp")} <span className="mono">{fileName}</span> (
              {(keyJson.length / 1024).toFixed(1)} KB)
            </span>
          )}

          <Field
            label={
              status?.exists
                ? t("Passphrase (đúng passphrase của vault)")
                : t("Đặt passphrase cho vault")
            }
            hint={t(
              "Không lưu ở đâu cả — quên là phải nhập lại từng SA. Đặt passphrase đủ mạnh.",
            )}
          >
            <Input
              type="password"
              value={passphrase}
              onChange={(e) => setPassphrase(e.target.value)}
              placeholder="passphrase"
              autoComplete="new-password"
            />
          </Field>

          <div>
            <Button variant="primary" loading={busy} disabled={!importable} onClick={() => void doImport()}>
              {status?.exists ? t("Thêm vào vault") : t("Tạo vault & nhập SA")}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}

/**
 * Vault chỉ trả `active` (credential đang dùng), không trả index. Không có cách chắc chắn map
 * index nào là active nếu trùng email, nên đoán bằng vị trí — dùng thuần cho gợi ý hiển thị,
 * không ảnh hưởng thao tác (xoá theo index thật).
 */
function activeIndexGuess(status: VaultStatus, i: number): number {
  // Backend đặt credential mới nhất làm active (thường ở cuối). Coi index cuối là active.
  return status.credentialCount - 1 === i ? i : -1;
}
