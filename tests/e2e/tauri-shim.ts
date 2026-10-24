// 注入到页面的 Tauri IPC mock（__TAURI_INTERNALS__），在应用脚本之前执行。
// 同时暴露 window.__emitTauriEvent 供测试驱动 download://progress 事件。
export function installTauriShim(fixtures: {
  settings: unknown;
  versions: unknown[];
  tasks: unknown[];
}) {
  const callbacks = new Map<number, (ev: unknown) => void>();
  const listeners: Record<string, number[]> = {};
  let nextId = 1;

  const mock: {
    settings: Record<string, unknown>;
    started: Array<Record<string, unknown>>;
    canceled: string[];
    paused: string[];
    resumed: string[];
  } = {
    settings: fixtures.settings as Record<string, unknown>,
    started: [],
    canceled: [],
    paused: [],
    resumed: [],
  };

  function mockInvoke(cmd: string, args: Record<string, unknown>): unknown {
    switch (cmd) {
      case "get_settings":
        return mock.settings;
      case "save_settings":
        mock.settings = { ...(mock.settings as object), ...(args.settings as object) };
        return null;
      case "list_versions":
        return fixtures.versions;
      case "list_download_tasks":
        return fixtures.tasks;
      case "start_download":
        mock.started.push(args);
        return `task-${mock.started.length}`;
      case "cancel_download":
        mock.canceled.push(String(args.taskId));
        return null;
      case "pause_download":
        mock.paused.push(String(args.taskId));
        return null;
      case "resume_download":
        mock.resumed.push(String(args.taskId));
        return null;
      case "clear_finished_tasks":
        return null;
      case "list_fabric_loaders":
        return [{ loader: { version: "0.16.9", stable: true } }];
      case "detect_game_dir":
        return "/home/tester/.minecraft";
      case "probe_default_dir":
        return "/home/tester/.minecraft";
      case "prepare_game_dir":
        return String(args.path);
      case "verify_installation":
        return {
          versionId: args.versionId,
          gameDir: args.gameDir,
          ok: false,
          checkedFiles: 3,
          missing: [{ path: "assets/objects/aa/abc", url: null, expectedSha1: "x", actualSha1: null, size: 1 }],
          corrupted: [],
          totalBytes: 123,
        };
      default:
        throw new Error(`[tauri-shim] 未 mock 的命令: ${cmd}`);
    }
  }

  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
    transformCallback(cb: (ev: unknown) => void) {
      const id = nextId++;
      callbacks.set(id, cb);
      return id;
    },
    async invoke(cmd: string, args: Record<string, unknown>) {
      if (cmd === "plugin:event|listen") {
        const ev = String(args.event);
        (listeners[ev] ??= []).push(args.handler as number);
        return nextId++;
      }
      if (cmd === "plugin:event|unlisten") return null;
      return mockInvoke(cmd, args ?? {});
    },
  };

  (window as unknown as Record<string, unknown>).__mcMock = mock;
  (window as unknown as Record<string, unknown>).__emitTauriEvent = (
    event: string,
    payload: unknown,
  ) => {
    for (const id of listeners[event] ?? []) {
      callbacks.get(id)?.({ event, id: Date.now(), payload });
    }
  };
}
