import { cn } from "../../lib/cn";

/** Minecraft 风格 XP 绿条 */
export function Progress({
  value,
  className,
  indeterminate,
}: {
  value: number; // 0..100
  className?: string;
  indeterminate?: boolean;
}) {
  const v = Math.max(0, Math.min(100, value));
  return (
    <div
      role="progressbar"
      aria-valuenow={indeterminate ? undefined : Math.round(v)}
      aria-valuemin={0}
      aria-valuemax={100}
      className={cn("mc-progress", className)}
    >
      <div
        className={cn("mc-progress-fill", indeterminate && "mc-progress-fill-indeterminate")}
        style={indeterminate ? undefined : { width: `${v}%` }}
      />
    </div>
  );
}

export function Badge({
  children,
  tone = "default",
  className,
}: {
  children: React.ReactNode;
  tone?: "default" | "success" | "warning" | "danger" | "info" | "gray";
  className?: string;
}) {
  const tones: Record<string, string> = {
    default: "mc-badge-gray",
    success: "mc-badge-green",
    warning: "mc-badge-gold",
    danger: "mc-badge-red",
    info: "mc-badge-blue",
    gray: "mc-badge-gray",
  };
  return (
    <span className={cn("mc-badge", tones[tone], className)}>{children}</span>
  );
}

export function Separator({ className }: { className?: string }) {
  return <div className={cn("h-0.5 w-full bg-black/50", className)} />;
}
