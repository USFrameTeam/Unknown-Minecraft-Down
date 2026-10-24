// E2E（UI 层，Tauri IPC 已 mock）：
// 1. 未引导时强制进入向导，第一页可见免责声明（含替代方案说明）；
// 2. 完成路径配置后进入主界面（主页）；
// 3. 主页展示下载进度（事件驱动）；
// 4. 原版下载流程 → 队列 → 进度 → 完成提示；
// 5. Forge 选项卡发起下载（带 javaPath）；
// 6. 多目录切换；
// 7. 取消任务二次确认；校验页表格。
import { test, expect, type Page } from "@playwright/test";
import { installTauriShim } from "./tauri-shim";

const VERSIONS = [
  { id: "1.20.1", type: "release", url: "https://x/1.20.1.json", releaseTime: "2023-06-12T13:25:51+00:00" },
  { id: "1.21", type: "release", url: "https://x/1.21.json", releaseTime: "2024-06-13T10:11:49+00:00" },
  { id: "24w14a", type: "snapshot", url: "https://x/24w14a.json", releaseTime: "2024-04-03T12:51:00+00:00" },
  { id: "b1.7.3", type: "old_beta", url: "https://x/b173.json", releaseTime: "2011-07-07T00:00:00+00:00" },
];

const ONBOARDED_SETTINGS = {
  onboarded: true,
  gameDirs: ["/home/tester/.minecraft"],
  activeGameDir: "/home/tester/.minecraft",
  mirror: "official",
  concurrency: 12,
  javaPath: null,
};

async function gotoWithShim(page: Page, fixtures: Record<string, unknown>) {
  await page.addInitScript(installTauriShim, fixtures as never);
  await page.goto("/");
}

test("未完成向导时强制引导，免责声明含替代方案说明", async ({ page }) => {
  await gotoWithShim(page, {
    settings: { onboarded: false, gameDirs: [], activeGameDir: null, mirror: "official", concurrency: 12, javaPath: null },
    versions: VERSIONS,
    tasks: [],
  });
  await expect(page.getByTestId("onboarding-welcome")).toBeVisible();
  await expect(page.getByTestId("disclaimer")).toContainText("仅作为备用方案");
  await expect(page.getByTestId("disclaimer")).toContainText("作为替代方案供临时使用");
  await expect(page.getByTestId("onboarding-next")).toBeDisabled();
});

test("完成向导后进入主页，进度事件驱动主页进度面板", async ({ page }) => {
  await gotoWithShim(page, {
    settings: { onboarded: false, gameDirs: [], activeGameDir: null, mirror: "official", concurrency: 12, javaPath: null },
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

  // 主界面默认主页
  await expect(page.getByTestId("home-panel")).toBeVisible();
  await expect(page.getByTestId("status-bar")).toBeVisible();

  // 模拟引擎进度事件 → 主页进度面板 + 底部状态栏
  await page.evaluate(() => {
    const w = window as unknown as { __emitTauriEvent: (e: string, p: unknown) => void };
    w.__emitTauriEvent("download://progress", {
      taskId: "task-1",
      versionId: "1.20.1",
      gameDir: "/home/tester/.minecraft",
      mirror: "official",
      status: "running",
      stage: "assets",
      downloadedBytes: 5_000_000,
      totalBytes: 10_000_000,
      speedBps: 2_000_000,
      filesDone: 100,
      filesTotal: 200,
      createdAt: Date.now(),
    });
  });
  await expect(page.getByTestId("home-progress")).toBeVisible();
  await expect(page.getByTestId("home-task-1.20.1")).toBeVisible();
  await expect(page.getByTestId("status-bar")).toContainText("下载中 ×1");
});

test("原版：版本页发起下载 → 队列进度 → 完成兼容性提示", async ({ page }) => {
  await gotoWithShim(page, {
    settings: ONBOARDED_SETTINGS,
    versions: VERSIONS,
    tasks: [],
  });

  await page.getByTestId("nav-versions").click();
  await expect(page.getByTestId("version-row-1.20.1")).toBeVisible();

  await page.getByTestId("download-1.20.1").click();
  await expect(page.getByTestId("confirm-dialog")).toContainText("下载 1.20.1");
  await page.getByTestId("confirm-accept").click();

  await page.getByTestId("nav-queue").click();
  await expect(page.getByTestId("task-1.20.1")).toBeVisible();

  await page.evaluate(() => {
    const w = window as unknown as { __emitTauriEvent: (e: string, p: unknown) => void };
    w.__emitTauriEvent("download://progress", {
      taskId: "task-1",
      versionId: "1.20.1",
      gameDir: "/home/tester/.minecraft",
      mirror: "official",
      status: "running",
      stage: "libraries",
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

test("Forge 选项卡：选构建 → 确认 → 以 javaPath 发起任务", async ({ page }) => {
  await gotoWithShim(page, {
    settings: { ...ONBOARDED_SETTINGS, javaPath: "/opt/java/bin/java" },
    versions: VERSIONS,
    tasks: [],
  });

  await page.getByTestId("nav-versions").click();
  await page.getByTestId("tab-forge").click();
  await page.getByTestId("forge-mc-select").selectOption("1.20.1");
  await expect(page.getByTestId("forge-build-select")).toHaveValue("47.3.0");

  await page.getByTestId("forge-download").click();
  await expect(page.getByTestId("confirm-dialog")).toContainText("Forge 47.3.0");
  await page.getByTestId("confirm-accept").click();

  const started = await page.evaluate(
    () => (window as unknown as { __mcMock: { started: Array<Record<string, unknown>> } }).__mcMock.started,
  );
  expect(started).toHaveLength(1);
  expect(started[0].source).toEqual({
    mcVersion: "1.20.1",
    forgeVersion: "47.3.0",
    branch: null,
  });
  expect(started[0].javaPath).toBe("/opt/java/bin/java");
});

test("多目录：设置页添加与切换，版本页目录选择器联动", async ({ page }) => {
  await gotoWithShim(page, {
    settings: {
      ...ONBOARDED_SETTINGS,
      gameDirs: ["/home/tester/.minecraft", "/data/other-mc"],
      activeGameDir: "/home/tester/.minecraft",
    },
    versions: VERSIONS,
    tasks: [],
  });

  // 版本页出现目录选择器，切换后新任务写入新目录
  await page.getByTestId("nav-versions").click();
  await expect(page.getByTestId("dir-select")).toBeVisible();
  await page.getByTestId("dir-select").selectOption("/data/other-mc");

  await page.getByTestId("download-1.21").click();
  await page.getByTestId("confirm-accept").click();
  const started = await page.evaluate(
    () => (window as unknown as { __mcMock: { started: Array<Record<string, unknown>> } }).__mcMock.started,
  );
  expect(started[0].gameDir).toBe("/data/other-mc");

  // 设置页目录列表可见两个目录
  await page.getByTestId("nav-settings").click();
  await expect(page.getByTestId("dir-list")).toContainText("/data/other-mc");
  await expect(page.getByTestId("dir-list")).toContainText("/home/tester/.minecraft");
});

test("取消任务需要二次确认", async ({ page }) => {
  await gotoWithShim(page, {
    settings: ONBOARDED_SETTINGS,
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
    settings: ONBOARDED_SETTINGS,
    versions: VERSIONS,
    tasks: [],
  });
  await page.getByTestId("nav-verify").click();
  await page.getByTestId("verify-version-input").fill("1.20.1");
  await page.getByTestId("verify-run").click();
  await expect(page.getByTestId("problem-warning")).toContainText("缺失文件（1）");
  await expect(page.getByTestId("problem-warning")).toContainText("assets/objects/aa/abc");
});
