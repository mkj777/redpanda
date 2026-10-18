# 0009: Releases, updates and autostart

Status: accepted

## Context

redpanda gets its first release. Since it keeps running in the notification area and serves Claude Code from there, it should start with Windows and stay current without the user reinstalling by hand. The NSIS installer installs per user into `%LOCALAPPDATA%\redpanda`, the folder where redpanda kept its database until now, so program and data would share one folder and the uninstaller's "Delete the application data" (which removes `%LOCALAPPDATA%\<identifier>`) would miss the mail cache.

## Decision

- **Builds**: pushing a tag `v*` builds the MSI and NSIS installers on GitHub Actions and attaches them to a draft release, which the maintainer publishes by hand. The OAuth client IDs come from repository secrets, see [ADR 0004](0004-bundled-oauth-client-ids.md).
- **Updates**: `tauri-plugin-updater` with signed update packages.
  - The minisign key pair was generated with `tauri signer generate`. The public key is in `tauri.conf.json`, the private key is the secret `TAURI_SIGNING_PRIVATE_KEY` (no password) and a copy is kept outside the repository by the maintainer. Losing it means current installs can't update anymore and need one manual reinstall.
  - The app reads `latest.json` from the latest published release. Release builds check 30 seconds after the start and then every six hours, and the settings can check on request. Nothing installs without the user clicking "Install now"; the NSIS installer then runs in passive mode and starts the new version.
  - Debug builds never check or install.
- **Autostart**: `tauri-plugin-autostart` writes the `HKCU\...\Run` entry with `--autostart`. It's on by default, switchable under Settings, General, and the entry is rewritten on every start so it follows the install path. With `--autostart` the window stays hidden (it starts hidden in the config and is shown in `setup` otherwise); the window state plugin no longer saves or restores visibility. Debug builds never register themselves.
- **Data folder**: the database moves to `%LOCALAPPDATA%\io.github.mkj777.redpanda`, where WebView2 already keeps its data. On start, if only the old `redpanda.db` (with `-wal` and `-shm`) exists, it is renamed into the new folder once. The single-instance plugin ensures no other redpanda has it open.
- The installers are not code-signed (Authenticode) yet.

## Consequences

- SmartScreen warns on the first download until the installers are code-signed and have built up reputation. The README explains "More info, Run anyway".
- Updates only reach users from the version that contains the updater on, which is v0.1.0.
- A dev build (`pnpm tauri dev`) and the installed app share the database and the single-instance lock: while the installed redpanda runs, starting the dev build only brings the installed window forward. Quit the installed one from the tray first.
