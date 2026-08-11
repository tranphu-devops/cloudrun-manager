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
 */

import { createContext, useCallback, useContext, useMemo } from "react";
import type { ReactNode } from "react";

import { setFormatLanguage } from "./format";
import { EN } from "./locales/en";
import type { Language } from "./types";

/** Tham số nội suy. Chỉ nhận scalar — object trong câu chữ luôn là dấu hiệu sai chỗ. */
export type TParams = Record<string, string | number>;

/** Từ điển của một ngôn ngữ: câu tiếng Việt → câu đã dịch. */
export type Dictionary = Record<string, string>;

const DICTS: Record<Language, Dictionary> = {
  // Tiếng Việt là ngôn ngữ gốc: key chính là giá trị, không cần từ điển.
  vi: {},
  en: EN,
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

/** Những key chưa có bản dịch sang `lang`. Dùng để soát, không dùng lúc render. */
export function missingKeys(lang: Language, keys: string[]): string[] {
  const dict = DICTS[lang] ?? {};
  return keys.filter((k) => !(k in dict));
}

export type TFn = (key: string, params?: TParams) => string;

interface I18nValue {
  lang: Language;
  t: TFn;
}

// Mặc định `en` để một component render ngoài Provider (test, storybook) không nổ.
const I18nContext = createContext<I18nValue>({
  lang: "en",
  t: (key, params) => translate("en", key, params),
});

export function I18nProvider({ lang, children }: { lang: Language; children: ReactNode }) {
  // Set trước khi cây con render, để `format.ts` (num, dateTime, regionLabel…) dùng đúng
  // locale ngay ở lần render đầu tiên sau khi đổi ngôn ngữ. Đặt trong useEffect thì lần
  // render đầu vẫn ra locale cũ và người dùng thấy chữ nhấp nháy.
  setFormatLanguage(lang);
  const t = useCallback<TFn>((key, params) => translate(lang, key, params), [lang]);
  const value = useMemo(() => ({ lang, t }), [lang, t]);
  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18nValue {
  return useContext(I18nContext);
}

/** Dùng nhiều nhất: `const t = useT()` rồi `t("…")`. */
export function useT(): TFn {
  return useContext(I18nContext).t;
}

/**
 * Định dạng ngày giờ theo ngôn ngữ đang chọn.
 *
 * Để ở đây thay vì `format.ts` vì nó cần biết `lang`. `format.ts` là hàm thuần, không
 * đọc context — giữ nguyên như vậy.
 */
export function localeTag(lang: Language): string {
  return lang === "vi" ? "vi-VN" : "en-US";
}

export const LANGUAGE_NAMES: Record<Language, string> = {
  en: "English",
  vi: "Tiếng Việt",
};
