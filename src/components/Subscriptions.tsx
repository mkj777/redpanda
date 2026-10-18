import { Check, Newspaper } from "lucide-react";
import type { TFunction } from "i18next";
import { useEffect, useState } from "react";
import { useShallow } from "zustand/react/shallow";
import { useTranslation } from "react-i18next";
import { api, type MessageSummary, type Subscription } from "@/lib/ipc";
import { cn, displayName, listDate } from "@/lib/format";
import { reportError } from "@/lib/actions";
import { subscriptionKey, useMail } from "@/stores/mail";
import { useUi } from "@/stores/ui";
import { MessageView } from "./ReadingPane";
import { Avatar, Button, Spinner } from "./ui";

function ask(s: Subscription) {
  const { accountId, sender, latestId, method, target, hasPage, unsubscribedAt } = s;
  useUi.getState().askUnsubscribe({
    messageId: latestId,
    accountId,
    sender,
    info: { method, target, hasPage, unsubscribedAt },
  });
}

/** "23 emails, 5 in the last month", or the last date for quiet senders. */
function stats(s: Subscription, t: TFunction, language: string) {
  const recent =
    s.recent > 0 ? t("subs.recent", { count: s.recent }) : t("subs.lastOn", { date: listDate(s.lastDate, language) });
  return `${t("subs.total", { count: s.total })}, ${recent}`;
}

function SubscriptionRow({ s, selected, showAccount }: { s: Subscription; selected: boolean; showAccount: boolean }) {
  const { t, i18n } = useTranslation();
  const color = useMail((st) => st.accounts.find((a) => a.id === s.accountId)?.color);
  const left = s.unsubscribedAt != null;
  return (
    <div
      role="option"
      aria-selected={selected}
      onClick={() => useMail.getState().selectSub(subscriptionKey(s))}
      className={cn(
        "group flex h-[60px] cursor-default items-center gap-3 border-b border-line pl-4 pr-3",
        selected ? "bg-selected" : "hover:bg-hover",
      )}
    >
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          {showAccount && color && <span className="size-2 shrink-0 rounded-full" style={{ background: color }} />}
          <span className={cn("truncate text-[13px]", left ? "text-fg/70" : "font-medium text-fg")}>
            {displayName(s.sender)}
          </span>
        </div>
        <div className="truncate text-xs text-muted">{stats(s, t, i18n.language)}</div>
      </div>
      {left ? (
        <span className="flex shrink-0 items-center gap-1 text-xs text-faint">
          <Check size={12} />
          {t("unsub.done")}
        </span>
      ) : (
        // Shown on hover and for the selected row, a button on every row would crowd the list.
        <Button
          size="sm"
          className={cn(!selected && "invisible group-hover:visible")}
          onClick={(e) => {
            e.stopPropagation();
            ask(s);
          }}
        >
          {t("unsub.button")}
        </Button>
      )}
    </div>
  );
}

/** Newsletter senders for the middle column: still subscribed first, then the ones left. */
export function SubscriptionList() {
  const { t } = useTranslation();
  const { subscriptions, selectedSub, loading, multiAccount } = useMail(
    useShallow((s) => ({
      subscriptions: s.subscriptions,
      selectedSub: s.selectedSub,
      loading: s.loading,
      multiAccount: s.accounts.length > 1,
    })),
  );
  if (loading && !subscriptions.length) {
    return (
      <div className="flex flex-1 items-center justify-center">
        <Spinner />
      </div>
    );
  }
  if (!subscriptions.length) {
    return <div className="flex flex-1 items-center justify-center p-8 text-center text-sm text-muted">{t("subs.empty")}</div>;
  }
  const row = (s: Subscription) => {
    const key = subscriptionKey(s);
    return <SubscriptionRow key={key} s={s} selected={key === selectedSub} showAccount={multiAccount} />;
  };
  const left = subscriptions.filter((s) => s.unsubscribedAt != null);
  return (
    <div role="listbox" aria-label={t("nav.subscriptions")} className="min-h-0 flex-1 overflow-y-auto">
      {subscriptions.filter((s) => s.unsubscribedAt == null).map(row)}
      {left.length > 0 && (
        <div className="flex h-8 items-center border-b border-line bg-bg px-4 text-xs font-medium text-muted">
          {t("subs.unsubscribed")}
        </div>
      )}
      {left.map(row)}
    </div>
  );
}

function MailRow({ m, onOpen }: { m: MessageSummary; onOpen: () => void }) {
  const { t, i18n } = useTranslation();
  return (
    <button onClick={onOpen} className="flex w-full items-center gap-3 border-b border-line py-2.5 text-left hover:bg-hover/60">
      <span className={cn("max-w-[55%] shrink-0 truncate text-[13px]", m.seen ? "text-fg/85" : "font-semibold text-fg")}>
        {m.subject || t("list.noSubject")}
      </span>
      <span className="min-w-0 flex-1 truncate text-[13px] text-muted">{m.preview}</span>
      <span className="shrink-0 text-xs tabular-nums text-muted">{listDate(m.date, i18n.language)}</span>
    </button>
  );
}

/** The selected sender in the reading pane: what it sends, how to leave and its mail. */
export function SubscriptionPane() {
  const { t, i18n } = useTranslation();
  const sub = useMail((s) => s.subscriptions.find((x) => subscriptionKey(x) === s.selectedSub) ?? null);
  const account = useMail((s) => s.accounts.find((a) => a.id === sub?.accountId));
  const revision = useMail((s) => s.revision);
  const [mails, setMails] = useState<MessageSummary[] | null>(null);
  const [open, setOpen] = useState<number | null>(null);
  const key = sub ? subscriptionKey(sub) : null;

  useEffect(() => {
    setMails(null);
    setOpen(null);
  }, [key]);

  useEffect(() => {
    if (!sub) return;
    let alive = true;
    api
      .listMessages({ kind: "sender", accountId: sub.accountId, email: sub.sender.email }, 0, 50, false)
      .then((page) => alive && setMails(page.items))
      .catch(reportError);
    return () => {
      alive = false;
    };
    // Reload for another sender or after a refresh.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, revision]);

  if (!sub) {
    return (
      <section className="flex h-full items-center justify-center bg-bg text-sm text-muted">
        <span className="flex items-center gap-2">
          <Newspaper size={16} />
          {t("subs.select")}
        </span>
      </section>
    );
  }

  const leftOn =
    sub.unsubscribedAt != null &&
    new Date(sub.unsubscribedAt * 1000).toLocaleDateString(i18n.language, { day: "numeric", month: "long", year: "numeric" });

  return (
    <section className="flex h-full flex-col bg-bg">
      <div className="flex h-12 shrink-0 items-center gap-2 border-b border-line bg-surface px-4 text-xs text-muted">
        {account && (
          <>
            <span className="size-2 shrink-0 rounded-full" style={{ background: account.color }} />
            <span className="truncate">{account.email}</span>
          </>
        )}
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="mx-auto max-w-[860px] px-6 pb-10">
          <header className="flex items-start gap-4 pt-6">
            <Avatar address={sub.sender} className="size-11 text-sm" />
            <div className="min-w-0 flex-1">
              <h1 className="selectable truncate text-lg font-semibold leading-snug">{displayName(sub.sender)}</h1>
              <div className="selectable truncate text-[13px] text-muted">{sub.sender.email}</div>
              <div className="mt-2 text-[13px] text-muted">{stats(sub, t, i18n.language)}</div>
              <div className="text-[13px] text-muted">{t(`subs.via.${sub.method}`, { target: sub.target })}</div>
            </div>
            {leftOn ? (
              <span className="flex shrink-0 items-center gap-1.5 pt-1 text-[13px] text-muted">
                <Check size={14} />
                {t("subs.unsubscribedOn", { date: leftOn })}
              </span>
            ) : (
              <Button variant="soft" className="shrink-0" onClick={() => ask(sub)}>
                {t("unsub.button")}
              </Button>
            )}
          </header>
          {leftOn && sub.sinceUnsubscribed > 0 && (
            <div className="mt-4 flex flex-wrap items-center gap-2 rounded-md border border-line bg-surface px-3 py-2 text-[13px] text-muted">
              <span className="flex-1">{t("subs.stillComing", { count: sub.sinceUnsubscribed })}</span>
              <Button size="sm" onClick={() => ask(sub)}>
                {t("subs.again")}
              </Button>
            </div>
          )}
          <h2 className="mb-1 mt-7 border-b border-line pb-2 text-xs font-medium text-muted">{t("subs.mails")}</h2>
          {mails === null ? (
            <div className="flex justify-center py-6">
              <Spinner />
            </div>
          ) : (
            mails.map((m) =>
              open === m.id ? (
                <div key={m.id} className="border-b border-line">
                  <MessageView summary={m} isLast unsubscribeLink={false} />
                </div>
              ) : (
                <MailRow key={m.id} m={m} onOpen={() => setOpen(m.id)} />
              ),
            )
          )}
        </div>
      </div>
    </section>
  );
}
