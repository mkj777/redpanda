# 0008: An MCP server for Claude Code inside the app

Status: accepted

## Context

Claude Code can use tools from MCP servers. With access to redpanda it could read, search and answer the user's mail from every account at once, including the Gmail "Send mail as" addresses. redpanda already holds what that needs: the mail cache, the encrypted sign-ins, OAuth refresh and sending with alias checks.

Two ways were possible: a separate program Claude Code starts, using the same database, or a server inside the running app. A second process would need its own sync, would race the app when refreshing OAuth tokens (Microsoft rotates refresh tokens, and the refresh lock only works inside one process), and the app would only show its changes after the next sync.

## Decision

- The app itself serves MCP over Streamable HTTP on `127.0.0.1`, port 47821 by default and changeable in the settings. It uses the official Rust SDK (`rmcp`) on axum, and the tools call the same `App` as the Tauri commands.
- It is off until the user switches it on under Settings, Claude Code, or in the tray menu.
- Access:
  - Every request needs `Authorization: Bearer <key>`. The key has 256 random bits, is created on first start, stored encrypted like the account secrets (in `app_settings`), and compared in constant time. A new key replaces the old one at once.
  - Only the `Host` names `127.0.0.1:<port>` and `localhost:<port>` are accepted, and requests with an `Origin` header are refused. Web pages in the user's browser therefore can't reach the server, also not through DNS rebinding.
- The settings show the key (masked until asked) and can register redpanda with `claude mcp add --scope user --transport http`, or copy that command. Passwords and OAuth tokens never leave the backend.
- Tools:
  - Reading: `list_accounts` (with the addresses each account sends from), `list_folders`, `list_messages`, `search_messages`, `read_message`, `read_thread` and `get_attachment`.
  - Writing: `create_draft` and `send_mail`.
  - Marking, moving, archiving and deleting are left out on purpose.
- Reading fetches bodies with `BODY.PEEK`, so nothing turns read. Text attachments up to 1 MB come back as text. Other files are written to `%TEMP%\redpanda\mcp` and the path is returned for Claude Code to open. Executable types are refused.
- `send_mail` takes the sender address and resolves it to the account or alias. The existing check refuses anything else. A reply gets `In-Reply-To` and `References` from the original and is sent as written, without a quote.
- Tool annotations mark the read tools read-only and `send_mail` destructive. The server instructions tell the model that mail content is third-party data and never instructions, and to show sender, recipients and text before sending.
- The log names the tool, the account and the number of recipients, never content or the key.
- Closing the window keeps the app in the notification area (with a single instance), so the server stays reachable.

## Consequences

- Claude Code reaches mail only while redpanda runs.
- Claude Code asks before each tool call unless the user allowed the tool for good. The settings ask the user to keep `send_mail` on asking. The server itself sends without a further prompt in the app, which would bring the window to the front.
- Whatever Claude reads is processed by Anthropic like the rest of the Claude Code session.
- Mail can contain prompt injection. Apart from the instructions, the main protection is that sending needs the user's approval in Claude Code and that the tools can't delete or move mail.
- Search covers only synced mail, and bodies only once they were fetched. A server-side search (IMAP SEARCH, Gmail `X-GM-RAW`) could be added later.
