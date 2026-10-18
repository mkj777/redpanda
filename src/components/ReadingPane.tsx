import * as Menu from "@radix-ui/react-dropdown-menu";
import {
  Archive,
  Download,
  ExternalLink,
  FileText,
  Forward,
  FolderInput,
  ImageOff,
  Mail,
  Paperclip,
  Reply,
  ReplyAll,
  Star,
  Trash2,
} from "lucide-react";
import { useEffect, useState } from "react";
import { useShallow } from "zustand/react/shallow";
import { useTranslation } from "react-i18next";
import { api, pickSavePath, type MessageDetail, type MessageSummary } from "@/lib/ipc";
import { cn, displayName, fileSize, fullDate } from "@/lib/format";
import { archive, markSeen, moveTo, remove, reply, reportError, toggleFlagged, toggleSeen } from "@/lib/actions";
import { ownAddresses } from "@/lib/reply";
import { useMail } from "@/stores/mail";
import { useSettings } from "@/stores/settings";
import { HtmlBody, TextBody } from "./MessageBody";
import { folderLabel } from "./Sidebar";
import { Avatar, Button, IconButton, Spinner } from "./ui";
import { UnsubscribeLink } from "./Unsubscribe";

function Attachments({ msg }: { msg: MessageDetail }) {
  const { t, i18n } = useTranslation();
  const files = msg.attachments.filter((a) => !a.inline);
  if (!files.length) return null;
  return (
    <div className="mt-4">
      <div className="mb-2 flex items-center gap-1.5 text-xs text-muted">
        <Paperclip size={13} />
        {t("reader.attachments", { count: files.length })}
      </div>
      <div className="flex flex-wrap gap-2">
        {files.map((a) => (
          <div key={a.index} className="flex h-10 max-w-72 items-center gap-2 rounded-md border border-line bg-surface pl-2.5 pr-1">
            <FileText size={16} className="shrink-0 text-muted" />
            <div className="min-w-0 flex-1 leading-tight">
              <div className="truncate text-[13px]" title={a.filename}>
                {a.filename}
              </div>
              <div className="text-[11px] text-muted">{fileSize(a.size, i18n.language)}</div>
            </div>
            <IconButton
              label={t("reader.open")}
              className="size-7"
              onClick={() => api.openAttachment(msg.id, a.index).catch(reportError)}
            >
              <ExternalLink size={14} />
            </IconButton>
            <IconButton
              label={t("reader.save")}
              className="size-7"
              onClick={async () => {
                const path = await pickSavePath(a.filename);
                if (path) await api.saveAttachment(msg.id, a.index, path).catch(reportError);
              }}
            >
              <Download size={14} />
            </IconButton>
          </div>
        ))}
      </div>
    </div>
  );
}

function RemoteBar({ msg, onAllow }: { msg: MessageDetail; onAllow: () => void }) {
  const { t } = useTranslation();
  return (
    <div className="mb-3 flex flex-wrap items-center gap-2 rounded-md border border-line bg-bg px-3 py-2 text-[13px] text-muted">
      <ImageOff size={14} className="shrink-0" />
      <span className="flex-1">{t("reader.remoteBlocked")}</span>
      <Button size="sm" onClick={onAllow}>
        {t("reader.loadImages")}
      </Button>
      {msg.from && (
        <Button
          size="sm"
          variant="ghost"
          onClick={async () => {
            await api.allowRemoteContent(msg.from!.email).catch(reportError);
            onAllow();
          }}
        >
          {t("reader.alwaysFrom", { sender: msg.from.email })}
        </Button>
      )}
    </div>
  );
}

export function MessageView({
  summary,
  isLast,
  unsubscribeLink = true,
}: {
  summary: MessageSummary;
  isLast: boolean;
  /** Off where the page around it offers unsubscribing already. */
  unsubscribeLink?: boolean;
}) {
  const { t, i18n } = useTranslation();
  const revision = useMail((s) => s.revision);
  const ownEmails = useMail(useShallow((s) => ownAddresses(s.accounts).map((e) => e.toLowerCase())));
  const [msg, setMsg] = useState<MessageDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [allowed, setAllowed] = useState(false);
  const autoLoad = useSettings((s) => s.autoLoadImages);
  const inJunk = useMail((s) => s.folders.find((f) => f.id === summary.folderId)?.role === "junk");
  const remote = allowed || (autoLoad && !inJunk);

  useEffect(() => {
    let alive = true;
    setError(null);
    api
      .getMessage(summary.id)
      .then((m) => {
        if (!alive) return;
        setMsg(m);
        // Never re-block images the user already allowed for this message.
        setAllowed((r) => r || m.remoteAllowed);
      })
      .catch((e) => alive && setError(String(e?.message ?? e)));
    return () => {
      alive = false;
    };
    // Refetch when the list refreshes, flags may have changed.
  }, [summary.id, revision]);

  if (error) return <div className="py-6 text-sm text-danger">{error}</div>;
  if (!msg)
    return (
      <div className="flex items-center gap-2 py-6 text-sm text-muted">
        <Spinner className="size-3.5" />
        {t("reader.loading")}
      </div>
    );

  const nameOrMe = (email: string, name: string | null) =>
    ownEmails.includes(email.toLowerCase()) ? t("reader.me") : name || email;
  const recipients = [...msg.to, ...msg.cc].map((a) => nameOrMe(a.email, a.name)).join(", ");

  return (
    <article className="py-4">
      <header className="mb-4 flex items-start gap-3">
        <Avatar address={msg.from} />
        <div className="min-w-0 flex-1">
          <div className="flex items-baseline gap-2">
            <span className="truncate text-sm font-semibold">{displayName(msg.from) || t("list.unknownSender")}</span>
            {msg.from?.name && <span className="selectable truncate text-xs text-muted">{msg.from.email}</span>}
            {unsubscribeLink && <UnsubscribeLink msg={msg} />}
          </div>
          <div className="truncate text-xs text-muted" title={[...msg.to, ...msg.cc].map((a) => a.email).join(", ")}>
            {t("reader.to", { list: recipients || t("list.noRecipient") })}
          </div>
        </div>
        <span className="shrink-0 pt-0.5 text-xs text-muted">{fullDate(msg.date, i18n.language)}</span>
        {!isLast && (
          <IconButton label={t("action.reply")} className="-mt-1 size-7" onClick={() => reply("reply", msg)}>
            <Reply size={14} />
          </IconButton>
        )}
      </header>
      {msg.html && msg.hasRemoteContent && !remote && <RemoteBar msg={msg} onAllow={() => setAllowed(true)} />}
      {msg.html ? <HtmlBody html={msg.html} allowRemote={remote} /> : <TextBody text={msg.text ?? ""} />}
      <Attachments msg={msg} />
    </article>
  );
}

function CollapsedMessage({ m, onOpen }: { m: MessageSummary; onOpen: () => void }) {
  const { i18n, t } = useTranslation();
  return (
    <button onClick={onOpen} className="flex w-full items-center gap-3 border-b border-line py-2.5 text-left hover:bg-hover/60">
      <span className="w-40 shrink-0 truncate text-[13px] font-medium">{displayName(m.from) || t("list.unknownSender")}</span>
      <span className="min-w-0 flex-1 truncate text-[13px] text-muted">{m.preview}</span>
      <span className="shrink-0 text-xs text-muted">{fullDate(m.date, i18n.language)}</span>
    </button>
  );
}

function MoveMenu({ summary }: { summary: MessageSummary }) {
  const { t } = useTranslation();
  const folders = useMail(
    useShallow((s) => s.folders.filter((f) => f.accountId === summary.accountId && f.id !== summary.folderId)),
  );
  return (
    <Menu.Root>
      <Menu.Trigger asChild>
        <IconButton label={t("action.move")}>
          <FolderInput size={16} />
        </IconButton>
      </Menu.Trigger>
      <Menu.Portal>
        <Menu.Content
          align="end"
          sideOffset={4}
          className="z-50 max-h-80 min-w-48 overflow-y-auto rounded-md border border-line bg-surface p-1 shadow-[0_2px_8px_rgba(0,0,0,0.1)]"
        >
          {folders.map((f) => (
            <Menu.Item
              key={f.id}
              onSelect={() => moveTo([summary], f.id)}
              style={{ paddingLeft: 8 + f.depth * 12 }}
              className="flex h-7 cursor-default items-center rounded pr-3 text-[13px] outline-none data-[highlighted]:bg-hover"
            >
              {folderLabel(f, t)}
            </Menu.Item>
          ))}
        </Menu.Content>
      </Menu.Portal>
    </Menu.Root>
  );
}

export function ReadingPane() {
  const { t } = useTranslation();
  const selectedId = useMail((s) => s.selectedId);
  const summary = useMail((s) => s.messages.find((m) => m.id === s.selectedId) ?? null);
  const threads = useSettings((s) => s.threads);
  // In the unread view a read message drops out of the list, so it is only marked once the user moves on.
  const markOnLeave = useMail((s) => s.view.kind === "unified" && s.view.which === "unread");
  const [thread, setThread] = useState<MessageSummary[]>([]);
  const [expanded, setExpanded] = useState<Set<number>>(new Set());

  const threadKey = summary ? `${summary.accountId}:${summary.threadKey}:${summary.threadCount}` : null;
  useEffect(() => {
    if (!summary) return;
    let alive = true;
    const load = threads && summary.threadCount > 1 ? api.getThread(summary.id) : Promise.resolve([summary]);
    load
      .then((list) => {
        if (!alive) return;
        const msgs = list.length ? list : [summary];
        setThread(msgs);
        const unread = msgs.filter((m) => !m.seen).map((m) => m.id);
        setExpanded(new Set([msgs[msgs.length - 1].id, ...unread]));
      })
      .catch(reportError);
    const timer = markOnLeave ? undefined : setTimeout(() => markSeen(summary), 400);
    return () => {
      alive = false;
      clearTimeout(timer);
      // Runs on every reload of the conversation too, but marks only when another message (or none) is selected.
      if (markOnLeave && useMail.getState().selectedId !== summary.id) void markSeen(summary);
    };
    // Reload on a different selection or a changed conversation.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedId, threadKey, threads]);

  if (!summary) {
    return (
      <section className="flex h-full items-center justify-center bg-bg text-sm text-muted">
        <span className="flex items-center gap-2">
          <Mail size={16} />
          {t("reader.empty")}
        </span>
      </section>
    );
  }

  const latest = thread[thread.length - 1] ?? summary;
  return (
    <section className="flex h-full flex-col bg-bg">
      <div className="flex h-12 shrink-0 items-center gap-0.5 border-b border-line bg-surface px-2">
        <IconButton label={t("action.reply")} shortcut="R" onClick={() => reply("reply", latest)}>
          <Reply size={16} />
        </IconButton>
        <IconButton label={t("action.replyAll")} shortcut="A" onClick={() => reply("replyAll", latest)}>
          <ReplyAll size={16} />
        </IconButton>
        <IconButton label={t("action.forward")} shortcut="F" onClick={() => reply("forward", latest)}>
          <Forward size={16} />
        </IconButton>
        <span className="mx-1.5 h-5 w-px bg-line" />
        <IconButton label={t("action.archive")} shortcut="E" onClick={() => archive([summary])}>
          <Archive size={16} />
        </IconButton>
        <IconButton label={t("action.delete")} shortcut="#" onClick={() => remove([summary])}>
          <Trash2 size={16} />
        </IconButton>
        <MoveMenu summary={summary} />
        <span className="flex-1" />
        <IconButton
          label={summary.flagged ? t("action.unstar") : t("action.star")}
          shortcut="S"
          active={summary.flagged}
          onClick={() => toggleFlagged([summary])}
        >
          <Star size={16} className={cn(summary.flagged && "fill-accent")} />
        </IconButton>
        <IconButton label={summary.seen ? t("action.markUnread") : t("action.markRead")} shortcut="U" onClick={() => toggleSeen([summary])}>
          <Mail size={16} />
        </IconButton>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="mx-auto max-w-[860px] px-6 pb-10">
          <h1 className="selectable pt-5 text-lg font-semibold leading-snug">{summary.subject || t("list.noSubject")}</h1>
          {thread.map((m, i) =>
            expanded.has(m.id) ? (
              <div key={m.id} className={cn(i < thread.length - 1 && "border-b border-line")}>
                <MessageView summary={m} isLast={i === thread.length - 1} />
              </div>
            ) : (
              <CollapsedMessage key={m.id} m={m} onOpen={() => setExpanded(new Set([...expanded, m.id]))} />
            ),
          )}
        </div>
      </div>
    </section>
  );
}
