import { useAppStore } from "../stores/appStore";
import { useDownloadStore } from "../stores/downloadStore";
import { formatBytes, formatSpeed, percent } from "../lib/format";
import { cn } from "../lib/cn";

export type Page = "home" | "versions" | "queue" | "verify" | "settings";

const NAV: Array<{ id: Page; label: string }> = [
  { id: "home", label: "主页" },
  { id: "versions", label: "版本选择" },
  { id: "queue", label: "下载队列" },
  { id: "verify", label: "安装校验" },
  { id: "settings", label: "设置" },
];

export function Layout({
  page,
  onNavigate,
  children,
}: {
  page: Page;
  onNavigate: (p: Page) => void;
  children: React.ReactNode;
}) {
  const gameDir = useAppStore((s) => s.activeGameDir);
  const mirror = useAppStore((s) => s.mirror);
  const tasks = useDownloadStore((s) => s.tasks);

  const active = Object.values(tasks).filter(
    (t) => t.status === "running" || t.status === "queued",
  );
  const totalDown = active.reduce((n, t) => n + t.downloadedBytes, 0);
  const totalAll = active.reduce((n, t) => n + t.totalBytes, 0);
  const speed = active.reduce((n, t) => n + t.speedBps, 0);
  const runningCount = active.length;

  return (
    <div className="flex h-full flex-col">
      <div className="flex min-h-0 flex-1">
        <aside className="mc-panel-dark flex w-52 flex-col" style={{ borderRightWidth: 2 }}>
          <div className="p-4 pb-3">
            <div className="mc-pixel text-[11px] font-bold text-[var(--mc-green-light)]">
              UNKNOWN
            </div>
            <div className="mc-pixel text-[11px] font-bold text-[var(--mc-green-light)]">
              MINECRAFT DOWN
            </div>
          </div>
          <nav className="mt-2 flex-1 px-2" data-testid="nav">
            {NAV.map((n) => (
              <button
                key={n.id}
                onClick={() => onNavigate(n.id)}
                className={cn(
                  "mc-tab mb-1.5 block w-full text-left",
                  page === n.id && "mc-tab-active",
                )}
                data-testid={`nav-${n.id}`}
              >
                <span className="mr-2 text-[var(--mc-green-light)]">▪</span>
                {n.label}
                {n.id === "queue" && runningCount > 0 && (
                  <span className="mc-badge mc-badge-green ml-2 px-1.5 py-0 text-[10px]">
                    {runningCount}
                  </span>
                )}
              </button>
            ))}
          </nav>
          <div className="border-t-2 border-black/60 p-3 text-[11px] leading-5 text-[var(--mc-text-dim)]">
            <div className="truncate" title={gameDir ?? ""}>
              目录：{gameDir ?? "未配置"}
            </div>
            <div>镜像：{mirror === "bmclapi" ? "BMCLAPI" : "Mojang 官方"}</div>
          </div>
        </aside>
        <main className="min-w-0 flex-1">{children}</main>
      </div>

      {/* 底部状态栏：常驻下载进度（主页之外的页面也能看到） */}
      <footer
        className="mc-panel-dark flex h-9 items-center gap-4 px-4 text-[11px]"
        style={{ borderTopWidth: 2, boxShadow: "none" }}
        data-testid="status-bar"
      >
        {runningCount > 0 ? (
          <>
            <span className="text-[var(--mc-green-light)]">
              ⬇ 下载中 ×{runningCount}
            </span>
            <div className="mc-progress h-3 max-w-64 flex-1">
              <div
                className="mc-progress-fill"
                style={{ width: `${percent(totalDown, totalAll)}%` }}
              />
            </div>
            <span>
              {formatBytes(totalDown)} / {formatBytes(totalAll)} · {formatSpeed(speed)}
            </span>
          </>
        ) : (
          <span className="text-[var(--mc-text-dim)]">就绪 · 没有正在进行的下载</span>
        )}
        <span className="ml-auto text-[var(--mc-text-dim)]">Unknown Minecraft Down v0.2</span>
      </footer>
    </div>
  );
}
