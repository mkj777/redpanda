import {
  AlertCircle,
  Archive,
  ChevronDown,
  ChevronRight,
  CloudOff,
  FileText,
  Folder as FolderIcon,
  Inbox,
  Mail,
  OctagonAlert,
  PenSquare,
  Plus,
  RefreshCw,
  Send,
  Settings,
  Star,
  Trash2,
  type LucideIcon,
} from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { Account, Folder, FolderRole, View } from "@/lib/ipc";
import { cn } from "@/lib/format";
import { compose } from "@/lib/actions";
import { useMail } from "@/stores/mail";
import { useUi } from "@/stores/ui";
import { Button, IconButton } from "./ui";

const roleIcons: Partial<Record<FolderRole, LucideIcon>> = {
  inbox: Inbox,
  sent: Send,
  drafts: FileText,
  trash: Trash2,
  junk: OctagonAlert,
  archive: Archive,
};

export function folderLabel(f: Folder, t: (k: string) => string) {
  return f.role !== "other" && roleIcons[f.role] ? t(`folder.${f.role}`) : f.name;
}

function isActive(view: View, target: View) {
  return JSON.stringify(view) === JSON.stringify(target);
}

function Row({
  icon: Icon,
  label,
  count,
  active,
  depth = 0,
  onClick,
  collapsed,
  title,
}: {
  icon: LucideIcon;
  label: string;
  count?: number;
  active: boolean;
  depth?: number;
  onClick: () => void;
  collapsed: boolean;
  title?: string;
}) {
  if (collapsed) {
    return (
      <IconButton label={count ? `${label} (${count})` : label} onClick={onClick} className={cn("mx-auto", active && "bg-selected text-fg")}>
        <Icon size={17} />
      </IconButton>
    );
  }
  return (
    <button
      onClick={onClick}
      title={title}
      style={{ paddingLeft: 8 + depth * 14 }}
      className={cn(
        "flex h-7 w-full items-center gap-2 rounded-md pr-2 text-left text-[13px] transition-colors duration-100",
        active ? "bg-selected font-medium text-fg" : "text-fg/85 hover:bg-hover",
      )}
    >
      <Icon size={15} className={cn("shrink-0", active ? "text-accent" : "text-muted")} />
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {!!count && <span className={cn("text-xs tabular-nums", active ? "text-fg" : "text-muted")}>{count}</span>}
    </button>
  );
}

function StatusIcon({ account }: { account: Account }) {
  const { t } = useTranslation();
  const setSettingsOpen = useUi((s) => s.setSettingsOpen);
  const label = t(`status.${account.status}`) + (account.statusError ? `: ${account.statusError}` : "");
  switch (account.status) {
    case "auth_required":
      return (
        <IconButton label={label} onClick={() => setSettingsOpen(true)} className="size-6 !text-danger">
          <AlertCircle size={14} />
        </IconButton>
      );
    case "offline":
    case "error":
      return (
        <span title={label} className="text-faint">
          <CloudOff size={13} />
        </span>
      );
    case "syncing":
      return (
        <span title={label} className="text-faint">
          <RefreshCw size={12} className="animate-spin [animation-duration:2s]" />
        </span>
      );
    default:
      return null;
  }
}

export function Sidebar({ collapsed }: { collapsed: boolean }) {
  const { t } = useTranslation();
  const { accounts, folders, unified, view, setView } = useMail();
  const setWizardOpen = useUi((s) => s.setWizardOpen);
  const setSettingsOpen = useUi((s) => s.setSettingsOpen);
  const [closed, setClosed] = useState<Record<string, boolean>>(() => {
    try {
      return JSON.parse(localStorage.getItem("redpanda-closed-accounts") ?? "{}");
    } catch {
      return {};
    }
  });
  const toggle = (id: string) => {
    const next = { ...closed, [id]: !closed[id] };
    setClosed(next);
    try {
      localStorage.setItem("redpanda-closed-accounts", JSON.stringify(next));
    } catch {
      /* storage unavailable */
    }
  };

  const unifiedRows: { view: View; icon: LucideIcon; label: string; count: number; hide?: boolean }[] = [
    { view: { kind: "unified", which: "starred" }, icon: Star, label: t("nav.starred"), count: 0 },
    { view: { kind: "unified", which: "inbox" }, icon: Inbox, label: t("nav.allInboxes"), count: unified.inboxUnread },
    { view: { kind: "unified", which: "unread" }, icon: Mail, label: t("nav.unread"), count: unified.unread },
    { view: { kind: "drafts" }, icon: FileText, label: t("nav.drafts"), count: unified.drafts },
    { view: { kind: "unified", which: "sent" }, icon: Send, label: t("nav.allSent"), count: 0 },
    { view: { kind: "unified", which: "junk" }, icon: OctagonAlert, label: t("nav.allJunk"), count: 0 },
    { view: { kind: "unified", which: "trash" }, icon: Trash2, label: t("nav.allTrash"), count: 0 },
  ];

  return (
    <nav className="flex h-full flex-col bg-sidebar" aria-label={t("nav.label")}>
      <div className={cn("flex h-12 shrink-0 items-center border-b border-line", collapsed ? "justify-center" : "px-3")}>
        {collapsed ? (
          <IconButton label={t("nav.compose")} shortcut="C" onClick={() => compose()}>
            <PenSquare size={17} />
          </IconButton>
        ) : (
          <Button variant="soft" className="w-full" onClick={() => compose()}>
            <PenSquare size={15} />
            {t("nav.compose")}
          </Button>
        )}
      </div>

      <div className={cn("min-h-0 flex-1 overflow-y-auto py-2", collapsed ? "flex flex-col gap-1 px-1" : "px-2")}>
        {unifiedRows
          .filter((r) => !r.hide)
          .map((r) => (
            <Row
              key={r.label}
              icon={r.icon}
              label={r.label}
              count={r.count}
              active={isActive(view, r.view)}
              onClick={() => setView(r.view)}
              collapsed={collapsed}
            />
          ))}

        {accounts.map((a) => {
          const own = folders.filter((f) => f.accountId === a.id);
          const inbox = own.find((f) => f.role === "inbox");
          if (collapsed) {
            return (
              <IconButton
                key={a.id}
                label={a.email}
                className="mx-auto mt-2"
                onClick={() => inbox && setView({ kind: "folder", folderId: inbox.id })}
              >
                <span className="size-2.5 rounded-full" style={{ background: a.color }} />
              </IconButton>
            );
          }
          const isClosed = closed[a.id];
          return (
            <div key={a.id} className="mt-4">
              <div className="group flex h-7 items-center gap-1 pl-1 pr-1">
                <button
                  onClick={() => toggle(a.id)}
                  className="flex min-w-0 flex-1 items-center gap-1.5 text-left text-xs font-medium text-muted hover:text-fg"
                >
                  {isClosed ? <ChevronRight size={13} /> : <ChevronDown size={13} />}
                  <span className="size-2 shrink-0 rounded-full" style={{ background: a.color }} />
                  <span className="truncate" title={a.email}>
                    {a.email}
                  </span>
                </button>
                <StatusIcon account={a} />
              </div>
              {!isClosed &&
                own.map((f) => (
                  <Row
                    key={f.id}
                    icon={roleIcons[f.role] ?? FolderIcon}
                    label={folderLabel(f, t)}
                    title={f.path}
                    count={f.role === "sent" || f.role === "drafts" || f.role === "trash" ? 0 : f.unreadCount}
                    depth={f.depth}
                    active={isActive(view, { kind: "folder", folderId: f.id })}
                    onClick={() => setView({ kind: "folder", folderId: f.id })}
                    collapsed={false}
                  />
                ))}
            </div>
          );
        })}
      </div>

      <div className={cn("flex shrink-0 border-t border-line p-2", collapsed ? "flex-col items-center gap-1" : "items-center gap-1")}>
        {collapsed ? (
          <IconButton label={t("nav.addAccount")} onClick={() => setWizardOpen(true)}>
            <Plus size={17} />
          </IconButton>
        ) : (
          <Button variant="ghost" size="sm" className="flex-1 justify-start text-muted" onClick={() => setWizardOpen(true)}>
            <Plus size={15} />
            {t("nav.addAccount")}
          </Button>
        )}
        <IconButton label={t("nav.settings")} onClick={() => setSettingsOpen(true)}>
          <Settings size={16} />
        </IconButton>
      </div>
    </nav>
  );
}
