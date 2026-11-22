import * as Ctx from "@radix-ui/react-context-menu";
import {
  Archive,
  ChevronRight,
  FolderInput,
  Forward,
  Mail,
  MailOpen,
  Reply,
  ReplyAll,
  ShieldAlert,
  ShieldCheck,
  Star,
  Trash2,
} from "lucide-react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useShallow } from "zustand/react/shallow";
import type { MessageSummary } from "@/lib/ipc";
import { cn } from "@/lib/format";
import { archive, markJunk, moveTo, remove, reply, spamAction, toggleFlagged, toggleSeen } from "@/lib/actions";
import { useMail } from "@/stores/mail";
import { folderLabel } from "./Sidebar";

const panel = "z-50 min-w-52 rounded-md border border-line bg-surface p-1 shadow-[0_2px_8px_rgba(0,0,0,0.1)]";
const row =
  "flex h-7 cursor-default select-none items-center gap-2.5 rounded px-2 text-[13px] outline-none data-[highlighted]:bg-hover";

function Item({ icon, label, onSelect }: { icon: ReactNode; label: string; onSelect: () => void }) {
  return (
    <Ctx.Item onSelect={onSelect} className={row}>
      <span className="flex w-4 justify-center text-muted">{icon}</span>
      {label}
    </Ctx.Item>
  );
}

function Separator() {
  return <Ctx.Separator className="-mx-1 my-1 h-px bg-line" />;
}

function MoveSub({ m }: { m: MessageSummary }) {
  const { t } = useTranslation();
  const folders = useMail(
    useShallow((s) => s.folders.filter((f) => f.accountId === m.accountId && f.id !== m.folderId)),
  );
  if (!folders.length) return null;
  return (
    <Ctx.Sub>
      <Ctx.SubTrigger className={cn(row, "data-[state=open]:bg-hover")}>
        <span className="flex w-4 justify-center text-muted">
          <FolderInput size={14} />
        </span>
        <span className="flex-1">{t("action.moveTo")}</span>
        <ChevronRight size={14} className="text-muted" />
      </Ctx.SubTrigger>
      <Ctx.Portal>
        <Ctx.SubContent sideOffset={4} collisionPadding={8} className={cn(panel, "max-h-80 overflow-y-auto")}>
          {folders.map((f) => (
            <Ctx.Item
              key={f.id}
              onSelect={() => moveTo([m], f.id)}
              style={{ paddingLeft: 8 + f.depth * 12 }}
              className={cn(row, "pr-3")}
            >
              {folderLabel(f, t)}
            </Ctx.Item>
          ))}
        </Ctx.SubContent>
      </Ctx.Portal>
    </Ctx.Sub>
  );
}

/** The right-click menu of a message in the list. It acts on the message clicked, which need
 * not be the one that is open. */
export function MessageContextMenu({ m, children }: { m: MessageSummary; children: ReactNode }) {
  const { t } = useTranslation();
  const role = useMail((s) => s.folders.find((f) => f.id === m.folderId)?.role);
  const spam = spamAction(role);
  return (
    // Not modal, so a right-click on another message opens its menu straight away.
    <Ctx.Root modal={false}>
      <Ctx.Trigger asChild>{children}</Ctx.Trigger>
      <Ctx.Portal>
        <Ctx.Content collisionPadding={8} className={panel}>
          <Item icon={<Reply size={14} />} label={t("action.reply")} onSelect={() => reply("reply", m)} />
          <Item icon={<ReplyAll size={14} />} label={t("action.replyAll")} onSelect={() => reply("replyAll", m)} />
          <Item icon={<Forward size={14} />} label={t("action.forward")} onSelect={() => reply("forward", m)} />
          <Separator />
          <Item
            icon={m.seen ? <Mail size={14} /> : <MailOpen size={14} />}
            label={m.seen ? t("action.markUnread") : t("action.markRead")}
            onSelect={() => toggleSeen([m])}
          />
          <Item
            icon={<Star size={14} className={cn(m.flagged && "fill-current")} />}
            label={m.flagged ? t("action.unstar") : t("action.star")}
            onSelect={() => toggleFlagged([m])}
          />
          <Separator />
          <Item icon={<Archive size={14} />} label={t("action.archive")} onSelect={() => archive([m])} />
          <MoveSub m={m} />
          {spam && (
            <Item
              icon={spam === "spam" ? <ShieldAlert size={14} /> : <ShieldCheck size={14} />}
              label={t(`action.${spam}`)}
              onSelect={() => markJunk([m], spam === "spam")}
            />
          )}
          <Item icon={<Trash2 size={14} />} label={t("action.delete")} onSelect={() => remove([m])} />
        </Ctx.Content>
      </Ctx.Portal>
    </Ctx.Root>
  );
}
