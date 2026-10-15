# 0002: Secrets encrypted in SQLite, key in the Windows Credential Manager

Status: accepted

## Context

redpanda stores IMAP/SMTP passwords and OAuth refresh tokens. The obvious place is the Windows Credential Manager, but a single credential blob is limited to about 2.5 KB, and Microsoft refresh tokens can get close to that limit. Storing several entries per account also makes cleanup error prone.

## Decision

- On first start, generate a random 256-bit key and store it in the Credential Manager through the `keyring` crate (service `redpanda`, user `master-key`).
- Encrypt every secret with ChaCha20-Poly1305 using that key and a random 96-bit nonce per value. Store ciphertext and nonce in the `secrets` table, keyed by account and kind (`password`, `oauth`).
- Secrets are never written to logs or plain files and never sent to the frontend.

## Consequences

- No size limit for tokens, one keyring entry for the whole app.
- Deleting an account deletes its secrets with a plain SQL `DELETE`.
- Copying the database to another machine or user profile does not leak credentials, because the key stays in the original Credential Manager.
- If the keyring entry is lost, all accounts need to sign in again. The app detects decryption failures and marks the accounts as "needs re-login".
