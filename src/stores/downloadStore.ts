// 下载队列状态：接收 download://progress 事件，维护每个任务的实时进度
import { create } from "zustand";
import { invoke } from "../lib/tauri";
import type { Mirror, TaskSnapshot, VersionSource } from "../types";

interface DownloadState {
  tasks: Record<string, TaskSnapshot>;
  order: string[];
  loading: boolean;
  lastError: string | null;

  /** 从后端拉取全部任务（启动恢复） */
  refresh: () => Promise<void>;
  applySnapshot: (snap: TaskSnapshot) => void;
  startTask: (
    source: VersionSource,
    displayId: string,
    gameDir: string,
    mirror: Mirror,
    concurrency: number,
    javaPath?: string | null,
  ) => Promise<string>;
  cancelTask: (taskId: string) => Promise<void>;
  pauseTask: (taskId: string) => Promise<void>;
  resumeTask: (taskId: string) => Promise<void>;
  clearFinished: () => Promise<void>;
  setError: (msg: string | null) => void;
}

export const useDownloadStore = create<DownloadState>()((set, get) => ({
  tasks: {},
  order: [],
  loading: false,
  lastError: null,

  refresh: async () => {
    set({ loading: true });
    try {
      const snaps = await invoke<TaskSnapshot[]>("list_download_tasks");
      const tasks: Record<string, TaskSnapshot> = {};
      for (const s of snaps) tasks[s.taskId] = s;
      set({
        tasks,
        order: snaps.map((s) => s.taskId),
        loading: false,
      });
    } catch (e) {
      set({ loading: false, lastError: String(e) });
    }
  },

  applySnapshot: (snap) => {
    const { tasks, order } = get();
    const isNew = !(snap.taskId in tasks);
    set({
      tasks: { ...tasks, [snap.taskId]: snap },
      order: isNew ? [...order, snap.taskId] : order,
    });
  },

  startTask: async (source, displayId, gameDir, mirror, concurrency, javaPath) => {
    const taskId = await invoke<string>("start_download", {
      source,
      gameDir,
      mirror,
      concurrency,
      javaPath: javaPath ?? null,
    });
    // 立即插入一个 queued 快照，等待引擎事件接管
    get().applySnapshot({
      taskId,
      versionId: displayId,
      gameDir,
      mirror,
      status: "queued",
      stage: undefined,
      downloadedBytes: 0,
      totalBytes: 0,
      speedBps: 0,
      filesDone: 0,
      filesTotal: 0,
      createdAt: Date.now(),
    });
    return taskId;
  },

  cancelTask: async (taskId) => {
    await invoke("cancel_download", { taskId });
  },

  pauseTask: async (taskId) => {
    await invoke("pause_download", { taskId });
  },

  resumeTask: async (taskId) => {
    await invoke("resume_download", { taskId });
  },

  clearFinished: async () => {
    await invoke("clear_finished_tasks");
    const { tasks, order } = get();
    const kept = order.filter((id) => {
      const st = tasks[id]?.status;
      return st === "queued" || st === "running" || st === "paused";
    });
    const nextTasks: Record<string, TaskSnapshot> = {};
    for (const id of kept) nextTasks[id] = tasks[id];
    set({ tasks: nextTasks, order: kept });
  },

  setError: (msg) => set({ lastError: msg }),
}));
