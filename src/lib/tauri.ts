// Tauri invoke/listen 封装：统一错误规整 + 类型安全。
// 测试环境（vitest / playwright mock）通过 window.__TAURI_INTERNALS__ 注入。

import { invoke as rawInvoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export const EVENT_PROGRESS = "download://progress";

export async function invoke<T>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  try {
    return await rawInvoke<T>(cmd, args ?? {});
  } catch (e: unknown) {
    throw normalizeError(e);
  }
}

export function normalizeError(e: unknown): Error {
  if (typeof e === "string") return new Error(e);
  if (e && typeof e === "object" && "message" in e) {
    const msg = String((e as { message: unknown }).message);
    const code = (e as { code?: unknown }).code;
    return new Error(code ? `[${code}] ${msg}` : msg);
  }
  return new Error(String(e));
}

export { listen };
export type { UnlistenFn } from "@tauri-apps/api/event";
