// 展示格式化工具（纯函数，便于单测）

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "—";
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = bytes / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return `${v >= 100 ? v.toFixed(0) : v.toFixed(1)} ${units[i]}`;
}

export function formatSpeed(bps: number): string {
  if (!Number.isFinite(bps) || bps <= 0) return "—";
  return `${formatBytes(bps)}/s`;
}

export function formatEta(remainingBytes: number, speedBps: number): string {
  if (!Number.isFinite(speedBps) || speedBps <= 0) return "—";
  if (!Number.isFinite(remainingBytes) || remainingBytes <= 0) return "0 秒";
  const secs = Math.round(remainingBytes / speedBps);
  if (secs < 60) return `${secs} 秒`;
  const mins = Math.floor(secs / 60);
  const rest = secs % 60;
  if (mins < 60) return `${mins} 分 ${rest} 秒`;
  const hours = Math.floor(mins / 60);
  return `${hours} 时 ${mins % 60} 分`;
}

export function percent(done: number, total: number): number {
  if (total <= 0) return 0;
  return Math.max(0, Math.min(100, (done / total) * 100));
}

export const STAGE_LABELS: Record<string, string> = {
  manifest: "获取版本清单",
  version_json: "解析版本数据",
  libraries: "下载库与客户端",
  assets: "下载资源文件",
  forge: "安装 Forge",
  verifying: "整理与校验",
  finished: "完成",
};

export const STATUS_LABELS: Record<string, string> = {
  queued: "排队中",
  running: "下载中",
  paused: "已暂停",
  completed: "已完成",
  failed: "失败",
  canceled: "已取消",
};
