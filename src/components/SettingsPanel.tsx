import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "../lib/tauri";
import { useAppStore } from "../stores/appStore";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { ConfirmDialog } from "./ui/dialog";
import { toast } from "./ui/toast";
import { Badge } from "./ui/progress";
import { DISCLAIMER_TEXT } from "../content/disclaimer";

export function SettingsPanel() {
  const gameDirs = useAppStore((s) => s.gameDirs);
  const activeGameDir = useAppStore((s) => s.activeGameDir);
  const concurrency = useAppStore((s) => s.concurrency);
  const javaPath = useAppStore((s) => s.javaPath);
  const addGameDir = useAppStore((s) => s.addGameDir);
  const removeGameDir = useAppStore((s) => s.removeGameDir);
  const setActiveGameDir = useAppStore((s) => s.setActiveGameDir);
  const save = useAppStore((s) => s.save);

  const [pendingDir, setPendingDir] = useState<string | null>(null);
  const [removing, setRemoving] = useState<string | null>(null);
  const [switching, setSwitching] = useState<string | null>(null);
  const [concurrencyDraft, setConcurrencyDraft] = useState(String(concurrency));
  const [javaDraft, setJavaDraft] = useState(javaPath ?? "");

  async function pickNewDir() {
    const picked = await open({
      directory: true,
      multiple: false,
      title: "选择 .minecraft（或启动器父目录）",
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

  async function autoDetectJava() {
    try {
      const found = await invoke<string | null>("detect_java");
      if (found) {
        setJavaDraft(found);
        await save({ javaPath: found });
        toast(`已检测到 Java：${found}`, "success");
      } else {
        toast("未找到 Java；Forge 1.13+ 安装需要，请手动填写路径", "error");
      }
    } catch (e) {
      toast(String(e), "error");
    }
  }

  async function applyJava() {
    try {
      await save({ javaPath: javaDraft.trim() || null });
      toast(javaDraft.trim() ? "Java 路径已保存" : "已恢复自动探测", "success");
    } catch (e) {
      toast(String(e), "error");
    }
  }

  return (
    <div className="h-full overflow-auto p-6" data-testid="settings-panel">
      <div className="mx-auto grid max-w-2xl gap-5">
        {/* 目录管理 */}
        <section className="mc-panel p-5">
          <div className="flex items-center gap-3">
            <h2 className="mc-pixel text-[13px] font-bold">游戏目录（多目录）</h2>
            <Button size="sm" className="ml-auto" onClick={pickNewDir} data-testid="add-dir">
              + 添加目录
            </Button>
          </div>
          <p className="mt-2 text-sm text-[var(--mc-text-dim)]">
            可登记多个 .minecraft，在版本选择页随时切换；每个任务记录自己的目录，
            互不影响。下载器绝不修改 PCL/HMCL/BakaXL 的私有配置。
          </p>
          <div className="mt-4 grid gap-2" data-testid="dir-list">
            {gameDirs.map((d) => (
              <div
                key={d}
                className="mc-panel-dark flex items-center gap-3 px-3 py-2"
                data-testid={`dir-item-${d}`}
              >
                <span className="min-w-0 flex-1 truncate font-mono text-xs" title={d}>
                  {d}
                </span>
                {d === activeGameDir ? (
                  <Badge tone="success">当前</Badge>
                ) : (
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={() => setSwitching(d)}
                    data-testid={`dir-activate-${d}`}
                  >
                    设为当前
                  </Button>
                )}
                <Button
                  size="sm"
                  variant="ghost"
                  disabled={gameDirs.length <= 1}
                  onClick={() => setRemoving(d)}
                >
                  移除
                </Button>
              </div>
            ))}
            {!gameDirs.length && (
              <div className="p-3 text-sm text-[var(--mc-text-dim)]">尚未登记任何目录</div>
            )}
          </div>
        </section>

        {/* 并发 */}
        <section className="mc-panel p-5">
          <h2 className="mc-pixel text-[13px] font-bold">下载并发数</h2>
          <p className="mt-2 text-sm text-[var(--mc-text-dim)]">
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

        {/* Java（Forge） */}
        <section className="mc-panel p-5">
          <h2 className="mc-pixel text-[13px] font-bold">Java（Forge 安装用）</h2>
          <p className="mt-2 text-sm text-[var(--mc-text-dim)]">
            仅用于执行 Forge 官方安装器的 processors（生成 Forge 文件），绝不用于启动游戏。
            留空 = 自动探测（JAVA_HOME / PATH / 常见安装位置）。
          </p>
          <div className="mt-3 flex flex-wrap items-center gap-2">
            <Input
              placeholder="/path/to/java（留空自动探测）"
              value={javaDraft}
              onChange={(e) => setJavaDraft(e.target.value)}
              className="min-w-64 flex-1 font-mono text-xs"
              data-testid="settings-java"
            />
            <Button variant="outline" onClick={autoDetectJava} data-testid="detect-java">
              自动探测
            </Button>
            <Button variant="outline" onClick={applyJava}>
              保存
            </Button>
          </div>
        </section>

        {/* 关于 */}
        <section className="mc-panel p-5">
          <h2 className="mc-pixel text-[13px] font-bold">关于与免责声明</h2>
          <div className="mt-3 whitespace-pre-wrap border-2 border-[#8a6d2b] bg-[#3a3222] p-4 text-xs leading-5 text-[#ffd94a]">
            {DISCLAIMER_TEXT}
          </div>
          <p className="mt-3 text-[11px] text-[var(--mc-text-dim)]">
            Unknown Minecraft Down v0.2 · Tauri v2 + React 19 · 下载源仅限 Mojang 官方 /
            BMCLAPI / Fabric Meta / Forge 官方 maven · 不收集任何账号信息
          </p>
        </section>
      </div>

      <ConfirmDialog
        open={!!pendingDir}
        title="添加该 .minecraft 目录？"
        description={pendingDir ?? ""}
        confirmText="添加"
        onCancel={() => setPendingDir(null)}
        onConfirm={() => {
          const dir = pendingDir;
          setPendingDir(null);
          if (dir)
            addGameDir(dir)
              .then(() => toast("目录已添加", "success"))
              .catch((e) => toast(String(e), "error"));
        }}
      />
      <ConfirmDialog
        open={!!removing}
        danger
        title="移除该目录？"
        description={`仅从列表移除（不会删除任何文件）：\n${removing ?? ""}`}
        confirmText="移除"
        onCancel={() => setRemoving(null)}
        onConfirm={() => {
          const dir = removing;
          setRemoving(null);
          if (dir) removeGameDir(dir).catch((e) => toast(String(e), "error"));
        }}
      />
      <ConfirmDialog
        open={!!switching}
        title="切换当前目录？"
        description={`新任务将下载到：\n${switching ?? ""}`}
        confirmText="切换"
        onCancel={() => setSwitching(null)}
        onConfirm={() => {
          const dir = switching;
          setSwitching(null);
          if (dir)
            setActiveGameDir(dir)
              .then(() => toast("已切换当前目录", "success"))
              .catch((e) => toast(String(e), "error"));
        }}
      />
    </div>
  );
}
