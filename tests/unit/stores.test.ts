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
    gameDir: null,
    mirror: "official",
    concurrency: 12,
  });
  useDownloadStore.setState({ tasks: {}, order: [] });
});

describe("appStore", () => {
  it("load 读取设置并解锁主界面", async () => {
    invokeMock.mockResolvedValueOnce({
      onboarded: true,
      gameDir: "/home/u/.minecraft",
      mirror: "bmclapi",
      concurrency: 8,
    });
    await useAppStore.getState().load();
    expect(useAppStore.getState().onboarded).toBe(true);
    expect(useAppStore.getState().gameDir).toBe("/home/u/.minecraft");
    expect(useAppStore.getState().mirror).toBe("bmclapi");
  });

  it("save 合并补丁并整体提交", async () => {
    invokeMock.mockResolvedValue(undefined);
    useAppStore.setState({ onboarded: false, gameDir: null, mirror: "official", concurrency: 12 });
    await useAppStore.getState().save({ onboarded: true, gameDir: "/x" });
    expect(invokeMock).toHaveBeenCalledWith("save_settings", {
      settings: {
        onboarded: true,
        gameDir: "/x",
        mirror: "official",
        concurrency: 12,
      },
    });
    expect(useAppStore.getState().onboarded).toBe(true);
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
