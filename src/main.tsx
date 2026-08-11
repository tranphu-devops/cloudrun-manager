import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import App from "./App";
import { LanguageGate } from "./components/LanguageGate";
import { ToastHost } from "./components/ui";
import "./styles.css";

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      // Lỗi ở đây gần như luôn là quyền/xác thực/nhập liệu — retry chỉ làm chậm việc
      // hiện thông báo hữu ích. Retry mạng đã được xử lý ở tầng Rust.
      retry: false,
      refetchOnWindowFocus: true,
      refetchOnReconnect: true,
    },
  },
});

const root = document.getElementById("root");
if (!root) throw new Error("không tìm thấy #root");

createRoot(root).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <LanguageGate>
        <ToastHost>
          <App />
        </ToastHost>
      </LanguageGate>
    </QueryClientProvider>
  </StrictMode>,
);
