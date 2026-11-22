-- Attachments saved to the Downloads folder, newest (highest id) first in the download list.
-- The file itself belongs to the user: removing an entry or the message never touches it.
-- An entry points at its mail by account and Message-ID, not by the local row id: SQLite hands
-- a row id out again once the newest mail is deleted, and the Message-ID stays the same when the
-- mail is archived or moved.
CREATE TABLE downloads (
    id           INTEGER PRIMARY KEY,
    account_id   TEXT NOT NULL,
    message_key  TEXT,
    att_index    INTEGER NOT NULL,
    path         TEXT NOT NULL,
    size         INTEGER NOT NULL,
    created_at   INTEGER NOT NULL
);
