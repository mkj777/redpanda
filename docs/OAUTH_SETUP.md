# OAuth setup

redpanda signs in to Gmail and Outlook with OAuth. Release builds ship the project's own client IDs (see [ADR 0004](adr/0004-bundled-oauth-client-ids.md)), so people who install redpanda from Releases do not need anything from this page.

This guide is for:

- the maintainer, who registers the clients once and stores them as GitHub secrets for release builds,
- anyone who builds redpanda from source and wants Google or Microsoft sign-in.

It takes about ten minutes per provider and is free. Password and app password accounts (GMX, web.de, iCloud, own domain, Gmail with an app password) work without any of this.

## Where the IDs go

redpanda takes each provider's ID from the first of these that has one:

1. Environment variables at runtime
2. `%APPDATA%\redpanda\oauth.json`
3. The IDs compiled into the build

| Variable | Meaning |
|---|---|
| `REDPANDA_GOOGLE_CLIENT_ID` | Google client ID |
| `REDPANDA_GOOGLE_CLIENT_SECRET` | Google client secret (Google requires it for desktop clients, it is not treated as secret) |
| `REDPANDA_MICROSOFT_CLIENT_ID` | Microsoft application (client) ID |

### Release builds

Add the three values as repository secrets under **Settings > Secrets and variables > Actions** with the names from the table. The [release workflow](../.github/workflows/release.yml) passes them to the build, which compiles them in. Push a tag such as `v0.2.0` to build the installers into a draft release.

The same works locally: set the variables before `pnpm tauri build` and the resulting installer contains the IDs.

### Local config

Create `%APPDATA%\redpanda\oauth.json` (copy [`oauth.example.json`](../oauth.example.json)):

```json
{
  "google": {
    "client_id": "1234567890-abc.apps.googleusercontent.com",
    "client_secret": "GOCSPX-..."
  },
  "microsoft": {
    "client_id": "00000000-0000-0000-0000-000000000000"
  }
}
```

The account wizard re-reads the file when it opens.

## Google (Gmail and Google Workspace)

1. Open the [Google Cloud console](https://console.cloud.google.com/) and create a project, for example `redpanda-mail`.
2. Go to **APIs & Services > Library**, search for **Gmail API** and enable it.
3. Go to **APIs & Services > OAuth consent screen** (in newer consoles: **Google Auth Platform > Branding**).
   - User type: **External**.
   - App name `redpanda`, your support email, and the GitHub repository as the home page.
   - Under **Data access** (or **Scopes**), add `https://mail.google.com/`, `openid` and `email`.
   - Under **Audience**, set the publishing status to **In production** (**Publish app**). Google points out that the restricted scope needs verification. You do not have to submit it: the client keeps working unverified.
4. Go to **APIs & Services > Credentials > Create credentials > OAuth client ID**.
   - Application type: **Desktop app**.
   - Copy the client ID and client secret.

What "unverified, in production" means:

- Users see "Google hasn't verified this app" before the consent screen and continue with **Advanced** and the link to redpanda. The wizard tells them.
- At most 100 Google accounts can ever sign in. The counter does not go down when someone removes access. Watch it under **Audience**.
- Refresh tokens do not expire after a week, unlike the **Testing** status, which also only lets listed test users in.
- Google Workspace admins and accounts with Advanced Protection can block unverified apps. Those users take an app password.
- Going beyond 100 users needs Google's verification for restricted scopes, including a yearly CASA security assessment by an approved lab.

Desktop clients accept any loopback redirect (`http://127.0.0.1:<port>`), so nothing else has to be configured.

## Microsoft (Outlook.com, Hotmail, Microsoft 365)

1. Open the [Microsoft Entra admin center](https://entra.microsoft.com/) with any Microsoft account and go to **Identity > Applications > App registrations > New registration**.
   - Name: `redpanda`.
   - Supported account types: **Accounts in any organizational directory and personal Microsoft accounts**.
   - Redirect URI: platform **Public client/native (mobile & desktop)**, value `http://localhost`.
2. Open the new app and copy the **Application (client) ID**.
3. Go to **Authentication** and set **Allow public client flows** to **Yes**.
4. Go to **API permissions > Add a permission > APIs my organization uses**, search for **Office 365 Exchange Online** and add the delegated permissions `IMAP.AccessAsUser.All` and `SMTP.Send`. Also add `offline_access`, `openid` and `email` from **Microsoft Graph**.
   - If **Office 365 Exchange Online** does not show up (personal tenants), use **Microsoft Graph** and add `IMAP.AccessAsUser.All` and `SMTP.Send` from there. Both resolve to the same scopes.

Notes:

- Microsoft allows any port on `http://localhost` for public clients, so the random loopback port works without extra entries.
- The consent dialog marks the app as "unverified" until you complete the free [publisher verification](https://learn.microsoft.com/entra/identity-platform/publisher-verification-overview) with a Microsoft AI Cloud Partner Program account. Some work tenants only allow verified publishers or need admin consent.
- For Microsoft 365 the mailbox must have IMAP and authenticated SMTP enabled (Exchange admin center > Recipients > Mailboxes > Manage email apps).

## Troubleshooting

| Message | Cause |
|---|---|
| "This build of redpanda has no Microsoft client ID" | Self-built binary without IDs and no `oauth.json` |
| "This app is blocked" or `access_denied` from Google | Workspace policy, Advanced Protection, the 100-user limit, or consent was declined. Use an app password. |
| `AUTHENTICATE failed` after sign-in | IMAP is disabled for the mailbox (Microsoft 365) or the scope was not granted |
| Sign-in needed again after a week | Your own Google client is still in **Testing** status |
