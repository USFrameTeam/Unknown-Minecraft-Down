import { useEffect } from "react";
import { cn } from "../../lib/cn";
import { Button } from "./button";

// 轻量确认对话框（破坏性操作二次确认）
export function ConfirmDialog({
  open,
  title,
  description,
  confirmText = "确定",
  cancelText = "取消",
  danger,
  onConfirm,
  onCancel,
}: {
  open: boolean;
  title: string;
  description?: string;
  confirmText?: string;
  cancelText?: string;
  danger?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onCancel();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onCancel]);

  if (!open) return null;
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
      data-testid="confirm-dialog"
    >
      <div className="w-full max-w-md rounded-lg border bg-card p-6 shadow-xl">
        <h2 className="text-base font-semibold">{title}</h2>
        {description && (
          <p className="mt-2 whitespace-pre-wrap text-sm text-muted-foreground">
            {description}
          </p>
        )}
        <div className="mt-6 flex justify-end gap-2">
          <Button variant="outline" onClick={onCancel}>
            {cancelText}
          </Button>
          <Button
            variant={danger ? "destructive" : "default"}
            onClick={onConfirm}
            data-testid="confirm-accept"
          >
            {confirmText}
          </Button>
        </div>
      </div>
    </div>
  );
}

export function Modal({
  open,
  className,
  children,
}: {
  open: boolean;
  className?: string;
  children: React.ReactNode;
}) {
  if (!open) return null;
  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/60 p-4">
      <div className={cn("w-full max-w-lg rounded-lg border bg-card shadow-xl", className)}>
        {children}
      </div>
    </div>
  );
}
