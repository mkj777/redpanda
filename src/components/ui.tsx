import * as RadixDialog from "@radix-ui/react-dialog";
import * as Tooltip from "@radix-ui/react-tooltip";
import { X } from "lucide-react";
import { forwardRef, type ButtonHTMLAttributes, type InputHTMLAttributes, type ReactNode } from "react";
import { cn, initials } from "@/lib/format";
import type { Address } from "@/lib/ipc";

type Variant = "primary" | "soft" | "secondary" | "ghost" | "danger";

const variants: Record<Variant, string> = {
  primary: "bg-accent text-accent-fg hover:bg-accent-hover border border-transparent",
  soft: "bg-accent/12 text-accent hover:bg-accent/18 border border-transparent",
  secondary: "bg-surface text-fg border border-line-strong hover:bg-hover",
  ghost: "text-fg hover:bg-hover border border-transparent",
  danger: "bg-surface text-danger border border-line-strong hover:bg-hover",
};

export const Button = forwardRef<
  HTMLButtonElement,
  ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant; size?: "sm" | "md" }
>(function Button({ variant = "secondary", size = "md", className, ...props }, ref) {
  return (
    <button
      ref={ref}
      className={cn(
        "inline-flex items-center justify-center gap-1.5 rounded-md font-medium transition-colors duration-100",
        "disabled:opacity-50 disabled:pointer-events-none whitespace-nowrap",
        size === "sm" ? "h-7 px-2.5 text-[13px]" : "h-8 px-3 text-sm",
        variants[variant],
        className,
      )}
      {...props}
    />
  );
});

export const IconButton = forwardRef<
  HTMLButtonElement,
  ButtonHTMLAttributes<HTMLButtonElement> & { label: string; active?: boolean; shortcut?: string }
>(function IconButton({ label, active, shortcut, className, children, ...props }, ref) {
  return (
    <Tooltip.Root delayDuration={400}>
      <Tooltip.Trigger asChild>
        <button
          ref={ref}
          aria-label={label}
          className={cn(
            "inline-flex size-8 shrink-0 items-center justify-center rounded-md text-muted transition-colors duration-100",
            "hover:bg-hover hover:text-fg disabled:opacity-40 disabled:pointer-events-none",
            active && "text-accent",
            className,
          )}
          {...props}
        >
          {children}
        </button>
      </Tooltip.Trigger>
      <Tooltip.Portal>
        <Tooltip.Content
          side="bottom"
          sideOffset={4}
          className="z-50 rounded-md border border-line bg-surface px-2 py-1 text-xs text-fg shadow-[0_2px_8px_rgba(0,0,0,0.08)]"
        >
          {label}
          {shortcut && <kbd className="ml-2 font-sans text-faint">{shortcut}</kbd>}
        </Tooltip.Content>
      </Tooltip.Portal>
    </Tooltip.Root>
  );
});

export const Input = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement>>(function Input(
  { className, ...props },
  ref,
) {
  return (
    <input
      ref={ref}
      className={cn(
        "h-8 w-full rounded-md border border-line-strong bg-surface px-2.5 text-sm text-fg outline-none",
        "placeholder:text-faint focus:border-accent focus-visible:outline-none",
        className,
      )}
      {...props}
    />
  );
});

export function Select<T extends string>({
  value,
  onChange,
  options,
  className,
  id,
}: {
  value: T;
  onChange: (v: T) => void;
  options: { value: T; label: string }[];
  className?: string;
  id?: string;
}) {
  return (
    <select
      id={id}
      value={value}
      onChange={(e) => onChange(e.target.value as T)}
      className={cn(
        "h-8 rounded-md border border-line-strong bg-surface px-2 text-sm text-fg outline-none focus:border-accent focus-visible:outline-none",
        className,
      )}
    >
      {options.map((o) => (
        <option key={o.value} value={o.value}>
          {o.label}
        </option>
      ))}
    </select>
  );
}

export function Field({ label, htmlFor, children, className }: { label: string; htmlFor?: string; children: ReactNode; className?: string }) {
  return (
    <div className={cn("flex flex-col gap-1", className)}>
      <label htmlFor={htmlFor} className="text-[13px] font-medium text-muted">
        {label}
      </label>
      {children}
    </div>
  );
}

export function Switch({
  checked,
  onChange,
  label,
  hint,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: string;
  hint?: string;
}) {
  return (
    <label className="flex cursor-pointer items-center justify-between gap-4 text-sm">
      <span>
        {label}
        {hint && <span className="mt-0.5 block text-xs text-muted">{hint}</span>}
      </span>
      <button
        role="switch"
        aria-checked={checked}
        onClick={() => onChange(!checked)}
        className={cn(
          "relative h-5 w-9 shrink-0 rounded-full border transition-colors duration-150",
          checked ? "border-accent bg-accent" : "border-line-strong bg-hover",
        )}
      >
        <span
          className={cn(
            "absolute top-0.5 size-3.5 rounded-full bg-white transition-[left] duration-150",
            checked ? "left-[18px]" : "left-0.5",
          )}
        />
      </button>
    </label>
  );
}

export function Avatar({ address, className }: { address: Address | null; className?: string }) {
  return (
    <div
      aria-hidden
      className={cn(
        "flex size-9 shrink-0 items-center justify-center rounded-full bg-hover text-xs font-semibold text-muted",
        className,
      )}
    >
      {initials(address)}
    </div>
  );
}

export function Spinner({ className }: { className?: string }) {
  return (
    <span
      className={cn("inline-block size-4 animate-spin rounded-full border-2 border-line-strong border-t-accent", className)}
      aria-hidden
    />
  );
}

export function Dialog({
  open,
  onOpenChange,
  title,
  children,
  width = 480,
  closeLabel,
  onEscapeKeyDown,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  children: ReactNode;
  width?: number;
  closeLabel: string;
  onEscapeKeyDown?: (e: KeyboardEvent) => void;
}) {
  return (
    <RadixDialog.Root open={open} onOpenChange={onOpenChange}>
      <RadixDialog.Portal>
        <RadixDialog.Overlay className="fixed inset-0 z-40 bg-black/30" />
        <RadixDialog.Content
          onEscapeKeyDown={onEscapeKeyDown}
          onOpenAutoFocus={(e) => {
            // Focus the first field instead of the close button (whose tooltip would pop up).
            const root = e.currentTarget as HTMLElement;
            e.preventDefault();
            // React's autoFocus already focused a field inside: keep it.
            if (root.contains(document.activeElement) && document.activeElement !== root) return;
            const field = root.querySelector<HTMLElement>("[autofocus], input:not([data-inline]), textarea, select");
            (field ?? root).focus();
          }}
          aria-describedby={undefined}
          style={{ width }}
          tabIndex={-1}
          className="fixed left-1/2 top-1/2 z-50 flex max-h-[88vh] max-w-[calc(100vw-32px)] -translate-x-1/2 -translate-y-1/2 flex-col rounded-lg border border-line bg-surface shadow-[0_2px_8px_rgba(0,0,0,0.12)] outline-none"
        >
          <div className="flex h-12 shrink-0 items-center justify-between border-b border-line pl-4 pr-2">
            <RadixDialog.Title className="text-[15px] font-semibold">{title}</RadixDialog.Title>
            <RadixDialog.Close asChild>
              <IconButton label={closeLabel}>
                <X size={16} />
              </IconButton>
            </RadixDialog.Close>
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto">{children}</div>
        </RadixDialog.Content>
      </RadixDialog.Portal>
    </RadixDialog.Root>
  );
}
