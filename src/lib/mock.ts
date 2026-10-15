// In-memory stand-in for the Rust backend, used when the UI runs in a normal browser
// (`pnpm dev`). All data is fictional.

import type {
  Account,
  Address,
  Draft,
  Folder,
  FolderRole,
  MessageDetail,
  MessageSummary,
  View,
} from "./ipc";

type Handler = (payload: unknown) => void;
const listeners = new Map<string, Set<Handler>>();

export function mockListen(event: string, handler: Handler) {
  if (!listeners.has(event)) listeners.set(event, new Set());
  listeners.get(event)!.add(handler);
  return Promise.resolve(() => listeners.get(event)?.delete(handler));
}

function emit(event: string, payload: unknown) {
  setTimeout(() => listeners.get(event)?.forEach((h) => h(payload)), 0);
}

const now = Math.floor(Date.now() / 1000);
const hour = 3600;
const day = 24 * hour;

const accounts: Account[] = [
  {
    id: "acc-gmail",
    email: "lena.berger@gmail.com",
    displayName: "Lena Berger",
    provider: "google",
    authType: "oauth_google",
    username: "lena.berger@gmail.com",
    imap: { host: "imap.gmail.com", port: 993, security: "tls" },
    smtp: { host: "smtp.gmail.com", port: 465, security: "tls" },
    color: "#c2410c",
    sentSavedByServer: true,
    isGmail: true,
    status: "idle",
    statusError: null,
  },
  {
    id: "acc-work",
    email: "l.berger@nordlicht-studio.de",
    displayName: "Lena Berger",
    provider: "microsoft",
    authType: "oauth_microsoft",
    username: "l.berger@nordlicht-studio.de",
    imap: { host: "outlook.office365.com", port: 993, security: "tls" },
    smtp: { host: "smtp.office365.com", port: 587, security: "start_tls" },
    color: "#2563eb",
    sentSavedByServer: true,
    isGmail: false,
    status: "idle",
    statusError: null,
  },
  {
    id: "acc-gmx",
    email: "lena.b@gmx.de",
    displayName: "Lena",
    provider: "gmx",
    authType: "password",
    username: "lena.b@gmx.de",
    imap: { host: "imap.gmx.net", port: 993, security: "tls" },
    smtp: { host: "mail.gmx.net", port: 587, security: "start_tls" },
    color: "#059669",
    sentSavedByServer: false,
    isGmail: false,
    status: "auth_required",
    statusError: "authentication failed: invalid credentials",
  },
];

let folderSeq = 1;
const folders: Folder[] = [];
function addFolder(accountId: string, path: string, name: string, role: FolderRole, depth = 0) {
  const f: Folder = { id: folderSeq++, accountId, path, name, depth, role, unreadCount: 0, totalCount: 0 };
  folders.push(f);
  return f;
}
for (const a of accounts) {
  addFolder(a.id, "INBOX", "Inbox", "inbox");
  addFolder(a.id, a.isGmail ? "[Gmail]/Drafts" : "Drafts", "Drafts", "drafts");
  addFolder(a.id, a.isGmail ? "[Gmail]/Sent Mail" : "Sent", a.isGmail ? "Sent Mail" : "Sent", "sent");
  if (!a.isGmail) addFolder(a.id, "Archive", "Archive", "archive");
  addFolder(a.id, a.isGmail ? "[Gmail]/Spam" : "Junk", a.isGmail ? "Spam" : "Junk", "junk");
  addFolder(a.id, a.isGmail ? "[Gmail]/Trash" : "Trash", "Trash", "trash");
}
addFolder("acc-gmail", "Reisen", "Reisen", "other");
addFolder("acc-work", "Projekte", "Projekte", "other");
addFolder("acc-work", "Projekte/Kunden", "Kunden", "other", 1);

const folderOf = (accountId: string, role: FolderRole) =>
  folders.find((f) => f.accountId === accountId && f.role === role)!;

interface MockMessage extends MessageDetail {
  deleted?: boolean;
}

let msgSeq = 1;
const messages: MockMessage[] = [];

function addr(name: string | null, email: string): Address {
  return { name, email };
}

function add(
  accountId: string,
  role: FolderRole,
  from: Address,
  subject: string,
  ageSeconds: number,
  body: { text?: string; html?: string },
  opts: Partial<MockMessage> = {},
) {
  const folder = folderOf(accountId, role);
  const account = accounts.find((a) => a.id === accountId)!;
  const text = body.text ?? null;
  const preview = (text ?? body.html?.replace(/<[^>]+>/g, " ") ?? "").replace(/\s+/g, " ").trim().slice(0, 200);
  const id = msgSeq++;
  messages.push({
    id,
    accountId,
    folderId: folder.id,
    threadKey: opts.threadKey ?? `t${id}`,
    from,
    to: [addr(account.displayName, account.email)],
    subject,
    date: now - ageSeconds,
    seen: false,
    flagged: false,
    answered: false,
    hasAttachments: false,
    preview,
    threadCount: 1,
    cc: [],
    replyTo: [],
    messageId: `m${id}@example.org`,
    references: [],
    text,
    html: body.html ?? null,
    hasRemoteContent: false,
    remoteAllowed: false,
    attachments: [],
    ...opts,
  });
}

add(
  "acc-work",
  "inbox",
  addr("Jonas Weber", "jonas.weber@nordlicht-studio.de"),
  "Freigabe Entwurf Startseite",
  25 * 60,
  {
    text: "Hallo Lena,\n\nder zweite Entwurf der Startseite liegt im geteilten Ordner. Kannst du bis Donnerstag drauf schauen? Vor allem die Navigation und die Bildauswahl im oberen Bereich.\n\nDanke dir!\nJonas\n\nhttps://example.org/entwurf-v2",
  },
  {
    hasAttachments: true,
    attachments: [
      { index: 0, filename: "Startseite_v2.pdf", mimeType: "application/pdf", size: 2_481_331, inline: false },
      { index: 1, filename: "Moodboard.png", mimeType: "image/png", size: 784_210, inline: false },
    ],
    threadKey: "home",
  },
);
add(
  "acc-gmail",
  "inbox",
  addr("Deutsche Bahn", "noreply@bahn.example"),
  "Ihre Reservierung: Hamburg Hbf nach München Hbf",
  2 * hour,
  {
    html: `<table width="100%" cellpadding="0" cellspacing="0" style="font-family:Arial,sans-serif;background:#f4f4f4"><tr><td align="center" style="padding:24px"><table width="560" style="background:#ffffff;border-radius:4px"><tr><td style="padding:24px"><img src="https://tracker.example/pixel.gif" width="1" height="1"><h2 style="margin:0 0 12px;color:#ec0016">Ihre Reservierung</h2><p style="color:#333">Fahrt am <b>Freitag, 17.10.</b> um 09:04 Uhr</p><table style="width:100%;border-collapse:collapse;margin-top:12px"><tr><td style="padding:8px;border-bottom:1px solid #ddd">Hamburg Hbf</td><td style="padding:8px;border-bottom:1px solid #ddd;text-align:right">09:04</td></tr><tr><td style="padding:8px">München Hbf</td><td style="padding:8px;text-align:right">15:21</td></tr></table><p style="margin-top:16px"><a href="https://example.org/buchung" style="color:#ec0016">Buchung anzeigen</a></p></td></tr></table></td></tr></table>`,
  },
  { hasRemoteContent: true },
);
add(
  "acc-gmail",
  "inbox",
  addr("Mara Lindqvist", "mara.lindqvist@example.com"),
  "Re: Wochenende in Kopenhagen",
  5 * hour,
  {
    text: "Klingt super! Ich hab mal nach Unterkünften in Vesterbro geschaut, die Wohnung mit dem Balkon wäre noch frei. Soll ich direkt buchen?\n\nMara\n\n> Am 14.10. schrieb Lena:\n> Wie wäre es mit dem ersten Novemberwochenende?",
  },
  { threadKey: "cph", threadCount: 3, flagged: true },
);
add(
  "acc-work",
  "inbox",
  addr("GitHub", "notifications@github.example"),
  "[nordlicht/website] Pull request #214: Fix image lazy loading",
  7 * hour,
  { text: "@jweber requested your review on this pull request.\n\nFixes the layout shift on the project pages." },
  { seen: true },
);
add(
  "acc-gmx",
  "inbox",
  addr("Stadtwerke", "service@stadtwerke.example"),
  "Ihre Jahresabrechnung 2026",
  1 * day + 3 * hour,
  { text: "Sehr geehrte Frau Berger,\n\nanbei erhalten Sie Ihre Jahresabrechnung. Der neue Abschlag beträgt ab November 64 Euro." },
  {
    seen: true,
    hasAttachments: true,
    attachments: [{ index: 0, filename: "Abrechnung_2026.pdf", mimeType: "application/pdf", size: 312_004, inline: false }],
  },
);
add(
  "acc-work",
  "inbox",
  addr("Pia Hoffmann", "pia.hoffmann@kunde.example"),
  "Termin nächste Woche",
  1 * day + 6 * hour,
  { text: "Hi Lena, passt dir Dienstag um 14 Uhr für die Abstimmung? Ansonsten ginge auch Mittwochvormittag.\n\nViele Grüße\nPia" },
);
add(
  "acc-gmail",
  "inbox",
  addr("Bücherhalle", "newsletter@buecherhalle.example"),
  "Neu im Oktober: Lesungen und Workshops",
  2 * day,
  {
    html: `<div style="font-family:Georgia,serif;max-width:600px;margin:auto"><h1 style="font-weight:normal">Neu im Oktober</h1><p>Lesung mit Autorin Hanne Vik am 23.10., Workshop "Buchbinden für Einsteiger" am 30.10.</p><p><a href="https://example.org/programm">Zum Programm</a></p></div>`,
  },
  { seen: true },
);
add(
  "acc-work",
  "inbox",
  addr("Jonas Weber", "jonas.weber@nordlicht-studio.de"),
  "Mittag?",
  3 * day,
  { text: "Gehen wir um 12:30 zum Thai an der Ecke?" },
  { seen: true, answered: true },
);
for (let i = 0; i < 18; i++) {
  add(
    i % 2 ? "acc-gmail" : "acc-work",
    "inbox",
    addr(["Ole Brandt", "Sofia Marino", "Team Kalender", "Felix Graf"][i % 4], `person${i}@example.com`),
    ["Rechnung Oktober", "Fotos vom Sommerfest", "Erinnerung: Abgabe Freitag", "Kurze Frage zum Angebot"][i % 4],
    (4 + i) * day,
    { text: "Hallo Lena,\n\nkurze Nachricht zum Stand der Dinge. Melde dich gern, wenn du Fragen hast.\n\nBeste Grüße" },
    { seen: true },
  );
}
add(
  "acc-gmail",
  "sent",
  addr("Lena Berger", "lena.berger@gmail.com"),
  "Wochenende in Kopenhagen",
  1 * day,
  { text: "Wie wäre es mit dem ersten Novemberwochenende?" },
  { seen: true, threadKey: "cph" },
);

function recount() {
  for (const f of folders) {
    const live = messages.filter((m) => !m.deleted && m.folderId === f.id);
    f.totalCount = live.length;
    f.unreadCount = live.filter((m) => !m.seen).length;
  }
}
recount();

let drafts: Draft[] = [];
let draftSeq = 1;

function summary(m: MockMessage): MessageSummary {
  const { cc, replyTo, messageId, references, text, html, hasRemoteContent, remoteAllowed, attachments, deleted, ...s } =
    m;
  void [cc, replyTo, messageId, references, text, html, hasRemoteContent, remoteAllowed, attachments, deleted];
  return s;
}

function visible(m: MockMessage) {
  const f = folders.find((x) => x.id === m.folderId)!;
  return !m.deleted && !["trash", "junk"].includes(f.role);
}

function listFor(view: Exclude<View, { kind: "drafts" }>) {
  let items = messages.filter((m) => !m.deleted);
  if (view.kind === "folder") items = items.filter((m) => m.folderId === view.folderId);
  else if (view.kind === "unified") {
    if (view.which === "inbox") items = items.filter((m) => folders.find((f) => f.id === m.folderId)!.role === "inbox");
    if (view.which === "unread") items = items.filter((m) => !m.seen && visible(m));
    if (view.which === "starred") items = items.filter((m) => m.flagged && visible(m));
  } else {
    const q = view.query.toLowerCase();
    items = items.filter((m) =>
      [m.subject, m.from?.name, m.from?.email, m.preview].some((s) => s?.toLowerCase().includes(q)),
    );
  }
  return items.sort((a, b) => b.date - a.date);
}

const delay = (ms = 120) => new Promise((r) => setTimeout(r, ms));

function changed(accountId: string, folderId: number | null = null) {
  recount();
  emit("mail:changed", { accountId, folderId });
}

function moveTo(ids: number[], role: FolderRole | number) {
  for (const id of ids) {
    const m = messages.find((x) => x.id === id);
    if (!m) continue;
    const target = typeof role === "number" ? folders.find((f) => f.id === role) : folderOf(m.accountId, role);
    if (!target) continue;
    m.folderId = target.id;
    changed(m.accountId, target.id);
  }
}

export async function mockInvoke(cmd: string, args: Record<string, unknown>): Promise<unknown> {
  await delay();
  switch (cmd) {
    case "list_accounts":
      return structuredClone(accounts);
    case "oauth_status":
      return { google: true, microsoft: false, configPath: "C:\\Users\\you\\AppData\\Roaming\\redpanda\\oauth.json" };
    case "discover": {
      const email = String(args.email);
      const domain = email.split("@")[1] ?? "";
      await delay(400);
      if (domain === "gmail.com")
        return {
          source: "provider",
          providerId: "google",
          providerName: "Google",
          authType: "oauth_google",
          allowPassword: true,
          imap: { host: "imap.gmail.com", port: 993, security: "tls" },
          smtp: { host: "smtp.gmail.com", port: 465, security: "tls" },
          username: email,
          hint: "hint.google_app_password",
          sentSavedByServer: true,
          isGmail: true,
        };
      if (["outlook.com", "hotmail.de", "hotmail.com"].includes(domain))
        return {
          source: "provider",
          providerId: "microsoft",
          providerName: "Microsoft",
          authType: "oauth_microsoft",
          allowPassword: false,
          imap: { host: "outlook.office365.com", port: 993, security: "tls" },
          smtp: { host: "smtp.office365.com", port: 587, security: "start_tls" },
          username: email,
          hint: null,
          sentSavedByServer: true,
          isGmail: false,
        };
      if (["gmx.de", "gmx.net", "web.de"].includes(domain))
        return {
          source: "provider",
          providerId: "gmx",
          providerName: domain === "web.de" ? "WEB.DE" : "GMX",
          authType: "password",
          allowPassword: true,
          imap: { host: `imap.${domain === "web.de" ? "web.de" : "gmx.net"}`, port: 993, security: "tls" },
          smtp: { host: `mail.${domain === "web.de" ? "web.de" : "gmx.net"}`, port: 587, security: "start_tls" },
          username: email,
          hint: domain === "web.de" ? "hint.enable_imap_webde" : "hint.enable_imap_gmx",
          sentSavedByServer: false,
          isGmail: false,
        };
      return {
        source: domain.includes(".") ? "guess" : "none",
        providerId: null,
        providerName: null,
        authType: "password",
        allowPassword: true,
        imap: domain.includes(".") ? { host: `imap.${domain}`, port: 993, security: "tls" } : null,
        smtp: domain.includes(".") ? { host: `smtp.${domain}`, port: 465, security: "tls" } : null,
        username: email,
        hint: null,
        sentSavedByServer: false,
        isGmail: false,
      };
    }
    case "add_account": {
      await delay(1200);
      const input = args.input as { email: string; displayName: string };
      const a: Account = {
        ...accounts[2],
        id: `acc-${Date.now()}`,
        email: input.email,
        displayName: input.displayName || input.email.split("@")[0],
        status: "syncing",
        statusError: null,
        color: "#7c3aed",
      };
      accounts.push(a);
      addFolder(a.id, "INBOX", "Inbox", "inbox");
      changed(a.id);
      return a;
    }
    case "reauth_account": {
      await delay(800);
      const a = accounts.find((x) => x.id === args.accountId);
      if (a) {
        a.status = "idle";
        a.statusError = null;
        emit("sync:status", { accountId: a.id, state: "idle", error: null });
      }
      return null;
    }
    case "cancel_oauth":
      return null;
    case "update_account": {
      const a = accounts.find((x) => x.id === args.accountId);
      if (a) {
        a.displayName = String(args.displayName);
        a.color = String(args.color);
      }
      return null;
    }
    case "remove_account": {
      const i = accounts.findIndex((x) => x.id === args.accountId);
      if (i >= 0) accounts.splice(i, 1);
      for (let j = folders.length - 1; j >= 0; j--) if (folders[j].accountId === args.accountId) folders.splice(j, 1);
      messages.forEach((m) => m.accountId === args.accountId && (m.deleted = true));
      changed(String(args.accountId));
      return null;
    }
    case "list_folders": {
      recount();
      const live = messages.filter((m) => !m.deleted);
      return {
        folders: structuredClone(folders.filter((f) => accounts.some((a) => a.id === f.accountId))),
        unified: {
          inboxUnread: folders.filter((f) => f.role === "inbox").reduce((n, f) => n + f.unreadCount, 0),
          unread: live.filter((m) => !m.seen && visible(m)).length,
          starred: live.filter((m) => m.flagged && visible(m)).length,
          drafts: drafts.length,
        },
      };
    }
    case "list_messages": {
      const { view, offset, limit } = args as { view: Exclude<View, { kind: "drafts" }>; offset: number; limit: number };
      const all = listFor(view);
      return { items: all.slice(offset, offset + limit).map(summary), hasMore: offset + limit < all.length };
    }
    case "open_folder":
    case "sync_now":
      return null;
    case "load_older":
      await delay(500);
      return false;
    case "get_message": {
      await delay(150);
      const m = messages.find((x) => x.id === args.id);
      if (!m) throw { code: "not_found", message: "not found" };
      return structuredClone(m);
    }
    case "get_thread": {
      const m = messages.find((x) => x.id === args.id)!;
      return messages
        .filter((x) => !x.deleted && x.accountId === m.accountId && x.threadKey === m.threadKey)
        .sort((a, b) => a.date - b.date)
        .map(summary);
    }
    case "set_flags": {
      const { ids, flag, value } = args as { ids: number[]; flag: "seen" | "flagged"; value: boolean };
      for (const id of ids) {
        const m = messages.find((x) => x.id === id);
        if (m) {
          m[flag] = value;
          changed(m.accountId, m.folderId);
        }
      }
      return null;
    }
    case "move_messages":
      moveTo(args.ids as number[], args.targetFolderId as number);
      return null;
    case "delete_messages":
      moveTo(args.ids as number[], "trash");
      return null;
    case "archive_messages": {
      const ids = args.ids as number[];
      const m = messages.find((x) => x.id === ids[0]);
      if (m && m.accountId === "acc-gmx" && !folders.some((f) => f.accountId === "acc-gmx" && f.role === "archive"))
        throw { code: "no_archive_folder", message: "no archive folder" };
      for (const id of ids) {
        const msg = messages.find((x) => x.id === id);
        if (!msg) continue;
        if (msg.accountId === "acc-gmail") {
          msg.deleted = true;
          changed(msg.accountId);
        } else moveTo([id], "archive");
      }
      return null;
    }
    case "create_archive_folder":
      addFolder(String(args.accountId), "Archive", "Archive", "archive");
      changed(String(args.accountId));
      return null;
    case "allow_remote_content":
    case "save_attachment":
    case "open_attachment":
      return null;
    case "send_message": {
      await delay(900);
      const d = args.draft as Draft;
      if (d.id) drafts = drafts.filter((x) => x.id !== d.id);
      changed(d.accountId);
      return null;
    }
    case "save_draft": {
      const d = { ...(args.draft as Draft), updatedAt: Math.floor(Date.now() / 1000) };
      if (d.id) drafts = drafts.map((x) => (x.id === d.id ? d : x));
      else {
        d.id = draftSeq++;
        drafts.push(d);
      }
      return d.id;
    }
    case "list_drafts":
      return structuredClone([...drafts].sort((a, b) => (b.updatedAt ?? 0) - (a.updatedAt ?? 0)));
    case "get_draft":
      return structuredClone(drafts.find((d) => d.id === args.id));
    case "delete_draft":
      drafts = drafts.filter((d) => d.id !== args.id);
      emit("mail:changed", { accountId: "", folderId: null });
      return null;
    case "suggest_contacts": {
      const p = String(args.prefix).toLowerCase();
      const seen = new Map<string, Address>();
      for (const m of messages) if (m.from) seen.set(m.from.email, m.from);
      return [...seen.values()]
        .filter((a) => a.email.toLowerCase().startsWith(p) || a.name?.toLowerCase().split(" ").some((w) => w.startsWith(p)))
        .slice(0, 8);
    }
    default:
      throw { code: "other", message: `mock: unknown command ${cmd}` };
  }
}
