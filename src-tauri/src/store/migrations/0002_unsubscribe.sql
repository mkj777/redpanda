-- How to leave a newsletter, read from List-Unsubscribe (JSON, see unsubscribe.rs). NULL for
-- ordinary mail.
ALTER TABLE messages ADD COLUMN unsubscribe TEXT;
CREATE INDEX messages_unsubscribe ON messages (account_id, from_addr) WHERE unsubscribe IS NOT NULL;

-- Messages synced before this migration have no unsubscribe data yet. Their folders read the
-- headers once on the next sync, folders added later get them with every message.
ALTER TABLE folders ADD COLUMN unsub_scanned INTEGER NOT NULL DEFAULT 1;
UPDATE folders SET unsub_scanned = 0;

CREATE TABLE unsubscribed (
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    sender     TEXT NOT NULL COLLATE NOCASE,
    method     TEXT NOT NULL,
    at         INTEGER NOT NULL,
    PRIMARY KEY (account_id, sender)
);
