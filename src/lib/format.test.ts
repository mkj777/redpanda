import { describe, expect, it } from "vitest";
import { fileSize, initials, isEmail, linkify, listDate, parseAddresses } from "./format";

describe("parseAddresses", () => {
  it("handles names, quotes and separators", () => {
    expect(parseAddresses('Anna <a@x.de>, b@y.de; "Doe, John" <j@z.de>')).toEqual([
      { name: "Anna", email: "a@x.de" },
      { name: null, email: "b@y.de" },
      { name: "Doe, John", email: "j@z.de" },
    ]);
    expect(parseAddresses("no address here")).toEqual([]);
  });
});

describe("isEmail", () => {
  it("accepts plain addresses only", () => {
    expect(isEmail("a@b.de")).toBe(true);
    expect(isEmail("a@b")).toBe(false);
    expect(isEmail("a b@c.de")).toBe(false);
  });
});

describe("linkify", () => {
  it("finds links without trailing punctuation", () => {
    const parts = linkify("See https://example.org/a?b=1. Or mailto:x@y.de");
    expect(parts.filter((p) => p.href).map((p) => p.href)).toEqual(["https://example.org/a?b=1", "mailto:x@y.de"]);
    expect(parts.map((p) => p.text).join("")).toBe("See https://example.org/a?b=1. Or mailto:x@y.de");
  });
});

describe("listDate", () => {
  const now = new Date(2026, 9, 15, 18, 0);
  it("shows the time for today and a date otherwise", () => {
    const today = new Date(2026, 9, 15, 9, 5).getTime() / 1000;
    expect(listDate(today, "de", now)).toBe("09:05");
    const thisYear = new Date(2026, 2, 3).getTime() / 1000;
    expect(listDate(thisYear, "de", now)).toMatch(/3\. M.r/);
    const older = new Date(2024, 0, 2).getTime() / 1000;
    expect(listDate(older, "de", now)).toBe("02.01.2024");
  });
});

describe("misc", () => {
  it("formats sizes and initials", () => {
    expect(fileSize(512, "en")).toBe("512 B");
    expect(fileSize(2_481_331, "en")).toBe("2.4 MB");
    expect(initials({ name: "Anna Schmidt", email: "a@x.de" })).toBe("AS");
    expect(initials({ name: null, email: "bob@x.de" })).toBe("BO");
  });
});
