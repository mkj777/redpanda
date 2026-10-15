CREATE TABLE accounts (
    id                   TEXT PRIMARY KEY,
    email                TEXT NOT NULL,
    display_name         TEXT NOT NULL DEFAULT '',
    provider             TEXT NOT NULL,
    auth_type            TEXT NOT NULL,
    username             TEXT NOT NULL,
    imap_host            TEXT NOT NULL,
    imap_port            INTEGER NOT NULL,
    imap_security        TEXT NOT NULL,
    smtp_host            TEXT NOT NULL,
    smtp_port            INTEGER NOT NULL,
    smtp_security        TEXT NOT NULL,
    color                TEXT NOT NULL,
    sent_saved_by_server INTEGER NOT NULL DEFAULT 0,
    is_gmail             INTEGER NOT NULL DEFAULT 0,
    sort_order           INTEGER NOT NULL DEFAULT 0,
    created_at           INTEGER NOT NULL
);

CREATE TABLE folders (
    id            INTEGER PRIMARY KEY,
    account_id    TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    path          TEXT NOT NULL,
    name          TEXT NOT NULL,
    depth         INTEGER NOT NULL DEFAULT 0,
    role          TEXT NOT NULL DEFAULT 'other',
    hidden        INTEGER NOT NULL DEFAULT 0,
    uidvalidity   INTEGER,
    uidnext       INTEGER,
    highestmodseq INTEGER,
    unread_count  INTEGER NOT NULL DEFAULT 0,
    total_count   INTEGER NOT NULL DEFAULT 0,
    -- 1 once every message down to UID 1 has been fetched
    fully_loaded  INTEGER NOT NULL DEFAULT 0,
    last_sync     INTEGER,
    UNIQUE (account_id, path)
);

CREATE TABLE messages (
    id              INTEGER PRIMARY KEY,
    account_id      TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    folder_id       INTEGER NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    uid             INTEGER NOT NULL,
    message_id      TEXT,
    in_reply_to     TEXT,
    refs            TEXT NOT NULL DEFAULT '[]',
    thread_key      TEXT NOT NULL,
    from_name       TEXT,
    from_addr       TEXT,
    to_json         TEXT NOT NULL DEFAULT '[]',
    cc_json         TEXT NOT NULL DEFAULT '[]',
    reply_to_json   TEXT NOT NULL DEFAULT '[]',
    subject         TEXT NOT NULL DEFAULT '',
    date            INTEGER NOT NULL,
    seen            INTEGER NOT NULL DEFAULT 0,
    flagged         INTEGER NOT NULL DEFAULT 0,
    answered        INTEGER NOT NULL DEFAULT 0,
    has_attachments INTEGER NOT NULL DEFAULT 0,
    preview         TEXT NOT NULL DEFAULT '',
    size            INTEGER NOT NULL DEFAULT 0,
    UNIQUE (folder_id, uid)
);

CREATE INDEX messages_folder_date ON messages (folder_id, date DESC);
CREATE INDEX messages_account_date ON messages (account_id, date DESC);
CREATE INDEX messages_thread ON messages (thread_key);
CREATE INDEX messages_flagged ON messages (flagged) WHERE flagged = 1;
CREATE INDEX messages_unseen ON messages (seen) WHERE seen = 0;

CREATE TABLE bodies (
    message_id       INTEGER PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
    text             TEXT,
    html             TEXT,
    has_remote       INTEGER NOT NULL DEFAULT 0,
    attachments_json TEXT NOT NULL DEFAULT '[]',
    raw              BLOB NOT NULL
);

CREATE VIRTUAL TABLE messages_fts USING fts5 (
    subject,
    sender,
    recipients,
    preview,
    body,
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER messages_fts_delete AFTER DELETE ON messages BEGIN
    DELETE FROM messages_fts WHERE rowid = old.id;
END;

CREATE TABLE secrets (
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    kind       TEXT NOT NULL,
    ciphertext BLOB NOT NULL,
    nonce      BLOB NOT NULL,
    PRIMARY KEY (account_id, kind)
);

CREATE TABLE drafts (
    id                      INTEGER PRIMARY KEY,
    account_id              TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    to_json                 TEXT NOT NULL DEFAULT '[]',
    cc_json                 TEXT NOT NULL DEFAULT '[]',
    bcc_json                TEXT NOT NULL DEFAULT '[]',
    subject                 TEXT NOT NULL DEFAULT '',
    body                    TEXT NOT NULL DEFAULT '',
    in_reply_to             TEXT,
    refs                    TEXT NOT NULL DEFAULT '[]',
    attachments_json        TEXT NOT NULL DEFAULT '[]',
    reply_to_message_id     INTEGER,
    forward_from_message_id INTEGER,
    updated_at              INTEGER NOT NULL
);

CREATE TABLE contacts (
    email      TEXT PRIMARY KEY COLLATE NOCASE,
    name       TEXT,
    weight     INTEGER NOT NULL DEFAULT 0,
    last_seen  INTEGER NOT NULL
);

CREATE TABLE remote_content_allowed (
    sender TEXT PRIMARY KEY COLLATE NOCASE
);
