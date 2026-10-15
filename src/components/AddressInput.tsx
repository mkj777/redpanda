import { X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, type Address } from "@/lib/ipc";
import { cn, displayName, isEmail, parseAddresses } from "@/lib/format";

export function AddressInput({
  id,
  label,
  value,
  onChange,
  autoFocus,
}: {
  id: string;
  label: string;
  value: Address[];
  onChange: (v: Address[]) => void;
  autoFocus?: boolean;
}) {
  const { t } = useTranslation();
  const [text, setText] = useState("");
  const [suggestions, setSuggestions] = useState<Address[]>([]);
  const [active, setActive] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const q = text.trim();
    if (q.length < 1) {
      setSuggestions([]);
      return;
    }
    let alive = true;
    const id = setTimeout(() => {
      api
        .suggestContacts(q)
        .then((list) => {
          if (!alive) return;
          const taken = new Set(value.map((a) => a.email.toLowerCase()));
          setSuggestions(list.filter((a) => !taken.has(a.email.toLowerCase())));
          setActive(0);
        })
        .catch(() => setSuggestions([]));
    }, 120);
    return () => {
      alive = false;
      clearTimeout(id);
    };
  }, [text, value]);

  const add = (list: Address[]) => {
    const taken = new Set(value.map((a) => a.email.toLowerCase()));
    const fresh = list.filter((a) => !taken.has(a.email.toLowerCase()));
    if (fresh.length) onChange([...value, ...fresh]);
    setText("");
    setSuggestions([]);
  };

  const commit = () => {
    const parsed = parseAddresses(text);
    if (parsed.length) add(parsed);
    else if (isEmail(text)) add([{ name: null, email: text.trim() }]);
  };

  return (
    <div className="relative flex min-h-10 items-center gap-2 border-b border-line px-4">
      <label htmlFor={id} className="w-12 shrink-0 text-[13px] text-muted">
        {label}
      </label>
      <div className="flex flex-1 flex-wrap items-center gap-1 py-1.5" onClick={() => inputRef.current?.focus()}>
        {value.map((a) => (
          <span
            key={a.email}
            title={a.email}
            className={cn(
              "inline-flex h-6 items-center gap-1 rounded-md border pl-2 pr-1 text-[13px]",
              isEmail(a.email) ? "border-line-strong bg-bg" : "border-danger text-danger",
            )}
          >
            {displayName(a)}
            <button
              aria-label={t("compose.removeRecipient", { name: displayName(a) })}
              onClick={(e) => {
                e.stopPropagation();
                onChange(value.filter((x) => x.email !== a.email));
              }}
              className="rounded p-0.5 text-faint hover:text-fg"
            >
              <X size={12} />
            </button>
          </span>
        ))}
        <input
          id={id}
          ref={inputRef}
          autoFocus={autoFocus}
          value={text}
          autoComplete="off"
          onChange={(e) => setText(e.target.value)}
          onBlur={() => setTimeout(commit, 120)}
          onPaste={(e) => {
            const pasted = e.clipboardData.getData("text");
            const parsed = parseAddresses(pasted);
            if (parsed.length > 1) {
              e.preventDefault();
              add(parsed);
            }
          }}
          onKeyDown={(e) => {
            if (suggestions.length && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
              e.preventDefault();
              setActive((i) => (i + (e.key === "ArrowDown" ? 1 : -1) + suggestions.length) % suggestions.length);
            } else if ((e.key === "Enter" || e.key === "Tab") && suggestions.length && text) {
              e.preventDefault();
              add([suggestions[active]]);
            } else if (e.key === "Enter" || e.key === "," || e.key === ";") {
              if (text.trim()) {
                e.preventDefault();
                commit();
              }
            } else if (e.key === "Backspace" && !text && value.length) {
              onChange(value.slice(0, -1));
            } else if (e.key === "Escape" && suggestions.length) {
              e.stopPropagation();
              setSuggestions([]);
            }
          }}
          className="h-6 min-w-32 flex-1 bg-transparent text-sm outline-none focus-visible:outline-none"
        />
      </div>
      {suggestions.length > 0 && (
        <div className="absolute left-16 top-full z-10 mt-1 w-80 rounded-md border border-line bg-surface p-1 shadow-[0_2px_8px_rgba(0,0,0,0.1)]">
          {suggestions.map((s, i) => (
            <button
              key={s.email}
              onMouseDown={(e) => {
                e.preventDefault();
                add([s]);
              }}
              className={cn("flex w-full flex-col rounded px-2 py-1 text-left", i === active && "bg-hover")}
            >
              <span className="truncate text-[13px]">{s.name || s.email}</span>
              {s.name && <span className="truncate text-xs text-muted">{s.email}</span>}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
