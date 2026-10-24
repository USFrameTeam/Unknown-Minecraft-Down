// 极简 className 合并（避免引入 tailwind-merge 依赖）
export function cn(...parts: Array<string | false | null | undefined>): string {
  return parts.filter(Boolean).join(" ");
}
