// 全局应用状态：设置（.minecraft 路径 / 镜像 / 并发数 / 引导完成标记）
import { create } from "zustand";
import { invoke } from "../lib/tauri";
import type { AppSettings, Mirror } from "../types";

interface AppState {
  ready: boolean;
  onboarded: boolean;
  gameDir: string | null;
  mirror: Mirror;
  concurrency: number;

  load: () => Promise<void>;
  /** 保存设置（合并后整体提交到后端持久化） */
  save: (patch: Partial<Omit<AppSettings, "onboarded">> & { onboarded?: boolean }) => Promise<void>;
  /** 仅更新本地（不落盘），供向导中间态使用 */
  setLocal: (patch: Partial<AppSettings>) => void;
}

export const useAppStore = create<AppState>()((set, get) => ({
  ready: false,
  onboarded: false,
  gameDir: null,
  mirror: "official",
  concurrency: 12,

  load: async () => {
    const s = await invoke<AppSettings>("get_settings");
    set({
      ready: true,
      onboarded: s.onboarded,
      gameDir: s.gameDir ?? null,
      mirror: s.mirror,
      concurrency: s.concurrency,
    });
  },

  save: async (patch) => {
    const cur = get();
    const merged: AppSettings = {
      onboarded: patch.onboarded ?? cur.onboarded,
      gameDir: patch.gameDir !== undefined ? patch.gameDir : cur.gameDir,
      mirror: patch.mirror ?? cur.mirror,
      concurrency: patch.concurrency ?? cur.concurrency,
    };
    await invoke("save_settings", { settings: merged });
    set({
      onboarded: merged.onboarded,
      gameDir: merged.gameDir ?? null,
      mirror: merged.mirror,
      concurrency: merged.concurrency,
    });
  },

  setLocal: (patch) => set(patch as Partial<AppState>),
}));
