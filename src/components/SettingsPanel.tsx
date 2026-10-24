import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "../lib/tauri";
import { useAppStore } from "../stores/appStore";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { ConfirmDialog } from "./ui/dialog";
import { toast } from "./ui/toast";
import { DISCLAIMER_TEXT } from "../content/disclaimer";
import { MirrorSwitch } from "./VersionPicker";

export function SettingsPanel() {
  const gameDir = useAppStore((s) => s.gameDir);
  const concurrency = useAppStore((s) => s.concurrency);
  const save = useAppStore((s) => s.save);

  const [pendingDir, setPendingDir] = useState<string | null>(null);
  const [concurrencyDraft, setConcurrencyDraft] = useState(String(concurrency));

  async function pickNewDir() {
    const picked = await open({
      directory: true,
      multiple: false,
      title: "选择新的 .minecraft（或启动器父目录）",
    });
    if (!picked) return;
    try {
      const resolved = await invoke<string>("detect_game_dir", { selectedPath: picked });
      setPendingDir(resolved);
    } catch (e) {
      toast(String(e), "error");
    }
  }

  async function applyConcurrency() {
    const n = Math.max(1, Math.min(32, parseInt(concurrencyDraft, 10) || 12));
    setConcurrencyDraft(String(n));
    try {
      await save({ concurrency: n });
      toast(`并发数已保存：${n}`, "success");
    } catch (e) {
      toast(String(e), "error");
    }
  }

  return (
    <div className="h-full overflow-auto p-6" data-testid="settings-panel">
      <div className="mx-auto grid max-w-2xl gap-6">
        <section className="rounded-lg border bg-card p-5">
          <h2 className="text-base font-semibold">游戏目录</h2>
          <p className="mt-1 text-sm text-muted-foreground">
            更改路径不会移动已下载的文件；新目录中的同名文件会重新下载。下载器绝不修改
            PCL/HMCL/BakaXL 的私有配置。
          </p>
          <div className="mt-3 flex items-center gap-3">
            <Input readOnly value={gameDir ?? "未配置"} className="flex-1 font-mono text-xs" data-testid="settings-game-dir" />
            <Button variant="outline" onClick={pickNewDir} data-testid="settings-change-dir">
              重新选择…
            </Button>
          </div>
        </section>

        <section className="rounded-lg border bg-card p-5">
          <h2 className="text-base font-semibold">下载并发数</h2>
          <p className="mt-1 text-sm text-muted-foreground">
            同时下载的文件数（1–32）。默认 12；网络较差时可调低。
          </p>
          <div className="mt-3 flex items-center gap-3">
            <Input
              type="number"
              min={1}
              max={32}
              value={concurrencyDraft}
              onChange={(e) => setConcurrencyDraft(e.target.value)}
              className="w-24"
              data-testid="settings-concurrency"
            />
            <Button variant="outline" onClick={applyConcurrency}>
              保存
            </Button>
          </div>
        </section>

        <section className="rounded-lg border bg-card p-5">
          <h2 className="text-base font-semibold">下载镜像</h2>
          <p className="mt-1 text-sm text-muted-foreground">
            官方源（Mojang）或 BMCLAPI 镜像（中国大陆更稳）。仅此二者，不含任何第三方资源站。
          </p>
          <div className="mt-3">
            <MirrorSwitch />
          </div>
        </section>

        <section className="rounded-lg border bg-card p-5">
          <h2 className="text-base font-semibold">关于与免责声明</h2>
          <div className="mt-3 rounded-md border border-yellow-500/40 bg-yellow-500/10 p-4 text-sm leading-relaxed text-yellow-200">
            {DISCLAIMER_TEXT}
          </div>
          <p className="mt-3 text-xs text-muted-foreground">
            MC-Downloader-GUI v0.1.0 · Tauri v2 + React 19 · 下载源仅限 Mojang
            官方与 BMCLAPI 镜像 · 不收集任何账号信息
          </p>
        </section>
      </div>

      <ConfirmDialog
        open={!!pendingDir}
        title="更改 .minecraft 目录？"
        description={`将切换到：\n${pendingDir}\n\n队列中的任务仍会写入各自记录的目录；新任务将使用新目录。确认更改？`}
        confirmText="确认更改"
        onCancel={() => setPendingDir(null)}
        onConfirm={() => {
          const dir = pendingDir;
          setPendingDir(null);
          if (dir) save({ gameDir: dir }).then(() => toast("游戏目录已更新", "success")).catch((e) => toast(String(e), "error"));
        }}
      />
    </div>
  );
}
