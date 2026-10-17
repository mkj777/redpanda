import { ChevronDown, ChevronRight, Info } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  api,
  openExternal,
  type DiscoveryResult,
  type NewAccountInput,
  type OAuthProvider,
  type OAuthStatus,
  type Security,
  type ServerConfig,
} from "@/lib/ipc";
import { errorText } from "@/lib/actions";
import { isEmail } from "@/lib/format";
import { useMail } from "@/stores/mail";
import { useUi } from "@/stores/ui";
import { Button, Dialog, Field, Input, Select, Spinner } from "./ui";

export const SETUP_GUIDE_URL = "https://github.com/mkj777/redpanda/blob/main/docs/OAUTH_SETUP.md";
const GOOGLE_APP_PASSWORDS_URL = "https://myaccount.google.com/apppasswords";

const emptyServer = (port: number, security: Security): ServerConfig => ({ host: "", port, security });

function ServerFields({
  prefix,
  label,
  value,
  onChange,
}: {
  prefix: string;
  label: string;
  value: ServerConfig;
  onChange: (v: ServerConfig) => void;
}) {
  const { t } = useTranslation();
  const securityOptions: { value: Security; label: string }[] = [
    { value: "tls", label: t("wizard.sec.tls") },
    { value: "start_tls", label: t("wizard.sec.start_tls") },
    { value: "none", label: t("wizard.sec.none") },
  ];
  return (
    <fieldset className="grid grid-cols-[1fr_88px_120px] gap-2">
      <legend className="mb-1.5 text-[13px] font-medium">{label}</legend>
      <Field label={t("wizard.host")} htmlFor={`${prefix}-host`}>
        <Input id={`${prefix}-host`} value={value.host} onChange={(e) => onChange({ ...value, host: e.target.value.trim() })} />
      </Field>
      <Field label={t("wizard.port")} htmlFor={`${prefix}-port`}>
        <Input
          id={`${prefix}-port`}
          inputMode="numeric"
          value={String(value.port || "")}
          onChange={(e) => onChange({ ...value, port: Number(e.target.value.replace(/\D/g, "")) || 0 })}
        />
      </Field>
      <Field label={t("wizard.security")} htmlFor={`${prefix}-sec`}>
        <Select id={`${prefix}-sec`} value={value.security} onChange={(security) => onChange({ ...value, security })} options={securityOptions} />
      </Field>
    </fieldset>
  );
}

export function AccountWizard() {
  const { t } = useTranslation();
  const open = useUi((s) => s.wizardOpen);
  const setOpen = useUi((s) => s.setWizardOpen);
  const toast = useUi((s) => s.toast);
  const [email, setEmail] = useState("");
  const [name, setName] = useState("");
  const [password, setPassword] = useState("");
  const [found, setFound] = useState<DiscoveryResult | null>(null);
  const [discovering, setDiscovering] = useState(false);
  const [usePassword, setUsePassword] = useState(false);
  const [advanced, setAdvanced] = useState(false);
  const [imap, setImap] = useState<ServerConfig>(emptyServer(993, "tls"));
  const [smtp, setSmtp] = useState<ServerConfig>(emptyServer(465, "tls"));
  const [username, setUsername] = useState("");
  const [busy, setBusy] = useState<null | "checking" | "browser">(null);
  const [error, setError] = useState<string | null>(null);
  const [oauth, setOauth] = useState<OAuthStatus | null>(null);

  useEffect(() => {
    if (!open) return;
    setEmail("");
    setName(useMail.getState().accounts[0]?.displayName ?? "");
    setPassword("");
    setFound(null);
    setUsePassword(false);
    setAdvanced(false);
    setError(null);
    setBusy(null);
    void api.oauthStatus().then(setOauth).catch(() => setOauth(null));
  }, [open]);

  useEffect(() => {
    setFound(null);
    setError(null);
    if (!isEmail(email)) return;
    let alive = true;
    const id = setTimeout(async () => {
      setDiscovering(true);
      try {
        const r = await api.discover(email.trim());
        if (!alive) return;
        setFound(r);
        setImap(r.imap ?? emptyServer(993, "tls"));
        setSmtp(r.smtp ?? emptyServer(465, "tls"));
        setUsername(r.username || email.trim());
        setUsePassword(false);
        setAdvanced(r.source === "none");
      } finally {
        if (alive) setDiscovering(false);
      }
    }, 500);
    return () => {
      alive = false;
      clearTimeout(id);
    };
  }, [email]);

  const oauthProvider: OAuthProvider | null =
    found?.authType === "oauth_google" ? "google" : found?.authType === "oauth_microsoft" ? "microsoft" : null;
  const providerLabel = oauthProvider === "google" ? "Google" : "Microsoft";
  const oauthConfigured = oauthProvider ? !!oauth?.[oauthProvider] : false;
  // Without a client ID, go straight to the app password where the provider allows one.
  const passwordOnly = oauthProvider !== null && oauth !== null && !oauthConfigured && !!found?.allowPassword;
  const showOAuth = oauthProvider && !usePassword && !passwordOnly;
  const appPassword = found?.authType === "app_password" || (oauthProvider !== null && !showOAuth);

  const submit = async (auth: NewAccountInput["auth"]) => {
    setError(null);
    setBusy(auth.type === "oauth" ? "browser" : "checking");
    try {
      await api.addAccount({
        email: email.trim(),
        displayName: name.trim(),
        providerId: found?.providerId ?? null,
        username: username.trim() || email.trim(),
        imap,
        smtp,
        auth,
      });
      await useMail.getState().refreshAll();
      toast(t("wizard.added", { email: email.trim() }));
      setOpen(false);
    } catch (e) {
      const msg = errorText(e);
      if (!msg.toLowerCase().includes("cancel")) setError(msg);
    } finally {
      setBusy(null);
    }
  };

  const serversValid = imap.host && imap.port && smtp.host && smtp.port;
  const canSubmitPassword = isEmail(email) && password && serversValid && !busy;

  let detected: string | null = null;
  if (found) {
    if (oauthProvider === "google") detected = t("wizard.detected.google");
    else if (oauthProvider === "microsoft") detected = t("wizard.detected.microsoft");
    else if (found.providerName) detected = t("wizard.detected.provider", { name: found.providerName });
    else if (found.source !== "none") detected = t("wizard.detected.servers");
    else detected = t("wizard.detected.none");
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o && busy === "browser") void api.cancelOauth();
        if (!busy || busy === "browser") setOpen(o);
      }}
      title={t("wizard.title")}
      width={520}
      closeLabel={t("common.close")}
    >
      <form
        className="flex flex-col gap-4 p-4"
        onSubmit={(e) => {
          e.preventDefault();
          if (showOAuth) {
            if (oauthConfigured) void submit({ type: "oauth", provider: oauthProvider! });
          } else if (canSubmitPassword) void submit({ type: "password", password });
        }}
      >
        <Field label={t("wizard.email")} htmlFor="wiz-email">
          <Input
            id="wiz-email"
            type="email"
            autoFocus
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            placeholder="name@example.com"
            disabled={!!busy}
          />
        </Field>
        <Field label={t("wizard.name")} htmlFor="wiz-name">
          <Input id="wiz-name" value={name} onChange={(e) => setName(e.target.value)} placeholder={t("wizard.namePlaceholder")} disabled={!!busy} />
        </Field>

        {found && !(showOAuth && oauthConfigured) && (
          <div className="flex flex-col gap-3 border-t border-line pt-4">
            {!showOAuth && <div className="text-[13px] font-medium">{detected}</div>}
            {found.hint && !showOAuth && (
              <div className="flex gap-2 rounded-md border border-line bg-bg p-2.5 text-[13px] text-muted">
                <Info size={14} className="mt-0.5 shrink-0" />
                <span>
                  {t(found.hint)}
                  {oauthProvider === "google" && (
                    <>
                      {" "}
                      <button type="button" className="text-accent hover:underline" onClick={() => openExternal(GOOGLE_APP_PASSWORDS_URL)}>
                        {t("wizard.createAppPassword")}
                      </button>
                    </>
                  )}
                </span>
              </div>
            )}

            {showOAuth ? (
              <div className="rounded-md border border-line bg-bg p-2.5 text-[13px] text-muted">
                {t("wizard.oauthMissing", { provider: providerLabel })}{" "}
                <button type="button" className="text-accent hover:underline" onClick={() => openExternal(SETUP_GUIDE_URL)}>
                  {t("wizard.setupGuide")}
                </button>
                {oauth?.configPath && <div className="selectable mt-1 break-all font-mono text-xs">{oauth.configPath}</div>}
              </div>
            ) : (
              <>
                <Field label={appPassword ? t("wizard.appPassword") : t("wizard.password")} htmlFor="wiz-password">
                  <Input
                    id="wiz-password"
                    type="password"
                    value={password}
                    onChange={(e) => setPassword(e.target.value)}
                    autoFocus
                    disabled={!!busy}
                  />
                </Field>
                <div>
                  <button
                    type="button"
                    className="flex items-center gap-1 text-[13px] text-muted hover:text-fg"
                    onClick={() => setAdvanced(!advanced)}
                    aria-expanded={advanced}
                  >
                    {advanced ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
                    {t("wizard.advanced")}
                    {!advanced && imap.host && <span className="ml-1 text-faint">{imap.host}</span>}
                  </button>
                  {advanced && (
                    <div className="mt-3 flex flex-col gap-4">
                      <ServerFields prefix="imap" label={t("wizard.imap")} value={imap} onChange={setImap} />
                      <ServerFields prefix="smtp" label={t("wizard.smtp")} value={smtp} onChange={setSmtp} />
                      <Field label={t("wizard.username")} htmlFor="wiz-user">
                        <Input id="wiz-user" value={username} onChange={(e) => setUsername(e.target.value)} />
                      </Field>
                    </div>
                  )}
                </div>
              </>
            )}
          </div>
        )}

        {error && <div className="rounded-md border border-danger/40 p-2.5 text-[13px] text-danger">{error}</div>}

        <Button type="submit" variant="primary" disabled={showOAuth ? !oauthConfigured || !!busy : !canSubmitPassword}>
          {(busy || discovering) && <Spinner className="size-3.5 border-white/40 border-t-white" />}
          {showOAuth
            ? oauthProvider === "google"
              ? t("wizard.signInGoogle")
              : t("wizard.signInMicrosoft")
            : t("wizard.signIn")}
        </Button>
        {busy === "browser" && (
          <div className="-mt-1 flex items-center justify-between text-[13px] text-muted">
            {t("wizard.waitingBrowser")}
            <Button type="button" size="sm" variant="ghost" onClick={() => api.cancelOauth()}>
              {t("common.cancel")}
            </Button>
          </div>
        )}
      </form>
    </Dialog>
  );
}
