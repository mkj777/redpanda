# Architecture

```
React UI (Vite, TS) --invoke/events--> Tauri commands (Rust)
                                         |-- accounts/   autodiscovery, OAuth, credentials
                                         |-- sync/       per-account IMAP sync task + IDLE
                                         |-- store/      SQLite (rusqlite, FTS5)
                                         |-- send/       SMTP via lettre
                                         `-- mime/       parse (mail-parser), sanitize (ammonia)
```

The frontend never talks to a mail server. It reads data through Tauri commands that query SQLite, and it triggers work (sync, actions, sending) through commands. The Rust side emits events so the UI knows when to refetch:

| Event | Payload | Meaning |
|---|---|---|
| `mail:changed` | `{ accountId, folderId? }` | Messages or folders of an account changed in the DB |
| `sync:status` | `{ accountId, state, error? }` | `idle`, `syncing`, `error`, `auth_required` |

## Modules (`src-tauri/src`)

### `accounts/`

- `providers.rs`: built-in table of well-known providers (hosts, ports, auth type, hints, whether the server stores sent mail itself).
- `discovery.rs`: resolves server settings from an email address. The chain stops at the first hit: provider table, MX classification (Google Workspace, Microsoft 365), Mozilla ISPDB / autoconfig XML, RFC 6186 SRV records, `imap.`/`smtp.` guessing with a TLS probe. The manual form is the last fallback.
- `oauth.rs`: authorization code flow with PKCE in the system browser. A loopback listener on `127.0.0.1:<random port>` receives the redirect. Tokens are refreshed before every IMAP/SMTP login. Client IDs come from environment variables, `%APPDATA%\redpanda\oauth.json` or, in release builds, from CI secrets compiled in with `option_env!` (see ADR 0004). They are never in the repository.
- `secrets.rs`: a random 256-bit key lives in the Windows Credential Manager. Passwords and tokens are encrypted with ChaCha20-Poly1305 and stored in SQLite (see ADR 0002).

### `store/`

SQLite in the app data directory with versioned migrations (`store/migrations/NNNN_*.sql`, tracked via `PRAGMA user_version`).

| Table | Content |
|---|---|
| `accounts` | email, display name, provider, auth type, IMAP/SMTP settings, color |
| `folders` | path, role, UIDVALIDITY, UIDNEXT, HIGHESTMODSEQ, unread count |
| `messages` | envelope data, flags, thread key, preview, size |
| `bodies` | plain text, sanitized HTML, attachment metadata |
| `messages_fts` | FTS5 index over subject, from, to, preview, body text |
| `secrets` | encrypted credentials per account |
| `drafts` | locally autosaved drafts |

### `sync/`

- One supervisor task per account, restarted with exponential backoff on errors.
- Initial sync fetches the newest 500 messages per folder (headers plus a short preview). Older mail loads on demand.
- Incremental sync checks UIDVALIDITY, fetches new UIDs, refreshes flags with CONDSTORE `CHANGEDSINCE` when available (otherwise for the newest window), and detects deletions within the window.
- A dedicated connection IDLEs on INBOX. Other folders are polled every 5 minutes and when opened.
- Actions (read, star, move, delete, archive) are applied to the DB first, then sent to the server.

### `mime/`

- `parse.rs`: turns raw RFC 5322 messages into text, HTML and attachments (mail-parser), builds previews and thread keys.
- `sanitize.rs`: ammonia allow list, inlines `cid:` images as data URLs, blocks remote images unless allowed.

### `send/`

- `compose.rs`: builds the outgoing message (reply headers, quoting, attachments).
- `smtp.rs`: delivery through lettre with password or XOAUTH2, then IMAP APPEND to Sent unless the provider stores sent mail itself.

### `titlebar.rs`

The window has no native frame (`decorations: false`); the page draws the title bar. Snap layouts need a native child window over the maximize button, see [ADR 0005](adr/0005-custom-title-bar.md).

## Frontend (`src`)

- `lib/ipc.ts`: typed wrappers around `invoke` and `listen`.
- `stores/`: Zustand stores for accounts, folders, messages and UI state.
- `components/`: TitleBar, Sidebar, MessageList (virtualized), ReadingPane, Composer, AccountWizard, Settings.
- `i18n/`: `en.json` and `de.json` with identical keys.

## HTML safety

Mail HTML is sanitized in Rust and rendered in an `<iframe sandbox srcdoc>` without `allow-scripts`, with a CSP that only allows inline styles and `data:` images (plus `https:` images after the user allows remote content). Links are intercepted and opened in the system browser.
