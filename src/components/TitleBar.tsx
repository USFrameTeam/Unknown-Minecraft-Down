import { getCurrentWindow } from "@tauri-apps/api/window";

/** 自定义标题栏：拖拽移动 + 最小化 / 关闭（无边框窗口）。
 * 窗口操作在浏览器/E2E 环境下静默失败（catch 吞掉）。 */
export function TitleBar() {
  const minimize = () => getCurrentWindow().minimize().catch(() => {});
  const close = () => getCurrentWindow().close().catch(() => {});

  return (
    <div
      className="mc-titlebar flex h-9 shrink-0 select-none items-center border-b-2 border-black bg-[#232325]"
      data-tauri-drag-region
      data-testid="titlebar"
    >
      <span
        className="mc-pixel px-3 text-[10px] font-bold text-[var(--mc-green-light)]"
        data-tauri-drag-region
      >
        UNKNOWN MINECRAFT DOWN
      </span>
      <span className="text-[10px] text-[var(--mc-text-dim)]" data-tauri-drag-region>
        v0.3.1
      </span>
      <div className="ml-auto flex h-full">
        <button
          onClick={minimize}
          data-testid="win-minimize"
          title="最小化"
          className="mc-winbtn h-full w-11 text-sm text-[var(--mc-text)] hover:brightness-125"
        >
          ▬
        </button>
        <button
          onClick={close}
          data-testid="win-close"
          title="关闭"
          className="mc-winbtn h-full w-11 text-sm text-[var(--mc-text)] hover:!bg-[#a33a3a]"
        >
          ✕
        </button>
      </div>
    </div>
  );
}
