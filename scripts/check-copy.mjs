// Fails on em-dashes and middle dots anywhere in source, docs and copy, on locale files whose
// keys differ, and on translation keys used in code that the English file does not define.
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, extname } from "node:path";

const root = new URL("..", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1");
const banned = [
  ["\u2014", "em-dash"],
  ["\u00b7", "middle dot"],
];
const scan = ["src", "src-tauri/src", "src-tauri/tests", "docs", "README.md", "index.html"];
const exts = new Set([".ts", ".tsx", ".json", ".css", ".rs", ".md", ".html", ".sql", ".xml"]);
let failures = 0;

function walk(path, out = []) {
  const s = statSync(path);
  if (s.isDirectory()) for (const e of readdirSync(path)) walk(join(path, e), out);
  else if (exts.has(extname(path))) out.push(path);
  return out;
}

for (const entry of scan) {
  for (const file of walk(join(root, entry))) {
    const lines = readFileSync(file, "utf8").split("\n");
    lines.forEach((line, i) => {
      for (const [ch, name] of banned) {
        if (line.includes(ch)) {
          console.error(`${file}:${i + 1}: ${name}`);
          failures++;
        }
      }
    });
  }
}

const flatten = (obj, prefix = "") =>
  Object.entries(obj).flatMap(([k, v]) => (typeof v === "object" ? flatten(v, `${prefix}${k}.`) : [`${prefix}${k}`]));
const en = JSON.parse(readFileSync(join(root, "src/i18n/en.json"), "utf8"));
const de = JSON.parse(readFileSync(join(root, "src/i18n/de.json"), "utf8"));
const enKeys = new Set(flatten(en));
const deKeys = new Set(flatten(de));
for (const k of enKeys) if (!deKeys.has(k)) (console.error(`de.json is missing ${k}`), failures++);
for (const k of deKeys) if (!enKeys.has(k)) (console.error(`en.json is missing ${k}`), failures++);

// Static keys used in code: t("a.b") and i18n.t("a.b").
const pluralBase = new Set([...enKeys].map((k) => k.replace(/_(one|other)$/, "")));
for (const file of walk(join(root, "src"))) {
  if (![".ts", ".tsx"].includes(extname(file))) continue;
  const text = readFileSync(file, "utf8");
  for (const m of text.matchAll(/\bt\(\s*"([a-zA-Z_.]+)"/g)) {
    if (!pluralBase.has(m[1])) {
      console.error(`${file}: unknown translation key ${m[1]}`);
      failures++;
    }
  }
}

if (failures) {
  console.error(`\n${failures} problem(s) found`);
  process.exit(1);
}
console.log("copy check passed");
