import { useAppStore } from "../stores/appStore";
import { useDownloadStore } from "../stores/downloadStore";
import type { Page } from "./Layout";
import { formatBytes, formatEta, formatSpeed, percent, STAGE_LABELS, STATUS_LABELS } from "../lib/format";
import { Progress, Badge } from "./ui/progress";
import { Button } from "./ui/button";
import { DISCLAIMER_TEXT } from "../content/disclaimer";

/** 主页：下载进度总览 */
export function HomePanel({ onNavigate }: { onNavigate: (p: Page) => void }) {
  const gameDir = useAppStore((s) => s.activeGameDir);
  const mirror = useAppStore((s) => s.mirror);
  const javaPath = useAppStore((s) => s.javaPath);
  const tasks = useDownloadStore((s) => s.tasks);

  const list = Object.values(tasks).sort((a, b) => b.createdAt - a.createdAt);
  const active = list.filter((t) => t.status === "running" || t.status === "queued");
  const paused = list.filter((t) => t.status === "paused");
  const recent = list
    .filter((t) => t.status === "completed" || t.status === "failed" || t.status === "canceled")
    .slice(0, 5);

  const totalDown = active.reduce((n, t) => n + t.downloadedBytes, 0);
  const totalAll = active.reduce((n, t) => n + t.totalBytes, 0);
  const speed = active.reduce((n, t) => n + t.speedBps, 0);

  return (
    <div className="h-full overflow-auto p-6" data-testid="home-panel">
      <div className="mx-auto grid max-w-3xl gap-5">
        {/* 欢迎头 */}
        <section className="mc-panel p-5">
          <div className="flex flex-wrap items-center gap-3">
            <h1 className="mc-pixel text-[15px] font-bold">主页</h1>
          </div>
          <p className="mt-2 text-sm text-muted-foreground">
            Minecraft Java 版文件下载器 —— 解析清单、下载官方文件、按标准目录放置并校验。
          </p>
          <div className="mt-3 flex flex-wrap gap-x-6 gap-y-1 text-xs text-[var(--mc-text-dim)]">
            <span>目录：{gameDir ?? "未配置"}</span>
            <span>镜像：{mirror === "bmclapi" ? "BMCLAPI" : "Mojang 官方"}</span>
            <span>Java（Forge 用）：{javaPath ?? "自动探测"}</span>
          </div>
        </section>

        {/* 下载进度总览（主页核心） */}
        <section className="mc-panel p-5" data-testid="home-progress">
          <div className="flex items-center gap-3">
            <h2 className="mc-pixel text-[12px] font-bold">下载进度</h2>
            <Button size="sm" variant="outline" className="ml-auto" onClick={() => onNavigate("versions")}>
              + 新任务
            </Button>
            <Button size="sm" variant="ghost" onClick={() => onNavigate("queue")}>
              队列 →
            </Button>
          </div>

          {active.length > 0 ? (
            <>
              <div className="mt-4">
                <Progress value={percent(totalDown, totalAll)} />
                <div className="mt-2 flex flex-wrap items-center justify-between gap-2 text-xs text-[var(--mc-text-dim)]">
                  <span>
                    {formatBytes(totalDown)} / {formatBytes(totalAll)}（{active.length} 个任务）
                  </span>
                  <span>
                    {formatSpeed(speed)} · 剩余{" "}
                    {formatEta(Math.max(0, totalAll - totalDown), speed)}
                  </span>
                </div>
              </div>
              <div className="mt-4 grid gap-2">
                {active.map((t) => (
                  <div
                    key={t.taskId}
                    className="mc-panel-dark flex items-center gap-3 px-3 py-2"
                    data-testid={`home-task-${t.versionId}`}
                  >
                    <span className="w-44 truncate text-sm font-medium">{t.versionId}</span>
                    <Badge tone="info">{STATUS_LABELS[t.status]}</Badge>
                    {t.stage && (
                      <span className="text-[11px] text-[var(--mc-text-dim)]">
                        {STAGE_LABELS[t.stage] ?? t.stage}
                      </span>
                    )}
                    <Progress
                      className="h-3.5 flex-1"
                      value={percent(t.downloadedBytes, t.totalBytes)}
                      indeterminate={t.status === "running" && t.totalBytes === 0}
                    />
                    <span className="w-24 text-right text-[11px] text-[var(--mc-text-dim)]">
                      {formatSpeed(t.speedBps)}
                    </span>
                  </div>
                ))}
              </div>
            </>
          ) : paused.length > 0 ? (
            <p className="mt-3 text-sm text-muted-foreground">
              {paused.length} 个任务已暂停 —— 到
              <button className="mx-1 text-[var(--mc-green-light)] underline" onClick={() => onNavigate("queue")}>
                下载队列
              </button>
              继续下载。
            </p>
          ) : (
            <p className="mt-3 text-sm text-muted-foreground">
              当前没有下载任务。到
              <button className="mx-1 text-[var(--mc-green-light)] underline" onClick={() => onNavigate("versions")}>
                版本选择
              </button>
              添加原版 / Fabric / Forge 版本。
            </p>
          )}
        </section>

        {/* 最近记录 */}
        {recent.length > 0 && (
          <section className="mc-panel p-5">
            <h2 className="mc-pixel text-[12px] font-bold">最近记录</h2>
            <div className="mt-3 grid gap-1.5">
              {recent.map((t) => (
                <div key={t.taskId} className="flex items-center gap-3 text-xs">
                  <span className="w-48 truncate">{t.versionId}</span>
                  <Badge
                    tone={
                      t.status === "completed"
                        ? "success"
                        : t.status === "failed"
                          ? "danger"
                          : "gray"
                    }
                  >
                    {STATUS_LABELS[t.status]}
                  </Badge>
                  <span className="text-[var(--mc-text-dim)]">
                    {formatBytes(t.downloadedBytes)}
                  </span>
                  {t.error && (
                    <span className="truncate text-[#ff8080]" title={t.error}>
                      {t.error}
                    </span>
                  )}
                </div>
              ))}
            </div>
          </section>
        )}

        {/* 免责声明 */}
        <section className="mc-panel p-5">
          <h2 className="mc-pixel text-[12px] font-bold text-[#ffd94a]">⚠ 免责声明</h2>
          <p className="mt-2 whitespace-pre-wrap text-xs leading-5 text-[var(--mc-text-dim)]">
            {DISCLAIMER_TEXT}
          </p>
        </section>
      </div>
    </div>
  );
}
