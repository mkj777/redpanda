import { Check } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { MessageDetail } from "@/lib/ipc";
import { displayName } from "@/lib/format";
import { unsubscribe } from "@/lib/actions";
import { accountById } from "@/stores/mail";
import { useUi } from "@/stores/ui";
import { Avatar, Button, Dialog, Spinner } from "./ui";

/** Senders get two days to process an unsubscribe (RFC 8058, Gmail's rules). */
const UNSUBSCRIBE_GRACE = 2 * 24 * 3600;

/** Asks before leaving a newsletter and says how it will happen. */
export function UnsubscribeDialog() {
  const { t } = useTranslation();
  const req = useUi((s) => s.unsubscribe);
  const close = () => useUi.getState().askUnsubscribe(null);
  const [busy, setBusy] = useState(false);

  const confirm = async () => {
    if (!req) return;
    setBusy(true);
    await unsubscribe(req);
    setBusy(false);
    close();
  };

  return (
    <Dialog
      open={!!req}
      onOpenChange={(open) => !open && !busy && close()}
      title={t("unsub.title")}
      width={420}
      closeLabel={t("common.close")}
    >
      {req && (
        <div className="flex flex-col gap-4 p-4">
          <div className="flex items-center gap-3">
            <Avatar address={req.sender} />
            <div className="min-w-0">
              <div className="truncate text-sm font-semibold">{displayName(req.sender)}</div>
              <div className="truncate text-xs text-muted">{req.sender.email}</div>
            </div>
          </div>
          <p className="text-[13px] leading-relaxed text-muted">
            {t(`unsub.how.${req.info.method}`, {
              target: req.info.target,
              account: accountById(req.accountId)?.email ?? "",
            })}
          </p>
          <div className="flex justify-end gap-2">
            <Button variant="ghost" onClick={close} disabled={busy}>
              {t("common.cancel")}
            </Button>
            <Button variant="primary" autoFocus onClick={confirm} disabled={busy}>
              {busy && <Spinner className="size-3.5 border-accent-fg/40 border-t-accent-fg" />}
              {req.info.method === "web" ? t("unsub.openPage") : t("unsub.confirm")}
            </Button>
          </div>
        </div>
      )}
    </Dialog>
  );
}

/** "Unsubscribe" next to the sender in the reading pane, like Gmail. */
export function UnsubscribeLink({ msg }: { msg: MessageDetail }) {
  const { t } = useTranslation();
  const info = msg.unsubscribe;
  if (!info || !msg.from) return null;
  // Mail that still arrives well after unsubscribing offers it again.
  const left = info.unsubscribedAt != null && msg.date <= info.unsubscribedAt + UNSUBSCRIBE_GRACE;
  if (left) {
    return (
      <span className="flex shrink-0 items-center gap-1 text-xs text-faint">
        <Check size={12} />
        {t("unsub.done")}
      </span>
    );
  }
  const from = msg.from;
  return (
    <button
      onClick={() => useUi.getState().askUnsubscribe({ messageId: msg.id, accountId: msg.accountId, sender: from, info })}
      className="shrink-0 rounded text-xs text-muted underline decoration-line-strong underline-offset-2 transition-colors duration-100 hover:text-fg hover:decoration-current"
    >
      {t("unsub.button")}
    </button>
  );
}
