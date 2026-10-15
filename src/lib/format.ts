import type { Address } from "./ipc";

export function cn(...parts: (string | false | null | undefined)[]) {
  return parts.filter(Boolean).join(" ");
}

/** Short date for the message list: time today, day and month this year, full date otherwise. */
export function listDate(unix: number, locale: string, now = new Date()) {
  const d = new Date(unix * 1000);
  const sameDay = d.toDateString() === now.toDateString();
  if (sameDay) return d.toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit" });
  const yesterday = new Date(now);
  yesterday.setDate(now.getDate() - 1);
  if (d.toDateString() === yesterday.toDateString()) {
    return new Intl.RelativeTimeFormat(locale, { numeric: "auto" }).format(-1, "day");
  }
  if (d.getFullYear() === now.getFullYear()) {
    return d.toLocaleDateString(locale, { day: "numeric", month: "short" });
  }
  return d.toLocaleDateString(locale, { day: "2-digit", month: "2-digit", year: "numeric" });
}

export function fullDate(unix: number, locale: string) {
  return new Date(unix * 1000).toLocaleString(locale, {
    weekday: "short",
    day: "numeric",
    month: "short",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function fileSize(bytes: number, locale: string) {
  const units = ["B", "KB", "MB", "GB"];
  let n = bytes;
  let u = 0;
  while (n >= 1024 && u < units.length - 1) {
    n /= 1024;
    u++;
  }
  return `${n.toLocaleString(locale, { maximumFractionDigits: u === 0 ? 0 : 1 })} ${units[u]}`;
}

export function displayName(a: Address | null | undefined) {
  if (!a) return "";
  return a.name?.trim() || a.email;
}

export function formatAddress(a: Address) {
  return a.name ? `${a.name} <${a.email}>` : a.email;
}

export function initials(a: Address | null | undefined) {
  const name = displayName(a);
  const parts = name.replace(/[<>"@.].*$/, "").split(/\s+/).filter(Boolean);
  const letters = parts.length > 1 ? parts[0][0] + parts[parts.length - 1][0] : name.slice(0, 2);
  return letters.toUpperCase();
}

/** Parses free text like `Anna <a@x.de>, b@y.de; "Doe, John" <j@z.de>` into addresses. */
export function parseAddresses(input: string): Address[] {
  const out: Address[] = [];
  const re = /(?:"([^"]*)"|([^"<>,;]*?))\s*<([^<>\s]+@[^<>\s]+)>|([^\s,;<>"]+@[^\s,;<>"]+)/g;
  for (const m of input.matchAll(re)) {
    if (m[3]) out.push({ name: (m[1] ?? m[2])?.trim() || null, email: m[3].trim() });
    else if (m[4]) out.push({ name: null, email: m[4].trim() });
  }
  return out;
}

export function isEmail(s: string) {
  return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(s.trim());
}

const URL_RE = /\b(https?:\/\/[^\s<>"')\]]+[^\s<>"')\].,;:!?])|\b(mailto:[^\s<>"']+)/gi;

export type TextPart = { text: string; href?: string };

/** Splits plain text into text and link parts. */
export function linkify(text: string): TextPart[] {
  const parts: TextPart[] = [];
  let last = 0;
  for (const m of text.matchAll(URL_RE)) {
    const i = m.index ?? 0;
    if (i > last) parts.push({ text: text.slice(last, i) });
    parts.push({ text: m[0], href: m[0] });
    last = i + m[0].length;
  }
  if (last < text.length) parts.push({ text: text.slice(last) });
  return parts;
}
