# 0011: Search the text of every mail, locally and on the server

Status: accepted

## Context

Search used an FTS5 index over subject, sender, recipients and a preview made from the first bytes of each message. The text of a message only got into the index once it was opened, and the preview of many HTML mails was raw markup. Looking for a word from inside a mail, like "MacBook Air" in an order confirmation, found nothing unless that mail had been opened before.

## Decision

Both, as decided by the user:

- **The text of every mail is loaded in the background.** After the headers of a folder are complete (see the fill-in in `sync/engine.rs`), the background worker reads `BODYSTRUCTURE` for 50 messages at a time, picks the plain text part (HTML when there is none), and fetches only that part with `BODY.PEEK[<section>]`, grouped by section so one request covers most of a batch. Attachments are never downloaded. The text goes into the index (the first 100,000 characters), the raw part is thrown away, and the preview is replaced by one made from the text.
  - `messages.text_state` tracks it: 0 pending, 1 indexed, 2 no readable text. Opening a mail indexes its full body as before and sets 1.
  - A batch whose answer cannot be parsed is read again one message at a time, so a single odd message only leaves out itself.
  - The progress line in the sidebar counts a message once its text is in.
- **Search asks the servers too.** After typing pauses, the app runs a search on every account through its interactive connection: `X-GM-RAW` on Gmail, so the hits match Gmail's own search, and `TEXT` per word elsewhere. Queries beyond ASCII go as a non-synchronizing literal where the server allows it (`LITERAL+` or `LITERAL-`, which Gmail has). The local ids of the hits are kept for that query and shown next to what the index finds. Hits that are not stored yet are fetched first. Accounts that fail or take longer than 20 seconds add nothing. `search_messages` for Claude does the same.
- **Attachment names are searchable, their content is not.** The index has an `attachments` column, filled from `BODYSTRUCTURE` (RFC 2047 and RFC 2231 names decoded) and from opened mails.

## Consequences

- Search works offline over everything once the background is done, and the server fills the gap until then.
- A first run downloads the text of every message once. Mails are small without their attachments, but large mailboxes still take a while, and the database grows by the text.
- The FTS table was rebuilt in migration 0007 to add the column; the previous index content is kept.
- The search query is never written to the log.
