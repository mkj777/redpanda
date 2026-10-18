import { describe, expect, it } from "vitest";
import type { Account, MessageDetail } from "./ipc";
import { buildReply, forwardSubject, replySubject } from "./reply";

const account: Account = {
  id: "a1",
  email: "me@example.com",
  displayName: "Me",
  provider: "custom",
  authType: "password",
  username: "me@example.com",
  imap: { host: "i", port: 993, security: "tls" },
  smtp: { host: "s", port: 465, security: "tls" },
  color: "#000",
  sentSavedByServer: false,
  isGmail: false,
  status: "idle",
  statusError: null,
};

const labels = {
  wrote: "On {{date}}, {{sender}} wrote:",
  forwarded: "---------- Forwarded message ----------",
  from: "From",
  date: "Date",
  subject: "Subject",
  to: "To",
  cc: "Cc",
};

function message(over: Partial<MessageDetail> = {}): MessageDetail {
  return {
    id: 7,
    accountId: "a1",
    folderId: 1,
    threadKey: "root@x",
    from: { name: "Anna", email: "anna@example.org" },
    to: [{ name: "Me", email: "me@example.com" }, { name: null, email: "bob@example.org" }],
    cc: [{ name: null, email: "carol@example.org" }, { name: null, email: "ME@example.com" }],
    replyTo: [],
    subject: "Quarterly report",
    date: 1772530920,
    seen: true,
    flagged: false,
    answered: false,
    hasAttachments: false,
    preview: "",
    threadCount: 1,
    messageId: "m2@x",
    references: ["root@x"],
    text: "Hello\n> earlier\nBye",
    html: null,
    hasRemoteContent: false,
    remoteAllowed: false,
    attachments: [],
    unsubscribe: null,
    ...over,
  };
}

const date = () => "3 Mar 2026";

describe("subjects", () => {
  it("does not stack prefixes", () => {
    expect(replySubject("Re: AW: Hi")).toBe("Re: Hi");
    expect(replySubject("Hi")).toBe("Re: Hi");
    expect(forwardSubject("Fwd: WG: Hi")).toBe("Fwd: Hi");
  });
});

describe("buildReply", () => {
  it("replies to the sender with threading headers and a quote", () => {
    const d = buildReply("reply", message(), account, ["me@example.com"], labels, date);
    expect(d.to).toEqual([{ name: "Anna", email: "anna@example.org" }]);
    expect(d.cc).toEqual([]);
    expect(d.subject).toBe("Re: Quarterly report");
    expect(d.inReplyTo).toBe("m2@x");
    expect(d.references).toEqual(["root@x", "m2@x"]);
    expect(d.replyToMessageId).toBe(7);
    expect(d.body).toContain("On 3 Mar 2026, Anna <anna@example.org> wrote:");
    expect(d.body).toContain("> Hello\n>> earlier\n> Bye");
  });

  it("prefers Reply-To", () => {
    const d = buildReply("reply", message({ replyTo: [{ name: null, email: "list@example.org" }] }), account, [], labels, date);
    expect(d.to.map((a) => a.email)).toEqual(["list@example.org"]);
  });

  it("reply all keeps everyone except me, without duplicates", () => {
    const d = buildReply("replyAll", message(), account, ["me@example.com"], labels, date);
    expect(d.to.map((a) => a.email)).toEqual(["anna@example.org"]);
    expect(d.cc.map((a) => a.email)).toEqual(["bob@example.org", "carol@example.org"]);
  });

  it("replying to my own sent message goes to its recipients", () => {
    const sent = message({
      from: { name: "Me", email: "me@example.com" },
      to: [{ name: null, email: "anna@example.org" }],
      cc: [{ name: null, email: "carol@example.org" }],
    });
    const d = buildReply("replyAll", sent, account, ["me@example.com"], labels, date);
    expect(d.to.map((a) => a.email)).toEqual(["anna@example.org"]);
    expect(d.cc.map((a) => a.email)).toEqual(["carol@example.org"]);
  });

  it("forward has no recipients and keeps attachments", () => {
    const d = buildReply("forward", message(), account, [], labels, date);
    expect(d.to).toEqual([]);
    expect(d.subject).toBe("Fwd: Quarterly report");
    expect(d.forwardFromMessageId).toBe(7);
    expect(d.inReplyTo).toBeUndefined();
    expect(d.body).toContain("---------- Forwarded message ----------\nFrom: Anna <anna@example.org>");
    expect(d.body).toContain("Cc: carol@example.org, ME@example.com");
  });
});
