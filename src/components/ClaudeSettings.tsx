import { Copy, Eye, EyeOff, RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { errorText } from "@/lib/actions";
import { api, listen, type McpStatus } from "@/lib/ipc";
import { cn } from "@/lib/format";
import { useUi } from "@/stores/ui";
import { Button, Field, IconButton, Input, Spinner, Switch } from "./ui";

/** The "Claude Code" settings tab: switches the MCP server and shows how to connect. */
export function ClaudeSettings() {
  const { t } = useTranslation();
  const toast = useUi((s) => s.toast);
  const [status, setStatus] = useState<McpStatus | null>(null);
  const [key, setKey] = useState<string | null>(null);
  const [port, setPort] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const load = () =>
      void api
        .mcpStatus()
        .then((s) => {
          setStatus(s);
          setPort(String(s.port));
        })
        .catch(() => undefined);
    load();
    // The tray menu can switch it too.
    const sub = listen("mcp:changed", load);
    return () => void sub.then((unlisten) => unlisten());
  }, []);

  const run = async (action: () => Promise<McpStatus | void>, done?: string) => {
    setBusy(true);
    setError(null);
    try {
      const next = await action();
      if (next) {
        setStatus(next);
        setPort(String(next.port));
      }
      if (done) toast(done);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  const copy = async (text: string) => {
    await navigator.clipboard.writeText(text);
    toast(t("claude.copied"));
  };

  if (!status) return null;

  const savePort = () => {
    const n = Number(port);
    if (n === status.port) return;
    if (!Number.isInteger(n) || n < 1024 || n > 65535) {
      setError(t("claude.portInvalid"));
      setPort(String(status.port));
      return;
    }
    void run(() => api.setMcpPort(n));
  };

  const state = !status.enabled
    ? null
    : status.running
      ? { ok: true, text: t("claude.running", { url: status.url }) }
      : status.error === "port_in_use"
        ? { ok: false, text: t("claude.portInUse", { port: status.port }) }
        : { ok: false, text: status.error ?? t("claude.starting") };

  return (
    <div className="flex flex-col gap-4 pt-4">
      <Switch
        checked={status.enabled}
        onChange={(v) => void run(() => api.setMcpEnabled(v))}
        label={t("claude.enable")}
        hint={t("claude.enableHint")}
      />
      {state && (
        <div className="flex items-center gap-2 text-[13px]">
          <span className={cn("size-2 shrink-0 rounded-full", state.ok ? "bg-emerald-500" : "bg-danger")} />
          <span className={state.ok ? "text-muted" : "text-danger"}>{state.text}</span>
          {busy && <Spinner className="size-3.5" />}
        </div>
      )}
      {status.enabled && (
        <>
          <div className="flex flex-col gap-2 rounded-md border border-line p-3">
            <div className="text-[13px] font-medium">{t("claude.connectTitle")}</div>
            <p className="text-xs text-muted">{t("claude.connectText")}</p>
            <div className="flex flex-wrap gap-2">
              <Button
                variant="primary"
                size="sm"
                disabled={busy || !status.claudeAvailable}
                title={status.claudeAvailable ? undefined : t("claude.notInstalled")}
                onClick={() => void run(() => api.mcpConnectClaude(), t("claude.connected"))}
              >
                {t("claude.connect")}
              </Button>
              <Button size="sm" disabled={busy} onClick={() => void api.mcpCommandLine().then(copy).catch((e) => setError(errorText(e)))}>
                <Copy size={13} />
                {t("claude.copyCommand")}
              </Button>
            </div>
            {!status.claudeAvailable && <p className="text-xs text-muted">{t("claude.notInstalled")}</p>}
          </div>

          <Field label={t("claude.key")}>
            <div className="flex items-center gap-1">
              <code className="h-8 min-w-0 flex-1 truncate rounded-md border border-line-strong bg-bg px-2.5 font-mono text-xs leading-8">
                {key ?? status.keyMasked ?? ""}
              </code>
              <IconButton
                label={key ? t("claude.hideKey") : t("claude.showKey")}
                onClick={() =>
                  key ? setKey(null) : void api.mcpRevealKey().then(setKey).catch((e) => setError(errorText(e)))
                }
              >
                {key ? <EyeOff size={15} /> : <Eye size={15} />}
              </IconButton>
              <IconButton
                label={t("claude.copyKey")}
                onClick={() => void api.mcpRevealKey().then(copy).catch((e) => setError(errorText(e)))}
              >
                <Copy size={15} />
              </IconButton>
              <IconButton
                label={t("claude.rotateKey")}
                disabled={busy}
                onClick={() =>
                  void run(async () => {
                    const next = await api.mcpRotateKey();
                    setKey(null);
                    return next;
                  }, t("claude.rotated"))
                }
              >
                <RefreshCw size={15} />
              </IconButton>
            </div>
            <p className="mt-1 text-xs text-muted">{t("claude.keyHint")}</p>
          </Field>

          <Field label={t("claude.port")} htmlFor="mcp-port">
            <div className="w-28">
              <Input
                id="mcp-port"
                inputMode="numeric"
                value={port}
                onChange={(e) => setPort(e.target.value.replace(/\D/g, ""))}
                onBlur={savePort}
                onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
              />
            </div>
          </Field>

          <p className="text-xs text-muted">{t("claude.safety")}</p>
        </>
      )}
      {error && <p className="text-[13px] text-danger">{error}</p>}
    </div>
  );
}
