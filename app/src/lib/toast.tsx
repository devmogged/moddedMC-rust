import { useSyncExternalStore } from "react";
import { CircleAlert, CircleCheck, X } from "lucide-react";
import { cn } from "./utils";

type Toast = { id: number; tone: "ok" | "error"; text: string };

let toasts: Toast[] = [];
let next = 1;
const listeners = new Set<() => void>();
const emit = () => listeners.forEach((l) => l());

function dismiss(id: number) {
  toasts = toasts.filter((t) => t.id !== id);
  emit();
}

/** A short notice in the corner; errors stay twice as long. */
export function toast(text: string, tone: Toast["tone"] = "ok") {
  const id = next++;
  toasts = [...toasts.slice(-3), { id, tone, text }];
  emit();
  window.setTimeout(() => dismiss(id), tone === "error" ? 8000 : 4000);
}

export function Toaster() {
  const list = useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => toasts,
  );
  return (
    <div className="pointer-events-none fixed right-4 bottom-4 z-50 flex w-80 flex-col gap-2">
      {list.map((t) => (
        <div
          key={t.id}
          role={t.tone === "error" ? "alert" : "status"}
          className="dropdown-glass pointer-events-auto flex animate-rise items-start gap-2.5 rounded-xl p-3 text-sm shadow-lg"
        >
          {t.tone === "error" ? (
            <CircleAlert className="mt-0.5 size-4 shrink-0 text-destructive-foreground" />
          ) : (
            <CircleCheck className="mt-0.5 size-4 shrink-0 text-success-foreground" />
          )}
          <span className={cn("flex-1", t.tone === "error" && "text-destructive-foreground")}>{t.text}</span>
          <button
            aria-label="Dismiss"
            onClick={() => dismiss(t.id)}
            className="text-muted-foreground hover:text-foreground"
          >
            <X className="size-4" />
          </button>
        </div>
      ))}
    </div>
  );
}
