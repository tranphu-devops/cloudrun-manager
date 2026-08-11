import { useEffect, useMemo, useRef, useState } from "react";

import { cn, regionLabel } from "../lib/format";
import { useT } from "../lib/i18n";
import type { ServiceSummary } from "../lib/types";
import { HealthDot } from "../features/service-list/Sidebar";

/**
 * Nhảy nhanh tới service bằng Ctrl+K.
 *
 * Với project cỡ trăm service, cuộn sidebar là cách chậm nhất để tới đúng service. Fuzzy
 * match ở đây cố tình đơn giản (khớp thứ tự ký tự) — đủ để `atsy` tìm ra `attendance-sync`
 * mà không cần thư viện nào.
 */
function fuzzyScore(needle: string, hay: string): number | null {
  if (!needle) return 0;
  const n = needle.toLowerCase();
  const h = hay.toLowerCase();

  if (h === n) return 1000;
  if (h.startsWith(n)) return 900 - h.length;

  const idx = h.indexOf(n);
  if (idx >= 0) return 700 - idx - h.length * 0.1;

  // Khớp rời rạc: ký tự phải xuất hiện đúng thứ tự.
  let hi = 0;
  let gaps = 0;
  for (const ch of n) {
    const found = h.indexOf(ch, hi);
    if (found < 0) return null;
    gaps += found - hi;
    hi = found + 1;
  }
  return 400 - gaps - h.length * 0.1;
}

export function CommandPalette({
  open,
  onClose,
  services,
  onPick,
}: {
  open: boolean;
  onClose: () => void;
  services: ServiceSummary[];
  onPick: (s: ServiceSummary) => void;
}) {
  const t = useT();
  const [query, setQuery] = useState("");
  const [cursor, setCursor] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLUListElement>(null);

  useEffect(() => {
    if (open) {
      setQuery("");
      setCursor(0);
      // Focus phải đợi element vào DOM.
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  const results = useMemo(() => {
    const scored = services
      .map((s) => {
        const byName = fuzzyScore(query, s.name);
        const byImage = query ? fuzzyScore(query, s.image ?? "") : null;
        const score = Math.max(byName ?? -Infinity, byImage === null ? -Infinity : byImage - 200);
        return score === -Infinity ? null : { s, score };
      })
      .filter((x): x is { s: ServiceSummary; score: number } => x !== null);

    scored.sort((a, b) => b.score - a.score || a.s.name.localeCompare(b.s.name));
    return scored.slice(0, 40).map((x) => x.s);
  }, [services, query]);

  // Con trỏ phải nằm trong danh sách sau khi lọc lại.
  useEffect(() => {
    setCursor((c) => Math.min(c, Math.max(results.length - 1, 0)));
  }, [results.length]);

  useEffect(() => {
    listRef.current?.querySelector('[data-active="true"]')?.scrollIntoView({ block: "nearest" });
  }, [cursor]);

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center p-16"
      style={{ background: "rgb(0 0 0 / 0.45)" }}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        className="w-[620px] overflow-hidden rounded-lg border shadow-2xl"
        style={{ background: "var(--surface-1)" }}
      >
        <input
          ref={inputRef}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={t("Nhảy tới service… (Enter để mở, Esc để đóng)")}
          className="w-full border-b bg-transparent px-3 py-2.5 text-[14px] outline-none"
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              onClose();
            } else if (e.key === "ArrowDown") {
              e.preventDefault();
              setCursor((c) => Math.min(c + 1, results.length - 1));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setCursor((c) => Math.max(c - 1, 0));
            } else if (e.key === "Enter") {
              e.preventDefault();
              const pick = results[cursor];
              if (pick) {
                onPick(pick);
                onClose();
              }
            }
          }}
        />

        <ul ref={listRef} className="max-h-[420px] overflow-y-auto">
          {results.length === 0 && (
            <li className="px-3 py-4 text-center text-[12px] text-[var(--ink-muted)]">
              {t("Không có service nào khớp “{query}”.", { query })}
            </li>
          )}
          {results.map((s, i) => (
            <li key={`${s.region}/${s.name}`}>
              <button
                data-active={i === cursor}
                onMouseEnter={() => setCursor(i)}
                onClick={() => {
                  onPick(s);
                  onClose();
                }}
                className={cn(
                  "flex w-full items-center gap-2 px-3 py-1.5 text-left text-[13px]",
                  i === cursor && "bg-[var(--surface-2)]",
                )}
              >
                <HealthDot health={s.health} message={s.healthMessage} />
                <span className="min-w-0 flex-1 truncate">{s.name}</span>
                <span className="shrink-0 text-[11px] text-[var(--ink-muted)]">
                  {regionLabel(s.region)}
                </span>
              </button>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
