// E2E（UI 层，Tauri IPC 已 mock）：
// 1. 未引导时强制进入向导，第一页可见免责声明；
// 2. 完成路径配置后进入主界面；
// 3. 选择版本 → 确认下载 → 队列出现任务 → 模拟进度事件 → 显示兼容性提示；
// 4. 取消任务需二次确认。
import { test, expect, type Page } from "@playwright/test";
import { installTauriShim } from "./tauri-shim";

const VERSIONS = [
  { id: "1.20.1", type: "release", url: "https://x/1.20.1.json", releaseTime: "2023-06-12T13:25:51+00:00" },
  { id: "1.21", type: "release", url: "https://x/1.21.json", releaseTime: "2024-06-13T10:11:49+00:00" },
  { id: "24w14a", type: "snapshot", url: "https://x/24w14a.json", releaseTime: "2024-04-03T12:51:00+00:00" },
  { id: "b1.7.3", type: "old_beta", url: "https://x/b173.json", releaseTime: "2011-07-07T00:00:00+00:00" },
];

async function gotoWithShim(page: Page, fixtures: Record<string, unknown>) {
  await page.addInitScript(installTauriShim, fixtures as never);
  await page.goto("/");
}

test("未完成向导时强制引导，且免责声明可见", async ({ page }) => {
  await gotoWithShim(page, {
    settings: { onboarded: false, gameDir: null, mirror: "official", concurrency: 12 },
    versions: VERSIONS,
    tasks: [],
  });
  await expect(page.getByTestId("onboarding-welcome")).toBeVisible();
  await expect(page.getByTestId("disclaimer")).toContainText("仅作为备用方案");
  await expect(page.getByTestId("onboarding-next")).toBeDisabled();
});

test("完成向导后进入主界面并可发起下载", async ({ page }) => {
  await gotoWithShim(page, {
    settings: { onboarded: false, gameDir: null, mirror: "official", concurrency: 12 },
    versions: VERSIONS,
    tasks: [],
  });

  // 第 1 页：勾选免责声明
  await page.getByTestId("disclaimer-checkbox").click();
  await page.getByTestId("onboarding-next").click();
  // 第 2 页：自动探测
  await page.getByTestId("btn-auto-detect").click();
  await expect(page.getByTestId("detected-path").locator("input")).toHaveValue(
    "/home/tester/.minecraft",
  );
  await page.getByTestId("onboarding-finish").click();

  // 主界面：版本列表
  await expect(page.getByTestId("version-picker")).toBeVisible();
  await expect(page.getByTestId("version-row-1.20.1")).toBeVisible();

  // 发起下载（确认对话框是破坏性操作二次确认）
  await page.getByTestId("download-1.20.1").click();
  await expect(page.getByTestId("confirm-dialog")).toContainText("下载 1.20.1");
  await page.getByTestId("confirm-accept").click();

  // 队列页出现任务
  await page.getByTestId("nav-queue").click();
  await expect(page.getByTestId("task-1.20.1")).toBeVisible();

  // 模拟引擎进度事件 → 进度条推进
  await page.evaluate(() => {
    const w = window as unknown as { __emitTauriEvent: (e: string, p: unknown) => void };
    w.__emitTauriEvent("download://progress", {
      taskId: "task-1",
      versionId: "1.20.1",
      gameDir: "/home/tester/.minecraft",
      mirror: "official",
      status: "running",
      stage: "assets",
      downloadedBytes: 6_000_000,
      totalBytes: 10_000_000,
      speedBps: 2_000_000,
      filesDone: 120,
      filesTotal: 200,
      createdAt: Date.now(),
    });
  });
  await expect(
    page.getByTestId("task-1.20.1").getByRole("progressbar"),
  ).toHaveAttribute("aria-valuenow", "60");
  await expect(page.getByTestId("task-1.20.1")).toContainText("下载中");

  // 完成事件 → 兼容性提示（明确说明需要启动器刷新识别）
  await page.evaluate(() => {
    const w = window as unknown as { __emitTauriEvent: (e: string, p: unknown) => void };
    w.__emitTauriEvent("download://progress", {
      taskId: "task-1",
      versionId: "1.20.1",
      gameDir: "/home/tester/.minecraft",
      mirror: "official",
      status: "completed",
      downloadedBytes: 10_000_000,
      totalBytes: 10_000_000,
      speedBps: 0,
      filesDone: 200,
      filesTotal: 200,
      createdAt: Date.now(),
    });
  });
  await expect(page.getByTestId("task-1.20.1")).toContainText("刷新版本 / 导入");
});

test("取消任务需要二次确认", async ({ page }) => {
  await gotoWithShim(page, {
    settings: { onboarded: true, gameDir: "/home/tester/.minecraft", mirror: "official", concurrency: 12 },
    versions: VERSIONS,
    tasks: [
      {
        taskId: "task-9",
        versionId: "1.21",
        gameDir: "/home/tester/.minecraft",
        mirror: "official",
        status: "running",
        stage: "libraries",
        downloadedBytes: 1000,
        totalBytes: 9000,
        speedBps: 500,
        filesDone: 3,
        filesTotal: 10,
        createdAt: 1,
      },
    ],
  });
  await page.getByTestId("nav-queue").click();
  await expect(page.getByTestId("task-1.21")).toBeVisible();
  await page.getByTestId("task-1.21").getByRole("button", { name: "取消" }).click();
  await expect(page.getByTestId("confirm-dialog")).toContainText("取消下载 1.21");
  await page.getByTestId("confirm-accept").click();
  const mock = await page.evaluate(
    () => (window as unknown as { __mcMock: { canceled: string[] } }).__mcMock.canceled,
  );
  expect(mock).toEqual(["task-9"]);
});

test("安装校验页展示缺失文件表格", async ({ page }) => {
  await gotoWithShim(page, {
    settings: { onboarded: true, gameDir: "/home/tester/.minecraft", mirror: "official", concurrency: 12 },
    versions: VERSIONS,
    tasks: [],
  });
  await page.getByTestId("nav-verify").click();
  await page.getByTestId("verify-version-input").fill("1.20.1");
  await page.getByTestId("verify-run").click();
  await expect(page.getByTestId("problem-warning")).toContainText("缺失文件（1）");
  await expect(page.getByTestId("problem-warning")).toContainText("assets/objects/aa/abc");
});
