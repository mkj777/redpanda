import { describe, expect, it } from "vitest";
import { spamAction } from "./actions";

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
