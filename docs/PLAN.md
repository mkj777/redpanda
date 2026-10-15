# redpanda roadmap

redpanda is a minimal desktop mail client for Windows that brings Gmail, Outlook/Microsoft 365 and any IMAP/SMTP mailbox into one window. It is built with Tauri 2: all network and mail work runs in Rust, the React frontend only renders local data and calls commands.

## Goals for v1

- Accounts: Gmail, Outlook/Microsoft 365, and any IMAP/SMTP provider (GMX, web.de, iCloud, own domain, and so on)
- OAuth in the system browser wherever the provider supports it, password or app password otherwise
- Server autodiscovery from the email address
- Unified inbox across all accounts
- Three resizable columns: folders, message list, reading pane
- Plain-text compose, reply, reply all, forward
- UI in English and German

## Non-goals for v1

- HTML compose
- Calendar and contacts
- Syncing drafts to the IMAP Drafts folder (drafts are stored locally)
- POP3, Exchange Web Services, JMAP

## Build order

Each milestone is usable on its own and lands on `main`.

| # | Milestone | Content |
|---|---|---|
| 1 | Scaffold | Tauri 2 + React + TS + Vite + Tailwind, i18n, three-panel layout |
| 2 | Store + IMAP with password | SQLite schema and migrations, custom IMAP account, folder list, initial header sync, message list from the DB |
| 3 | Reading pane | Lazy body fetch, MIME parsing, sanitizing, sandboxed iframe, remote image blocking, attachments |
| 4 | Autodiscovery + account wizard | Provider table, MX, Mozilla ISPDB / autoconfig, RFC 6186 SRV, guessing, manual form |
| 5 | OAuth Google + Microsoft | PKCE loopback flow, token storage and refresh, XOAUTH2 for IMAP and SMTP |
| 6 | Incremental sync + IDLE + actions | Flags, move, delete, archive, unified views, live UI updates |
| 7 | Compose + send | SMTP (password and XOAUTH2), reply/forward, Sent append, local drafts |
| 8 | Polish | FTS search, keyboard shortcuts, settings, dark mode, README with OAuth guide, CI |

## Verification

- Rust unit tests: autodiscovery (provider table, MX classification, ISPDB XML parsing from fixtures), MIME parsing and sanitizing with fixture mails, threading, PKCE construction, secret encryption round trip.
- IMAP/SMTP integration test against a local GreenMail container: sync, flag changes, move, delete, send plus append.
- Frontend: Vitest for stores and i18n key parity, plus a check that fails on em-dashes and middle dots in locale files.
- Manual end-to-end with `pnpm tauri dev` against a Gmail, an Outlook and a GMX account.
- `pnpm tauri build` produces a working MSI/NSIS installer.

See [ARCHITECTURE.md](ARCHITECTURE.md) for the module layout and [OAUTH_SETUP.md](OAUTH_SETUP.md) for registering your own OAuth clients.
