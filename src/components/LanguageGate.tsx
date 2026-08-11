import type { ReactNode } from "react";

import { I18nProvider } from "../lib/i18n";
import { useSettings } from "../lib/queries";

/**
 * Đưa ngôn ngữ đã lưu trong `Settings` xuống cả cây React.
 *
 * Phải nằm **trên** `ToastHost` (toast cũng có chữ) nhưng **dưới** `QueryClientProvider`
 * (ngôn ngữ đọc từ query `get_settings`). Trong lúc query chưa xong thì dùng `en` — cùng
 * mặc định với `Settings::default()` bên Rust, nên không có cú nháy đổi ngôn ngữ ở lần
 * mở app đầu tiên.
 *
 * **Không** remount cây con khi đổi ngôn ngữ (không đặt `key={lang}`). Remount sẽ đóng luôn
 * dialog Cài đặt — tức là đóng đúng chỗ người dùng vừa bấm — và xoá service đang chọn. Đổi
 * context là đủ: `format.ts` set locale ngay trong lượt render của Provider, và không có chỗ
 * nào `useMemo` một chuỗi đã format (nếu sau này có, nhớ cho `lang` vào deps của nó).
 */
export function LanguageGate({ children }: { children: ReactNode }) {
  const settings = useSettings();
  const lang = settings.data?.language ?? "en";
  return <I18nProvider lang={lang}>{children}</I18nProvider>;
}
