import { useEffect, useMemo, useState } from "react";
import { invoke } from "../lib/tauri";
import { useAppStore } from "../stores/appStore";
import { useDownloadStore } from "../stores/downloadStore";
import type {
  FabricLoaderSummary,
  ForgeBuildSummary,
  Mirror,
  VersionSummary,
} from "../types";
import { Badge } from "./ui/progress";
import { Button } from "./ui/button";
import { Input, Select } from "./ui/input";
import { ConfirmDialog } from "./ui/dialog";
import { toast } from "./ui/toast";
import { FORGE_JAVA_HINT } from "../content/disclaimer";

const TYPE_LABELS: Record<string, string> = {
  release: "正式版",
  snapshot: "快照",
  old_beta: "旧 Beta",
  old_alpha: "旧 Alpha",
};

type SourceTab = "vanilla" | "fabric" | "forge";

export function VersionPicker() {
  const mirror = useAppStore((s) => s.mirror);
  const concurrency = useAppStore((s) => s.concurrency);
  const javaPath = useAppStore((s) => s.javaPath);

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

  // Forge
  const [forgeMc, setForgeMc] = useState("");
  const [builds, setBuilds] = useState<ForgeBuildSummary[]>([]);
  const [buildPick, setBuildPick] = useState<ForgeBuildSummary | null>(null);
  const [forgeLoading, setForgeLoading] = useState(false);
  const [confirmForge, setConfirmForge] = useState(false);

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

  useEffect(() => {
    if (tab !== "forge" || !forgeMc) {
      setBuilds([]);
      setBuildPick(null);
      return;
    }
    let cancelled = false;
    setForgeLoading(true);
    invoke<ForgeBuildSummary[]>("list_forge_builds", { mcVersion: forgeMc })
      .then((bs) => {
        if (cancelled) return;
        setBuilds(bs);
        setBuildPick(bs[0] ?? null);
      })
      .catch((e) => {
        if (!cancelled) {
          setBuilds([]);
          toast(`获取 Forge 构建列表失败: ${e}`, "error");
        }
      })
      .finally(() => {
        if (!cancelled) setForgeLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [tab, forgeMc]);

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
    withDir((dir) => {
      startTask({ id: v.id }, v.id, dir, mirror, concurrency)
        .then(() => toast(`已加入下载队列：${v.id}`, "success"))
        .catch((e) => toast(String(e), "error"));
      setConfirming(null);
    });
  }

  function startFabric() {
    if (!fabricMc || !loaderPick) return;
    withDir((dir) => {
      const displayId = `fabric-loader-${loaderPick}-${fabricMc}`;
      startTask(
        { mcVersion: fabricMc, loaderVersion: loaderPick },
        displayId,
        dir,
        mirror,
        concurrency,
      )
        .then(() => toast(`已加入下载队列：${displayId}`, "success"))
        .catch((e) => toast(String(e), "error"));
      setConfirmFabric(false);
    });
  }

  function startForge() {
    const b = buildPick;
    if (!b) return;
    withDir((dir) => {
      startTask(
        { mcVersion: b.mcVersion, forgeVersion: b.forgeVersion, branch: b.branch ?? null },
        b.versionId,
        dir,
        mirror,
        concurrency,
        javaPath,
      )
        .then(() => toast(`已加入下载队列：${b.versionId}`, "success"))
        .catch((e) => toast(String(e), "error"));
      setConfirmForge(false);
    });
  }

  const startTask = useDownloadStore((s) => s.startTask);
  const withDir = useWithActiveDir();

  return (
    <div className="flex h-full flex-col" data-testid="version-picker">
      <div className="flex flex-wrap items-center gap-3 border-b-2 border-black/60 p-4">
        <div className="flex items-center gap-1" data-testid="source-tabs">
          {(
            [
              ["vanilla", "原版"],
              ["fabric", "Fabric"],
              ["forge", "Forge"],
            ] as [SourceTab, string][]
          ).map(([t, label]) => (
            <button
              key={t}
              onClick={() => setTab(t)}
              className={`mc-tab ${tab === t ? "mc-tab-active" : ""}`}
              data-testid={`tab-${t}`}
            >
              {label}
            </button>
          ))}
        </div>

        <DirSelector />

        <div className="ml-auto flex items-center gap-2">
          <MirrorSwitch />
        </div>
      </div>

      {tab === "forge" && !javaPath && (
        <div className="mx-4 mt-3 border-2 border-[#8a6d2b] bg-[#3a3222] p-2 text-xs text-[#ffd94a]">
          {FORGE_JAVA_HINT}
        </div>
      )}

      <div className="flex flex-wrap items-center gap-3 px-4 py-3">
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
        ) : tab === "fabric" ? (
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
              variant="primary"
              disabled={!fabricMc || !loaderPick}
              onClick={() => setConfirmFabric(true)}
              data-testid="fabric-download"
            >
              下载 Fabric 版
            </Button>
          </>
        ) : (
          <>
            <Select
              value={forgeMc}
              onChange={(e) => setForgeMc(e.target.value)}
              className="w-44"
              data-testid="forge-mc-select"
            >
              <option value="">选择游戏版本…</option>
              {releaseVersions.slice(0, 120).map((v) => (
                <option key={v.id} value={v.id}>
                  {v.id}
                </option>
              ))}
            </Select>
            <Select
              value={buildPick?.forgeVersion ?? ""}
              onChange={(e) => {
                setBuildPick(builds.find((b) => b.forgeVersion === e.target.value) ?? null);
              }}
              disabled={!builds.length}
              className="w-48"
              data-testid="forge-build-select"
            >
              {builds.slice(0, 60).map((b) => (
                <option key={b.versionId} value={b.forgeVersion}>
                  Forge {b.forgeVersion}
                  {b.branch ? `（${b.branch}）` : ""}
                </option>
              ))}
            </Select>
            <Button
              variant="primary"
              disabled={!buildPick}
              onClick={() => setConfirmForge(true)}
              data-testid="forge-download"
            >
              下载 Forge 版
            </Button>
            {forgeLoading && (
              <span className="text-xs text-[var(--mc-text-dim)]">加载构建列表…</span>
            )}
          </>
        )}
      </div>

      {loadError && (
        <div className="mx-4 mb-3 border-2 border-[#9e3131] bg-[#3a2222] p-3 text-sm text-[#ff8080]">
          版本清单获取失败：{loadError}
        </div>
      )}
      {loading && <div className="px-4 text-sm text-[var(--mc-text-dim)]">加载版本清单…</div>}

      {tab === "vanilla" && (
        <div className="flex-1 overflow-auto px-4 pb-4" data-testid="version-list">
          <div className="grid gap-1.5">
            {filtered.slice(0, shown).map((v) => (
              <div
                key={v.id}
                className="mc-panel-dark flex items-center gap-3 px-4 py-2.5"
                data-testid={`version-row-${v.id}`}
              >
                <Badge tone={v.type === "release" ? "success" : "info"}>
                  {TYPE_LABELS[v.type] ?? v.type}
                </Badge>
                <span className="font-medium">{v.id}</span>
                <span className="text-xs text-[var(--mc-text-dim)]">
                  {new Date(v.releaseTime).toLocaleDateString()}
                </span>
                <Button
                  size="sm"
                  variant="primary"
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
              <div className="p-6 text-center text-sm text-[var(--mc-text-dim)]">
                没有匹配的版本
              </div>
            )}
          </div>
        </div>
      )}

      {tab === "fabric" && (
        <div className="flex-1 overflow-auto px-4 pb-4 text-sm text-[var(--mc-text-dim)]">
          <p>
            Fabric 安装会下载 Fabric Loader 及其依赖库，并自动解析其继承的原版版本
            （继承链 libraries / assets / 客户端 jar 一并下载）。
          </p>
        </div>
      )}

      {tab === "forge" && (
        <div className="flex-1 overflow-auto px-4 pb-4 text-sm text-[var(--mc-text-dim)]">
          <p>
            Forge 安装复刻官方安装器流程：下载官方安装器（校验 SHA-1）→ 解析
            install_profile → 下载依赖库 → 以本机 Java 执行官方 processors 生成
            补丁客户端（产物与官方安装器逐字节一致）。旧版本（≤1.12.2）无
            processors，纯下载完成。
          </p>
          <p className="mt-2">
            注意：Forge 1.13+ 安装期间需要可访问 Mojang 官方端点（下载官方映射）。
          </p>
        </div>
      )}

      <ConfirmDialog
        open={!!confirming}
        title={`下载 ${confirming?.id ?? ""}？`}
        description={`镜像：${mirror === "bmclapi" ? "BMCLAPI" : "Mojang 官方"}\n\n文件将按标准 .minecraft 目录放置；已存在且校验通过的文件会自动跳过。`}
        confirmText="开始下载"
        onCancel={() => setConfirming(null)}
        onConfirm={() => confirming && startVanilla(confirming)}
      />
      <ConfirmDialog
        open={confirmFabric}
        title={`下载 Fabric Loader ${loaderPick}（${fabricMc}）？`}
        description={`将创建版本 fabric-loader-${loaderPick}-${fabricMc}，并下载其继承的原版文件。`}
        confirmText="开始下载"
        onCancel={() => setConfirmFabric(false)}
        onConfirm={startFabric}
      />
      <ConfirmDialog
        open={confirmForge}
        title={`下载 Forge ${buildPick?.forgeVersion ?? ""}（${buildPick?.mcVersion ?? ""}）？`}
        description={`将创建版本 ${buildPick?.versionId ?? ""}。1.13+ 版本会以本机 Java 执行官方安装器 processors（仅用于生成 Forge 文件，不会启动游戏）。`}
        confirmText="开始下载"
        onCancel={() => setConfirmForge(false)}
        onConfirm={startForge}
      />
    </div>
  );
}

/** 取激活目录并做前置校验（无目录时提示去设置）。 */
function useWithActiveDir() {
  const gameDirs = useAppStore((s) => s.gameDirs);
  return (fn: (dir: string) => void) => {
    const dir = useAppStore.getState().activeGameDir;
    if (!dir || !gameDirs.length) {
      toast("未配置 .minecraft 目录，请先到设置页配置", "error");
      return;
    }
    fn(dir);
  };
}

/** 多目录切换器：显示当前激活目录，可切换。 */
export function DirSelector() {
  const gameDirs = useAppStore((s) => s.gameDirs);
  const activeGameDir = useAppStore((s) => s.activeGameDir);
  const setActiveGameDir = useAppStore((s) => s.setActiveGameDir);

  if (gameDirs.length <= 1) {
    return (
      <span
        className="max-w-56 truncate px-2 text-xs text-[var(--mc-text-dim)]"
        title={activeGameDir ?? ""}
        data-testid="dir-display"
      >
        📁 {activeGameDir ?? "未配置"}
      </span>
    );
  }
  return (
    <Select
      className="w-56 text-xs"
      value={activeGameDir ?? ""}
      onChange={(e) => setActiveGameDir(e.target.value).catch((err) => toast(String(err), "error"))}
      data-testid="dir-select"
      title="切换下载目录"
    >
      {gameDirs.map((d) => (
        <option key={d} value={d}>
          📁 {d}
        </option>
      ))}
    </Select>
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
    <div className="flex" data-testid="mirror-switch">
      {(
        [
          ["official", "官方源"],
          ["bmclapi", "BMCLAPI"],
        ] as [Mirror, string][]
      ).map(([m, label]) => (
        <button
          key={m}
          onClick={() => switchTo(m)}
          className={`mc-tab !px-3 !py-1.5 !text-xs ${mirror === m ? "mc-tab-active" : ""}`}
        >
          {label}
        </button>
      ))}
    </div>
  );
}
