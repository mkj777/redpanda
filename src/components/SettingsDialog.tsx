import { useState } from "react";
import { useTranslation } from "react-i18next";
import { api, type Account } from "@/lib/ipc";
import { errorText } from "@/lib/actions";
import { cn } from "@/lib/format";
import { useMail } from "@/stores/mail";
import { useSettings, type Language, type Theme } from "@/stores/settings";
import { useUi } from "@/stores/ui";
import { Button, Dialog, Field, Input, Select, Spinner, Switch } from "./ui";

const COLORS = ["#c2410c", "#2563eb", "#059669", "#7c3aed", "#db2777", "#0891b2", "#ca8a04", "#4b5563"];

function AccountRow({ account }: { account: Account }) {
  const { t } = useTranslation();
  const toast = useUi((s) => s.toast);
  const [name, setName] = useState(account.displayName);
  const [confirmRemove, setConfirmRemove] = useState(false);
  const [reauth, setReauth] = useState<null | "password" | "busy">(null);
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const isOAuth = account.authType.startsWith("oauth");

  const saveProfile = async (displayName: string, color: string) => {
    await api.updateAccount(account.id, displayName, color);
    await useMail.getState().refreshAccounts();
  };

  const signIn = async (pw?: string) => {
    setReauth("busy");
    setError(null);
    try {
      await api.reauthAccount(account.id, pw);
      setReauth(null);
      setPassword("");
      toast(t("settings.signedIn"));
      await useMail.getState().refreshAccounts();
    } catch (e) {
      setError(errorText(e));
      setReauth(isOAuth ? null : "password");
    }
  };

  return (
    <div className="flex flex-col gap-3 border-b border-line py-4 last:border-b-0">
      <div className="flex items-center gap-2">
        <span className="size-2.5 shrink-0 rounded-full" style={{ background: account.color }} />
        <span className="min-w-0 flex-1 truncate text-sm font-medium">{account.email}</span>
        <span className={cn("text-xs", account.status === "auth_required" ? "text-danger" : "text-muted")}>
          {t(`status.${account.status}`)}
        </span>
      </div>
      <div className="grid grid-cols-[1fr_auto] items-end gap-3">
        <Field label={t("settings.displayName")} htmlFor={`name-${account.id}`}>
          <Input
            id={`name-${account.id}`}
            value={name}
            onChange={(e) => setName(e.target.value)}
            onBlur={() => name.trim() !== account.displayName && saveProfile(name.trim(), account.color)}
          />
        </Field>
        <div className="flex h-8 items-center gap-1" role="radiogroup" aria-label={t("settings.color")}>
          {COLORS.map((c) => (
            <button
              key={c}
              role="radio"
              aria-checked={c === account.color}
              aria-label={c}
              onClick={() => saveProfile(name.trim() || account.displayName, c)}
              className={cn("size-5 rounded-full border-2", c === account.color ? "border-fg" : "border-transparent")}
              style={{ background: c }}
            />
          ))}
        </div>
      </div>
      {reauth === "password" && (
        <form
          className="flex items-end gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (password) void signIn(password);
          }}
        >
          <Field label={t("wizard.password")} htmlFor={`pw-${account.id}`} className="flex-1">
            <Input id={`pw-${account.id}`} type="password" autoFocus value={password} onChange={(e) => setPassword(e.target.value)} />
          </Field>
          <Button type="submit" variant="primary" disabled={!password}>
            {t("common.save")}
          </Button>
        </form>
      )}
      {error && <div className="text-[13px] text-danger">{error}</div>}
      <div className="flex items-center gap-2">
        <Button
          size="sm"
          disabled={reauth === "busy"}
          onClick={() => (isOAuth ? signIn() : setReauth(reauth === "password" ? null : "password"))}
        >
          {reauth === "busy" && <Spinner className="size-3" />}
          {t("settings.relogin")}
        </Button>
        <span className="flex-1" />
        {confirmRemove ? (
          <>
            <span className="text-xs text-muted">{t("settings.removeConfirm")}</span>
            <Button size="sm" variant="ghost" onClick={() => setConfirmRemove(false)}>
              {t("common.cancel")}
            </Button>
            <Button
              size="sm"
              variant="danger"
              onClick={async () => {
                await api.removeAccount(account.id);
                await useMail.getState().refreshAll();
              }}
            >
              {t("settings.remove")}
            </Button>
          </>
        ) : (
          <Button size="sm" variant="ghost" className="!text-danger" onClick={() => setConfirmRemove(true)}>
            {t("settings.remove")}
          </Button>
        )}
      </div>
    </div>
  );
}

const SHORTCUTS: [string, string][] = [
  ["J / K", "shortcuts.nav"],
  ["C", "shortcuts.compose"],
  ["R", "shortcuts.reply"],
  ["A", "shortcuts.replyAll"],
  ["F", "shortcuts.forward"],
  ["E", "shortcuts.archive"],
  ["#", "shortcuts.delete"],
  ["S", "shortcuts.star"],
  ["U", "shortcuts.unread"],
  ["/", "shortcuts.search"],
  ["Ctrl + Enter", "shortcuts.send"],
];

export function SettingsDialog() {
  const { t } = useTranslation();
  const open = useUi((s) => s.settingsOpen);
  const setOpen = useUi((s) => s.setSettingsOpen);
  const setWizardOpen = useUi((s) => s.setWizardOpen);
  const accounts = useMail((s) => s.accounts);
  const settings = useSettings();
  const [tab, setTab] = useState<"accounts" | "general" | "shortcuts">("accounts");

  return (
    <Dialog open={open} onOpenChange={setOpen} title={t("settings.title")} width={560} closeLabel={t("common.close")}>
      <div role="tablist" className="flex gap-4 border-b border-line px-4">
        {(["accounts", "general", "shortcuts"] as const).map((k) => (
          <button
            key={k}
            role="tab"
            aria-selected={tab === k}
            onClick={() => setTab(k)}
            className={cn(
              "-mb-px h-10 border-b-2 text-[13px]",
              tab === k ? "border-accent font-medium text-fg" : "border-transparent text-muted hover:text-fg",
            )}
          >
            {t(`settings.tab.${k}`)}
          </button>
        ))}
      </div>
      <div className="px-4 pb-4">
        {tab === "accounts" && (
          <>
            {accounts.map((a) => (
              <AccountRow key={a.id} account={a} />
            ))}
            {!accounts.length && <p className="py-6 text-sm text-muted">{t("settings.noAccounts")}</p>}
            <Button
              className="mt-2"
              onClick={() => {
                setOpen(false);
                setWizardOpen(true);
              }}
            >
              {t("nav.addAccount")}
            </Button>
          </>
        )}
        {tab === "general" && (
          <div className="flex flex-col gap-4 pt-4">
            <Field label={t("settings.language")} htmlFor="set-lang">
              <Select<Language>
                id="set-lang"
                value={settings.language}
                onChange={settings.setLanguage}
                options={[
                  { value: "system", label: t("settings.lang.system") },
                  { value: "en", label: "English" },
                  { value: "de", label: "Deutsch" },
                ]}
              />
            </Field>
            <Field label={t("settings.theme")} htmlFor="set-theme">
              <Select<Theme>
                id="set-theme"
                value={settings.theme}
                onChange={settings.setTheme}
                options={[
                  { value: "system", label: t("settings.themeOption.system") },
                  { value: "light", label: t("settings.themeOption.light") },
                  { value: "dark", label: t("settings.themeOption.dark") },
                ]}
              />
            </Field>
            <Switch checked={settings.threads} onChange={settings.setThreads} label={t("settings.threads")} />
          </div>
        )}
        {tab === "shortcuts" && (
          <table className="mt-2 w-full text-[13px]">
            <tbody>
              {SHORTCUTS.map(([keys, label]) => (
                <tr key={keys} className="border-b border-line last:border-b-0">
                  <td className="py-2 text-fg">{t(label)}</td>
                  <td className="py-2 text-right">
                    <kbd className="rounded border border-line-strong bg-bg px-1.5 py-0.5 font-sans text-xs">{keys}</kbd>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </Dialog>
  );
}
