import { useCallback, useState } from "react";

export type ToastKind = "ok" | "error" | "info";
export type Toast = { id: number; kind: ToastKind; text: string };

const LIFETIME_MS: Record<ToastKind, number> = { ok: 5000, info: 8000, error: 14000 };

export function useToasts() {
  const [toasts, setToasts] = useState<Toast[]>([]);

  const dismiss = useCallback((id: number) => {
    setToasts((all) => all.filter((t) => t.id !== id));
  }, []);

  const push = useCallback(
    (kind: ToastKind, text: string) => {
      const id = Date.now() + Math.random();
      setToasts((all) => [...all, { id, kind, text }]);
      window.setTimeout(() => dismiss(id), LIFETIME_MS[kind]);
    },
    [dismiss],
  );

  return { toasts, push, dismiss };
}

interface ToastStackProps {
  toasts: Toast[];
  onDismiss: (id: number) => void;
}

export function ToastStack({ toasts, onDismiss }: ToastStackProps) {
  return (
    <div className="toasts" aria-live="polite">
      {toasts.map((t) => (
        <div key={t.id} className={`toast toast-${t.kind}`} role={t.kind === "error" ? "alert" : "status"}>
          <span className="toast-mark" aria-hidden>
            {t.kind === "ok" ? "✓" : t.kind === "error" ? "!" : "i"}
          </span>
          <p>{t.text}</p>
          <button className="icon-btn" onClick={() => onDismiss(t.id)} aria-label="Dismiss">
            ×
          </button>
        </div>
      ))}
    </div>
  );
}
