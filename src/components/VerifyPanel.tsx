import { useState } from "react";
import { invoke } from "../lib/tauri";
import { useAppStore } from "../stores/appStore";
import { useDownloadStore } from "../stores/downloadStore";
import type { VerificationReport } from "../types";
import { formatBytes } from "../lib/format";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { Badge } from "./ui/progress";

export function VerifyPanel() {
  const gameDir = useAppStore((s) => s.gameDir);
  const mirror = useAppStore((s) => s.mirror);
  const tasks = useDownloadStore((s) => s.tasks);
  const order = useDownloadStore((s) => s.order);
  const [versionId, setVersionId] = useState("");
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<VerificationReport | null>(null);
  const [error, setError] = useState<string | null>(null);

  const completedIds = order
    .map((id) => tasks[id])
    .filter((t) => t && t.status === "completed")
    .map((t) => t.versionId);

  async function run() {
    if (!gameDir || !versionId.trim()) return;
    setBusy(true);
    setError(null);
    setReport(null);
    try {
      const r = await invoke<VerificationReport>("verify_installation", {
        versionId: versionId.trim(),
        gameDir,
        mirror,
      });
      setReport(r);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="flex h-full flex-col" data-testid="verify-panel">
      <div className="flex flex-wrap items-center gap-3 border-b p-4">
        <h2 className="text-base font-semibold">安装校验</h2>
        <Input
          placeholder="版本 id，如 1.20.1"
          value={versionId}
          onChange={(e) => setVersionId(e.target.value)}
          className="w-56"
          data-testid="verify-version-input"
        />
        {completedIds.length > 0 && (
          <select
            className="h-9 rounded-md border border-input bg-card px-2 text-sm"
            onChange={(e) => e.target.value && setVersionId(e.target.value)}
            defaultValue=""
          >
            <option value="">从已完成的任务中选择…</option>
            {completedIds.map((id) => (
              <option key={id} value={id}>
                {id}
              </option>
            ))}
          </select>
        )}
        <Button
          onClick={run}
          disabled={busy || !versionId.trim() || !gameDir}
          data-testid="verify-run"
        >
          {busy ? "校验中…" : "开始校验"}
        </Button>
        <span className="ml-auto text-xs text-muted-foreground">
          目标：{gameDir ?? "未配置"}
        </span>
      </div>

      <div className="flex-1 overflow-auto p-4">
        {error && (
          <div className="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive">
            {error}
          </div>
        )}
        {!report && !error && (
          <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
            输入版本 id 后一键校验：逐文件核对 SHA-1，报告缺失与损坏
          </div>
        )}
        {report && (
          <div className="grid gap-4">
            <div className="flex flex-wrap items-center gap-3 rounded-lg border bg-card p-4">
              {report.ok ? (
                <Badge tone="success">通过</Badge>
              ) : (
                <Badge tone="danger">存在问题</Badge>
              )}
              <span className="text-sm">
                共校验 {report.checkedFiles} 个文件 ·{" "}
                {formatBytes(report.totalBytes)}
              </span>
              <span className="ml-auto text-xs text-muted-foreground">
                {report.versionId} @ {report.gameDir}
              </span>
            </div>

            {report.missing.length > 0 && (
              <ProblemTable
                title={`缺失文件（${report.missing.length}）`}
                entries={report.missing}
                tone="warning"
              />
            )}
            {report.corrupted.length > 0 && (
              <ProblemTable
                title={`损坏文件（${report.corrupted.length}）`}
                entries={report.corrupted}
                tone="danger"
              />
            )}
          </div>
        )}
      </div>
    </div>
  );
}

function ProblemTable({
  title,
  entries,
  tone,
}: {
  title: string;
  entries: VerificationReport["missing"];
  tone: "warning" | "danger";
}) {
  return (
    <div className="overflow-hidden rounded-lg border bg-card" data-testid={`problem-${tone}`}>
      <div className="border-b p-3 text-sm font-medium">{title}</div>
      <table className="w-full text-left text-xs">
        <thead className="text-muted-foreground">
          <tr>
            <th className="px-3 py-2 font-normal">路径</th>
            <th className="px-3 py-2 font-normal">期望 SHA-1</th>
            <th className="px-3 py-2 font-normal">实际 SHA-1</th>
          </tr>
        </thead>
        <tbody>
          {entries.slice(0, 200).map((e, i) => (
            <tr key={`${e.path}-${i}`} className="border-t">
              <td className="px-3 py-2 font-mono">{e.path}</td>
              <td className="px-3 py-2 font-mono text-muted-foreground">
                {e.expectedSha1 ?? "—"}
              </td>
              <td className="px-3 py-2 font-mono text-muted-foreground">
                {e.actualSha1 ?? "（不存在）"}
              </td>
            </tr>
          ))}
          {entries.length > 200 && (
            <tr className="border-t">
              <td colSpan={3} className="px-3 py-2 text-muted-foreground">
                仅显示前 200 条，共 {entries.length} 条
              </td>
            </tr>
          )}
        </tbody>
      </table>
    </div>
  );
}
