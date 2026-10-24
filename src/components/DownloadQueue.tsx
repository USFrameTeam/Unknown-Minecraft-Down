import { useState } from "react";
import { useDownloadStore } from "../stores/downloadStore";
import type { TaskSnapshot } from "../types";
import { formatBytes, formatEta, formatSpeed, percent, STAGE_LABELS, STATUS_LABELS } from "../lib/format";
import { Badge, Progress } from "./ui/progress";
import { Button } from "./ui/button";
import { ConfirmDialog } from "./ui/dialog";
import { toast } from "./ui/toast";
import { COMPATIBILITY_HINT } from "../content/disclaimer";

function statusTone(s: TaskSnapshot["status"]) {
  switch (s) {
    case "running":
      return "info" as const;
    case "completed":
      return "success" as const;
    case "failed":
      return "danger" as const;
    case "paused":
    case "queued":
      return "warning" as const;
    case "canceled":
      return "default" as const;
  }
}

function TaskCard({ task }: { task: TaskSnapshot }) {
  const pauseTask = useDownloadStore((s) => s.pauseTask);
  const resumeTask = useDownloadStore((s) => s.resumeTask);
  const cancelTask = useDownloadStore((s) => s.cancelTask);
  const [confirmCancel, setConfirmCancel] = useState(false);

  const pct = percent(task.downloadedBytes, task.totalBytes);
  const remaining = Math.max(0, task.totalBytes - task.downloadedBytes);
  const active = task.status === "running";

  return (
    <div className="rounded-lg border bg-card p-4" data-testid={`task-${task.versionId}`}>
      <div className="flex flex-wrap items-center gap-2">
        <span className="font-semibold">{task.versionId}</span>
        <Badge tone={statusTone(task.status)}>{STATUS_LABELS[task.status]}</Badge>
        {task.stage && active && (
          <span className="text-xs text-muted-foreground">
            {STAGE_LABELS[task.stage] ?? task.stage}
          </span>
        )}
        <span className="ml-auto text-xs text-muted-foreground">
          {formatBytes(task.downloadedBytes)} / {formatBytes(task.totalBytes)}
          {active && (
            <>
              {" · "}
              {formatSpeed(task.speedBps)} · 剩余 {formatEta(remaining, task.speedBps)}
            </>
          )}
        </span>
      </div>

      <Progress className="mt-3" value={pct} indeterminate={active && task.totalBytes === 0} />

      <div className="mt-2 flex items-center gap-2">
        <span className="text-xs text-muted-foreground">
          文件 {task.filesDone}/{task.filesTotal || "—"}
          {task.currentFile ? ` · ${task.currentFile}` : ""}
        </span>
        <div className="ml-auto flex gap-2">
          {(task.status === "running" || task.status === "queued") && (
            <Button size="sm" variant="outline" onClick={() => pauseTask(task.taskId).catch((e) => toast(String(e), "error"))}>
              暂停
            </Button>
          )}
          {(task.status === "paused" || task.status === "failed" || task.status === "canceled") && (
            <Button size="sm" variant="outline" onClick={() => resumeTask(task.taskId).catch((e) => toast(String(e), "error"))}>
              {task.status === "paused" ? "继续" : "重试"}
            </Button>
          )}
          {task.status !== "completed" && (
            <Button size="sm" variant="ghost" onClick={() => setConfirmCancel(true)}>
              取消
            </Button>
          )}
        </div>
      </div>

      {task.status === "failed" && task.error && (
        <div className="mt-2 rounded border border-destructive/40 bg-destructive/10 p-2 text-xs text-destructive">
          {task.error}
        </div>
      )}
      {task.status === "completed" && (
        <div className="mt-2 rounded border border-primary/40 bg-primary/10 p-2 text-xs text-primary">
          {COMPATIBILITY_HINT}
        </div>
      )}

      <ConfirmDialog
        open={confirmCancel}
        title={`取消下载 ${task.versionId}？`}
        description="已下载的部分会保留（.part 断点），但任务将被终止。这是破坏性操作。"
        danger
        confirmText="确认取消"
        onCancel={() => setConfirmCancel(false)}
        onConfirm={() => {
          setConfirmCancel(false);
          cancelTask(task.taskId).catch((e) => toast(String(e), "error"));
        }}
      />
    </div>
  );
}

export function DownloadQueue() {
  const tasks = useDownloadStore((s) => s.tasks);
  const order = useDownloadStore((s) => s.order);
  const clearFinished = useDownloadStore((s) => s.clearFinished);
  const [confirmClear, setConfirmClear] = useState(false);
  const [confirmPauseAll, setConfirmPauseAll] = useState(false);

  const list = order.map((id) => tasks[id]).filter(Boolean) as TaskSnapshot[];
  const activeList = list.filter((t) => t.status === "running" || t.status === "queued");

  return (
    <div className="flex h-full flex-col" data-testid="download-queue">
      <div className="flex items-center gap-3 border-b p-4">
        <h2 className="text-base font-semibold">下载队列</h2>
        <div className="ml-auto flex gap-2">
          <Button
            size="sm"
            variant="outline"
            disabled={!activeList.length}
            onClick={() => setConfirmPauseAll(true)}
          >
            全部暂停
          </Button>
          <Button size="sm" variant="ghost" onClick={() => setConfirmClear(true)}>
            清除已完成
          </Button>
        </div>
      </div>
      <div className="flex-1 overflow-auto p-4">
        {!list.length && (
          <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
            暂无下载任务 —— 到「版本选择」页添加一个吧
          </div>
        )}
        <div className="grid gap-3">
          {list.map((t) => (
            <TaskCard key={t.taskId} task={t} />
          ))}
        </div>
      </div>

      <ConfirmDialog
        open={confirmPauseAll}
        title="暂停全部下载任务？"
        description="可以随时点“继续”从断点恢复。"
        confirmText="全部暂停"
        onCancel={() => setConfirmPauseAll(false)}
        onConfirm={() => {
          setConfirmPauseAll(false);
          for (const t of activeList) {
            useDownloadStore.getState().pauseTask(t.taskId).catch((e) => toast(String(e), "error"));
          }
        }}
      />
      <ConfirmDialog
        open={confirmClear}
        title="清除所有已结束的任务？"
        description="仅移除列表记录，不影响已下载的文件。"
        confirmText="清除"
        onCancel={() => setConfirmClear(false)}
        onConfirm={() => {
          setConfirmClear(false);
          clearFinished().catch((e) => toast(String(e), "error"));
        }}
      />
    </div>
  );
}
