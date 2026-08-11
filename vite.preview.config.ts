import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { resolve } from "node:path";

/**
 * Build UI với dữ liệu giả, không cần Tauri và không cần GCP.
 *
 * Dùng để làm/kiểm tra giao diện: `npm run preview:ui` rồi mở http://localhost:1422.
 * Alias thay tầng IPC bằng mock trong `preview/`.
 */
export default defineConfig({
  root: resolve(__dirname, "preview"),
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@tauri-apps/api/core": resolve(__dirname, "preview/mock-core.ts"),
      "@tauri-apps/plugin-opener": resolve(__dirname, "preview/mock-opener.ts"),
      "/src": resolve(__dirname, "src"),
    },
  },
  // 1422 chứ không phải 1421: `vite.config.ts` (dev server thật của Tauri) đang giữ 1421
  // với strictPort, nên dùng chung cổng sẽ làm `tauri dev` chết nếu preview đang bật.
  server: { port: 1422, strictPort: true },
  build: { outDir: resolve(__dirname, "dist-preview"), emptyOutDir: true },
});
