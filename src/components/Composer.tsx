import { FileText, Paperclip, Send, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, isTauri, pickFiles, type Account, type Draft } from "@/lib/ipc";
import { errorText } from "@/lib/actions";
import { isEmail } from "@/lib/format";
import { useMail } from "@/stores/mail";
import { useUi } from "@/stores/ui";
import { AddressInput } from "./AddressInput";
import { Button, Dialog, IconButton, Select, Spinner } from "./ui";

const AUTOSAVE_MS = 2500;

function isBlank(d: Draft) {
  return !d.to.length && !d.cc.length && !d.bcc.length && !d.subject.trim() && !d.body.trim() && !d.attachments.length;
}

function fileName(path: string) {
  return path.split(/[\\/]/).pop() ?? path;
}

const senderKey = (accountId: string, fromEmail?: string | null) => `${accountId} ${fromEmail ?? ""}`;

/** Every address the user can send from: each account's own, then its aliases. */
function senders(accounts: Account[], draft: Draft) {
  const list = accounts.flatMap((a) => [
    { accountId: a.id, fromEmail: null as string | null, label: `${a.displayName} <${a.email}>` },
    ...a.aliases.map((x) => ({ accountId: a.id, fromEmail: x.email, label: `${x.name || a.displayName} <${x.email}>` })),
  ]);
  // An alias the account lost since the draft was saved stays visible, sending then says why it fails.
  const account = accounts.find((a) => a.id === draft.accountId);
  const current = senderKey(draft.accountId, draft.fromEmail);
  if (account && draft.fromEmail && !list.some((s) => senderKey(s.accountId, s.fromEmail) === current)) {
    list.push({ accountId: account.id, fromEmail: draft.fromEmail, label: `${account.displayName} <${draft.fromEmail}>` });
  }
  return list.map((s) => ({ ...s, value: senderKey(s.accountId, s.fromEmail) }));
}

export function Composer() {
  const { t } = useTranslation();
  const composer = useUi((s) => s.composer);
  const closeComposer = useUi((s) => s.closeComposer);
  const toast = useUi((s) => s.toast);
  const accounts = useMail((s) => s.accounts);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [showCc, setShowCc] = useState(false);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [savedAt, setSavedAt] = useState<number | null>(null);
  const dirty = useRef(false);
  const bodyRef = useRef<HTMLTextAreaElement>(null);
  const draftRef = useRef<Draft | null>(null);
  const saving = useRef<Promise<unknown> | null>(null);
  const sendingRef = useRef(false);
  draftRef.current = draft;

  useEffect(() => {
    if (!composer) return;
    setDraft(composer.draft);
    setShowCc(composer.draft.cc.length > 0 || composer.draft.bcc.length > 0);
    setError(null);
    setSavedAt(null);
    dirty.current = false;
    if (composer.kind === "reply" || composer.kind === "replyAll" || composer.kind === "forward") {
      // Start typing above the quote.
      requestAnimationFrame(() => {
        bodyRef.current?.focus();
        bodyRef.current?.setSelectionRange(0, 0);
        bodyRef.current?.scrollTo(0, 0);
      });
    }
  }, [composer]);

  const save = async () => {
    const d = draftRef.current;
    if (!d || !dirty.current || isBlank(d) || sendingRef.current) return d?.id ?? null;
    dirty.current = false;
    try {
      const pending = api.saveDraft(d);
      saving.current = pending;
      const id = await pending;
      setDraft((cur) => (cur ? { ...cur, id } : cur));
      draftRef.current = { ...d, id };
      setSavedAt(Date.now());
      return id;
    } catch {
      dirty.current = true;
      return d.id ?? null;
    }
  };

  useEffect(() => {
    if (!draft || !dirty.current) return;
    const id = setTimeout(save, AUTOSAVE_MS);
    return () => clearTimeout(id);
  });

  if (!composer || !draft) return null;

  const update = (patch: Partial<Draft>) => {
    dirty.current = true;
    setError(null);
    setDraft({ ...draft, ...patch });
  };

  const refresh = () => Promise.all([useMail.getState().refreshFolders(), useMail.getState().refreshList()]);

  const close = async () => {
    const d = draftRef.current;
    if (d && isBlank(d)) {
      if (d.id) await api.deleteDraft(d.id).catch(() => undefined);
    } else if (dirty.current) {
      await save();
      toast(t("compose.saved"));
    }
    closeComposer();
    void refresh();
  };

  const discard = async () => {
    if (draft.id) await api.deleteDraft(draft.id).catch(() => undefined);
    closeComposer();
    void refresh();
  };

  const send = async () => {
    // Give a pending address input the chance to commit on blur.
    (document.activeElement as HTMLElement | null)?.blur();
    await new Promise((r) => setTimeout(r, 160));
    const d = draftRef.current!;
    const all = [...d.to, ...d.cc, ...d.bcc];
    if (!all.length) return setError(t("compose.noRecipients"));
    const bad = all.find((a) => !isEmail(a.email));
    if (bad) return setError(t("compose.invalidRecipient", { email: bad.email }));
    setSending(true);
    sendingRef.current = true;
    setError(null);
    try {
      // An autosave may still be in flight; wait so its draft id is known and gets removed.
      await saving.current?.catch(() => undefined);
      await api.sendMessage({ ...d, id: draftRef.current?.id ?? d.id });
      closeComposer();
      toast(t("compose.sent"));
      void refresh();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setSending(false);
      sendingRef.current = false;
    }
  };

  const from = senders(accounts, draft);
  const title =
    composer.kind === "reply" || composer.kind === "replyAll"
      ? t("compose.titleReply")
      : composer.kind === "forward"
        ? t("compose.titleForward")
        : t("compose.title");

  return (
    <Dialog
      open
      onOpenChange={(open) => !open && !sending && void close()}
      title={title}
      width={760}
      closeLabel={t("common.close")}
    >
      <div
        className="flex h-[min(680px,80vh)] flex-col"
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
            e.preventDefault();
            void send();
          }
        }}
      >
        <div className="flex min-h-10 items-center gap-2 border-b border-line px-4">
          <label htmlFor="compose-from" className="w-12 shrink-0 text-[13px] text-muted">
            {t("compose.from")}
          </label>
          <Select
            id="compose-from"
            value={senderKey(draft.accountId, draft.fromEmail)}
            onChange={(key) => {
              const s = from.find((o) => o.value === key);
              if (s) update({ accountId: s.accountId, fromEmail: s.fromEmail });
            }}
            options={from}
            className="h-7 flex-1 border-transparent px-0 focus:border-transparent"
          />
          {!showCc && (
            <Button size="sm" variant="ghost" className="text-muted" onClick={() => setShowCc(true)}>
              {t("compose.showCcBcc")}
            </Button>
          )}
        </div>
        <AddressInput
          id="compose-to"
          label={t("compose.to")}
          value={draft.to}
          onChange={(to) => update({ to })}
          autoFocus={composer.kind === "new" || composer.kind === "forward"}
        />
        {showCc && (
          <>
            <AddressInput id="compose-cc" label={t("compose.cc")} value={draft.cc} onChange={(cc) => update({ cc })} />
            <AddressInput id="compose-bcc" label={t("compose.bcc")} value={draft.bcc} onChange={(bcc) => update({ bcc })} />
          </>
        )}
        <div className="flex min-h-10 items-center gap-2 border-b border-line px-4">
          <label htmlFor="compose-subject" className="w-12 shrink-0 text-[13px] text-muted">
            {t("compose.subject")}
          </label>
          <input
            id="compose-subject"
            value={draft.subject}
            onChange={(e) => update({ subject: e.target.value })}
            className="h-8 flex-1 bg-transparent text-sm outline-none focus-visible:outline-none"
          />
        </div>
        <textarea
          ref={bodyRef}
          value={draft.body}
          onChange={(e) => update({ body: e.target.value })}
          placeholder={t("compose.bodyPlaceholder")}
          aria-label={t("compose.bodyPlaceholder")}
          className="min-h-0 flex-1 resize-none bg-transparent px-4 py-3 text-sm leading-relaxed outline-none placeholder:text-faint focus-visible:outline-none"
        />
        {(draft.attachments.length > 0 || draft.forwardFromMessageId) && (
          <div className="flex flex-wrap gap-2 border-t border-line px-4 py-2">
            {draft.forwardFromMessageId && (
              <span className="inline-flex h-7 items-center gap-1.5 rounded-md border border-line px-2 text-xs text-muted">
                <Paperclip size={12} />
                {t("compose.forwardedAttachments")}
              </span>
            )}
            {draft.attachments.map((p) => (
              <span key={p} title={p} className="inline-flex h-7 items-center gap-1.5 rounded-md border border-line-strong pl-2 pr-1 text-xs">
                <FileText size={12} className="text-muted" />
                {fileName(p)}
                <button
                  aria-label={t("compose.removeAttachment", { name: fileName(p) })}
                  onClick={() => update({ attachments: draft.attachments.filter((x) => x !== p) })}
                  className="rounded p-0.5 text-faint hover:text-fg"
                >
                  <X size={12} />
                </button>
              </span>
            ))}
          </div>
        )}
        <div className="flex h-14 shrink-0 items-center gap-2 border-t border-line px-4">
          <Button variant="primary" onClick={send} disabled={sending}>
            {sending ? <Spinner className="size-3.5 border-white/40 border-t-white" /> : <Send size={14} />}
            {sending ? t("compose.sending") : t("compose.send")}
          </Button>
          <IconButton
            label={t("compose.attach")}
            disabled={!isTauri}
            onClick={async () => {
              const files = await pickFiles();
              if (files.length) update({ attachments: [...new Set([...draft.attachments, ...files])] });
            }}
          >
            <Paperclip size={16} />
          </IconButton>
          <span className="min-w-0 flex-1 truncate text-xs">
            {error ? (
              <span className="text-danger">{error}</span>
            ) : savedAt ? (
              <span className="text-muted">{t("compose.saved")}</span>
            ) : (
              <span className="text-faint">{t("compose.sendHint")}</span>
            )}
          </span>
          <Button variant="ghost" onClick={discard} disabled={sending}>
            {t("compose.discard")}
          </Button>
        </div>
      </div>
    </Dialog>
  );
}
