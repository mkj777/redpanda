// Message actions shared by the list, the reading pane and keyboard shortcuts.

import i18n from "@/i18n";
import { api, toAppError, type AppError, type MessageDetail, type MessageSummary } from "./ipc";
import { buildReply, ownAddresses, type ReplyMode } from "./reply";
import { displayName, fullDate } from "./format";
import { useMail } from "@/stores/mail";
import { useSettings } from "@/stores/settings";
import { useUi, type UnsubscribeRequest } from "@/stores/ui";

export function errorText(e: unknown): string {
  const err: AppError = toAppError(e);
  const key = `errors.${err.code}`;
  if (err.code === "invalid" || !i18n.exists(key)) {
    if (err.message === "blocked_file_type") return i18n.t("reader.blockedType");
    if (err.message === "unknown_sender") return i18n.t("compose.fromUnavailable");
    return err.message;
  }
  return i18n.t(key);
}

export function reportError(e: unknown) {
  useUi.getState().toast(errorText(e), "error");
}

function threadsFor() {
  const { view } = useMail.getState();
  return useSettings.getState().threads && view.kind !== "search";
}

export async function toggleSeen(msgs: MessageSummary[]) {
  if (!msgs.length) return;
  const value = !msgs.every((m) => m.seen);
  try {
    await api.setFlags(
      msgs.map((m) => m.id),
      "seen",
      value,
      threadsFor(),
    );
  } catch (e) {
    reportError(e);
  }
}

export async function markSeen(m: MessageSummary) {
  if (m.seen) return;
  try {
    await api.setFlags([m.id], "seen", true, threadsFor());
  } catch (e) {
    reportError(e);
  }
}

export async function toggleFlagged(msgs: MessageSummary[]) {
  if (!msgs.length) return;
  const value = !msgs.every((m) => m.flagged);
  try {
    await api.setFlags(
      msgs.map((m) => m.id),
      "flagged",
      value,
      false,
    );
  } catch (e) {
    reportError(e);
  }
}

export async function archive(msgs: MessageSummary[]) {
  if (!msgs.length) return;
  const ids = msgs.map((m) => m.id);
  try {
    await api.archiveMessages(ids, threadsFor());
    useMail.getState().dropFromList(ids);
    useUi.getState().toast(i18n.t("toast.archived", { count: ids.length }));
  } catch (e) {
    const err = toAppError(e);
    if (err.code === "no_archive_folder") {
      const accountId = msgs[0].accountId;
      useUi.getState().toast(i18n.t("archive.missing"), "info", {
        label: i18n.t("archive.create"),
        run: async () => {
          try {
            await api.createArchiveFolder(accountId);
            await useMail.getState().refreshFolders();
            await archive(msgs);
          } catch (e2) {
            reportError(e2);
          }
        },
      });
    } else reportError(e);
  }
}

export async function remove(msgs: MessageSummary[]) {
  if (!msgs.length) return;
  const ids = msgs.map((m) => m.id);
  try {
    await api.deleteMessages(ids, threadsFor());
    useMail.getState().dropFromList(ids);
    useUi.getState().toast(i18n.t("toast.deleted", { count: ids.length }));
  } catch (e) {
    reportError(e);
  }
}

export async function moveTo(msgs: MessageSummary[], folderId: number) {
  if (!msgs.length) return;
  const ids = msgs.map((m) => m.id);
  try {
    await api.moveMessages(ids, folderId, threadsFor());
    useMail.getState().dropFromList(ids);
  } catch (e) {
    reportError(e);
  }
}

/** Leaves a newsletter once the user confirmed. When one-click or the mail fails, the toast
 * offers the unsubscribe page instead. */
export async function unsubscribe(req: UnsubscribeRequest, page = false) {
  const sender = displayName(req.sender);
  try {
    const method = await api.unsubscribe(req.messageId, page);
    useUi.getState().toast(method === "web" ? i18n.t("unsub.pageOpened") : i18n.t("unsub.doneToast", { sender }));
  } catch (e) {
    if (!page && req.info.hasPage && req.info.method !== "web") {
      useUi.getState().toast(i18n.t("unsub.failed", { sender }), "error", {
        label: i18n.t("unsub.openPage"),
        run: () => void unsubscribe(req, true),
      });
    } else reportError(e);
  }
}

export function quoteLabels() {
  return {
    wrote: i18n.t("quote.wrote"),
    forwarded: i18n.t("quote.forwarded"),
    from: i18n.t("quote.from"),
    date: i18n.t("quote.date"),
    subject: i18n.t("quote.subject"),
    to: i18n.t("quote.to"),
    cc: i18n.t("quote.cc"),
  };
}

export function compose(accountId?: string) {
  const { accounts, view, folders } = useMail.getState();
  const fromView =
    view.kind === "folder" ? folders.find((f) => f.id === view.folderId)?.accountId : undefined;
  const id = accountId ?? fromView ?? accounts[0]?.id;
  if (!id) {
    useUi.getState().setWizardOpen(true);
    return;
  }
  useUi.getState().openComposer("new", {
    accountId: id,
    to: [],
    cc: [],
    bcc: [],
    subject: "",
    body: "",
    references: [],
    attachments: [],
  });
}

export function composeTo(email: string) {
  compose();
  const c = useUi.getState().composer;
  if (c) useUi.getState().openComposer("new", { ...c.draft, to: [{ name: null, email }] });
}

export async function reply(mode: ReplyMode, msg: MessageSummary | MessageDetail) {
  try {
    const detail = "text" in msg ? msg : await api.getMessage(msg.id);
    const { accounts } = useMail.getState();
    const account = accounts.find((a) => a.id === detail.accountId);
    if (!account) return;
    const draft = buildReply(
      mode,
      detail,
      account,
      ownAddresses(accounts),
      quoteLabels(),
      (d) => fullDate(d, i18n.language),
    );
    useUi.getState().openComposer(mode, draft);
  } catch (e) {
    reportError(e);
  }
}
