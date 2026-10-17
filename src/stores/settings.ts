import { create } from "zustand";
import { persist } from "zustand/middleware";

export type Language = "system" | "en" | "de";
export type Theme = "system" | "light" | "dark";

interface SettingsState {
  language: Language;
  theme: Theme;
  threads: boolean;
  /** Load external images without asking, except in Junk. */
  autoLoadImages: boolean;
  setLanguage: (l: Language) => void;
  setTheme: (t: Theme) => void;
  setThreads: (v: boolean) => void;
  setAutoLoadImages: (v: boolean) => void;
}

export const useSettings = create<SettingsState>()(
  persist(
    (set) => ({
      language: "system",
      theme: "system",
      threads: true,
      autoLoadImages: false,
      setLanguage: (language) => set({ language }),
      setTheme: (theme) => set({ theme }),
      setThreads: (threads) => set({ threads }),
      setAutoLoadImages: (autoLoadImages) => set({ autoLoadImages }),
    }),
    { name: "redpanda-settings" },
  ),
);
