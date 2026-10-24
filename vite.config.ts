import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// Tauri 前端开发服务器：固定端口 1420
export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: "es2022",
    outDir: "dist",
    sourcemap: false,
  },
  test: {
    // Playwright 的 spec 由 playwright test 运行，不进入 Vitest
    exclude: ["**/node_modules/**", "tests/e2e/**"],
  },
});
