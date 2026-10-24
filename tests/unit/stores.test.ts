// store 单测：通过 mock @tauri-apps/api/core 拦截 invoke
import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import { useAppStore } from "../../src/stores/appStore";
import { useDownloadStore } from "../../src/stores/downloadStore";
import type { TaskSnapshot } from "../../src/types";

function snap(partial: Partial<TaskSnapshot>): TaskSnapshot {
  return {
    taskId: "t1",
    versionId: "1.20.1",
    gameDir: "/tmp/mc",
    mirror: "official",
    status: "running",
    downloadedBytes: 0,
    totalBytes: 1000,
    speedBps: 0,
    filesDone: 0,
    filesTotal: 0,
    createdAt: 1,
    ...partial,
  };
}

beforeEach(() => {
  invokeMock.mockReset();
  useAppStore.setState({
    ready: false,
    onboarded: false,
    gameDirs: [],
    activeGameDir: null,
    mirror: "official",
    concurrency: 12,
    javaPath: null,
  });
  useDownloadStore.setState({ tasks: {}, order: [] });
});

describe("appStore（多目录）", () => {
  it("load 读取设置（含旧单目录迁移前的后端已归一化数据）", async () => {
    invokeMock.mockResolvedValueOnce({
      onboarded: true,
      gameDirs: ["/a/.minecraft", "/b/.minecraft"],
      activeGameDir: "/b/.minecraft",
      mirror: "bmclapi",
      concurrency: 8,
      javaPath: "/usr/bin/java",
    });
    await useAppStore.getState().load();
    const s = useAppStore.getState();
    expect(s.onboarded).toBe(true);
    expect(s.gameDirs).toEqual(["/a/.minecraft", "/b/.minecraft"]);
    expect(s.activeGameDir).toBe("/b/.minecraft");
    expect(s.javaPath).toBe("/usr/bin/java");
  });

  it("save 合并补丁并整体提交", async () => {
    invokeMock.mockResolvedValue(undefined);
    useAppStore.setState({
      onboarded: false,
      gameDirs: ["/a"],
      activeGameDir: "/a",
      mirror: "official",
      concurrency: 12,
      javaPath: null,
    });
    await useAppStore.getState().save({ onboarded: true });
    expect(invokeMock).toHaveBeenCalledWith("save_settings", {
      settings: {
        onboarded: true,
        gameDirs: ["/a"],
        activeGameDir: "/a",
        mirror: "official",
        concurrency: 12,
        javaPath: null,
      },
    });
    expect(useAppStore.getState().onboarded).toBe(true);
  });

  it("addGameDir 去重且首个目录自动激活", async () => {
    invokeMock.mockResolvedValue(undefined);
    await useAppStore.getState().addGameDir("/a");
    await useAppStore.getState().addGameDir("/a"); // 重复忽略
    let s = useAppStore.getState();
    expect(s.gameDirs).toEqual(["/a"]);
    expect(s.activeGameDir).toBe("/a");
    await useAppStore.getState().addGameDir("/b");
    s = useAppStore.getState();
    expect(s.gameDirs).toEqual(["/a", "/b"]);
    expect(s.activeGameDir).toBe("/a");
  });

  it("removeGameDir 移除激活目录时顺延", async () => {
    invokeMock.mockResolvedValue(undefined);
    const s0 = useAppStore.getState();
    await s0.addGameDir("/a");
    await s0.addGameDir("/b");
    await useAppStore.getState().setActiveGameDir("/a");
    expect(useAppStore.getState().activeGameDir).toBe("/a");
    await useAppStore.getState().removeGameDir("/a");
    const s = useAppStore.getState();
    expect(s.gameDirs).toEqual(["/b"]);
    expect(s.activeGameDir).toBe("/b");
    // 最后一个目录不允许移除
    await useAppStore.getState().removeGameDir("/b");
    expect(useAppStore.getState().gameDirs).toEqual(["/b"]);
  });
});

describe("downloadStore", () => {
  it("applySnapshot 新任务入队、旧任务更新且不重复", () => {
    const s = useDownloadStore.getState();
    s.applySnapshot(snap({ taskId: "a", status: "queued" }));
    s.applySnapshot(snap({ taskId: "a", status: "running", downloadedBytes: 400 }));
    const st = useDownloadStore.getState();
    expect(st.order).toEqual(["a"]);
    expect(st.tasks["a"].status).toBe("running");
    expect(st.tasks["a"].downloadedBytes).toBe(400);
  });

  it("refresh 从后端恢复任务列表", async () => {
    invokeMock.mockResolvedValueOnce([
      snap({ taskId: "r1", status: "paused" }),
      snap({ taskId: "r2", status: "completed" }),
    ]);
    await useDownloadStore.getState().refresh();
    const st = useDownloadStore.getState();
    expect(st.order).toEqual(["r1", "r2"]);
    expect(st.tasks["r2"].status).toBe("completed");
  });

  it("startTask 携带 javaPath（Forge 需要）", async () => {
    invokeMock.mockResolvedValueOnce("task-77");
    await useDownloadStore
      .getState()
      .startTask(
        { mcVersion: "1.20.1", forgeVersion: "47.3.0", branch: null },
        "1.20.1-forge-47.3.0",
        "/a",
        "official",
        12,
        "/usr/bin/java",
      );
    expect(invokeMock).toHaveBeenCalledWith("start_download", {
      source: { mcVersion: "1.20.1", forgeVersion: "47.3.0", branch: null },
      gameDir: "/a",
      mirror: "official",
      concurrency: 12,
      javaPath: "/usr/bin/java",
    });
  });

  it("cancel/pause/resume 调用对应命令", async () => {
    invokeMock.mockResolvedValue(undefined);
    await useDownloadStore.getState().cancelTask("t");
    await useDownloadStore.getState().pauseTask("t");
    await useDownloadStore.getState().resumeTask("t");
    expect(invokeMock).toHaveBeenNthCalledWith(1, "cancel_download", { taskId: "t" });
    expect(invokeMock).toHaveBeenNthCalledWith(2, "pause_download", { taskId: "t" });
    expect(invokeMock).toHaveBeenNthCalledWith(3, "resume_download", { taskId: "t" });
  });

  it("clearFinished 只保留未结束任务", async () => {
    invokeMock.mockResolvedValue(undefined);
    const s = useDownloadStore.getState();
    s.applySnapshot(snap({ taskId: "a", status: "completed" }));
    s.applySnapshot(snap({ taskId: "b", status: "running" }));
    await useDownloadStore.getState().clearFinished();
    const st = useDownloadStore.getState();
    expect(st.order).toEqual(["b"]);
    expect(invokeMock).toHaveBeenCalledWith("clear_finished_tasks", {});
  });
});
