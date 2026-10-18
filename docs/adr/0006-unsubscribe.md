# 0006: Unsubscribing from newsletters

Status: accepted

## Context

Gmail shows "Unsubscribe" next to the sender of a newsletter and has a view to manage subscriptions. Both rest on two headers that bulk senders put into every mail:

- `List-Unsubscribe` (RFC 2369) with a `mailto:` address, an `https:` address or both.
- `List-Unsubscribe-Post: List-Unsubscribe=One-Click` (RFC 8058): a plain POST to the https address unsubscribes, without a page to click through. Gmail and Yahoo require it from senders of more than 5000 mails a day since February 2024, so almost every newsletter has it.

A mail can claim anything in its headers, and unsubscribing from spam only tells the spammer that the address is read. Gmail therefore offers the button only for authenticated senders with a good reputation.

## Decision

- The sync reads the headers with every new message and stores the result as JSON in `messages.unsubscribe` (`unsubscribe.rs`). Messages synced before the column existed get it once per folder: the next sync fetches only `BODY.PEEK[HEADER.FIELDS (...)]` for the stored UIDs (`folders.unsub_scanned`).
- Unsubscribing takes the best way the sender offers:
  1. One-click POST, only when a DKIM signature covers both list headers (as RFC 8058 requires) and the topmost `Authentication-Results`, the one the receiving server added, reports `dkim=pass` for its domain. redpanda does not verify DKIM itself.
  2. A mail to the `mailto:` address, sent through the account's SMTP server. It is not saved to Sent and the address does not go into the contacts.
  3. The unsubscribe page, opened in the browser. It is also the fallback when the POST or the mail fails.
- The user always confirms first, in a dialog that says which way is used and where it goes. The frontend only learns the method and the host or address; the URL stays in the backend, which reads it again from the database for the given message.
- Nothing is offered for mail in the spam folder, and the backend refuses it there too.
- The one-click POST goes only to `https` URLs whose host is not on the local machine or network (no `localhost`, `.local`, private or link-local addresses). It carries no cookies or credentials and follows no redirects, which RFC 8058 rules out anyway.
- The "Subscriptions" view lists every sender with list headers per account, the most active first, and the ones already left at the end. Copies of one message in several Gmail labels count once. Mail that still arrives after unsubscribing is shown with an offer to try again, since senders may take up to two days.

## Consequences

- Unsubscribing contacts the sender from the user's machine, so the sender sees the user's IP address at that moment. Gmail sends the POST from Google's servers instead.
- Servers that add no `Authentication-Results` get no one-click and fall back to the mail or the page. A sender could forge that header only where the receiving server adds none of its own.
- The state "unsubscribed" is local (`unsubscribed` table) and not synced between devices.
