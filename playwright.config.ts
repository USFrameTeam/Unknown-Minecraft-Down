import { defineConfig } from "@playwright/test";

// E2E：普通 Chromium + Tauri IPC mock（__TAURI_INTERNALS__ 注入），
// 覆盖 UI 流程逻辑；真实窗口的端到端验证见 tests/dsh-plugin 方案。
export default defineConfig({
  testDir: "tests/e2e",
  timeout: 30_000,
  expect: { timeout: 15_000 },
  use: {
    baseURL: "http://localhost:1420",
    viewport: { width: 1120, height: 740 },
  },
  webServer: {
    command: "npm run dev",
    url: "http://localhost:1420",
    reuseExistingServer: true,
    timeout: 60_000,
  },
  projects: [{ name: "chromium", use: { browserName: "chromium" } }],
});
