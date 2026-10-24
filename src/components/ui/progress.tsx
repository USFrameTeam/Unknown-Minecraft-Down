import { cn } from "../../lib/cn";

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
      className={cn("h-2 w-full overflow-hidden rounded-full bg-muted", className)}
    >
      <div
        className={cn(
          "h-full rounded-full bg-primary transition-[width] duration-300 ease-out",
          indeterminate && "animate-pulse w-1/3",
        )}
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
  tone?: "default" | "success" | "warning" | "danger" | "info";
  className?: string;
}) {
  const tones: Record<string, string> = {
    default: "bg-secondary text-secondary-foreground",
    success: "bg-primary/20 text-primary",
    warning: "bg-yellow-500/20 text-yellow-400",
    danger: "bg-destructive/20 text-destructive",
    info: "bg-sky-500/20 text-sky-400",
  };
  return (
    <span
      className={cn(
        "inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium",
        tones[tone],
        className,
      )}
    >
      {children}
    </span>
  );
}

export function Separator({ className }: { className?: string }) {
  return <div className={cn("h-px w-full bg-border", className)} />;
}
