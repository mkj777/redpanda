# 0001: Tauri 2 with a React frontend and all mail logic in Rust

Status: accepted

## Context

redpanda is a Windows desktop mail client. It needs IMAP (with IDLE), SMTP, OAuth, MIME parsing and a local store. The UI should look like a modern web app and be quick to iterate on.

## Decision

- Tauri 2 as the shell. The frontend is React 19, TypeScript, Vite, Tailwind 4, Zustand and Radix primitives.
- All network and mail work happens in Rust: `async-imap` on tokio, `lettre` for SMTP, `mail-parser` for MIME, `ammonia` for HTML sanitizing, `rusqlite` for storage.
- TLS uses rustls everywhere (no OpenSSL), so building on Windows needs no extra native libraries.
- The frontend only renders data from SQLite and calls commands. Rust emits `mail:changed` and `sync:status` events so the UI refetches.

## Consequences

- Small installer and memory footprint compared to Electron.
- Credentials and raw mail never pass through JavaScript except for the rendered, sanitized body.
- The Rust side carries most of the complexity, so it carries most of the tests.
