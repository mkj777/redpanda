import { create } from "zustand";
import type { Draft } from "@/lib/ipc";

export interface Toast {
  id: number;
  text: string;
  tone: "info" | "error";
  action?: { label: string; run: () => void };
}

export type ComposeKind = "new" | "reply" | "replyAll" | "forward" | "draft";

interface UiState {
  composer: { kind: ComposeKind; draft: Draft } | null;
  wizardOpen: boolean;
  settingsOpen: boolean;
  toasts: Toast[];
  openComposer: (kind: ComposeKind, draft: Draft) => void;
  closeComposer: () => void;
  setWizardOpen: (open: boolean) => void;
  setSettingsOpen: (open: boolean) => void;
  toast: (text: string, tone?: Toast["tone"], action?: Toast["action"]) => void;
  dismissToast: (id: number) => void;
}

let toastSeq = 1;

export const useUi = create<UiState>()((set, get) => ({
  composer: null,
  wizardOpen: false,
  settingsOpen: false,
  toasts: [],
  openComposer: (kind, draft) => set({ composer: { kind, draft } }),
  closeComposer: () => set({ composer: null }),
  setWizardOpen: (wizardOpen) => set({ wizardOpen }),
  setSettingsOpen: (settingsOpen) => set({ settingsOpen }),
  toast: (text, tone = "info", action) => {
    const id = toastSeq++;
    set({ toasts: [...get().toasts.slice(-3), { id, text, tone, action }] });
    setTimeout(() => get().dismissToast(id), tone === "error" ? 8000 : 4000);
  },
  dismissToast: (id) => set({ toasts: get().toasts.filter((t) => t.id !== id) }),
}));
