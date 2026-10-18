import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { errorText, installUpdate } from "@/lib/actions";
import { api, listen, type UpdateStatus } from "@/lib/ipc";
import { Button, Spinner } from "./ui";

/** Version and updates, at the end of the "General" settings tab. */
export function UpdateSettings() {
  const { t } = useTranslation();
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const [checking, setChecking] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    void api.updateStatus().then(setStatus).catch(() => undefined);
    const sub = listen("update:available", (available) => setStatus((s) => (s ? { ...s, available } : s)));
    return () => void sub.then((unlisten) => unlisten());
  }, []);

  if (!status) return null;

  const check = async () => {
    setChecking(true);
    setMessage(null);
    try {
      const available = await api.checkForUpdate();
      setStatus({ ...status, available });
      if (!available) setMessage(t("update.upToDate"));
    } catch (e) {
      setMessage(errorText(e));
    } finally {
      setChecking(false);
    }
  };

  return (
    <div className="flex flex-col gap-2 border-t border-line pt-4">
      <div className="text-[13px] font-medium">{t("update.version", { version: status.version })}</div>
      {!status.supported ? (
        <p className="text-xs text-muted">{t("update.devBuild")}</p>
      ) : status.available ? (
        <div className="flex flex-wrap items-center gap-3">
          <span className="text-[13px] text-fg">{t("update.available", { version: status.available.version })}</span>
          <Button variant="primary" size="sm" onClick={() => void installUpdate()}>
            {t("update.install")}
          </Button>
        </div>
      ) : (
        <div className="flex flex-wrap items-center gap-3">
          <Button size="sm" disabled={checking} onClick={() => void check()}>
            {checking && <Spinner className="size-3.5" />}
            {t("update.check")}
          </Button>
          {message && <span className="text-xs text-muted">{message}</span>}
        </div>
      )}
    </div>
  );
}
