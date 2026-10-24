import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "../lib/tauri";
import { useAppStore } from "../stores/appStore";
import { toast } from "./ui/toast";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { DISCLAIMER_TEXT } from "../content/disclaimer";

// 首次启动强制向导：未完成 .minecraft 配置前禁止使用下载功能
type Step = "welcome" | "path";

export function Onboarding() {
  const save = useAppStore((s) => s.save);
  const setLocal = useAppStore((s) => s.setLocal);
  const mirror = useAppStore((s) => s.mirror);
  const [step, setStep] = useState<Step>("welcome");
  const [agreed, setAgreed] = useState(false);
  const [detected, setDetected] = useState<string | null>(null);
  const [detectError, setDetectError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function autoDetect() {
    setBusy(true);
    setDetectError(null);
    try {
      const def = await invoke<string | null>("probe_default_dir");
      if (!def) {
        setDetectError("未找到默认位置的 .minecraft，请使用“选择文件夹”。");
        return;
      }
      await useDetected(def);
    } catch (e) {
      setDetectError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function chooseFolder() {
    setBusy(true);
    setDetectError(null);
    try {
      const picked = await open({
        directory: true,
        multiple: false,
        title: "选择 .minecraft 文件夹，或启动器（PCL/HMCL）所在的父文件夹",
      });
      if (!picked) return;
      await useDetected(picked as string);
    } catch (e) {
      setDetectError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function useDetected(path: string) {
    try {
      const resolved = await invoke<string>("detect_game_dir", { selectedPath: path });
      setDetected(resolved);
    } catch (e) {
      setDetected(null);
      setDetectError(String(e));
    }
  }

  // 用户明确要求把选中的空文件夹当作新的 .minecraft
  async function useAsNew() {
    const picked = await open({
      directory: true,
      multiple: false,
      title: "选择一个空文件夹作为新的 .minecraft",
    });
    if (!picked) return;
    setBusy(true);
    try {
      const prepared = await invoke<string>("prepare_game_dir", { path: picked });
      setDetected(prepared);
      setDetectError(null);
    } catch (e) {
      setDetectError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function finish() {
    if (!detected) return;
    try {
      setLocal({ gameDir: detected });
      await save({ gameDir: detected, onboarded: true, mirror });
      toast("配置完成，开始使用下载器", "success");
    } catch (e) {
      toast(`保存配置失败: ${e}`, "error");
    }
  }

  return (
    <div className="flex h-full items-center justify-center overflow-auto bg-background p-6">
      {step === "welcome" && (
        <div className="w-full max-w-2xl rounded-lg border bg-card p-8 shadow-xl" data-testid="onboarding-welcome">
          <h1 className="text-2xl font-bold">欢迎使用 MC-Downloader-GUI</h1>
          <p className="mt-2 text-sm text-muted-foreground">
            Minecraft Java 版「仅下载器」：解析版本清单、下载官方文件、按标准目录放置并校验。
          </p>
          <div
            className="mt-6 rounded-md border border-yellow-500/40 bg-yellow-500/10 p-4 text-sm leading-relaxed text-yellow-200"
            data-testid="disclaimer"
          >
            {DISCLAIMER_TEXT}
          </div>
          <label className="mt-6 flex cursor-pointer items-start gap-3 text-sm" data-testid="disclaimer-checkbox">
            <input
              type="checkbox"
              checked={agreed}
              onChange={(e) => setAgreed(e.target.checked)}
              className="mt-0.5 h-4 w-4 accent-[var(--color-primary)]"
            />
            我已阅读并理解以上说明，仅将本工具用于纯净下载与目录构建。
          </label>
          <div className="mt-6 flex justify-end">
            <Button disabled={!agreed} onClick={() => setStep("path")} data-testid="onboarding-next">
              下一步：配置游戏目录
            </Button>
          </div>
        </div>
      )}

      {step === "path" && (
        <div className="w-full max-w-2xl rounded-lg border bg-card p-8 shadow-xl" data-testid="onboarding-path">
          <h1 className="text-xl font-bold">配置 .minecraft 目录</h1>
          <p className="mt-2 text-sm text-muted-foreground">
            可以选择 <b>.minecraft</b> 文件夹本身，或 PCL / HMCL / BakaXL 等
            <b>启动器所在的父文件夹</b>（会自动向下探测）。也可以选择空文件夹，创建全新的
            .minecraft。
          </p>
          <div className="mt-6 flex flex-wrap gap-3">
            <Button variant="outline" onClick={autoDetect} disabled={busy} data-testid="btn-auto-detect">
              自动探测默认路径
            </Button>
            <Button variant="outline" onClick={chooseFolder} disabled={busy} data-testid="btn-choose-folder">
              选择文件夹…
            </Button>
            <Button variant="ghost" onClick={useAsNew} disabled={busy}>
              新建 .minecraft…
            </Button>
          </div>
          {detectError && (
            <div className="mt-4 rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive">
              {detectError}
            </div>
          )}
          {detected && (
            <div className="mt-4 rounded-md border border-primary/40 bg-primary/10 p-4" data-testid="detected-path">
              <div className="text-xs text-muted-foreground">已识别的 .minecraft 目录</div>
              <div className="mt-1 flex items-center gap-2">
                <Input readOnly value={detected} className="flex-1" />
              </div>
            </div>
          )}
          <div className="mt-8 flex justify-between">
            <Button variant="ghost" onClick={() => setStep("welcome")}>
              上一步
            </Button>
            <Button disabled={!detected} onClick={finish} data-testid="onboarding-finish">
              完成
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}
