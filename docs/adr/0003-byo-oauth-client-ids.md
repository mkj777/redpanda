# 0003: Bring your own OAuth client IDs

Status: accepted

## Context

Gmail and Microsoft accounts should sign in with OAuth. A shared, published Google client with the `https://mail.google.com/` scope requires Google's verification for restricted scopes, including a paid third-party security assessment every year. Committing client IDs and secrets to a public repository would also let anyone impersonate the app.

## Decision

- No client IDs are committed. redpanda reads them at runtime from `%APPDATA%\redpanda\oauth.json`, with environment variables (`REDPANDA_GOOGLE_CLIENT_ID`, `REDPANDA_GOOGLE_CLIENT_SECRET`, `REDPANDA_MICROSOFT_CLIENT_ID`) taking precedence.
- The repository ships `oauth.example.json` and a step by step guide in `docs/OAUTH_SETUP.md`.
- When no ID is configured, the Google/Microsoft sign-in button explains what is missing and links to the guide. Password accounts work regardless.

## Consequences

- Every user registers their own Google Cloud and Microsoft Entra apps once.
- Google clients in testing mode expire refresh tokens after 7 days, so Gmail users sign in again weekly unless they publish their own client.
- No secrets in git history, no verification costs for the project.
