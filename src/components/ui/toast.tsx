import { create } from "zustand";

// 极简 toast：队列 + 自动消失 — MC 风格
interface ToastItem {
  id: number;
  message: string;
  tone: "info" | "error" | "success";
}

interface ToastState {
  items: ToastItem[];
  push: (message: string, tone?: ToastItem["tone"]) => void;
  dismiss: (id: number) => void;
}

let nextId = 1;

export const useToastStore = create<ToastState>()((set, get) => ({
  items: [],
  push: (message, tone = "info") => {
    const id = nextId++;
    set({ items: [...get().items, { id, message, tone }] });
    window.setTimeout(() => get().dismiss(id), 4200);
  },
  dismiss: (id) => set({ items: get().items.filter((t) => t.id !== id) }),
}));

export function toast(message: string, tone: ToastItem["tone"] = "info") {
  useToastStore.getState().push(message, tone);
}

export function ToastContainer() {
  const items = useToastStore((s) => s.items);
  const dismiss = useToastStore((s) => s.dismiss);
  return (
    <div className="pointer-events-none fixed bottom-12 right-4 z-[60] flex w-80 flex-col gap-2">
      {items.map((t) => (
        <button
          key={t.id}
          onClick={() => dismiss(t.id)}
          className={
            "mc-panel pointer-events-auto px-4 py-3 text-left text-sm " +
            (t.tone === "error"
              ? "text-[#ff8080]"
              : t.tone === "success"
                ? "text-[#8ee67a]"
                : "text-[var(--mc-text)]")
          }
        >
          {t.message}
        </button>
      ))}
    </div>
  );
}
