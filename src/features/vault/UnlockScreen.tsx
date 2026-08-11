import { useState } from "react";

import { Button, ErrorBox, Input } from "../../components/ui";
import { apiV2, asCmdError } from "../../lib/ipc";
import type { CmdError, VaultStatus } from "../../lib/types";

/**
 * Màn mở khoá vault.
 *
 * Hiện khi đã có vault credential nhưng chưa mở khoá trong phiên này. Passphrase chỉ nằm
 * trong RAM lúc mở — không lưu đâu cả, kể cả dạng hash. Có đường thoát "dùng gcloud": vault
 * là tuỳ chọn, người dùng v1 vẫn xác thực bằng gcloud CLI như cũ.
 */
export function UnlockScreen({
  status,
  onUnlocked,
  onUseGcloud,
}: {
  status: VaultStatus;
  onUnlocked: (s: VaultStatus) => void;
  onUseGcloud: () => void;
}) {
  const [passphrase, setPassphrase] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<CmdError | null>(null);

  const unlock = async () => {
    if (!passphrase) return;
    setBusy(true);
    setError(null);
    try {
      const s = await apiV2.unlockVault(passphrase);
      setPassphrase(""); // Không giữ passphrase trong state lâu hơn mức cần.
      onUnlocked(s);
    } catch (e) {
      setError(asCmdError(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex h-full flex-col items-center justify-center p-6">
      <div
        className="w-full max-w-md rounded-lg border p-6 shadow-xl"
        style={{ background: "var(--surface-1)" }}
      >
        <div className="mb-4 text-center">
          <div className="text-2xl" aria-hidden>
            🔐
          </div>
          <h1 className="mt-2 text-[16px] font-semibold">Mở khoá credential</h1>
          <p className="mt-1 text-[12px] text-[var(--ink-muted)]">
            Đã có {status.credentialCount} service account được lưu mã hoá trên máy này. Nhập passphrase
            để dùng cho phiên làm việc.
          </p>
        </div>

        {status.active && (
          <div className="mb-3 rounded-md border p-2.5 text-[12px]" style={{ background: "var(--surface-2)" }}>
            <div className="text-[var(--ink-muted)]">Service account đang chọn</div>
            <div className="mono selectable mt-0.5 break-all">{status.active.clientEmail}</div>
          </div>
        )}

        <form
          onSubmit={(e) => {
            e.preventDefault();
            void unlock();
          }}
          className="flex flex-col gap-3"
        >
          <Input
            type="password"
            value={passphrase}
            onChange={(e) => setPassphrase(e.target.value)}
            placeholder="passphrase"
            autoFocus
            autoComplete="current-password"
            className="w-full"
          />
          <ErrorBox error={error} />
          <Button type="submit" variant="primary" loading={busy} disabled={!passphrase} className="w-full">
            Mở khoá
          </Button>
        </form>

        <div className="mt-4 border-t pt-3 text-center">
          <button
            type="button"
            className="text-[12px] text-[var(--ink-secondary)] underline hover:text-[var(--ink-primary)]"
            onClick={onUseGcloud}
          >
            Bỏ qua — dùng gcloud CLI như cũ
          </button>
          <p className="mt-1 text-[11px] text-[var(--ink-muted)]">
            App sẽ xác thực bằng tài khoản gcloud của máy, không đụng tới vault.
          </p>
        </div>
      </div>
    </div>
  );
}
