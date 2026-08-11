import type { ReactNode } from "react";

import { cn } from "../lib/format";

export type View = "services" | "statistics" | "jobs" | "billing" | "recommendations";

const ITEMS: Array<{ id: View; icon: string; label: string }> = [
  { id: "services", icon: "◧", label: "Service" },
  { id: "statistics", icon: "▦", label: "Thống kê" },
  { id: "jobs", icon: "⏱", label: "Jobs" },
  { id: "billing", icon: "₫", label: "Chi phí" },
  { id: "recommendations", icon: "💡", label: "Gợi ý" },
];

/**
 * Thanh điều hướng dọc bên trái.
 *
 * Icon + chữ luôn đi cùng nhau (không icon-only) — với app nội bộ ít người dùng, một nhãn rõ
 * đáng giá hơn vài pixel. `badge` để chấm cảnh báo (ví dụ số job cron cần sửa) khi cần.
 */
export function NavRail({
  view,
  onChange,
  badges,
}: {
  view: View;
  onChange: (v: View) => void;
  badges?: Partial<Record<View, ReactNode>>;
}) {
  return (
    <nav
      className="flex w-[92px] shrink-0 flex-col gap-1 border-r p-2"
      style={{ background: "var(--surface-1)" }}
      aria-label="Điều hướng"
    >
      {ITEMS.map((it) => {
        const active = it.id === view;
        return (
          <button
            key={it.id}
            onClick={() => onChange(it.id)}
            aria-current={active ? "page" : undefined}
            className={cn(
              "relative flex flex-col items-center gap-1 rounded-md px-1 py-2 text-[11px] transition-colors",
              active
                ? "font-semibold text-[var(--ink-primary)]"
                : "text-[var(--ink-secondary)] hover:bg-[var(--surface-2)] hover:text-[var(--ink-primary)]",
            )}
            style={active ? { background: "color-mix(in oklab, var(--series-1) 14%, transparent)" } : undefined}
          >
            <span className="text-[17px] leading-none" aria-hidden>
              {it.icon}
            </span>
            {it.label}
            {badges?.[it.id] && <span className="absolute right-1 top-1">{badges[it.id]}</span>}
            {active && (
              <span
                className="absolute inset-y-1 left-0 w-0.5 rounded-r"
                style={{ background: "var(--series-1)" }}
              />
            )}
          </button>
        );
      })}
    </nav>
  );
}
