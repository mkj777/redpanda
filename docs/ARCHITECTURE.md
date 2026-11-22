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
- `aliases.rs`: reads the "Send mail as" addresses of Gmail accounts from the Gmail API after a full sync, at most once an hour (see ADR 0007).
- `secrets.rs`: a random 256-bit key lives in the Windows Credential Manager. Passwords and tokens are encrypted with ChaCha20-Poly1305 and stored in SQLite (see ADR 0002).

### `store/`

SQLite in the app data directory with versioned migrations (`store/migrations/NNNN_*.sql`, tracked via `PRAGMA user_version`).

| Table | Content |
|---|---|
| `accounts` | email, display name, provider, auth type, IMAP/SMTP settings, color |
| `folders` | path, role, UIDVALIDITY, UIDNEXT, HIGHESTMODSEQ, unread count |
| `messages` | envelope data, flags, thread key, preview, size, unsubscribe options |
| `bodies` | plain text, sanitized HTML, attachment metadata |
| `messages_fts` | FTS5 index over subject, from, to, preview, body text and attachment names |
| `secrets` | encrypted credentials per account |
| `aliases` | further addresses an account may send from |
| `drafts` | locally autosaved drafts, with the address they go out from |
| `unsubscribed` | newsletter senders the user left, per account |
| `downloads` | the last 20 attachments saved to the Downloads folder, for the download list |

### `sync/`

- One supervisor task per account, restarted with exponential backoff on errors.
- Initial sync fetches the newest 500 messages per folder (headers plus a short preview). After every full sync the background worker loads the headers of older mail in batches of 500 until every visible folder is complete, so lists and search cover the whole mailbox. UIDs do not follow dates in every mailbox, so a window of the highest UIDs can miss recent mail. Scrolling past the end of a list still loads older mail on demand. Once the headers are in, the same worker loads the text of every message for search, see [ADR 0011](adr/0011-full-text-search.md).
- Incremental sync checks UIDVALIDITY, fetches new UIDs, refreshes flags with CONDSTORE `CHANGEDSINCE` when available (otherwise for the newest window), and detects deletions within the window.
- A dedicated connection IDLEs on INBOX and syncs it once every time it (re)connects, so mail that arrived while it was down shows up too. Other folders are polled every 5 minutes and when opened.
- Each account has four IMAP connections: one for user actions and opening mail, one for the background sync, one that IDLEs, and one for searches on the server, so typing a search never holds up opening a mail.
- All IMAP connections use TCP keepalive (first probe after 60 s), so one the network dropped silently fails within about 90 seconds and reconnects, instead of an IDLE that stops hearing of new mail.
- Actions (read, star, move, delete, archive) are applied to the DB first, then sent to the server.
- Folders synced before the unsubscribe column existed read the list headers of their stored messages once (`HEADER.FIELDS`).

### `mime/`

- `parse.rs`: turns raw RFC 5322 messages into text, HTML and attachments (mail-parser), builds previews and thread keys. Attachment names go through `safe_file_name` before anything is written to disk.
- `sanitize.rs`: ammonia allow list, inlines `cid:` images as data URLs, blocks remote images unless allowed.

### `send/`

- `compose.rs`: builds the outgoing message (reply headers, quoting, attachments). The sender is the account's own address or one of its aliases, checked against the `aliases` table before sending.
- `smtp.rs`: delivery through lettre with password or XOAUTH2, then IMAP APPEND to Sent unless the provider stores sent mail itself.

### `unsubscribe.rs`

Reads `List-Unsubscribe` and `List-Unsubscribe-Post` during sync and unsubscribes by one-click POST, by mail or through the sender's page, see [ADR 0006](adr/0006-unsubscribe.md).

### Downloads and dragging out

`download_attachment` saves into the Downloads folder under a free name and lists it in `downloads`. `drag_attachment` and `drag_download` run a native file drag (the `drag` crate) on the main thread, see [ADR 0012](adr/0012-downloads-and-drag-out.md).

### `titlebar.rs`

The window has no native frame (`decorations: false`); the page draws the title bar. Snap layouts need a native child window over the maximize button, see [ADR 0005](adr/0005-custom-title-bar.md).

## Frontend (`src`)

- `lib/ipc.ts`: typed wrappers around `invoke` and `listen`.
- `stores/`: Zustand stores for accounts, folders, messages and UI state.
- `components/`: TitleBar with the download list (Downloads), Sidebar, MessageList (virtualized) with its right-click menu (MessageMenu), ReadingPane, Subscriptions (newsletter senders), Unsubscribe (confirmation dialog), Composer, AccountWizard, Settings.
- `i18n/`: `en.json` and `de.json` with identical keys.

## HTML safety

Mail HTML is sanitized in Rust and rendered in an `<iframe sandbox srcdoc>` without `allow-scripts`, with a CSP that only allows inline styles and `data:` images (plus `https:` images after the user allows remote content). Links are intercepted and opened in the system browser.
