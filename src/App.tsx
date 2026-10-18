import * as Tooltip from "@radix-ui/react-tooltip";
import { Plus } from "lucide-react";
import { useEffect, useState } from "react";
import { Panel, PanelGroup, PanelResizeHandle } from "react-resizable-panels";
import { useTranslation } from "react-i18next";
import i18n, { resolveLanguage } from "@/i18n";
import { api, listen } from "@/lib/ipc";
import { compose, errorText } from "@/lib/actions";
import { useMail } from "@/stores/mail";
import { useSettings } from "@/stores/settings";
import { useUi } from "@/stores/ui";
import { useShortcuts } from "@/hooks/useShortcuts";
import { AccountWizard } from "./components/AccountWizard";
import { Composer } from "./components/Composer";
import { MessageList } from "./components/MessageList";
import { ReadingPane } from "./components/ReadingPane";
import { SettingsDialog } from "./components/SettingsDialog";
import { Sidebar } from "./components/Sidebar";
import { SubscriptionPane } from "./components/Subscriptions";
import { TitleBar } from "./components/TitleBar";
import { Toasts } from "./components/Toasts";
import { Button } from "./components/ui";
import { UnsubscribeDialog } from "./components/Unsubscribe";

const RAIL_PX = 52;

function useTheme() {
  const theme = useSettings((s) => s.theme);
  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const dark = theme === "dark" || (theme === "system" && media.matches);
      document.documentElement.classList.toggle("dark", dark);
    };
    apply();
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [theme]);
}

function useLanguage() {
  const language = useSettings((s) => s.language);
  useEffect(() => {
    const lng = resolveLanguage(language);
    void i18n.changeLanguage(lng).then(() => {
      const t = i18n.t;
      void api
        .setTrayTexts({
          open: t("tray.open"),
          compose: t("tray.compose"),
          sync: t("tray.sync"),
          claude: t("tray.claude"),
          quit: t("tray.quit"),
          // The backend fills in the number.
          unread: t("tray.unread", { n: "{{count}}" }),
          hintTitle: t("tray.hintTitle"),
          hintBody: t("tray.hintBody"),
        })
        .catch(() => undefined);
    });
    document.documentElement.lang = lng;
  }, [language]);
}

function useBackendEvents() {
  const [ready, setReady] = useState(false);
  useEffect(() => {
    const mail = useMail.getState();
    let timer: ReturnType<typeof setTimeout> | undefined;
    const scheduleRefresh = () => {
      clearTimeout(timer);
      timer = setTimeout(() => {
        void mail.refreshFolders();
        void mail.refreshList();
      }, 250);
    };
    const subs = [
      listen("mail:changed", scheduleRefresh),
      listen("accounts:changed", () => void useMail.getState().refreshAccounts()),
      listen("sync:status", (s) => {
        useMail.getState().setAccountStatus(s.accountId, s.state, s.error);
        if (s.state === "idle") scheduleRefresh();
      }),
      listen("action:failed", (f) => useUi.getState().toast(errorText(f.error), "error")),
      listen("ui:compose", () => compose()),
    ];
    mail.refreshAll().finally(() => setReady(true));
    return () => {
      clearTimeout(timer);
      subs.forEach((p) => p.then((unlisten) => unlisten()));
    };
  }, []);
  return ready;
}

function ResizeHandle() {
  return (
    <PanelResizeHandle className="relative z-10 w-px bg-line transition-colors duration-150 after:absolute after:inset-y-0 after:-left-1 after:-right-1 after:content-[''] hover:bg-accent data-[resize-handle-state=drag]:bg-accent" />
  );
}

function Start() {
  const { t } = useTranslation();
  const setWizardOpen = useUi((s) => s.setWizardOpen);
  return (
    <div className="flex h-full flex-col items-center justify-center gap-4 bg-bg">
      <img src="/icon.svg" alt="" className="size-14 rounded-xl" />
      <p className="text-sm text-muted">{t("start.text")}</p>
      <Button variant="primary" onClick={() => setWizardOpen(true)}>
        <Plus size={15} />
        {t("nav.addAccount")}
      </Button>
    </div>
  );
}

export default function App() {
  useTheme();
  useLanguage();
  useShortcuts();
  const ready = useBackendEvents();
  const hasAccounts = useMail((s) => s.accounts.length > 0);
  const subscriptions = useMail((s) => s.view.kind === "subscriptions");
  const [collapsed, setCollapsed] = useState(false);
  const [width, setWidth] = useState(window.innerWidth);

  useEffect(() => {
    const onResize = () => setWidth(window.innerWidth);
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  const railPercent = (RAIL_PX / width) * 100;
  const sidebarMin = Math.max(12, (190 / width) * 100);
  const listMin = Math.max(20, (280 / width) * 100);

  return (
    <Tooltip.Provider>
      <div className="flex h-full flex-col">
        <TitleBar />
        <main className="min-h-0 flex-1">
          {!ready ? null : hasAccounts ? (
            <PanelGroup direction="horizontal" autoSaveId="redpanda-layout" className="h-full">
              <Panel
                id="sidebar"
                order={1}
                defaultSize={Math.max(18, sidebarMin)}
                minSize={sidebarMin}
                maxSize={32}
                collapsible
                collapsedSize={railPercent}
                // Derived from the size so a collapsed layout restored from storage shows the rail too.
                onResize={(size) => setCollapsed(size <= railPercent + 0.5)}
                className="border-r-0"
              >
                <Sidebar collapsed={collapsed} />
              </Panel>
              <ResizeHandle />
              <Panel id="list" order={2} defaultSize={Math.max(30, listMin)} minSize={listMin}>
                <MessageList />
              </Panel>
              <ResizeHandle />
              <Panel id="reader" order={3} defaultSize={52} minSize={28}>
                {subscriptions ? <SubscriptionPane /> : <ReadingPane />}
              </Panel>
            </PanelGroup>
          ) : (
            <Start />
          )}
        </main>
      </div>
      <Composer />
      <AccountWizard />
      <SettingsDialog />
      <UnsubscribeDialog />
      <Toasts />
    </Tooltip.Provider>
  );
}
