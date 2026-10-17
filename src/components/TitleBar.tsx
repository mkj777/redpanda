import { getCurrentWindow } from "@tauri-apps/api/window";
import { forwardRef, useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, isTauri, listen, type CaptionState } from "@/lib/ipc";
import { cn } from "@/lib/format";

// Segoe Fluent Icons on Windows 11, Segoe MDL2 Assets on Windows 10: same code points as the system buttons.
const GLYPH = { minimize: "", maximize: "", restore: "", close: "" };

const appWindow = () => getCurrentWindow();

interface CaptionButtonProps {
  label: string;
  glyph: string;
  close?: boolean;
  /** Set by the native overlay on the maximize button, where the page never sees the mouse. */
  state?: CaptionState;
  onClick: () => void;
}

const CaptionButton = forwardRef<HTMLButtonElement, CaptionButtonProps>(function CaptionButton(
  { label, glyph, close, state = "none", onClick },
  ref,
) {
  return (
    <button
      ref={ref}
      type="button"
      tabIndex={-1}
      aria-label={label}
      title={label}
      data-state={state}
      onClick={onClick}
      className={cn(
        "flex h-full w-[46px] items-center justify-center text-[10px] text-fg outline-none transition-colors duration-100 [font-family:'Segoe_Fluent_Icons','Segoe_MDL2_Assets'] group-data-[inactive]:text-faint",
        close
          ? "hover:bg-[#c42b1c] hover:text-white active:bg-[#c42b1c]/90 active:text-white/80"
          : "hover:bg-black/[0.06] hover:text-fg active:bg-black/[0.04] data-[state=hover]:bg-black/[0.06] data-[state=hover]:text-fg data-[state=press]:bg-black/[0.04] dark:hover:bg-white/[0.06] dark:active:bg-white/[0.04] dark:data-[state=hover]:bg-white/[0.06] dark:data-[state=press]:bg-white/[0.04]",
      )}
    >
      <span aria-hidden>{glyph}</span>
    </button>
  );
});

/** Replaces the Windows title bar: drag area with the app name and the three caption buttons. */
export function TitleBar() {
  const { t } = useTranslation();
  const [maximized, setMaximized] = useState(false);
  const [focused, setFocused] = useState(true);
  const [maxState, setMaxState] = useState<CaptionState>("none");
  const maxRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!isTauri) return;
    const win = appWindow();
    // Snapping, Win+Up and double-clicks maximize too, so follow the window instead of the button.
    const sync = () => void win.isMaximized().then(setMaximized);
    sync();
    const subs = [
      win.onResized(sync),
      win.onFocusChanged((e) => setFocused(e.payload)),
      listen("titlebar:maximize", setMaxState),
    ];
    return () => subs.forEach((p) => p.then((unlisten) => unlisten()));
  }, []);

  // Tells the native snap layout overlay where the maximize button is.
  useLayoutEffect(() => {
    const el = maxRef.current;
    if (!el || !isTauri) return;
    const report = () => {
      const r = el.getBoundingClientRect();
      void api.setMaximizeRect({
        right: window.innerWidth - r.right,
        top: r.top,
        width: r.width,
        height: r.height,
      });
    };
    report();
    const observer = new ResizeObserver(report);
    observer.observe(el);
    window.addEventListener("resize", report);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", report);
      void api.setMaximizeRect(null);
    };
  }, []);

  const run = (action: (w: ReturnType<typeof appWindow>) => Promise<void>) => () => {
    if (isTauri) void action(appWindow());
  };

  return (
    <header
      data-inactive={focused ? undefined : ""}
      className="group relative z-[45] flex h-8 shrink-0 items-center border-b border-line bg-sidebar"
    >
      <div className="drag flex h-full min-w-0 flex-1 items-center gap-2 pl-3">
        <img src="/icon.svg" alt="" className="size-4 shrink-0 group-data-[inactive]:opacity-60" />
        <span className="truncate text-xs text-muted group-data-[inactive]:text-faint">redpanda</span>
      </div>
      <div className="no-drag flex h-full">
        <CaptionButton label={t("window.minimize")} glyph={GLYPH.minimize} onClick={run((w) => w.minimize())} />
        <CaptionButton
          ref={maxRef}
          label={maximized ? t("window.restore") : t("window.maximize")}
          glyph={maximized ? GLYPH.restore : GLYPH.maximize}
          state={maxState}
          onClick={run((w) => w.toggleMaximize())}
        />
        <CaptionButton close label={t("window.close")} glyph={GLYPH.close} onClick={run((w) => w.close())} />
      </div>
    </header>
  );
}
