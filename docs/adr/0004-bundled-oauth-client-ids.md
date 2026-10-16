# 0004: Bundled OAuth client IDs

Status: accepted, supersedes [0003](0003-byo-oauth-client-ids.md)

## Context

ADR 0003 made every user register their own Google and Microsoft OAuth clients. That works for developers but rules out everyone else, and there is no way around OAuth for Microsoft: Outlook.com and Microsoft 365 no longer accept passwords or app passwords for IMAP and SMTP.

Registering a Microsoft Entra app is free and needs no review. Google treats full mail access (`https://mail.google.com/`) as a restricted scope. A published but unverified Google client works with two limits: users see a "Google hasn't verified this app" warning before the consent screen, and at most 100 Google accounts can ever sign in. Lifting those limits takes Google's verification including a paid yearly security assessment (CASA).

## Decision

- The project registers one Google client (type "Desktop app", publishing status "In production", not verified) and one Microsoft Entra app (public client, any organizational directory plus personal accounts).
- Their IDs are stored as GitHub Actions secrets and compiled into release builds with `option_env!` (`REDPANDA_GOOGLE_CLIENT_ID`, `REDPANDA_GOOGLE_CLIENT_SECRET`, `REDPANDA_MICROSOFT_CLIENT_ID`). They are not committed.
- Local configuration still wins: environment variables first, then `%APPDATA%\redpanda\oauth.json`, then the bundled IDs. Self-built binaries without secrets behave as before.
- When a build has no Google ID, the account wizard goes straight to the app password form for Gmail. Gmail works with an app password whenever the account has 2-step verification.

## Consequences

- Microsoft accounts work for everyone without setup. Some work tenants require admin consent for unverified publishers; the free Microsoft publisher verification removes the "unverified" label.
- Gmail sign-in works for the first 100 accounts and shows Google's warning. The wizard explains how to continue. After that, or for Workspace domains whose admin blocks unverified apps, users fall back to an app password.
- Desktop clients cannot keep a secret: the IDs, and Google's desktop "client secret", can be read from the binary. Google documents this for installed apps, PKCE protects the authorization code, and tokens never leave the user's machine. Keeping the IDs out of git only stops forks from accidentally shipping them under redpanda's name.
- If redpanda outgrows 100 Gmail users, the Google client needs verification (CASA tier 2) or Gmail sign-in stays limited to app passwords for new users.
