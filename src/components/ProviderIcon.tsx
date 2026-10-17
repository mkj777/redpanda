import { Mail } from "lucide-react";
import type { Account } from "@/lib/ipc";

type Brand = "google" | "microsoft" | "apple" | null;

function brandOf(account: Account): Brand {
  if (account.provider === "google" || account.authType === "oauth_google" || account.isGmail) return "google";
  if (account.provider === "microsoft" || account.authType === "oauth_microsoft") return "microsoft";
  if (account.provider === "icloud") return "apple";
  return null;
}

function Logo({ brand, size }: { brand: Brand; size: number }) {
  switch (brand) {
    case "google":
      return (
        <svg width={size} height={size} viewBox="0 0 48 48" aria-hidden>
          <path fill="#EA4335" d="M24 9.5c3.54 0 6.71 1.22 9.21 3.6l6.85-6.85C35.9 2.38 30.47 0 24 0 14.62 0 6.51 5.38 2.56 13.22l7.98 6.19C12.43 13.72 17.74 9.5 24 9.5z" />
          <path fill="#4285F4" d="M46.98 24.55c0-1.57-.15-3.09-.38-4.55H24v9.02h12.94c-.58 2.96-2.26 5.48-4.78 7.18l7.73 6c4.51-4.18 7.09-10.36 7.09-17.65z" />
          <path fill="#FBBC05" d="M10.53 28.59c-.48-1.45-.76-2.99-.76-4.59s.27-3.14.76-4.59l-7.98-6.19C.92 16.46 0 20.12 0 24c0 3.88.92 7.54 2.56 10.78l7.97-6.19z" />
          <path fill="#34A853" d="M24 48c6.48 0 11.93-2.13 15.89-5.81l-7.73-6c-2.15 1.45-4.92 2.3-8.16 2.3-6.26 0-11.57-4.22-13.47-9.91l-7.98 6.19C6.51 42.62 14.62 48 24 48z" />
        </svg>
      );
    case "microsoft":
      return (
        <svg width={size} height={size} viewBox="0 0 23 23" aria-hidden>
          <path fill="#F25022" d="M1 1h10v10H1z" />
          <path fill="#7FBA00" d="M12 1h10v10H12z" />
          <path fill="#00A4EF" d="M1 12h10v10H1z" />
          <path fill="#FFB900" d="M12 12h10v10H12z" />
        </svg>
      );
    case "apple":
      return (
        <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden className="text-fg">
          <path
            fill="currentColor"
            d="M12.152 6.896c-.948 0-2.415-1.078-3.96-1.04-2.04.027-3.91 1.183-4.961 3.014-2.117 3.675-.546 9.103 1.519 12.09 1.013 1.454 2.208 3.09 3.792 3.039 1.52-.065 2.09-.987 3.935-.987 1.831 0 2.35.987 3.96.948 1.637-.026 2.676-1.48 3.676-2.948 1.156-1.688 1.636-3.325 1.662-3.415-.039-.013-3.182-1.221-3.22-4.857-.026-3.04 2.48-4.494 2.597-4.559-1.429-2.09-3.623-2.324-4.39-2.376-2-.156-3.675 1.09-4.61 1.09zM15.53 3.83c.843-1.012 1.4-2.427 1.245-3.83-1.207.052-2.662.805-3.532 1.818-.78.896-1.454 2.338-1.273 3.714 1.338.104 2.715-.688 3.559-1.701"
          />
        </svg>
      );
    default:
      return <Mail size={size} className="text-muted" aria-hidden />;
  }
}

/** Provider logo in a small tile, with the account color as a dot in the corner. */
export function ProviderIcon({ account }: { account: Account }) {
  return (
    <span className="relative inline-flex size-8 shrink-0 items-center justify-center rounded-md border border-line bg-bg">
      <Logo brand={brandOf(account)} size={16} />
      <span
        className="absolute -bottom-0.5 -right-0.5 size-2.5 rounded-full border-2 border-surface"
        style={{ background: account.color }}
      />
    </span>
  );
}
