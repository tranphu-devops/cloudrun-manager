import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// Cổng cố định 1421 vì tauri.conf.json trỏ devUrl tới đây.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: {
    port: 1421,
    strictPort: true,
    watch: {
      // src-tauri do cargo watch lo, vite không cần theo dõi.
      ignored: ["**/src-tauri/**", "**/crates/**", "**/target/**"],
    },
  },
  build: {
    // WebView2 trên Windows 11 và WebKit trên macOS đều hỗ trợ ES2021.
    target: "es2021",
    minify: "esbuild",
    sourcemap: false,
  },
});
