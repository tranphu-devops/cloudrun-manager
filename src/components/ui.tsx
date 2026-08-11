/**
 * Bộ primitive nhỏ theo phong cách shadcn/ui nhưng viết tay trong repo.
 *
 * Lý do không dùng shadcn CLI: nó kéo theo cả cây Radix và cần chạy generator. Với một
 * app nội bộ ~10 màn hình thì tự viết vài component gọn hơn và không phụ thuộc thêm
 * package nào — quan trọng vì repo này sẽ build trên máy Windows của người khác.
 */

import {
  createContext,
  useContext,
  useEffect,
  useId,
  useRef,
  useState,
  type ReactNode,
} from "react";

import { cn } from "../lib/format";
import type { CmdError } from "../lib/types";

// ---------------------------------------------------------------------------
// Button
// ---------------------------------------------------------------------------

type ButtonVariant = "primary" | "secondary" | "ghost" | "danger";

export function Button({
  variant = "secondary",
  size = "md",
  loading = false,
  className,
  children,
  ...rest
}: React.ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: ButtonVariant;
  size?: "sm" | "md";
  loading?: boolean;
}) {
  const base =
    "inline-flex items-center justify-center gap-1.5 rounded-md border font-medium transition-colors disabled:opacity-45 disabled:cursor-not-allowed whitespace-nowrap";
  const sizes = { sm: "h-7 px-2.5 text-[12px]", md: "h-8 px-3 text-[13px]" }[size];
  const variants: Record<ButtonVariant, string> = {
    primary: "border-transparent text-white hover:brightness-110",
    secondary: "hover:bg-[var(--surface-2)]",
    ghost: "border-transparent hover:bg-[var(--surface-2)]",
    danger: "border-transparent text-white hover:brightness-110",
  };
  const style: React.CSSProperties =
    variant === "primary"
      ? { background: "var(--series-1)" }
      : variant === "danger"
        ? { background: "var(--status-critical)" }
        : { background: "var(--surface-1)" };

  return (
    <button
      {...rest}
      style={{ ...style, ...rest.style }}
      disabled={rest.disabled || loading}
      className={cn(base, sizes, variants[variant], className)}
    >
      {loading && <Spinner size={12} />}
      {children}
    </button>
  );
}

export function Spinner({ size = 14 }: { size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      className="animate-spin shrink-0"
      aria-hidden
    >
      <circle
        cx="12"
        cy="12"
        r="9"
        fill="none"
        stroke="currentColor"
        strokeWidth="3"
        strokeOpacity="0.25"
      />
      <path
        d="M21 12a9 9 0 0 0-9-9"
        fill="none"
        stroke="currentColor"
        strokeWidth="3"
        strokeLinecap="round"
      />
    </svg>
  );
}

// ---------------------------------------------------------------------------
// Input / Select / Toggle
// ---------------------------------------------------------------------------

export function Input({
  className,
  invalid,
  ...rest
}: React.InputHTMLAttributes<HTMLInputElement> & { invalid?: boolean }) {
  return (
    <input
      {...rest}
      className={cn(
        "h-8 min-w-0 rounded-md border px-2 text-[13px] outline-none",
        "bg-[var(--surface-1)] placeholder:text-[var(--ink-muted)]",
        invalid && "border-[var(--status-critical)]",
        className,
      )}
    />
  );
}

export function Select({
  className,
  children,
  ...rest
}: React.SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <select
      {...rest}
      className={cn(
        "h-8 rounded-md border px-2 text-[13px] outline-none bg-[var(--surface-1)]",
        className,
      )}
    >
      {children}
    </select>
  );
}

export function Toggle({
  checked,
  onChange,
  label,
  hint,
  tone = "neutral",
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: ReactNode;
  hint?: string;
  tone?: "neutral" | "warn";
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      title={hint}
      onClick={() => onChange(!checked)}
      className="inline-flex items-center gap-2 rounded-md border px-2 h-8 text-[12px] hover:bg-[var(--surface-2)]"
      style={{ background: "var(--surface-1)" }}
    >
      <span
        className="relative inline-block h-3.5 w-6 rounded-full transition-colors"
        style={{
          background: checked
            ? tone === "warn"
              ? "var(--status-warning)"
              : "var(--series-1)"
            : "var(--axis)",
        }}
      >
        <span
          className="absolute top-0.5 h-2.5 w-2.5 rounded-full bg-white transition-all"
          style={{ left: checked ? 12 : 2 }}
        />
      </span>
      {label}
    </button>
  );
}

// ---------------------------------------------------------------------------
// Card / Badge / Label
// ---------------------------------------------------------------------------

export function Card({
  title,
  actions,
  children,
  className,
  bodyClassName,
}: {
  title?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
  className?: string;
  bodyClassName?: string;
}) {
  return (
    <section
      className={cn("rounded-lg border", className)}
      style={{ background: "var(--surface-1)" }}
    >
      {(title || actions) && (
        <header className="flex items-center justify-between gap-2 border-b px-3 py-2">
          <h2 className="text-[12px] font-semibold tracking-wide uppercase text-[var(--ink-secondary)]">
            {title}
          </h2>
          {actions}
        </header>
      )}
      <div className={cn("p-3", bodyClassName)}>{children}</div>
    </section>
  );
}

type BadgeTone = "neutral" | "good" | "warning" | "serious" | "critical" | "info";

const BADGE_COLOR: Record<BadgeTone, string> = {
  neutral: "var(--ink-muted)",
  good: "var(--status-good)",
  warning: "var(--status-warning)",
  serious: "var(--status-serious)",
  critical: "var(--status-critical)",
  info: "var(--series-1)",
};

/**
 * Badge trạng thái.
 *
 * Luôn có chữ đi kèm dấu màu — màu một mình không bao giờ mang nghĩa. Đây là yêu cầu
 * bắt buộc với status color, và cũng là điều đúng đắn cho người mù màu.
 */
export function Badge({
  tone = "neutral",
  children,
  icon,
  title,
}: {
  tone?: BadgeTone;
  children: ReactNode;
  icon?: string;
  title?: string;
}) {
  return (
    <span
      title={title}
      className="inline-flex items-center gap-1 rounded border px-1.5 py-0.5 text-[11px] font-medium"
      style={{ borderColor: BADGE_COLOR[tone], color: BADGE_COLOR[tone] }}
    >
      {icon && <span aria-hidden>{icon}</span>}
      {children}
    </span>
  );
}

export function Field({
  label,
  hint,
  children,
  className,
}: {
  label: ReactNode;
  hint?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  const id = useId();
  return (
    <label htmlFor={id} className={cn("flex flex-col gap-1", className)}>
      <span className="text-[11px] font-medium text-[var(--ink-secondary)]">{label}</span>
      {children}
      {hint && <span className="text-[11px] text-[var(--ink-muted)]">{hint}</span>}
    </label>
  );
}

export function KeyValue({ items }: { items: Array<[ReactNode, ReactNode]> }) {
  return (
    <dl className="grid grid-cols-[minmax(120px,auto)_1fr] gap-x-4 gap-y-1.5 text-[12px]">
      {items.map(([k, v], i) => (
        <div key={i} className="contents">
          <dt className="text-[var(--ink-muted)]">{k}</dt>
          <dd className="selectable min-w-0 break-words">{v}</dd>
        </div>
      ))}
    </dl>
  );
}

// ---------------------------------------------------------------------------
// Tabs
// ---------------------------------------------------------------------------

export function Tabs<T extends string>({
  value,
  onChange,
  tabs,
}: {
  value: T;
  onChange: (v: T) => void;
  tabs: Array<{ id: T; label: string; badge?: ReactNode }>;
}) {
  return (
    <div role="tablist" className="flex items-center gap-0.5 border-b px-1">
      {tabs.map((t) => {
        const active = t.id === value;
        return (
          <button
            key={t.id}
            role="tab"
            aria-selected={active}
            onClick={() => onChange(t.id)}
            className={cn(
              "relative -mb-px flex items-center gap-1.5 px-3 py-2 text-[13px] transition-colors",
              active
                ? "font-semibold text-[var(--ink-primary)]"
                : "text-[var(--ink-secondary)] hover:text-[var(--ink-primary)]",
            )}
          >
            {t.label}
            {t.badge}
            {active && (
              <span
                className="absolute inset-x-1 -bottom-px h-0.5 rounded"
                style={{ background: "var(--series-1)" }}
              />
            )}
          </button>
        );
      })}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Dialog
// ---------------------------------------------------------------------------

/**
 * Modal đơn giản.
 *
 * Cố tình KHÔNG dùng `window.confirm`/`alert`: dialog gốc của WebView chặn toàn bộ
 * event loop, và trên Tauri thì có thể làm treo cả IPC.
 */
export function Dialog({
  open,
  onClose,
  title,
  children,
  footer,
  width = 640,
}: {
  open: boolean;
  onClose: () => void;
  title: ReactNode;
  children: ReactNode;
  footer?: ReactNode;
  width?: number;
}) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    ref.current?.focus();
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center overflow-y-auto p-8"
      style={{ background: "rgb(0 0 0 / 0.45)" }}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        ref={ref}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        className="rounded-lg border shadow-2xl outline-none"
        style={{ background: "var(--surface-1)", width, maxWidth: "100%" }}
      >
        <header className="flex items-center justify-between border-b px-4 py-3">
          <h2 className="text-[14px] font-semibold">{title}</h2>
          <Button variant="ghost" size="sm" onClick={onClose} aria-label="Đóng">
            ✕
          </Button>
        </header>
        <div className="max-h-[62vh] overflow-y-auto px-4 py-3">{children}</div>
        {footer && (
          <footer className="flex items-center justify-end gap-2 border-t px-4 py-3">
            {footer}
          </footer>
        )}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Thông báo lỗi / trạng thái
// ---------------------------------------------------------------------------

/**
 * Hộp lỗi.
 *
 * Message từ Rust đã được viết thành câu có hướng xử lý, nên hiện nguyên văn và giữ
 * xuống dòng. Chi tiết kỹ thuật cho vào `<details>` để không làm nhiễu.
 */
export function ErrorBox({
  error,
  onRetry,
  retryLabel = "Thử lại",
}: {
  error: CmdError | null | undefined;
  onRetry?: () => void;
  retryLabel?: string;
}) {
  if (!error) return null;

  const tone: BadgeTone = (
    [
      "conflict",
      "readOnly",
      "needsConfirm",
      "projectLocked",
      "vaultPassphrase",
      "vaultMissing",
      "vaultLocked",
      "jobRunning",
    ] as string[]
  ).includes(error.kind)
    ? "warning"
    : "critical";

  const heading =
    {
      auth: "Chưa xác thực được với GCP",
      permission: "Thiếu quyền",
      conflict: "Service đã bị thay đổi",
      readOnly: "Đang ở chế độ chỉ đọc",
      needsConfirm: "Cần xác nhận",
      network: "Lỗi kết nối",
      invalid: "Dữ liệu chưa hợp lệ",
      notFound: "Không tìm thấy",
      rateLimit: "Bị giới hạn tốc độ",
      projectLocked: "Project không được phép",
      vaultPassphrase: "Passphrase không đúng",
      vaultMissing: "Chưa có credential",
      vaultCorrupt: "File credential có vấn đề",
      vaultLocked: "Vault đang khoá",
      jobRunning: "Job đang chạy",
      other: "Có lỗi xảy ra",
    }[error.kind] ?? "Có lỗi xảy ra";

  return (
    <div
      className="rounded-md border p-3 text-[12px]"
      style={{ borderColor: BADGE_COLOR[tone], background: "var(--surface-1)" }}
    >
      <div className="mb-1.5 flex items-center gap-2">
        <Badge tone={tone} icon={tone === "warning" ? "⚠" : "✕"}>
          {heading}
        </Badge>
        {error.status !== null && (
          <span className="text-[11px] text-[var(--ink-muted)]">HTTP {error.status}</span>
        )}
      </div>
      <p className="selectable whitespace-pre-wrap leading-relaxed">{error.message}</p>
      {error.detail && (
        <details className="mt-2">
          <summary className="cursor-pointer text-[11px] text-[var(--ink-muted)]">
            Chi tiết kỹ thuật
          </summary>
          <pre className="selectable mono mt-1 max-h-40 overflow-auto whitespace-pre-wrap rounded border p-2 text-[11px]">
            {error.detail}
          </pre>
        </details>
      )}
      {onRetry && (
        <div className="mt-2">
          <Button size="sm" onClick={onRetry}>
            {retryLabel}
          </Button>
        </div>
      )}
    </div>
  );
}

export function Notice({
  tone = "info",
  icon,
  children,
}: {
  tone?: BadgeTone;
  icon?: string;
  children: ReactNode;
}) {
  return (
    <div
      className="flex gap-2 rounded-md border p-2.5 text-[12px] leading-relaxed"
      style={{ borderColor: BADGE_COLOR[tone], background: "var(--surface-1)" }}
    >
      {icon && (
        <span aria-hidden style={{ color: BADGE_COLOR[tone] }}>
          {icon}
        </span>
      )}
      <div className="selectable min-w-0 whitespace-pre-wrap">{children}</div>
    </div>
  );
}

export function EmptyState({ icon, title, hint }: { icon?: string; title: string; hint?: ReactNode }) {
  return (
    <div className="flex flex-col items-center justify-center gap-1.5 py-10 text-center">
      {icon && (
        <span className="text-2xl opacity-40" aria-hidden>
          {icon}
        </span>
      )}
      <p className="text-[13px] font-medium">{title}</p>
      {hint && <p className="max-w-md text-[12px] text-[var(--ink-muted)]">{hint}</p>}
    </div>
  );
}

export function Loading({ label = "Đang tải…" }: { label?: string }) {
  return (
    <div className="flex items-center justify-center gap-2 py-8 text-[12px] text-[var(--ink-muted)]">
      <Spinner />
      {label}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Copy
// ---------------------------------------------------------------------------

/**
 * Nút copy.
 *
 * `clearAfterMs` dùng cho giá trị nhạy cảm: sau khoảng đó tự xoá clipboard, để một lần
 * copy secret không nằm mãi trong clipboard rồi bị dán nhầm vào chat.
 */
export function CopyButton({
  text,
  label = "Copy",
  clearAfterMs,
  size = "sm",
}: {
  text: string;
  label?: string;
  clearAfterMs?: number;
  size?: "sm" | "md";
}) {
  const [state, setState] = useState<"idle" | "done" | "cleared">("idle");

  return (
    <Button
      size={size}
      variant="ghost"
      title={clearAfterMs ? `Clipboard sẽ được xoá sau ${clearAfterMs / 1000}s` : undefined}
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(text);
          setState("done");
          if (clearAfterMs) {
            window.setTimeout(async () => {
              try {
                // Chỉ xoá nếu clipboard vẫn đang là giá trị mình đã ghi — người dùng có
                // thể đã copy thứ khác, xoá của họ là sai.
                const cur = await navigator.clipboard.readText();
                if (cur === text) await navigator.clipboard.writeText("");
              } catch {
                // Không đọc được clipboard (thiếu quyền) thì thôi, không xoá mù.
              }
              setState("cleared");
            }, clearAfterMs);
          } else {
            window.setTimeout(() => setState("idle"), 1200);
          }
        } catch {
          setState("idle");
        }
      }}
    >
      {state === "done" ? "✓ Đã copy" : state === "cleared" ? "Đã xoá clipboard" : label}
    </Button>
  );
}

export function Kbd({ children }: { children: ReactNode }) {
  return (
    <kbd
      className="mono rounded border px-1 py-0.5 text-[10px]"
      style={{ background: "var(--surface-2)" }}
    >
      {children}
    </kbd>
  );
}

// ---------------------------------------------------------------------------
// Toast
// ---------------------------------------------------------------------------

type Toast = { id: number; tone: BadgeTone; title: string; body?: string };
const ToastCtx = createContext<(t: Omit<Toast, "id">) => void>(() => {});

export function useToast() {
  return useContext(ToastCtx);
}

export function ToastHost({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<Toast[]>([]);

  const push = (t: Omit<Toast, "id">) => {
    const id = Date.now() + Math.random();
    setItems((v) => [...v, { ...t, id }]);
    // Toast báo lỗi giữ lâu hơn: người dùng cần thời gian đọc câu hướng dẫn.
    window.setTimeout(
      () => setItems((v) => v.filter((x) => x.id !== id)),
      t.tone === "critical" || t.tone === "warning" ? 12_000 : 5_000,
    );
  };

  return (
    <ToastCtx.Provider value={push}>
      {children}
      <div className="pointer-events-none fixed bottom-4 right-4 z-[60] flex w-96 flex-col gap-2">
        {items.map((t) => (
          <div
            key={t.id}
            className="pointer-events-auto rounded-md border p-3 shadow-lg"
            style={{ background: "var(--surface-1)", borderColor: BADGE_COLOR[t.tone] }}
          >
            <div className="flex items-start justify-between gap-2">
              <p className="text-[13px] font-semibold">{t.title}</p>
              <button
                className="text-[var(--ink-muted)]"
                onClick={() => setItems((v) => v.filter((x) => x.id !== t.id))}
                aria-label="Đóng"
              >
                ✕
              </button>
            </div>
            {t.body && (
              <p className="selectable mt-1 whitespace-pre-wrap text-[12px] leading-relaxed text-[var(--ink-secondary)]">
                {t.body}
              </p>
            )}
          </div>
        ))}
      </div>
    </ToastCtx.Provider>
  );
}
