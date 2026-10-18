import type { Account, Address, Draft, MessageDetail } from "./ipc";
import { formatAddress } from "./format";

export type ReplyMode = "reply" | "replyAll" | "forward";

export interface QuoteLabels {
  /** Template with {{date}} and {{sender}}. */
  wrote: string;
  forwarded: string;
  from: string;
  date: string;
  subject: string;
  to: string;
  cc: string;
}

const RE_PREFIX = /^\s*((re|aw|antw|sv|vs)\s*(\[\d+\])?\s*:\s*)+/i;
const FWD_PREFIX = /^\s*((fwd?|wg|wtr|vs)\s*:\s*)+/i;

export function replySubject(subject: string) {
  return `Re: ${subject.replace(RE_PREFIX, "").trim()}`;
}

export function forwardSubject(subject: string) {
  return `Fwd: ${subject.replace(FWD_PREFIX, "").trim()}`;
}

function sameEmail(a: string, b: string) {
  return a.trim().toLowerCase() === b.trim().toLowerCase();
}

function dedupe(list: Address[], exclude: string[]) {
  const seen = new Set(exclude.map((e) => e.toLowerCase()));
  return list.filter((a) => {
    const key = a.email.toLowerCase();
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

function quote(text: string) {
  return text
    .replace(/\r\n/g, "\n")
    .trimEnd()
    .split("\n")
    .map((l) => (l.startsWith(">") ? `>${l}` : `> ${l}`))
    .join("\n");
}

function fill(template: string, values: Record<string, string>) {
  return template.replace(/\{\{(\w+)\}\}/g, (_, k) => values[k] ?? "");
}

/** All addresses the user has: every account's own and its aliases. */
export function ownAddresses(accounts: Account[]) {
  return accounts.flatMap((a) => [a.email, ...a.aliases.map((x) => x.email)]);
}

/**
 * The alias to answer from, like Gmail: the one the message went to, or the one it was sent
 * from. Undefined for the account's own address.
 */
export function replyFrom(msg: MessageDetail, account: Account) {
  const candidates = [...(msg.from ? [msg.from] : []), ...msg.to, ...msg.cc];
  for (const c of candidates) {
    if (sameEmail(c.email, account.email)) return undefined;
    const alias = account.aliases.find((a) => sameEmail(a.email, c.email));
    if (alias) return alias.email;
  }
  return undefined;
}

/**
 * Builds the draft for reply, reply all or forward. `ownEmails` are the user's addresses,
 * which never end up as recipients of a reply.
 */
export function buildReply(
  mode: ReplyMode,
  msg: MessageDetail,
  account: Account,
  ownEmails: string[],
  labels: QuoteLabels,
  formatDate: (unix: number) => string,
): Draft {
  const own = [...ownEmails, ...ownAddresses([account])];
  const fromMe = msg.from ? own.some((e) => sameEmail(e, msg.from!.email)) : false;
  const text = msg.text ?? "";
  const base: Draft = {
    accountId: account.id,
    fromEmail: replyFrom(msg, account),
    to: [],
    cc: [],
    bcc: [],
    subject: "",
    body: "",
    references: [],
    attachments: [],
  };

  if (mode === "forward") {
    const header = [
      labels.forwarded,
      msg.from ? `${labels.from}: ${formatAddress(msg.from)}` : null,
      `${labels.date}: ${formatDate(msg.date)}`,
      `${labels.subject}: ${msg.subject}`,
      msg.to.length ? `${labels.to}: ${msg.to.map(formatAddress).join(", ")}` : null,
      msg.cc.length ? `${labels.cc}: ${msg.cc.map(formatAddress).join(", ")}` : null,
    ]
      .filter(Boolean)
      .join("\n");
    return {
      ...base,
      subject: forwardSubject(msg.subject),
      body: `\n\n${header}\n\n${text.trimEnd()}\n`,
      forwardFromMessageId: msg.id,
    };
  }

  // Replying to a message I sent goes back to its recipients.
  const primary = fromMe ? msg.to : msg.replyTo.length ? msg.replyTo : msg.from ? [msg.from] : [];
  const notOwn = (a: Address) => !own.some((e) => sameEmail(e, a.email));
  let to = dedupe(primary, []);
  // Keep a note-to-self reply addressed to myself instead of to nobody.
  if (to.some(notOwn)) to = to.filter(notOwn);
  let cc: Address[] = [];
  if (mode === "replyAll") {
    const others = fromMe ? msg.cc : [...msg.to, ...msg.cc];
    cc = dedupe(others.filter(notOwn), to.map((a) => a.email));
  }
  const references = [...msg.references];
  if (msg.messageId && !references.includes(msg.messageId)) references.push(msg.messageId);
  const intro = fill(labels.wrote, {
    date: formatDate(msg.date),
    sender: msg.from ? formatAddress(msg.from) : "",
  });
  return {
    ...base,
    to,
    cc,
    subject: replySubject(msg.subject),
    body: `\n\n${intro}\n${quote(text)}\n`,
    inReplyTo: msg.messageId,
    references,
    replyToMessageId: msg.id,
  };
}
