/** Định dạng hiển thị. Tất cả dùng locale vi-VN. */

export function cn(...parts: Array<string | false | null | undefined>) {
  return parts.filter(Boolean).join(" ");
}

export function num(v: number | null | undefined, digits = 0): string {
  if (v === null || v === undefined || Number.isNaN(v)) return "–";
  return v.toLocaleString("vi-VN", {
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

/** Khoảng thời gian tính bằng giây → câu tiếng Việt gọn. */
export function agoSeconds(s: number): string {
  if (s < 5) return "vừa xong";
  if (s < 60) return `${Math.round(s)}s trước`;
  if (s < 3600) return `${Math.round(s / 60)} phút trước`;
  if (s < 86400) return `${Math.round(s / 3600)} giờ trước`;
  return `${Math.round(s / 86400)} ngày trước`;
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
  return d.toLocaleString("vi-VN", {
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
    return d.toLocaleTimeString("vi-VN", { hour: "2-digit", minute: "2-digit" });
  }
  return d.toLocaleString("vi-VN", {
    day: "2-digit",
    month: "2-digit",
    hour: "2-digit",
  });
}

export function timeTooltip(t: number): string {
  return new Date(t).toLocaleString("vi-VN", {
    day: "2-digit",
    month: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

/** `asia-northeast1` → `asia-northeast1 (Tokyo)` cho những region hay dùng. */
const REGION_CITY: Record<string, string> = {
  "asia-northeast1": "Tokyo",
  "asia-northeast2": "Osaka",
  "asia-northeast3": "Seoul",
  "asia-southeast1": "Singapore",
  "asia-southeast2": "Jakarta",
  "asia-east1": "Đài Loan",
  "asia-east2": "Hong Kong",
  "asia-south1": "Mumbai",
  "us-central1": "Iowa",
  "us-east1": "South Carolina",
  "us-west1": "Oregon",
  "europe-west1": "Bỉ",
  "europe-west4": "Hà Lan",
};

export function regionLabel(r: string): string {
  const city = REGION_CITY[r];
  return city ? `${r} · ${city}` : r;
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

/** Duration protobuf `300s` → `5 phút`. */
export function humanTimeout(t: string | null): string {
  if (!t) return "–";
  const m = /^(\d+(?:\.\d+)?)s$/.exec(t.trim());
  if (!m || !m[1]) return t;
  const s = Number(m[1]);
  if (s < 60) return `${s}s`;
  const min = Math.floor(s / 60);
  const rest = Math.round(s - min * 60);
  return rest === 0 ? `${min} phút` : `${min} phút ${rest}s`;
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
