import * as Menu from "@radix-ui/react-dropdown-menu";
import { MoreHorizontal, RefreshCw, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, type Account, type AppSettings } from "@/lib/ipc";
import { errorText } from "@/lib/actions";
import { cn } from "@/lib/format";
import { useMail } from "@/stores/mail";
import { useSettings, type Language, type Theme } from "@/stores/settings";
import { useUi } from "@/stores/ui";
import { ClaudeSettings } from "./ClaudeSettings";
import { ProviderIcon } from "./ProviderIcon";
import { Button, Dialog, Field, IconButton, Input, Select, Spinner, Switch } from "./ui";

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

  const menuItem = "flex h-7 cursor-default items-center gap-2 rounded px-2 text-[13px] outline-none data-[highlighted]:bg-hover";
  const showStatus = account.status === "error" || account.status === "auth_required" || account.status === "offline";

  return (
    <div className="border-b border-line py-2.5 last:border-b-0">
      <div className="flex items-center gap-3">
        <ProviderIcon account={account} />
        <div className="min-w-0 flex-1">
          <input
            data-inline
            aria-label={t("settings.displayName")}
            value={name}
            onChange={(e) => setName(e.target.value)}
            onBlur={() => name.trim() !== account.displayName && saveProfile(name.trim(), account.color)}
            onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
            className="-ml-1 h-6 w-full rounded border border-transparent bg-transparent px-1 text-sm font-medium text-fg outline-none hover:border-line focus:border-accent"
          />
          <div className="truncate text-xs text-muted">{account.email}</div>
          {account.aliases.length > 0 && (
            <div className="truncate text-xs text-faint" title={account.aliases.map((a) => a.email).join("\n")}>
              {t("settings.aliases", { list: account.aliases.map((a) => a.email).join(", ") })}
            </div>
          )}
        </div>
        {showStatus && (
          <span className={cn("shrink-0 text-xs", account.status === "offline" ? "text-muted" : "text-danger")}>
            {t(`status.${account.status}`)}
          </span>
        )}
        {reauth === "busy" && <Spinner className="size-3.5" />}
        <Menu.Root>
          <Menu.Trigger asChild>
            <IconButton label={t("settings.accountMenu")}>
              <MoreHorizontal size={16} />
            </IconButton>
          </Menu.Trigger>
          <Menu.Portal>
            <Menu.Content
              align="end"
              sideOffset={4}
              className="z-50 min-w-48 rounded-md border border-line bg-surface p-1 shadow-[0_2px_8px_rgba(0,0,0,0.1)]"
            >
              <div className="flex gap-1 px-2 py-1.5" role="radiogroup" aria-label={t("settings.color")}>
                {COLORS.map((c) => (
                  <Menu.Item
                    key={c}
                    role="radio"
                    aria-checked={c === account.color}
                    aria-label={c}
                    onSelect={() => saveProfile(name.trim() || account.displayName, c)}
                    className={cn(
                      "size-4 cursor-default rounded-full border-2 outline-none data-[highlighted]:border-line-strong",
                      c === account.color ? "!border-fg" : "border-transparent",
                    )}
                    style={{ background: c }}
                  />
                ))}
              </div>
              <Menu.Separator className="my-1 h-px bg-line" />
              <Menu.Item
                className={menuItem}
                disabled={reauth === "busy"}
                onSelect={() => (isOAuth ? signIn() : setReauth("password"))}
              >
                <RefreshCw size={14} className="text-muted" />
                {t("settings.relogin")}
              </Menu.Item>
              <Menu.Item className={cn(menuItem, "text-danger")} onSelect={() => setConfirmRemove(true)}>
                <Trash2 size={14} />
                {t("settings.remove")}
              </Menu.Item>
            </Menu.Content>
          </Menu.Portal>
        </Menu.Root>
      </div>
      {reauth === "password" && (
        <form
          className="mt-2 flex items-center gap-2 pl-11"
          onSubmit={(e) => {
            e.preventDefault();
            if (password) void signIn(password);
          }}
        >
          <Input
            type="password"
            autoFocus
            aria-label={t("wizard.password")}
            placeholder={t("wizard.password")}
            value={password}
            onChange={(e) => setPassword(e.target.value)}
          />
          <Button type="button" variant="ghost" onClick={() => setReauth(null)}>
            {t("common.cancel")}
          </Button>
          <Button type="submit" variant="primary" disabled={!password}>
            {t("common.save")}
          </Button>
        </form>
      )}
      {error && <div className="mt-1.5 pl-11 text-[13px] text-danger">{error}</div>}
      {confirmRemove && (
        <div className="mt-2 flex items-center gap-2 pl-11">
          <span className="flex-1 text-xs text-muted">{t("settings.removeConfirm")}</span>
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
        </div>
      )}
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
  const [tab, setTab] = useState<"accounts" | "general" | "claude" | "shortcuts">("accounts");
  const [appSettings, setAppSettings] = useState<AppSettings | null>(null);

  useEffect(() => {
    if (open) void api.getAppSettings().then(setAppSettings).catch(() => undefined);
  }, [open]);

  const setCloseToTray = async (closeToTray: boolean) => {
    await api.setCloseToTray(closeToTray);
    setAppSettings((s) => (s ? { ...s, closeToTray } : s));
  };

  return (
    <Dialog open={open} onOpenChange={setOpen} title={t("settings.title")} width={560} closeLabel={t("common.close")}>
      <div role="tablist" className="flex gap-4 border-b border-line px-4">
        {(["accounts", "general", "claude", "shortcuts"] as const).map((k) => (
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
            <Switch checked={settings.autoLoadImages} onChange={settings.setAutoLoadImages} label={t("settings.autoLoadImages")} />
            {appSettings && (
              <Switch
                checked={appSettings.closeToTray}
                onChange={(v) => void setCloseToTray(v)}
                label={t("settings.closeToTray")}
                hint={t("settings.closeToTrayHint")}
              />
            )}
          </div>
        )}
        {tab === "claude" && <ClaudeSettings />}
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
