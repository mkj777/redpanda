<p align="center"><img src="src-tauri/icons/128x128@2x.png" width="96" alt=""></p>

# redpanda

A minimal desktop mail client for Windows. Gmail, Outlook/Microsoft 365 and any IMAP/SMTP mailbox in one window, built with Tauri and Rust.

> Status: early. It works day to day for its author, but expect rough edges. See the [roadmap](docs/PLAN.md).

## Features

- **Accounts**: Gmail and Outlook/Microsoft 365 with OAuth sign-in in your browser, everything else (GMX, WEB.DE, iCloud, Yahoo, Posteo, mailbox.org, your own domain) with a password or app password
- **Autodiscovery**: type your address and redpanda finds the server settings (built-in provider list, MX records for Google Workspace and Microsoft 365 domains, Thunderbird's ISPDB and autoconfig, SRV records, educated guesses). A manual form is always there.
- **Unified views**: all inboxes, unread and starred across every account, each account marked with its color
- **Three resizable columns** that remember their width; the folder column collapses to an icon rail
- **Conversations**: messages grouped by thread, can be turned off in settings
- **Push**: IMAP IDLE on the inbox, other folders are checked every few minutes
- **Privacy**: mail HTML is sanitized in Rust and shown in a sandboxed frame without scripts; remote images stay blocked until you allow them, per message or per sender
- **Writing**: plain-text messages, reply, reply all, forward (with the original attachments), attachments, address autocomplete, drafts saved locally while you type
- **Search** over everything that has been synced, instantly (SQLite FTS5)
- **Keyboard**: `j`/`k` to move, `c` compose, `r`/`a`/`f` reply, reply all, forward, `e` archive, `#` delete, `s` star, `u` read/unread, `/` search
- English and German, light and dark theme following Windows

## Install

Prebuilt installers will be published under [Releases](https://github.com/mkj777/redpanda/releases). Until then, build it yourself (below).

## Build from source

Requirements: Windows 10/11, [Rust](https://rustup.rs) (stable), [Node.js](https://nodejs.org) 22 or newer, [pnpm](https://pnpm.io), and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) (Microsoft C++ Build Tools and WebView2, which Windows 11 already ships).

```sh
pnpm install
pnpm tauri dev      # run with hot reload
pnpm tauri build    # MSI and NSIS installers in src-tauri/target/release/bundle
```

`pnpm dev` alone starts the UI in a normal browser against built-in sample data, which is handy for working on the interface.

### Gmail and Outlook sign-in

Google and Microsoft sign-in needs OAuth client IDs that you register yourself, once, for free. redpanda does not ship shared IDs: Google only allows full mailbox access for verified apps, and that verification includes a paid security audit. The step-by-step guide is in **[docs/OAUTH_SETUP.md](docs/OAUTH_SETUP.md)**. In short:

1. Create a Google Cloud OAuth client of type "Desktop app" and/or a Microsoft Entra app registration (public client, redirect `http://localhost`).
2. Copy [`oauth.example.json`](oauth.example.json) to `%APPDATA%\redpanda\oauth.json` and fill in the IDs.
3. Restart redpanda.

Without it, Gmail still works with an app password, and every other provider works anyway.

### Provider notes

- **GMX / WEB.DE**: enable "POP3/IMAP Abruf" in the web mail settings first.
- **iCloud, Yahoo**: create an app-specific password in your account settings.
- **Telekom (t-online.de)**: use the separate email password, not the Telekom login password.

## Privacy

- Everything runs on your computer. redpanda talks only to your mail servers, to Google/Microsoft during sign-in, and, while you set up an account, to Thunderbird's autoconfig service and your mail domain's own autoconfig address to look up server settings.
- Mail is cached in a local SQLite database in `%LOCALAPPDATA%\redpanda`.
- Passwords and tokens are encrypted (ChaCha20-Poly1305) with a key kept in the Windows Credential Manager. They never reach the user interface or log files. See [ADR 0002](docs/adr/0002-secrets-encrypted-in-sqlite.md).
- No telemetry.

## Development

```sh
pnpm typecheck && pnpm test && pnpm check:copy    # frontend
cd src-tauri && cargo fmt --check && cargo clippy --all-targets && cargo test
```

The end-to-end IMAP/SMTP test runs against a local [GreenMail](https://greenmail-mail-test.github.io/greenmail/) server:

```sh
docker run -d --rm -p 3025:3025 -p 3143:3143 greenmail/standalone:2.1.3
cd src-tauri && cargo test --test greenmail -- --ignored
```

Set `REDPANDA_LOG=redpanda_lib=debug` for verbose logs. The architecture is described in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md), decisions in [docs/adr](docs/adr).

## License

[MIT](LICENSE)
