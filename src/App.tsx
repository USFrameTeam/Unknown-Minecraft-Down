import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke, listen } from "./lib/tauri";
import { EVENT_PROGRESS } from "./lib/tauri";
import { useAppStore } from "./stores/appStore";
import { useDownloadStore } from "./stores/downloadStore";
import type { TaskSnapshot } from "./types";
import { Layout, type Page } from "./components/Layout";
import { Onboarding } from "./components/Onboarding";
import { HomePanel } from "./components/HomePanel";
import { VersionPicker } from "./components/VersionPicker";
import { DownloadQueue } from "./components/DownloadQueue";
import { VerifyPanel } from "./components/VerifyPanel";
import { SettingsPanel } from "./components/SettingsPanel";
import { TitleBar } from "./components/TitleBar";
import { ToastContainer } from "./components/ui/toast";

export default function App() {
  const ready = useAppStore((s) => s.ready);
  const onboarded = useAppStore((s) => s.onboarded);
  const load = useAppStore((s) => s.load);
  const refresh = useDownloadStore((s) => s.refresh);
  const applySnapshot = useDownloadStore((s) => s.applySnapshot);
  const [page, setPage] = useState<Page>("home");

  useEffect(() => {
    load().catch(() => {
      // 设置读取失败时按未引导处理
      useAppStore.setState({ ready: true, onboarded: false });
    });
    refresh();
  }, [load, refresh]);

  // 订阅下载进度事件；卸载时取消监听，防止内存泄漏
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    listen<TaskSnapshot>(EVENT_PROGRESS, (e) => applySnapshot(e.payload)).then(
      (u) => {
        if (disposed) u();
        else unlisten = u;
      },
    );
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [applySnapshot]);

  // 启动优化：窗口默认隐藏（tauri.conf visible:false），
  // 前端首帧渲染完成后显示，避免启动白屏闪烁；
  // 上次会话仍在运行的任务延后到 UI 就绪后续跑，不阻塞启动。
  useEffect(() => {
    if (!ready) return;
    getCurrentWindow().show().catch(() => {});
    const t = window.setTimeout(() => {
      if (useAppStore.getState().onboarded) {
        invoke("auto_resume_pending").catch(() => {});
      }
    }, 400);
    return () => window.clearTimeout(t);
  }, [ready]);

  if (!ready) {
    return (
      <div className="flex h-full flex-col">
        <TitleBar />
        <div className="flex flex-1 items-center justify-center text-sm text-[var(--mc-text-dim)]">
          加载中…
        </div>
      </div>
    );
  }

  // 硬约束：未完成首次启动向导（含免责声明），禁止进入主界面
  if (!onboarded) {
    return (
      <div className="flex h-full flex-col">
        <TitleBar />
        <Onboarding />
        <ToastContainer />
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col">
      <TitleBar />
      <div className="min-h-0 flex-1">
        <Layout page={page} onNavigate={setPage}>
          {page === "home" && <HomePanel onNavigate={setPage} />}
          {page === "versions" && <VersionPicker />}
          {page === "queue" && <DownloadQueue />}
          {page === "verify" && <VerifyPanel />}
          {page === "settings" && <SettingsPanel />}
        </Layout>
      </div>
      <ToastContainer />
    </div>
  );
}
