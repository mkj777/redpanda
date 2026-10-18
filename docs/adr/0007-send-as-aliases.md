# 0007: Sending from Gmail aliases

Status: accepted

## Context

A Gmail or Google Workspace account can send from further addresses. Users add them in Gmail under "Settings > Accounts > Send mail as" and confirm a code Gmail mails there, or a Workspace admin gives the user alias addresses in the domain. A common case: someone moves to a Workspace mailbox for an own domain and keeps sending from the old @gmail.com address through it.

Gmail's SMTP server accepts such an address in the From header and rewrites any other address to the account's own. Other providers may relay whatever From they get, so a client must not let the user send from arbitrary addresses.

## Decision

- For accounts signed in with Google, redpanda reads `users.settings.sendAs.list` from the Gmail API with the token IMAP and SMTP use. The `https://mail.google.com/` scope covers it, and the Gmail API is already enabled for reading the profile address (see the OAuth setup guide).
- It keeps addresses that are not the primary one and either have no verification status (domain aliases) or the status `accepted`. Pending addresses are left out. Names lose control characters.
- The list is stored in the `aliases` table and read again after a full sync, at most once an hour. When the request fails, the stored list stays.
- A draft carries the address it goes out from (`from_email`, empty for the account's own). The backend sends only from the account's own address or one of its stored aliases and refuses everything else, whatever the frontend asks for.
- Replies and forwards go out from the address the message was sent to, or the one it was sent from, like Gmail's default. New mail starts with the account's own address.
- The mail that unsubscribes from a newsletter goes out from the address the newsletter went to.

## Consequences

- Password and app password accounts, and Microsoft accounts, have no aliases for now. Adding them by hand would need a check that the server accepts the address.
- redpanda hands alias mail to Gmail's SMTP server like any other mail. How it goes on from there (for example through another server configured for the alias) is up to Gmail, and Gmail files the copy in Sent.
- An alias removed in Gmail disappears after the next refresh. A draft that still uses it fails to send with a message asking for another sender.
