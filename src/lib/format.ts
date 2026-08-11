/**
 * Định dạng hiển thị.
 *
 * # Vì sao có biến ngôn ngữ ở tầm module thay vì tham số
 *
 * Mấy hàm này được gọi ở khoảng một trăm chỗ, phần lớn nằm sâu trong JSX (`{num(x)}`).
 * Thêm tham số `lang` vào từng chữ ký nghĩa là sửa cả trăm call site và kéo `useT()` vào
 * những component thuần trình bày. Nên ngôn ngữ để ở tầm module, `I18nProvider` set một
 * lần khi đổi — cùng cách `moment.locale()` hay `dayjs.locale()` làm.
 *
 * Hệ quả cần biết: hàm ở đây **không còn thuần tuyệt đối**. Chúng đọc một biến ngoài. Đổi
 * ngôn ngữ mà React không render lại thì chữ cũ vẫn nằm đó — `I18nProvider` xử lý bằng
 * cách set trước khi render và `key` theo `lang` ở gốc cây.
 */

import type { Language } from "./types";

let CURRENT: Language = "en";

/** `I18nProvider` gọi. Đừng gọi từ chỗ khác — hai nguồn sự thật là lỗi chờ xảy ra. */
export function setFormatLanguage(lang: Language) {
  CURRENT = lang;
}

function tag(): string {
  return CURRENT === "vi" ? "vi-VN" : "en-US";
}

export function cn(...parts: Array<string | false | null | undefined>) {
  return parts.filter(Boolean).join(" ");
}

export function num(v: number | null | undefined, digits = 0): string {
  if (v === null || v === undefined || Number.isNaN(v)) return "–";
  return v.toLocaleString(tag(), {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  });
}

/** Số cho badge: gọn nhưng không mất thông tin ở khoảng nhỏ. */
export function compact(v: number | null | undefined): string {
  if (v === null || v === undefined || Number.isNaN(v)) return "–";
  if (v === 0) return "0";
  if (v < 1) return v.toFixed(2).replace(/0+$/, "").replace(/\.$/, "");
  if (v < 10) return v.toFixed(1);
  if (v < 1000) return Math.round(v).toString();
  if (v < 1_000_000) return `${(v / 1000).toFixed(1)}k`;
  return `${(v / 1_000_000).toFixed(1)}M`;
}

export function percent(v: number | null | undefined, digits = 1): string {
  if (v === null || v === undefined || Number.isNaN(v)) return "–";
  return `${(v * 100).toFixed(digits)}%`;
}

export function ms(v: number | null | undefined): string {
  if (v === null || v === undefined || Number.isNaN(v)) return "–";
  if (v < 1) return `${(v * 1000).toFixed(0)}µs`;
  if (v < 1000) return `${v.toFixed(v < 10 ? 1 : 0)}ms`;
  return `${(v / 1000).toFixed(2)}s`;
}

/** Khoảng thời gian tính bằng giây → câu gọn. */
export function agoSeconds(s: number): string {
  const vi = CURRENT === "vi";
  if (s < 5) return vi ? "vừa xong" : "just now";
  if (s < 60) return vi ? `${Math.round(s)}s trước` : `${Math.round(s)}s ago`;
  if (s < 3600) {
    const m = Math.round(s / 60);
    return vi ? `${m} phút trước` : `${m} min ago`;
  }
  if (s < 86400) {
    const h = Math.round(s / 3600);
    return vi ? `${h} giờ trước` : `${h}h ago`;
  }
  const d = Math.round(s / 86400);
  return vi ? `${d} ngày trước` : `${d}d ago`;
}

export function ago(iso: string | null | undefined): string {
  if (!iso) return "–";
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return "–";
  return agoSeconds((Date.now() - t) / 1000);
}

export function dateTime(iso: string | null | undefined): string {
  if (!iso) return "–";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "–";
  return d.toLocaleString(tag(), {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

/** Nhãn trục thời gian: chỉ hiện giờ:phút nếu cửa sổ ngắn, thêm ngày nếu dài. */
export function timeAxis(t: number, windowMinutes: number): string {
  const d = new Date(t);
  if (windowMinutes <= 1440) {
    return d.toLocaleTimeString(tag(), { hour: "2-digit", minute: "2-digit" });
  }
  return d.toLocaleString(tag(), {
    day: "2-digit",
    month: "2-digit",
    hour: "2-digit",
  });
}

export function timeTooltip(t: number): string {
  return new Date(t).toLocaleString(tag(), {
    day: "2-digit",
    month: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

/**
 * `asia-northeast1` → `asia-northeast1 · Tokyo` cho những region hay dùng.
 *
 * Tên thành phố giữ nguyên dạng tiếng Anh ở cả hai ngôn ngữ, trừ vài chỗ tiếng Việt có
 * tên riêng thật sự phổ biến. Dịch "Iowa" thành gì đó khác chỉ làm khó tra cứu.
 */
const REGION_CITY: Record<string, { en: string; vi?: string }> = {
  "asia-northeast1": { en: "Tokyo" },
  "asia-northeast2": { en: "Osaka" },
  "asia-northeast3": { en: "Seoul" },
  "asia-southeast1": { en: "Singapore" },
  "asia-southeast2": { en: "Jakarta" },
  "asia-east1": { en: "Taiwan", vi: "Đài Loan" },
  "asia-east2": { en: "Hong Kong" },
  "asia-south1": { en: "Mumbai" },
  "us-central1": { en: "Iowa" },
  "us-east1": { en: "South Carolina" },
  "us-west1": { en: "Oregon" },
  "europe-west1": { en: "Belgium", vi: "Bỉ" },
  "europe-west4": { en: "Netherlands", vi: "Hà Lan" },
};

export function regionLabel(r: string): string {
  const entry = REGION_CITY[r];
  if (!entry) return r;
  const city = CURRENT === "vi" ? (entry.vi ?? entry.en) : entry.en;
  return `${r} · ${city}`;
}

/** Rút gọn image URI dài thành `repo/name:tag` để bảng không bị đẩy ngang. */
export function shortImage(image: string | null): string {
  if (!image) return "–";
  const parts = image.split("/");
  return parts.length <= 2 ? image : parts.slice(-2).join("/");
}

export function shortSha(image: string | null): string | null {
  if (!image) return null;
  const at = image.indexOf("@sha256:");
  return at >= 0 ? image.slice(at + 8, at + 20) : null;
}

/** Duration protobuf `300s` → `5 phút` / `5 min`. */
export function humanTimeout(t: string | null): string {
  if (!t) return "–";
  const m = /^(\d+(?:\.\d+)?)s$/.exec(t.trim());
  if (!m || !m[1]) return t;
  const s = Number(m[1]);
  if (s < 60) return `${s}s`;
  const min = Math.floor(s / 60);
  const rest = Math.round(s - min * 60);
  const unit = CURRENT === "vi" ? "phút" : "min";
  return rest === 0 ? `${min} ${unit}` : `${min} ${unit} ${rest}s`;
}

export const SEVERITY_ORDER = [
  "DEFAULT",
  "DEBUG",
  "INFO",
  "NOTICE",
  "WARNING",
  "ERROR",
  "CRITICAL",
  "ALERT",
  "EMERGENCY",
] as const;
