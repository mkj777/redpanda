import * as Menu from "@radix-ui/react-dropdown-menu";
import { Download as DownloadIcon, FileText, FolderOpen, X } from "lucide-react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { api, type Download } from "@/lib/ipc";
import { cn, fileSize, listDate } from "@/lib/format";
import { fileDrag, onFile, useDownloads } from "@/stores/downloads";

/** A small button inside a list entry. It keeps its clicks to itself, so the entry around it
 * doesn't open the file as well. */
function EntryButton({ label, onClick, children }: { label: string; onClick: () => void; children: ReactNode }) {
  const own = (e: React.SyntheticEvent) => e.stopPropagation();
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onPointerDown={own}
      onPointerUp={own}
      onClick={(e) => {
        e.stopPropagation();
        onClick();
      }}
      className="inline-flex size-7 items-center justify-center rounded-md text-muted hover:bg-line hover:text-fg"
    >
      {children}
    </button>
  );
}

function Entry({ d }: { d: Download }) {
  const { t, i18n } = useTranslation();
  const remove = useDownloads((s) => s.remove);
  return (
    <Menu.Item
      textValue={d.name}
      onSelect={(e) => {
        // A deleted file stays in the list, highlighted like the others, so it can be removed.
        if (d.exists) void onFile(() => api.openDownload(d.id));
        else e.preventDefault();
      }}
      onKeyDown={(e) => {
        if (e.key === "Delete") void remove(d.id);
      }}
      {...(d.exists ? fileDrag(() => api.dragDownload(d.id)) : {})}
      className="group flex h-11 cursor-default select-none items-center gap-2.5 rounded pl-2 pr-1 outline-none data-[highlighted]:bg-hover"
    >
      <FileText size={16} className={cn("shrink-0", d.exists ? "text-muted" : "text-faint")} />
      <div className="min-w-0 flex-1 leading-tight">
        <div className={cn("truncate text-[13px]", !d.exists && "text-muted")} title={d.name}>
          {d.name}
        </div>
        <div className="flex gap-2 text-[11px] text-muted">
          {d.exists ? <span>{fileSize(d.size, i18n.language)}</span> : <span>{t("downloads.gone")}</span>}
          <span className="text-faint">{listDate(d.createdAt, i18n.language)}</span>
        </div>
      </div>
      <div className="hidden items-center group-hover:flex group-data-[highlighted]:flex">
        {d.exists && (
          <EntryButton label={t("downloads.reveal")} onClick={() => void onFile(() => api.revealDownload(d.id))}>
            <FolderOpen size={14} />
          </EntryButton>
        )}
        <EntryButton label={t("downloads.remove")} onClick={() => void remove(d.id)}>
          <X size={14} />
        </EntryButton>
      </div>
    </Menu.Item>
  );
}

/** The download list behind a button in the title bar: open a file, show it in Explorer, or drag
 * it into another app. */
export function DownloadsButton() {
  const { t } = useTranslation();
  const list = useDownloads((s) => s.list);
  const clear = useDownloads((s) => s.clear);
  return (
    <Menu.Root
      onOpenChange={(open) => {
        // Files deleted in Explorer meanwhile show as gone.
        if (open) void useDownloads.getState().refresh();
      }}
    >
      <Menu.Trigger asChild>
        <button
          type="button"
          tabIndex={-1}
          aria-label={t("downloads.title")}
          title={t("downloads.title")}
          className="flex h-full w-[46px] items-center justify-center text-fg outline-none transition-colors duration-100 hover:bg-black/[0.06] active:bg-black/[0.04] data-[state=open]:bg-black/[0.06] group-data-[inactive]:text-faint dark:hover:bg-white/[0.06] dark:active:bg-white/[0.04] dark:data-[state=open]:bg-white/[0.06]"
        >
          <DownloadIcon size={14} strokeWidth={1.75} />
        </button>
      </Menu.Trigger>
      <Menu.Portal>
        <Menu.Content
          align="end"
          sideOffset={4}
          collisionPadding={8}
          className="z-50 w-80 rounded-md border border-line bg-surface p-1 shadow-[0_2px_8px_rgba(0,0,0,0.1)]"
        >
          <Menu.Label className="px-2 pb-1 pt-1.5 text-xs font-semibold text-muted">{t("downloads.title")}</Menu.Label>
          {list.length ? (
            <div className="max-h-[60vh] overflow-y-auto">
              {list.map((d) => (
                <Entry key={d.id} d={d} />
              ))}
            </div>
          ) : (
            <div className="px-2 pb-5 pt-4 text-center text-[13px] text-muted">{t("downloads.empty")}</div>
          )}
          {list.length > 0 && (
            <>
              <Menu.Separator className="-mx-1 my-1 h-px bg-line" />
              <Menu.Item
                onSelect={() => void clear()}
                className="flex h-7 cursor-default select-none items-center rounded px-2 text-[13px] outline-none data-[highlighted]:bg-hover"
              >
                {t("downloads.clear")}
              </Menu.Item>
            </>
          )}
        </Menu.Content>
      </Menu.Portal>
    </Menu.Root>
  );
}
