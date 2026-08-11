/**
 * i18n cho tầng React.
 *
 * # Vì sao key là chuỗi tiếng Việt, không phải `settings.readOnly.label`
 *
 * App này viết tiếng Việt trước, tiếng Anh thêm sau. Đặt key trừu tượng cho ~700 chuỗi
 * có sẵn nghĩa là (a) bịa 700 cái tên, (b) mọi chỗ trong JSX mất luôn nội dung — đọc code
 * không còn biết nút đó ghi gì. Nên key **chính là câu tiếng Việt**, kiểu gettext:
 *
 * ```tsx
 * <button>{t("Áp dụng thay đổi")}</button>
 * ```
 *
 * Hệ quả quan trọng: **thiếu bản dịch thì rơi về tiếng Việt**, không ra ô trống hay
 * `undefined`. Với một công cụ vận hành thì hiện sai ngôn ngữ còn hơn hiện chuỗi rỗng ở
 * chỗ đáng lẽ là cảnh báo.
 *
 * Đánh đổi: sửa câu tiếng Việt là mất bản dịch của câu đó (nó im lặng rơi về tiếng Việt).
 * `missingKeys()` bên dưới liệt kê những key đang rơi để soát lại — dev build gọi nó khi
 * chuyển ngôn ngữ.
 *
 * # Phạm vi
 *
 * CHỈ chuỗi do React sinh ra. Message lỗi từ Rust (`CmdError.message`, cron lint,
 * `CostReport.errorSources`) vẫn là tiếng Việt: dịch chúng cần đổi `CmdError` thành
 * key + tham số ở tầng Rust, là một việc riêng.
 *
 * # `t()` so với `tNode()` — đừng cắt câu để nhồi markup vào giữa
 *
 * Một câu cần `<code>`/`<strong>` ở giữa **không được** viết thành ba lời gọi `t()` rời
 * nhau kiểu `{t("Account hiện tại không có")} <code>…</code> {t("trên project này…")}`.
 * Cách đó dựa vào việc EN và VI xếp mệnh đề theo đúng thứ tự tiếng Việt — chỉ cần một ngôn
 * ngữ đảo trật tự từ (tiếng Nhật đặt động từ ở cuối, không phải giữa câu) là vị trí mảnh
 * markup sai chỗ, và người dịch không sửa được vì thứ tự do JSX quyết định, không do bản
 * dịch. Bài học rút ra từ đúng lỗi đó khi thêm tiếng Nhật vào bản gốc.
 *
 * Dùng `tNode()`: giữ nguyên cả câu làm một key, đặt `{name}` ngay chỗ markup cần chèn:
 *
 * ```tsx
 * {tNode("Account hiện tại không có {perm} trên project này, nên chỉ xem được metadata.", {
 *   perm: <code className="mono">secretmanager.versions.access</code>,
 * })}
 * ```
 *
 * Bản dịch tự do di chuyển `{perm}` tới đúng vị trí ngữ pháp của ngôn ngữ đó.
 */

import { createContext, Fragment, useCallback, useContext, useMemo } from "react";
import type { ReactNode } from "react";

import { setFormatLanguage } from "./format";
import { EN } from "./locales/en";
import { JA } from "./locales/ja";
import type { Language } from "./types";

/** Tham số nội suy. Chỉ nhận scalar — object trong câu chữ luôn là dấu hiệu sai chỗ. */
export type TParams = Record<string, string | number>;

/** Từ điển của một ngôn ngữ: câu tiếng Việt → câu đã dịch. */
export type Dictionary = Record<string, string>;

const DICTS: Record<Language, Dictionary> = {
  // Tiếng Việt là ngôn ngữ gốc: key chính là giá trị, không cần từ điển.
  vi: {},
  en: EN,
  ja: JA,
};

/**
 * Thay `{name}` bằng giá trị.
 *
 * Không tự escape gì cả — React đã escape khi render text node. Chỉ dùng cho text, đừng
 * đổ kết quả vào `dangerouslySetInnerHTML`.
 */
function interpolate(template: string, params?: TParams): string {
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (whole, key: string) =>
    key in params ? String(params[key]) : whole,
  );
}

export function translate(lang: Language, key: string, params?: TParams): string {
  const hit = DICTS[lang]?.[key];
  return interpolate(hit ?? key, params);
}

/** Tham số của `tNode`: mảnh JSX chèn vào giữa câu, xem chú thích ở đầu file. */
export type TNodeParams = Record<string, ReactNode>;

/**
 * Như `translate`, nhưng giữ nguyên `ReactNode` tại chỗ thay vì ép về chuỗi. Tách theo
 * đúng token `{name}` — phần còn lại của câu render dạng text node bình thường (React tự
 * escape).
 */
export function translateNode(lang: Language, key: string, params: TNodeParams): ReactNode {
  const template = DICTS[lang]?.[key] ?? key;
  const pieces = template.split(/(\{\w+\})/g);
  return pieces.map((piece, i) => {
    const m = /^\{(\w+)\}$/.exec(piece);
    if (m && m[1] && m[1] in params) {
      return <Fragment key={i}>{params[m[1]]}</Fragment>;
    }
    return piece;
  });
}

/** Những key chưa có bản dịch sang `lang`. Dùng để soát, không dùng lúc render. */
export function missingKeys(lang: Language, keys: string[]): string[] {
  const dict = DICTS[lang] ?? {};
  return keys.filter((k) => !(k in dict));
}

export type TFn = (key: string, params?: TParams) => string;
export type TNodeFn = (key: string, params: TNodeParams) => ReactNode;

interface I18nValue {
  lang: Language;
  t: TFn;
  tNode: TNodeFn;
}

// Mặc định `en` để một component render ngoài Provider (test, storybook) không nổ.
const I18nContext = createContext<I18nValue>({
  lang: "en",
  t: (key, params) => translate("en", key, params),
  tNode: (key, params) => translateNode("en", key, params),
});

export function I18nProvider({ lang, children }: { lang: Language; children: ReactNode }) {
  // Set trước khi cây con render, để `format.ts` (num, dateTime, regionLabel…) dùng đúng
  // locale ngay ở lần render đầu tiên sau khi đổi ngôn ngữ. Đặt trong useEffect thì lần
  // render đầu vẫn ra locale cũ và người dùng thấy chữ nhấp nháy.
  setFormatLanguage(lang);
  const t = useCallback<TFn>((key, params) => translate(lang, key, params), [lang]);
  const tNode = useCallback<TNodeFn>((key, params) => translateNode(lang, key, params), [lang]);
  const value = useMemo(() => ({ lang, t, tNode }), [lang, t, tNode]);
  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18nValue {
  return useContext(I18nContext);
}

/** Dùng nhiều nhất: `const t = useT()` rồi `t("…")`. */
export function useT(): TFn {
  return useContext(I18nContext).t;
}

/** Cho câu cần chèn markup ở giữa — xem chú thích ở đầu file trước khi dùng `t()` cho việc này. */
export function useTNode(): TNodeFn {
  return useContext(I18nContext).tNode;
}

export const LANGUAGE_NAMES: Record<Language, string> = {
  en: "English",
  vi: "Tiếng Việt",
  ja: "日本語",
};
