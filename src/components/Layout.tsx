import { useAppStore } from "../stores/appStore";
import { cn } from "../lib/cn";

export type Page = "versions" | "queue" | "verify" | "settings";

const NAV: Array<{ id: Page; label: string }> = [
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
  const gameDir = useAppStore((s) => s.gameDir);
  const mirror = useAppStore((s) => s.mirror);

  return (
    <div className="flex h-full">
      <aside className="flex w-48 flex-col border-r bg-card">
        <div className="p-4">
          <div className="text-sm font-bold">MC-Downloader-GUI</div>
          <div className="mt-0.5 text-[11px] text-muted-foreground">
            Minecraft Java 版仅下载器
          </div>
        </div>
        <nav className="mt-2 flex-1 px-2" data-testid="nav">
          {NAV.map((n) => (
            <button
              key={n.id}
              onClick={() => onNavigate(n.id)}
              className={cn(
                "mb-1 w-full rounded-md px-3 py-2 text-left text-sm",
                page === n.id
                  ? "bg-secondary font-medium"
                  : "text-muted-foreground hover:bg-accent",
              )}
              data-testid={`nav-${n.id}`}
            >
              {n.label}
            </button>
          ))}
        </nav>
        <div className="border-t p-3 text-[11px] leading-relaxed text-muted-foreground">
          <div className="truncate" title={gameDir ?? ""}>
            目录：{gameDir ?? "未配置"}
          </div>
          <div>镜像：{mirror === "bmclapi" ? "BMCLAPI" : "Mojang 官方"}</div>
        </div>
      </aside>
      <main className="min-w-0 flex-1">{children}</main>
    </div>
  );
}
