import { useEffect, useMemo, useState } from "react";
import { invoke } from "../lib/tauri";
import { useAppStore } from "../stores/appStore";
import { useDownloadStore } from "../stores/downloadStore";
import type { FabricLoaderSummary, Mirror, VersionSummary } from "../types";
import { Badge } from "./ui/progress";
import { Button } from "./ui/button";
import { Input, Select } from "./ui/input";
import { ConfirmDialog } from "./ui/dialog";
import { toast } from "./ui/toast";

const TYPE_LABELS: Record<string, string> = {
  release: "正式版",
  snapshot: "快照",
  old_beta: "旧 Beta",
  old_alpha: "旧 Alpha",
};

type SourceTab = "vanilla" | "fabric";

export function VersionPicker() {
  const mirror = useAppStore((s) => s.mirror);
  const gameDir = useAppStore((s) => s.gameDir);
  const concurrency = useAppStore((s) => s.concurrency);
  const startTask = useDownloadStore((s) => s.startTask);

  const [tab, setTab] = useState<SourceTab>("vanilla");
  const [versions, setVersions] = useState<VersionSummary[]>([]);
  const [loading, setLoading] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [typeFilter, setTypeFilter] = useState<string>("release");
  const [shown, setShown] = useState(50);
  const [confirming, setConfirming] = useState<VersionSummary | null>(null);

  // Fabric
  const [fabricMc, setFabricMc] = useState("");
  const [loaders, setLoaders] = useState<FabricLoaderSummary[]>([]);
  const [loaderPick, setLoaderPick] = useState("");
  const [confirmFabric, setConfirmFabric] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setLoadError(null);
    invoke<VersionSummary[]>("list_versions", { mirror })
      .then((vs) => {
        if (!cancelled) setVersions(vs);
      })
      .catch((e) => {
        if (!cancelled) setLoadError(String(e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [mirror]);

  useEffect(() => {
    if (tab !== "fabric" || !fabricMc) {
      setLoaders([]);
      return;
    }
    let cancelled = false;
    invoke<FabricLoaderSummary[]>("list_fabric_loaders", { mcVersion: fabricMc })
      .then((ls) => {
        if (cancelled) return;
        setLoaders(ls);
        const firstStable = ls.find((l) => l.loader.stable) ?? ls[0];
        setLoaderPick(firstStable?.loader.version ?? "");
      })
      .catch((e) => {
        if (!cancelled) {
          setLoaders([]);
          toast(`获取 Fabric Loader 列表失败: ${e}`, "error");
        }
      });
    return () => {
      cancelled = true;
    };
  }, [tab, fabricMc]);

  const releaseVersions = useMemo(
    () => versions.filter((v) => v.type === "release"),
    [versions],
  );

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return versions.filter(
      (v) =>
        (typeFilter === "all" || v.type === typeFilter) &&
        (!q || v.id.toLowerCase().includes(q)),
    );
  }, [versions, typeFilter, query]);

  function startVanilla(v: VersionSummary) {
    if (!gameDir) return;
    startTask({ id: v.id }, v.id, gameDir, mirror, concurrency)
      .then(() => toast(`已加入下载队列：${v.id}`, "success"))
      .catch((e) => toast(String(e), "error"));
    setConfirming(null);
  }

  function startFabric() {
    if (!gameDir || !fabricMc || !loaderPick) return;
    const displayId = `fabric-loader-${loaderPick}-${fabricMc}`;
    startTask(
      { mcVersion: fabricMc, loaderVersion: loaderPick },
      displayId,
      gameDir,
      mirror,
      concurrency,
    )
      .then(() => toast(`已加入下载队列：${displayId}`, "success"))
      .catch((e) => toast(String(e), "error"));
    setConfirmFabric(false);
  }

  return (
    <div className="flex h-full flex-col" data-testid="version-picker">
      <div className="flex flex-wrap items-center gap-3 border-b p-4">
        <div className="flex rounded-md border p-0.5">
          {(["vanilla", "fabric"] as SourceTab[]).map((t) => (
            <button
              key={t}
              onClick={() => setTab(t)}
              className={
                "rounded px-3 py-1 text-sm " +
                (tab === t ? "bg-secondary font-medium" : "text-muted-foreground")
              }
            >
              {t === "vanilla" ? "原版（Mojang）" : "Fabric"}
            </button>
          ))}
        </div>

        {tab === "vanilla" ? (
          <>
            <Input
              placeholder="搜索版本 id，如 1.20.1"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              className="w-56"
              data-testid="version-search"
            />
            <Select
              value={typeFilter}
              onChange={(e) => setTypeFilter(e.target.value)}
              className="w-36"
              data-testid="version-type-filter"
            >
              <option value="release">正式版</option>
              <option value="snapshot">快照</option>
              <option value="old_beta">旧 Beta</option>
              <option value="old_alpha">旧 Alpha</option>
              <option value="all">全部</option>
            </Select>
          </>
        ) : (
          <>
            <Select
              value={fabricMc}
              onChange={(e) => setFabricMc(e.target.value)}
              className="w-44"
              data-testid="fabric-mc-select"
            >
              <option value="">选择游戏版本…</option>
              {releaseVersions.slice(0, 80).map((v) => (
                <option key={v.id} value={v.id}>
                  {v.id}
                </option>
              ))}
            </Select>
            <Select
              value={loaderPick}
              onChange={(e) => setLoaderPick(e.target.value)}
              disabled={!loaders.length}
              className="w-52"
              data-testid="fabric-loader-select"
            >
              {loaders.map((l) => (
                <option key={l.loader.version} value={l.loader.version}>
                  {l.loader.version}
                  {l.loader.stable ? "" : "（beta）"}
                </option>
              ))}
            </Select>
            <Button
              disabled={!fabricMc || !loaderPick}
              onClick={() => setConfirmFabric(true)}
              data-testid="fabric-download"
            >
              下载 Fabric 版
            </Button>
          </>
        )}

        <div className="ml-auto flex items-center gap-2">
          <MirrorSwitch />
        </div>
      </div>

      {loadError && (
        <div className="m-4 rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive">
          版本清单获取失败：{loadError}
        </div>
      )}
      {loading && <div className="p-4 text-sm text-muted-foreground">加载版本清单…</div>}

      {tab === "vanilla" && (
        <div className="flex-1 overflow-auto p-4" data-testid="version-list">
          <div className="grid gap-2">
            {filtered.slice(0, shown).map((v) => (
              <div
                key={v.id}
                className="flex items-center gap-3 rounded-md border bg-card px-4 py-2.5"
                data-testid={`version-row-${v.id}`}
              >
                <Badge tone={v.type === "release" ? "success" : "info"}>
                  {TYPE_LABELS[v.type] ?? v.type}
                </Badge>
                <span className="font-medium">{v.id}</span>
                <span className="text-xs text-muted-foreground">
                  {new Date(v.releaseTime).toLocaleDateString()}
                </span>
                <Button
                  size="sm"
                  className="ml-auto"
                  onClick={() => setConfirming(v)}
                  data-testid={`download-${v.id}`}
                >
                  下载
                </Button>
              </div>
            ))}
            {filtered.length > shown && (
              <Button variant="ghost" onClick={() => setShown(shown + 100)}>
                显示更多（剩余 {filtered.length - shown} 个）
              </Button>
            )}
            {!loading && !filtered.length && !loadError && (
              <div className="p-6 text-center text-sm text-muted-foreground">
                没有匹配的版本
              </div>
            )}
          </div>
        </div>
      )}

      {tab === "fabric" && (
        <div className="flex-1 overflow-auto p-4 text-sm text-muted-foreground">
          <p>
            Fabric 安装会下载 Fabric Loader 及其依赖库，并自动解析其继承的原版版本
            （继承链 libraries / assets / 客户端 jar 一并下载）。
          </p>
          <p className="mt-2">
            注意：Forge 需要安装器内嵌处理流程，本下载器暂不支持；请使用
            PCL/HMCL 的 Forge 安装功能。
          </p>
        </div>
      )}

      <ConfirmDialog
        open={!!confirming}
        title={`下载 ${confirming?.id ?? ""}？`}
        description={`目标目录：${gameDir ?? "—"}\n镜像：${mirror === "bmclapi" ? "BMCLAPI" : "Mojang 官方"}\n\n文件将按标准 .minecraft 目录放置；已存在且校验通过的文件会自动跳过。`}
        confirmText="开始下载"
        onCancel={() => setConfirming(null)}
        onConfirm={() => confirming && startVanilla(confirming)}
      />
      <ConfirmDialog
        open={confirmFabric}
        title={`下载 Fabric Loader ${loaderPick}（${fabricMc}）？`}
        description={`目标目录：${gameDir ?? "—"}\n\n将创建版本 ${`fabric-loader-${loaderPick}-${fabricMc}`}，并下载其继承的原版文件。`}
        confirmText="开始下载"
        onCancel={() => setConfirmFabric(false)}
        onConfirm={startFabric}
      />
    </div>
  );
}

export function MirrorSwitch() {
  const mirror = useAppStore((s) => s.mirror);
  const save = useAppStore((s) => s.save);
  function switchTo(m: Mirror) {
    if (m === mirror) return;
    save({ mirror: m }).catch((e) => toast(String(e), "error"));
  }
  return (
    <div className="flex rounded-md border p-0.5" data-testid="mirror-switch">
      {(
        [
          ["official", "官方源"],
          ["bmclapi", "BMCLAPI"],
        ] as [Mirror, string][]
      ).map(([m, label]) => (
        <button
          key={m}
          onClick={() => switchTo(m)}
          className={
            "rounded px-3 py-1 text-xs " +
            (mirror === m ? "bg-secondary font-medium" : "text-muted-foreground")
          }
        >
          {label}
        </button>
      ))}
    </div>
  );
}
