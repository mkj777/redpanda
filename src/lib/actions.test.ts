import { afterEach, describe, expect, it } from "vitest";
import { rowView, spamAction } from "./actions";
import type { View } from "./ipc";
import { useMail } from "@/stores/mail";
import { useSettings } from "@/stores/settings";

describe("spamAction", () => {
  it("offers not spam in the spam folder and spam elsewhere, but never for own mail", () => {
    expect(spamAction("junk")).toBe("notSpam");
    expect(spamAction("inbox")).toBe("spam");
    expect(spamAction("archive")).toBe("spam");
    expect(spamAction("trash")).toBe("spam");
    expect(spamAction(undefined)).toBe("spam");
    expect(spamAction("sent")).toBeNull();
    expect(spamAction("drafts")).toBeNull();
  });
});

describe("rowView", () => {
  const { view, threads } = { view: useMail.getState().view, threads: useSettings.getState().threads };
  afterEach(() => {
    useMail.setState({ view });
    useSettings.setState({ threads });
  });
  const inView = (view: View, threads = true) => {
    useMail.setState({ view });
    useSettings.setState({ threads });
    return rowView();
  };

  it("is the view where rows are conversations, and null where each row is one mail", () => {
    const starred: View = { kind: "unified", which: "starred" };
    expect(inView(starred)).toEqual(starred);
    expect(inView({ kind: "folder", folderId: 3 })).toEqual({ kind: "folder", folderId: 3 });
    expect(inView({ kind: "search", query: "x" })).toBeNull();
    expect(inView({ kind: "subscriptions" })).toBeNull();
    expect(inView(starred, false)).toBeNull();
  });
});
