import { useEffect } from "react";
import { cn } from "../../lib/cn";
import { Button } from "./button";

// 轻量确认对话框（破坏性操作二次确认）— MC 风格
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
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4"
      data-testid="confirm-dialog"
    >
      <div className="mc-panel w-full max-w-md p-6">
        <h2 className="mc-pixel text-[13px] font-bold">{title}</h2>
        {description && (
          <p className="mt-3 whitespace-pre-wrap text-sm text-muted-foreground">
            {description}
          </p>
        )}
        <div className="mt-6 flex justify-end gap-3">
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
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/70 p-4">
      <div className={cn("mc-panel w-full max-w-lg", className)}>{children}</div>
    </div>
  );
}
