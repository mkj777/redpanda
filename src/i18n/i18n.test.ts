import { describe, expect, it } from "vitest";
import en from "./en.json";
import de from "./de.json";

type Tree = { [k: string]: string | Tree };

function flatten(obj: Tree, prefix = ""): Record<string, string> {
  return Object.entries(obj).reduce<Record<string, string>>((acc, [k, v]) => {
    if (typeof v === "string") acc[`${prefix}${k}`] = v;
    else Object.assign(acc, flatten(v, `${prefix}${k}.`));
    return acc;
  }, {});
}

const enFlat = flatten(en);
const deFlat = flatten(de);

describe("locales", () => {
  it("have the same keys", () => {
    expect(Object.keys(deFlat).sort()).toEqual(Object.keys(enFlat).sort());
  });

  it("use the same placeholders", () => {
    for (const [k, v] of Object.entries(enFlat)) {
      const vars = (s: string) => [...s.matchAll(/\{\{(\w+)\}\}/g)].map((m) => m[1]).sort();
      expect(vars(deFlat[k]), k).toEqual(vars(v));
    }
  });

  it("contain no em-dashes or middle dots", () => {
    for (const [k, v] of [...Object.entries(enFlat), ...Object.entries(deFlat)]) {
      expect(v, k).not.toMatch(/[\u2014\u00b7]/);
    }
  });
});
