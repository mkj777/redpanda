import { useVirtualizer } from "@tanstack/react-virtual";
import { Archive, MailOpen, Mail, Paperclip, RefreshCw, Search, Star, Trash2, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, type Draft, type MessageSummary, type UnifiedView } from "@/lib/ipc";
import { cn, displayName, listDate } from "@/lib/format";
import { archive, remove, toggleSeen } from "@/lib/actions";
import { useMail } from "@/stores/mail";
import { useUi } from "@/stores/ui";
import { folderLabel } from "./Sidebar";
import { IconButton, Spinner } from "./ui";

const ROW = 76;

const UNIFIED_TITLES: Record<UnifiedView, string> = {
  inbox: "nav.allInboxes",
  unread: "nav.unread",
  starred: "nav.starred",
  sent: "nav.allSent",
  junk: "nav.allJunk",
  trash: "nav.allTrash",
};

function useViewTitle() {
  const { t } = useTranslation();
  const { view, folders, accounts } = useMail();
  switch (view.kind) {
    case "unified":
      return { title: t(UNIFIED_TITLES[view.which]), sub: null };
    case "drafts":
      return { title: t("nav.drafts"), sub: null };
    case "search":
      return { title: t("list.results"), sub: null };
    case "folder": {
      const f = folders.find((x) => x.id === view.folderId);
      const a = accounts.find((x) => x.id === f?.accountId);
      return { title: f ? folderLabel(f, t) : "", sub: a?.email ?? null };
    }
  }
}

function MessageRow({
  m,
  selected,
  showAccount,
  showRecipient,
  onSelect,
}: {
  m: MessageSummary;
  selected: boolean;
  showAccount: boolean;
  showRecipient: boolean;
  onSelect: () => void;
}) {
  const { t, i18n } = useTranslation();
  const color = useMail((s) => s.accounts.find((a) => a.id === m.accountId)?.color);
  const who = showRecipient
    ? `${t("list.toPrefix")} ${m.to.map(displayName).join(", ") || t("list.noRecipient")}`
    : displayName(m.from) || t("list.unknownSender");
  return (
    <div
      role="option"
      aria-selected={selected}
      onClick={onSelect}
      className={cn(
        "group relative flex h-full cursor-default flex-col justify-center gap-0.5 border-b border-line px-4",
        selected ? "bg-selected" : "hover:bg-hover",
      )}
    >
      {!m.seen && <span className="absolute inset-y-2 left-0 w-[3px] rounded-r bg-accent" aria-hidden />}
      <div className="flex items-center gap-2">
        {showAccount && color && <span className="size-2 shrink-0 rounded-full" style={{ background: color }} />}
        <span className={cn("min-w-0 flex-1 truncate text-[13px]", m.seen ? "text-fg/80" : "font-semibold text-fg")}>
          {who}
        </span>
        {m.threadCount > 1 && <span className="text-xs tabular-nums text-muted">{m.threadCount}</span>}
        {m.hasAttachments && <Paperclip size={13} className="shrink-0 text-muted" />}
        {m.flagged && <Star size={13} className="shrink-0 fill-accent text-accent" />}
        <span className="shrink-0 text-xs tabular-nums text-muted group-hover:invisible">
          {listDate(m.date, i18n.language)}
        </span>
      </div>
      <div className={cn("truncate text-[13px]", m.seen ? "text-fg/80" : "font-medium text-fg")}>
        {m.subject || t("list.noSubject")}
      </div>
      <div className="truncate text-[13px] text-muted">{m.preview}</div>
      <div
        className="absolute right-2 top-1.5 hidden items-center rounded-md border border-line bg-surface group-hover:flex"
        onClick={(e) => e.stopPropagation()}
      >
        <IconButton label={t("action.archive")} className="size-7" onClick={() => archive([m])}>
          <Archive size={14} />
        </IconButton>
        <IconButton label={t("action.delete")} className="size-7" onClick={() => remove([m])}>
          <Trash2 size={14} />
        </IconButton>
        <IconButton label={m.seen ? t("action.markUnread") : t("action.markRead")} className="size-7" onClick={() => toggleSeen([m])}>
          {m.seen ? <Mail size={14} /> : <MailOpen size={14} />}
        </IconButton>
      </div>
    </div>
  );
}

function DraftList() {
  const { t, i18n } = useTranslation();
  const drafts = useMail((s) => s.drafts);
  const openComposer = useUi((s) => s.openComposer);
  if (!drafts.length) return <Empty text={t("list.draftsEmpty")} />;
  return (
    <div className="min-h-0 flex-1 overflow-y-auto">
      {drafts.map((d: Draft) => (
        <div
          key={d.id ?? 0}
          onClick={() => openComposer("draft", d)}
          className="group flex h-[76px] cursor-default flex-col justify-center gap-0.5 border-b border-line px-4 hover:bg-hover"
        >
          <div className="flex items-center gap-2">
            <span className="min-w-0 flex-1 truncate text-[13px] text-fg">
              {d.to.map(displayName).join(", ") || t("list.noRecipient")}
            </span>
            <span className="text-xs text-muted group-hover:hidden">{listDate(d.updatedAt ?? 0, i18n.language)}</span>
            <IconButton
              label={t("compose.discard")}
              className="hidden size-6 group-hover:inline-flex"
              onClick={async (e) => {
                e.stopPropagation();
                if (d.id) await api.deleteDraft(d.id);
                await Promise.all([useMail.getState().refreshList(), useMail.getState().refreshFolders()]);
              }}
            >
              <X size={14} />
            </IconButton>
          </div>
          <div className="truncate text-[13px]">{d.subject || t("list.noSubject")}</div>
          <div className="truncate text-[13px] text-muted">{d.body.trim().slice(0, 160)}</div>
        </div>
      ))}
    </div>
  );
}

function Empty({ text }: { text: string }) {
  return <div className="flex flex-1 items-center justify-center p-8 text-center text-sm text-muted">{text}</div>;
}

export function MessageList() {
  const { t } = useTranslation();
  const { view, messages, hasMore, serverHasOlder, loading, loadingOlder, selectedId, select, loadMore, search, accounts, folders } =
    useMail();
  const [query, setQuery] = useState(view.kind === "search" ? view.query : "");
  const parentRef = useRef<HTMLDivElement>(null);
  const { title, sub } = useViewTitle();

  useEffect(() => {
    if (view.kind !== "search") setQuery("");
  }, [view]);

  useEffect(() => {
    const id = setTimeout(() => search(query), 200);
    return () => clearTimeout(id);
  }, [query, search]);

  const virtualizer = useVirtualizer({
    count: messages.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => ROW,
    overscan: 8,
  });

  const items = virtualizer.getVirtualItems();
  const lastIndex = items.length ? items[items.length - 1].index : -1;
  const canLoad = hasMore || (view.kind === "folder" && serverHasOlder);
  useEffect(() => {
    if (canLoad && !loading && lastIndex >= messages.length - 8 && messages.length > 0) void loadMore();
  }, [lastIndex, messages.length, canLoad, loading, loadMore]);

  const selectedIndex = useMemo(() => messages.findIndex((m) => m.id === selectedId), [messages, selectedId]);
  useEffect(() => {
    if (selectedIndex >= 0) virtualizer.scrollToIndex(selectedIndex, { align: "auto" });
  }, [selectedIndex, virtualizer]);

  const multiAccount = accounts.length > 1 && view.kind !== "folder";
  const folderRole = view.kind === "folder" ? folders.find((f) => f.id === view.folderId)?.role : undefined;
  const showRecipient = folderRole === "sent" || folderRole === "drafts";

  return (
    <section className="flex h-full flex-col bg-surface" aria-label={title}>
      <div className="drag flex h-12 shrink-0 items-center gap-2 border-b border-line px-3">
        <div className="relative flex-1">
          <Search size={14} className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-faint" />
          <input
            id="search-input"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") {
                setQuery("");
                (e.target as HTMLInputElement).blur();
              }
            }}
            placeholder={t("list.searchPlaceholder")}
            className="h-8 w-full rounded-md border border-line bg-bg pl-8 pr-7 text-sm outline-none placeholder:text-faint focus:border-accent focus-visible:outline-none"
          />
          {query && (
            <button
              aria-label={t("list.clearSearch")}
              onClick={() => setQuery("")}
              className="absolute right-1.5 top-1/2 -translate-y-1/2 rounded p-0.5 text-faint hover:text-fg"
            >
              <X size={14} />
            </button>
          )}
        </div>
      </div>
      <div className="flex h-9 shrink-0 items-center gap-2 border-b border-line pl-4 pr-2">
        <span className="truncate text-[13px] font-semibold">{title}</span>
        {sub && <span className="truncate text-xs text-muted">{sub}</span>}
        <span className="flex-1" />
        <IconButton
          label={t("list.refresh")}
          className="size-7"
          onClick={() => {
            const accountId = view.kind === "folder" ? folders.find((f) => f.id === view.folderId)?.accountId : undefined;
            void api.syncNow(accountId);
          }}
        >
          <RefreshCw size={14} />
        </IconButton>
      </div>

      {view.kind === "drafts" ? (
        <DraftList />
      ) : loading && !messages.length ? (
        <div className="flex flex-1 items-center justify-center">
          <Spinner />
        </div>
      ) : !messages.length ? (
        <Empty text={view.kind === "search" ? t("list.emptySearch", { query: view.query }) : t("list.empty")} />
      ) : (
        <div ref={parentRef} role="listbox" aria-label={title} className="min-h-0 flex-1 overflow-y-auto">
          <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
            {items.map((row) => {
              const m = messages[row.index];
              return (
                <div
                  key={m.id}
                  style={{ position: "absolute", top: 0, left: 0, right: 0, height: ROW, transform: `translateY(${row.start}px)` }}
                >
                  <MessageRow
                    m={m}
                    selected={m.id === selectedId}
                    showAccount={multiAccount}
                    showRecipient={showRecipient}
                    onSelect={() => select(m.id)}
                  />
                </div>
              );
            })}
          </div>
          {loadingOlder && (
            <div className="flex items-center justify-center gap-2 py-3 text-xs text-muted">
              <Spinner className="size-3" />
              {t("list.loadingOlder")}
            </div>
          )}
        </div>
      )}
    </section>
  );
}
