import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import en from "./en.json";
import de from "./de.json";
import type { Language } from "@/stores/settings";

export const resources = { en: { translation: en }, de: { translation: de } } as const;

export function resolveLanguage(setting: Language): "en" | "de" {
  if (setting !== "system") return setting;
  const nav = typeof navigator !== "undefined" ? navigator.language : "en";
  return nav.toLowerCase().startsWith("de") ? "de" : "en";
}

function initialLanguage(): "en" | "de" {
  try {
    const raw = localStorage.getItem("redpanda-settings");
    const lang = raw ? (JSON.parse(raw).state?.language as Language) : "system";
    return resolveLanguage(lang ?? "system");
  } catch {
    return resolveLanguage("system");
  }
}

void i18n.use(initReactI18next).init({
  resources,
  lng: initialLanguage(),
  fallbackLng: "en",
  interpolation: { escapeValue: false },
  returnNull: false,
});

export default i18n;
