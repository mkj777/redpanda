# 0010: A stdio bridge for Claude Desktop

Status: accepted

## Context

The MCP server from [ADR 0008](0008-mcp-server.md) speaks Streamable HTTP on `127.0.0.1`. Claude Code connects to it directly. Claude Desktop can't:

- Its connectors reach remote servers from Anthropic's cloud, where `127.0.0.1` means nothing. Putting the mail server on the internet behind a tunnel is not an option.
- `claude_desktop_config.json` only starts local servers as programs that talk JSON-RPC over stdin and stdout. A plain URL is not reliably accepted there.

So Claude Desktop needs a small program on the computer that turns stdio into requests to the running app. Third-party proxies like `mcp-remote` would work, but they would get the key and see every mail that passes through.

## Decision

- **The bridge is redpanda itself**: `redpanda.exe --mcp-stdio`. With this argument `run()` hands over to `mcp::bridge` before Tauri starts, so neither the single-instance check nor a window is involved.
- **It forwards to the HTTP server**: every line from stdin goes as a POST to `http://127.0.0.1:<port>/mcp`. The answer, plain JSON or an event stream, goes back to stdout line by line. Requests run side by side; `initialize` and notifications keep their order.
  - The bridge keeps the `Mcp-Session-Id` from `initialize` and sends the agreed `MCP-Protocol-Version` with every later message. For the stateless protocol of 2026-07-28 it takes the version from the request's `_meta` and adds the routing headers `Mcp-Method` and `Mcp-Name` (SEP-2243).
  - When redpanda restarted (no connection, or `404` for the session) or the key changed (`401`), the bridge reads the settings again, repeats Claude Desktop's `initialize` for a new session and sends the message once more. None of these reached a tool, so a mail is never sent twice. A request that still fails gets a JSON-RPC error with the reason, so Claude Desktop never waits forever.
  - It opens a new connection per message, so a restarted app never meets an old one.
- **Port and key come from redpanda**: the bridge opens the database read-only, reads `mcp_enabled`, `mcp_port` and `mcp_key`, and decrypts the key with the master key from the Credential Manager, without creating one. Claude Desktop's config therefore holds only the path of `redpanda.exe` and the argument.
- **Starting redpanda**: when nothing answers on the port, the bridge starts `redpanda.exe --background` and waits up to 30 seconds. `--background` starts hidden in the notification area like `--autostart`, and a second start with it doesn't show the window either. The new process gets none of Claude Desktop's pipes (the bridge clears their inherit flag first), runs detached and, where the job allows it, outside Claude Desktop's job, so it keeps running when Claude Desktop quits. With access switched off, the bridge starts nothing and answers with a hint where to switch it on.
- **Registering**: Settings, Claude Code has a second box "Connect to Claude Desktop". It writes the `redpanda` entry under `mcpServers` and keeps everything else in the file. The file sits in the data folder Claude Desktop wrote its own `config.json` to last: `%LOCALAPPDATA%\Claude-Data\<package>` for the Store version, `%APPDATA%\Claude` for the installer version. Claude Desktop reads it on start, so it needs a restart.
- **Logging**: the bridge logs to stderr, which Claude Desktop keeps as `mcp-server-redpanda.log`. As in the app, no mail content and no key.

## Consequences

- Claude Desktop gets the same tools and the same protections as Claude Code: the same key check, `Host` check and tool list. Claude Desktop asks before tool calls like Claude Code does.
- Claude Desktop starts its MCP servers without its Store package identity, so the bridge and a redpanda it starts see the real `%LOCALAPPDATA%` and registry.
- While Claude Desktop runs, the bridge is one more small redpanda process that waits for messages.
- Moving the installation needs the entry to be written again.
- If Claude Desktop's job forbids leaving it, a redpanda the bridge started ends together with Claude Desktop. The bridge logs that case.
