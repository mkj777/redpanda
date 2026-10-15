import { useEffect, useMemo, useRef, useState } from "react";
import { openExternal } from "@/lib/ipc";
import { composeTo } from "@/lib/actions";
import { linkify } from "@/lib/format";

function handleLink(href: string) {
  if (href.toLowerCase().startsWith("mailto:")) {
    const email = decodeURIComponent(href.slice(7).split("?")[0]);
    if (email) composeTo(email);
    return;
  }
  if (/^https?:\/\//i.test(href)) void openExternal(href);
}

function srcdoc(html: string, allowRemote: boolean) {
  const img = allowRemote ? "data: https: http:" : "data:";
  const csp = `default-src 'none'; img-src ${img}; style-src 'unsafe-inline'; font-src data:${allowRemote ? " https:" : ""}`;
  return `<!doctype html><html><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="${csp}">
<style>
html{background:#fff;overflow-y:hidden}
body{margin:0;padding:16px 20px;font:14px/1.5 -apple-system,"Segoe UI",system-ui,sans-serif;color:#1c1917;overflow-wrap:anywhere}
img{max-width:100%;height:auto}
table{max-width:100%}
a{color:#b45309}
blockquote{margin:0 0 0 4px;padding-left:12px;border-left:2px solid #ddd;color:#555}
pre{white-space:pre-wrap}
</style></head><body>${html}</body></html>`;
}

/**
 * Sanitized mail HTML in a sandboxed iframe: no scripts (no `allow-scripts`), remote content
 * blocked by CSP until allowed. `allow-same-origin` lets the parent measure the height and
 * intercept link clicks; without `allow-scripts` the mail itself can still run nothing.
 */
export function HtmlBody({ html, allowRemote }: { html: string; allowRemote: boolean }) {
  const ref = useRef<HTMLIFrameElement>(null);
  const [height, setHeight] = useState(120);
  const doc = useMemo(() => srcdoc(html, allowRemote), [html, allowRemote]);

  useEffect(() => {
    const frame = ref.current;
    if (!frame) return;
    let observer: ResizeObserver | null = null;
    const onLoad = () => {
      const d = frame.contentDocument;
      if (!d) return;
      // +2 for the iframe's own border.
      const measure = () => setHeight(Math.max(60, d.documentElement.scrollHeight + 2));
      measure();
      observer = new ResizeObserver(measure);
      observer.observe(d.documentElement);
      d.addEventListener("click", (e) => {
        const a = (e.target as Element | null)?.closest?.("a");
        if (!a) return;
        e.preventDefault();
        const href = a.getAttribute("href");
        if (href) handleLink(href);
      });
    };
    frame.addEventListener("load", onLoad);
    return () => {
      frame.removeEventListener("load", onLoad);
      observer?.disconnect();
    };
  }, [doc]);

  return (
    <iframe
      ref={ref}
      title="message"
      sandbox="allow-same-origin"
      srcDoc={doc}
      style={{ height }}
      className="block w-full rounded-md border border-line bg-[var(--mail-bg)]"
    />
  );
}

function TextLine({ line }: { line: string }) {
  const parts = useMemo(() => linkify(line), [line]);
  return (
    <div className={line.trimStart().startsWith(">") ? "text-muted" : undefined}>
      {parts.map((p, i) =>
        p.href ? (
          <a
            key={i}
            href={p.href}
            onClick={(e) => {
              e.preventDefault();
              handleLink(p.href!);
            }}
            className="text-accent underline-offset-2 hover:underline"
          >
            {p.text}
          </a>
        ) : (
          <span key={i}>{p.text}</span>
        ),
      )}
      {line === "" && <>&nbsp;</>}
    </div>
  );
}

/** Plain-text mail with clickable links; quoted lines are dimmed. */
export function TextBody({ text }: { text: string }) {
  const lines = useMemo(() => text.replace(/\r\n/g, "\n").trimEnd().split("\n"), [text]);
  return (
    <div className="selectable whitespace-pre-wrap break-words px-1 text-[14px] leading-relaxed">
      {lines.map((l, i) => (
        <TextLine key={i} line={l} />
      ))}
    </div>
  );
}
