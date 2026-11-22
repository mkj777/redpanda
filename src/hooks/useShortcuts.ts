import { useEffect } from "react";
import { archive, compose, remove, reply, toggleFlagged, toggleSeen } from "@/lib/actions";
import { useMail } from "@/stores/mail";
import { useUi } from "@/stores/ui";

function isTyping(target: EventTarget | null) {
  const el = target as HTMLElement | null;
  if (!el) return false;
  return el.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(el.tagName);
}

/** A menu is open (right-click menu, download list, move menu): its arrows, typeahead letters
 * and Delete belong to it, not to the selected mail. */
function menuOpen() {
  return document.querySelector('[role="menu"]') !== null;
}

/** Gmail-style single key shortcuts. Inactive while typing, while a dialog or a menu is open, and
 * for keys something else already handled, such as Escape closing a menu. */
export function useShortcuts() {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey || isTyping(e.target) || menuOpen()) return;
      const ui = useUi.getState();
      if (ui.composer || ui.wizardOpen || ui.settingsOpen || ui.unsubscribe) return;
      const mail = useMail.getState();
      const selected = mail.messages.find((m) => m.id === mail.selectedId);
      const handled = (() => {
        switch (e.key) {
          case "j":
          case "ArrowDown":
            mail.selectRelative(1);
            return true;
          case "k":
          case "ArrowUp":
            mail.selectRelative(-1);
            return true;
          case "c":
            compose();
            return true;
          case "/":
            document.getElementById("search-input")?.focus();
            return true;
          case "Escape":
            mail.select(null);
            return true;
        }
        if (!selected) return false;
        switch (e.key) {
          case "r":
            void reply("reply", selected);
            return true;
          case "a":
            void reply("replyAll", selected);
            return true;
          case "f":
            void reply("forward", selected);
            return true;
          case "e":
            void archive([selected]);
            return true;
          case "#":
          case "Delete":
            void remove([selected]);
            return true;
          case "s":
            void toggleFlagged([selected]);
            return true;
          case "u":
            void toggleSeen([selected]);
            return true;
        }
        return false;
      })();
      if (handled) e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
}
