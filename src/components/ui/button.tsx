import { forwardRef, type ButtonHTMLAttributes } from "react";
import { cn } from "../../lib/cn";

type Variant = "default" | "primary" | "secondary" | "destructive" | "outline" | "ghost";
type Size = "sm" | "md" | "lg" | "icon";

const variants: Record<Variant, string> = {
  default: "mc-btn-primary",
  primary: "mc-btn-primary",
  secondary: "",
  destructive: "mc-btn-danger",
  outline: "mc-btn-outline",
  ghost: "mc-btn-ghost",
};

const sizes: Record<Size, string> = {
  sm: "mc-btn-sm",
  md: "",
  lg: "text-base px-6 py-3",
  icon: "w-9 h-9 p-0",
};

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: Size;
}

/** Minecraft 风格石质按钮（.mc-btn 系列类，见 index.css） */
export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant = "default", size = "md", ...props }, ref) => (
    <button
      ref={ref}
      className={cn("mc-btn", variants[variant], sizes[size], className)}
      {...props}
    />
  ),
);
Button.displayName = "Button";
