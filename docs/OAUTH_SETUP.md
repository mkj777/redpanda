# OAuth setup

redpanda signs in to Gmail and Outlook with OAuth. Because the code is open source and Google treats full mail access (`https://mail.google.com/`) as a restricted scope, there is no shared client ID. Everyone who builds redpanda registers their own clients once. It takes about ten minutes per provider and is free.

Password and app password accounts (GMX, web.de, iCloud, own domain) work without any of this.

## Where the IDs go

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

Environment variables override the file:

| Variable | Meaning |
|---|---|
| `REDPANDA_GOOGLE_CLIENT_ID` | Google client ID |
| `REDPANDA_GOOGLE_CLIENT_SECRET` | Google client secret (Google requires it for desktop clients, it is not treated as secret) |
| `REDPANDA_MICROSOFT_CLIENT_ID` | Microsoft application (client) ID |

Restart redpanda after changing the file.

## Google (Gmail and Google Workspace)

1. Open the [Google Cloud console](https://console.cloud.google.com/) and create a project, for example `redpanda-mail`.
2. Go to **APIs & Services > Library**, search for **Gmail API** and enable it.
3. Go to **APIs & Services > OAuth consent screen** (in newer consoles: **Google Auth Platform > Branding**).
   - User type: **External** (or **Internal** if you only use a Workspace domain).
   - Fill in the app name and your email address.
   - Under **Data access** (or **Scopes**), add `https://mail.google.com/`, `openid` and `email`.
   - Under **Audience** (or **Test users**), keep the publishing status at **Testing** and add every Google account you want to use as a test user.
4. Go to **APIs & Services > Credentials > Create credentials > OAuth client ID**.
   - Application type: **Desktop app**.
   - Copy the client ID and client secret into `oauth.json`.

Notes:

- In testing mode Google expires refresh tokens after 7 days. redpanda then asks you to sign in again. Publishing the app removes that limit but requires Google's verification for the restricted scope, which includes a paid security assessment. For personal use, staying in testing mode is the practical choice.
- Desktop clients accept any loopback redirect (`http://127.0.0.1:<port>`), so nothing else has to be configured.

## Microsoft (Outlook.com, Hotmail, Microsoft 365)

1. Open the [Microsoft Entra admin center](https://entra.microsoft.com/) with any Microsoft account and go to **Identity > Applications > App registrations > New registration**.
   - Name: `redpanda`.
   - Supported account types: **Accounts in any organizational directory and personal Microsoft accounts**.
   - Redirect URI: platform **Public client/native (mobile & desktop)**, value `http://localhost`.
2. Open the new app and copy the **Application (client) ID** into `oauth.json`.
3. Go to **Authentication** and set **Allow public client flows** to **Yes**.
4. Go to **API permissions > Add a permission > APIs my organization uses**, search for **Office 365 Exchange Online** and add the delegated permissions `IMAP.AccessAsUser.All` and `SMTP.Send`. Also add `offline_access`, `openid` and `email` from **Microsoft Graph**.
   - If **Office 365 Exchange Online** does not show up (personal tenants), use **Microsoft Graph** and add `IMAP.AccessAsUser.All` and `SMTP.Send` from there. Both resolve to the same scopes.

Notes:

- Microsoft allows any port on `http://localhost` for public clients, so the random loopback port works without extra entries.
- Work accounts may need admin consent for the permissions, depending on the tenant policy.
- For Microsoft 365 the mailbox must have IMAP and authenticated SMTP enabled (Exchange admin center > Recipients > Mailboxes > Manage email apps).

## Troubleshooting

| Message | Cause |
|---|---|
| "No Google client ID configured" | `oauth.json` is missing or has no `google.client_id` |
| `access_denied` in the browser | Your account is not listed as a test user (Google) or consent was declined |
| `AUTHENTICATE failed` after sign-in | IMAP is disabled for the mailbox (Microsoft 365) or the scope was not granted |
| Sign-in needed again after a week | Google testing mode, see above |
