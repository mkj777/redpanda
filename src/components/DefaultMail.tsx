import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, type DefaultMailStatus } from "@/lib/ipc";
import { reportError } from "@/lib/actions";
import { Button, Dialog } from "./ui";

/** Asked at start while redpanda is not the default mail app, until it is or the user opts out. */
export function DefaultMailPrompt() {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);

  useEffect(() => {
    void api
      .defaultMailStatus()
      .then((s) => setOpen(s.supported && s.ask && !s.isDefault))
      .catch(() => undefined);
  }, []);

  const neverAsk = () => {
    setOpen(false);
    void api.setDefaultMailAsk(false).catch(reportError);
  };
  const makeDefault = () => {
    setOpen(false);
    void api.openDefaultApps().catch(reportError);
  };

  return (
    <Dialog open={open} onOpenChange={setOpen} title={t("defaultMail.title")} width={420} closeLabel={t("common.close")}>
      <div className="flex flex-col gap-4 p-4">
        <p className="text-[13px] leading-relaxed text-muted">{t("defaultMail.text")}</p>
        <div className="flex justify-end gap-2">
          <Button variant="ghost" onClick={neverAsk}>
            {t("defaultMail.never")}
          </Button>
          <Button variant="ghost" onClick={() => setOpen(false)}>
            {t("defaultMail.later")}
          </Button>
          <Button variant="primary" autoFocus onClick={makeDefault}>
            {t("defaultMail.make")}
          </Button>
        </div>
      </div>
    </Dialog>
  );
}

/** Status and button in the "General" settings tab. Checks again when the window gets focus,
 *  since the choice is made in the Windows settings. */
export function DefaultMailSetting() {
  const { t } = useTranslation();
  const [status, setStatus] = useState<DefaultMailStatus | null>(null);

  useEffect(() => {
    const load = () => void api.defaultMailStatus().then(setStatus).catch(() => undefined);
    load();
    window.addEventListener("focus", load);
    return () => window.removeEventListener("focus", load);
  }, []);

  if (!status?.supported) return null;

  return (
    <div className="flex flex-col gap-2">
      <div className="text-[13px] font-medium">{t("defaultMail.setting")}</div>
      {status.isDefault ? (
        <p className="text-xs text-muted">{t("defaultMail.isDefault")}</p>
      ) : (
        <div>
          <Button size="sm" onClick={() => void api.openDefaultApps().catch(reportError)}>
            {t("defaultMail.setDefault")}
          </Button>
        </div>
      )}
    </div>
  );
}
