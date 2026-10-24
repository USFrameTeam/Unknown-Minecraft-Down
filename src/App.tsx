import { useEffect, useState } from "react";
import { listen } from "./lib/tauri";
import { EVENT_PROGRESS } from "./lib/tauri";
import { useAppStore } from "./stores/appStore";
import { useDownloadStore } from "./stores/downloadStore";
import type { TaskSnapshot } from "./types";
import { Layout, type Page } from "./components/Layout";
import { Onboarding } from "./components/Onboarding";
import { VersionPicker } from "./components/VersionPicker";
import { DownloadQueue } from "./components/DownloadQueue";
import { VerifyPanel } from "./components/VerifyPanel";
import { SettingsPanel } from "./components/SettingsPanel";
import { ToastContainer } from "./components/ui/toast";

export default function App() {
  const ready = useAppStore((s) => s.ready);
  const onboarded = useAppStore((s) => s.onboarded);
  const load = useAppStore((s) => s.load);
  const refresh = useDownloadStore((s) => s.refresh);
  const applySnapshot = useDownloadStore((s) => s.applySnapshot);
  const [page, setPage] = useState<Page>("versions");

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

  if (!ready) {
    return <div className="flex h-full items-center justify-center text-sm text-muted-foreground">加载中…</div>;
  }

  // 硬约束：未完成首次启动向导（含免责声明），禁止进入主界面
  if (!onboarded) {
    return (
      <>
        <Onboarding />
        <ToastContainer />
      </>
    );
  }

  return (
    <>
      <Layout page={page} onNavigate={setPage}>
        {page === "versions" && <VersionPicker />}
        {page === "queue" && <DownloadQueue />}
        {page === "verify" && <VerifyPanel />}
        {page === "settings" && <SettingsPanel />}
      </Layout>
      <ToastContainer />
    </>
  );
}
