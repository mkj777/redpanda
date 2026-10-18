import { beforeEach, describe, expect, it, vi } from "vitest";
import type { MessageSummary } from "@/lib/ipc";
import { subscriptionKey, useMail } from "./mail";

const msg = (id: number): MessageSummary => ({
  id,
  accountId: "a",
  folderId: 1,
  threadKey: `t${id}`,
  from: null,
  to: [],
  subject: `S${id}`,
  date: 1000 - id,
  seen: false,
  flagged: false,
  answered: false,
  hasAttachments: false,
  preview: "",
  threadCount: 1,
});

describe("mail store", () => {
  beforeEach(() => {
    useMail.setState({ messages: [1, 2, 3, 4].map(msg), selectedId: 2 });
  });

  it("moves the selection to the next message when the selected one is removed", () => {
    useMail.getState().dropFromList([2]);
    expect(useMail.getState().messages.map((m) => m.id)).toEqual([1, 3, 4]);
    expect(useMail.getState().selectedId).toBe(3);
  });

  it("falls back to the previous message at the end of the list", () => {
    useMail.setState({ selectedId: 4 });
    useMail.getState().dropFromList([4, 3]);
    expect(useMail.getState().selectedId).toBe(2);
  });

  it("keyboard navigation stays within bounds", () => {
    const s = useMail.getState();
    s.selectRelative(1);
    expect(useMail.getState().selectedId).toBe(3);
    s.selectRelative(10);
    expect(useMail.getState().selectedId).toBe(4);
    s.selectRelative(-10);
    expect(useMail.getState().selectedId).toBe(1);
  });
});

describe("subscriptions view", () => {
  it("lists senders, keeps the selection across refreshes and drops it on another view", async () => {
    useMail.setState({ view: { kind: "unified", which: "inbox" }, messages: [], selectedId: null, selectedSub: null });
    useMail.getState().setView({ kind: "subscriptions" });
    await vi.waitFor(() => expect(useMail.getState().subscriptions.length).toBeGreaterThan(0), { timeout: 3000 });
    const subs = useMail.getState().subscriptions;
    expect(subs[subs.length - 1].unsubscribedAt).not.toBeNull();
    expect(subs.some((s) => s.sender.email === "deals@gewinn.example")).toBe(false);

    const key = subscriptionKey(subs[0]);
    useMail.getState().selectSub(key);
    await useMail.getState().refreshList();
    expect(useMail.getState().selectedSub).toBe(key);

    useMail.getState().setView({ kind: "unified", which: "inbox" });
    expect(useMail.getState().selectedSub).toBeNull();
  });
});
