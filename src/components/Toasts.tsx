import { X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/format";
import { useUi } from "@/stores/ui";

export function Toasts() {
  const { t } = useTranslation();
  const toasts = useUi((s) => s.toasts);
  const dismiss = useUi((s) => s.dismissToast);
  return (
    <div className="pointer-events-none fixed bottom-4 left-4 z-[60] flex w-[360px] max-w-[calc(100vw-32px)] flex-col gap-2" aria-live="polite">
      {toasts.map((toast) => (
        <div
          key={toast.id}
          role={toast.tone === "error" ? "alert" : "status"}
          className={cn(
            "pointer-events-auto flex items-center gap-3 rounded-md border bg-surface px-3 py-2 text-[13px] shadow-[0_2px_8px_rgba(0,0,0,0.1)]",
            toast.tone === "error" ? "border-danger/50 text-fg" : "border-line text-fg",
          )}
        >
          <span className="min-w-0 flex-1">{toast.text}</span>
          {toast.action && (
            <button
              className="shrink-0 font-medium text-accent hover:underline"
              onClick={() => {
                toast.action!.run();
                dismiss(toast.id);
              }}
            >
              {toast.action.label}
            </button>
          )}
          <button aria-label={t("common.close")} onClick={() => dismiss(toast.id)} className="shrink-0 text-faint hover:text-fg">
            <X size={14} />
          </button>
        </div>
      ))}
    </div>
  );
}
