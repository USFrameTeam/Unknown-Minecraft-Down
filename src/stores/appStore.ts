// 全局应用状态：多目录、镜像、并发数、Java 路径、引导完成标记
import { create } from "zustand";
import { invoke } from "../lib/tauri";
import type { AppSettings, Mirror } from "../types";

interface AppState {
  ready: boolean;
  onboarded: boolean;
  gameDirs: string[];
  activeGameDir: string | null;
  mirror: Mirror;
  concurrency: number;
  javaPath: string | null;

  load: () => Promise<void>;
  /** 合并补丁后整体提交到后端持久化 */
  save: (patch: Partial<AppSettings>) => Promise<void>;
  /** 仅更新本地（不落盘），供向导中间态使用 */
  setLocal: (patch: Partial<AppSettings>) => void;
  /** 登记一个 .minecraft 目录（重复忽略） */
  addGameDir: (dir: string) => Promise<void>;
  /** 移除目录；若为激活目录则顺延到第一个 */
  removeGameDir: (dir: string) => Promise<void>;
  /** 切换激活目录（新任务写入目标） */
  setActiveGameDir: (dir: string) => Promise<void>;
}

export const useAppStore = create<AppState>()((set, get) => ({
  ready: false,
  onboarded: false,
  gameDirs: [],
  activeGameDir: null,
  mirror: "official",
  concurrency: 12,
  javaPath: null,

  load: async () => {
    const s = await invoke<AppSettings>("get_settings");
    set({
      ready: true,
      onboarded: s.onboarded,
      gameDirs: s.gameDirs ?? [],
      activeGameDir: s.activeGameDir ?? null,
      mirror: s.mirror,
      concurrency: s.concurrency,
      javaPath: s.javaPath ?? null,
    });
  },

  save: async (patch) => {
    const cur = get();
    const merged: AppSettings = {
      onboarded: patch.onboarded ?? cur.onboarded,
      gameDirs: patch.gameDirs ?? cur.gameDirs,
      activeGameDir:
        patch.activeGameDir !== undefined ? patch.activeGameDir : cur.activeGameDir,
      mirror: patch.mirror ?? cur.mirror,
      concurrency: patch.concurrency ?? cur.concurrency,
      javaPath: patch.javaPath !== undefined ? patch.javaPath : cur.javaPath,
    };
    await invoke("save_settings", { settings: merged });
    set({
      onboarded: merged.onboarded,
      gameDirs: merged.gameDirs,
      activeGameDir: merged.activeGameDir ?? null,
      mirror: merged.mirror,
      concurrency: merged.concurrency,
      javaPath: merged.javaPath ?? null,
    });
  },

  setLocal: (patch) => set(patch as Partial<AppState>),

  addGameDir: async (dir) => {
    const { gameDirs, activeGameDir, save } = get();
    if (gameDirs.includes(dir)) return;
    const next = [...gameDirs, dir];
    await save({
      gameDirs: next,
      activeGameDir: activeGameDir ?? dir,
    });
  },

  removeGameDir: async (dir) => {
    const { gameDirs, activeGameDir, save } = get();
    const next = gameDirs.filter((d) => d !== dir);
    if (!next.length) return; // 至少保留一个目录
    await save({
      gameDirs: next,
      activeGameDir: activeGameDir === dir ? next[0] : activeGameDir,
    });
  },

  setActiveGameDir: async (dir) => {
    if (!get().gameDirs.includes(dir)) return;
    await get().save({ activeGameDir: dir });
  },
}));
