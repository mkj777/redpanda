# 0011: Search the text of every mail, locally and on the server

Status: accepted

## Context

Search used an FTS5 index over subject, sender, recipients and a preview made from the first bytes of each message. The text of a message only got into the index once it was opened, and the preview of many HTML mails was raw markup. Looking for a word from inside a mail, like "MacBook Air" in an order confirmation, found nothing unless that mail had been opened before.

## Decision

Both, as decided by the user:

- **The text of every mail is loaded in the background.** After the headers of a folder are complete (see the fill-in in `sync/engine.rs`), the background worker reads `BODYSTRUCTURE` for 50 messages at a time, picks the plain text part (HTML when there is none), and fetches only that part with `BODY.PEEK[<section>]`, grouped by section so one request covers most of a batch. Attachments are never downloaded. The text goes into the index (the first 100,000 characters), the raw part is thrown away, and the preview is replaced by one made from the text.
  - `messages.text_state` tracks it: 0 pending, 1 indexed, 2 no readable text. Opening a mail indexes its full body as before and sets 1.
  - A batch whose answer cannot be parsed is read again one message at a time, so a single odd message only leaves out itself. A message the server leaves out of its answer without an error is tried again a few times before it counts as unreadable.
  - New mail gets its text right after the sync that stored it. The lists hear about loaded texts at most every five seconds.
  - The progress line in the sidebar counts a message once its text is in.
- **Search asks the servers too.** After typing pauses, every account is searched on its own fourth IMAP connection, so a search never holds up opening or moving mail. Gmail allows 15 connections per account, redpanda uses four. A newer search replaces one still waiting, and one nobody waits for any more stops between folders. Accounts that need signing in again or are offline are left out, since a failing login again and again can get an account locked.
  - On Gmail it is one `X-GM-RAW` search over All Mail, so the hits match Gmail's own search, archived mail included (not Spam and Trash, as on the web). Hits are matched to stored messages by Message-ID. Archived mail that is in no folder redpanda shows is stored with the hidden All Mail folder, where only search finds it, and each full sync keeps those rows' flags in step and drops what is gone.
  - Elsewhere it is `TEXT` per word in every visible folder, with quotes and a trailing `*` taken out as in the local search.
  - Control characters never reach the server, since a line break would end the command. Text beyond ASCII only goes as a non-synchronizing literal (`LITERAL+` or `LITERAL-`, which Gmail has); a server without them is not asked.
  - Hits are kept per query as folder and UID for the last few searches, and shown next to what the index finds. Missing ones are fetched only below the loaded range of a folder whose first sync is done and whose UIDVALIDITY still matches, and only when the user changed nothing meanwhile, so a mail just deleted does not come back. `search_messages` for Claude does the same without touching what the search box keeps.
- **Attachment names are searchable, their content is not.** The index has an `attachments` column, filled from `BODYSTRUCTURE` (RFC 2047 and RFC 2231 names decoded) and from opened mails.

## Consequences

- Search works offline over everything once the background is done, and the server fills the gap until then.
- A first run downloads the text of every message once. Mails are small without their attachments, but large mailboxes still take a while, and the database grows by the text.
- The FTS table was rebuilt in migration 0007 to add the column; the previous index content is kept.
- The search query is never written to the log, and neither is the raw answer the IMAP parser puts into its errors, which can hold mail content.
