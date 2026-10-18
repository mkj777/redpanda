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
- **Push**: IMAP IDLE on the inbox, other folders are checked every few minutes. Closing the window keeps redpanda running in the notification area.
- **Privacy**: mail HTML is sanitized in Rust and shown in a sandboxed frame without scripts; remote images stay blocked until you allow them, per message or per sender
- **Writing**: plain-text messages, reply, reply all, forward (with the original attachments), attachments, address autocomplete, drafts saved locally while you type, sending from your Gmail "Send mail as" addresses
- **Newsletters**: "Unsubscribe" next to the sender like in Gmail, with one click where the sender supports it and otherwise by email or on its page, plus a view of every newsletter sender and how often it writes
- **Search** over everything that has been synced, instantly (SQLite FTS5)
- **Keyboard**: `j`/`k` to move, `c` compose, `r`/`a`/`f` reply, reply all, forward, `e` archive, `#` delete, `s` star, `u` read/unread, `/` search
- **Claude Code**: an optional MCP server lets [Claude Code](https://claude.com/claude-code) read, search and send your mail through redpanda (below)
- English and German, light and dark theme following Windows

## Claude Code

redpanda can serve as an MCP server, so Claude Code reads and writes mail for you from every account you signed in to, without ever seeing a password or token.

1. In redpanda open Settings, Claude Code, and switch on "Allow access for Claude Code".
2. Click "Add to Claude Code". Without the `claude` command on your PATH, copy the command and run it in a terminal.
3. Start a new Claude Code session and check with `/mcp` that redpanda is connected.

The tools: `list_accounts`, `list_folders`, `list_messages`, `search_messages`, `read_message`, `read_thread`, `get_attachment`, `create_draft` and `send_mail`. Reading through Claude doesn't mark mail as read. Claude Code asks before every tool call it hasn't been allowed; keep `send_mail` on asking, so you see every email before it goes out.

The server listens on `127.0.0.1` only, needs the key shown in the settings, and only runs while redpanda does (also in the notification area). Details in [ADR 0008](docs/adr/0008-mcp-server.md).

## Install

Download `redpanda_x.y.z_x64-setup.exe` from [Releases](https://github.com/mkj777/redpanda/releases) and run it. It installs for your user only and needs no administrator rights. The installer isn't code-signed yet, so Windows SmartScreen may warn first: choose **More info** and then **Run anyway**.

- **Updates**: redpanda checks GitHub for a new version shortly after starting and every six hours, and offers to install it. Settings, General shows the version and has a button to check now. Updates are signed; redpanda only installs packages signed with the project's key.
- **Start with Windows**: on by default. redpanda then starts in the notification area without a window. Switch it off under Settings, General.
- **Uninstall** through Windows Settings, Apps. Tick "Delete the application data" to remove the local mail cache too, including the encrypted passwords and tokens. The encryption key stays in the Windows Credential Manager under "redpanda" until you delete it there.

## Build from source

Requirements: Windows 10/11, [Rust](https://rustup.rs) (stable), [Node.js](https://nodejs.org) 22 or newer, [pnpm](https://pnpm.io), and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) (Microsoft C++ Build Tools and WebView2, which Windows 11 already ships).

```sh
pnpm install
pnpm tauri dev      # run with hot reload
pnpm tauri build    # MSI and NSIS installers in src-tauri/target/release/bundle
```

`pnpm dev` alone starts the UI in a normal browser against built-in sample data, which is handy for working on the interface.

### Gmail and Outlook sign-in

The installers from Releases include redpanda's own OAuth client IDs, so signing in with Google or Microsoft needs no setup:

- **Outlook.com and Microsoft 365** sign in through Microsoft in your browser.
- **Gmail** signs in through Google too. Google has not verified redpanda yet, so it shows a warning first ("Google hasn't verified this app"): choose **Advanced** and then the link to redpanda. Unverified apps are limited to 100 Google accounts. If sign-in is refused, use an app password instead (needs 2-step verification, create it at [myaccount.google.com/apppasswords](https://myaccount.google.com/apppasswords)).

If you build redpanda yourself, the IDs are not included. Register your own clients and put them in `%APPDATA%\redpanda\oauth.json`, as described in **[docs/OAUTH_SETUP.md](docs/OAUTH_SETUP.md)**. Without them, Gmail works with an app password and every other provider works anyway; only Microsoft accounts need an ID.

### Provider notes

- **GMX / WEB.DE**: enable "POP3/IMAP Abruf" in the web mail settings first.
- **iCloud, Yahoo**: create an app-specific password in your account settings.
- **Telekom (t-online.de)**: use the separate email password, not the Telekom login password.

## Privacy

- Everything runs on your computer. Sign-in tokens stay on your machine; the bundled client IDs only identify the app to Google and Microsoft. redpanda talks only to your mail servers, to Google/Microsoft during sign-in, and, while you set up an account, to Thunderbird's autoconfig service and your mail domain's own autoconfig address to look up server settings. For Gmail accounts it reads the "Send mail as" addresses from your Gmail settings. When you unsubscribe from a newsletter, it also contacts that sender's unsubscribe address.
- Mail is cached in a local SQLite database in `%LOCALAPPDATA%\io.github.mkj777.redpanda`.
- Passwords and tokens are encrypted (ChaCha20-Poly1305) with a key kept in the Windows Credential Manager. They never reach the user interface or log files. See [ADR 0002](docs/adr/0002-secrets-encrypted-in-sqlite.md).
- With access for Claude Code switched on, Claude Code can read and send mail through redpanda's local MCP server. What Claude reads goes to Anthropic like everything else in a Claude Code session. It's off by default.
- No telemetry. The update check only downloads `latest.json` from GitHub.

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
