-- Search covers the text of every message and the names of its attachments. The text is loaded
-- in the background after the headers. text_state: 0 = not loaded yet, 1 = in the index,
-- 2 = has no readable text or could not be read.
ALTER TABLE messages ADD COLUMN text_state INTEGER NOT NULL DEFAULT 0;
UPDATE messages SET text_state = 1 WHERE id IN (SELECT message_id FROM bodies);
CREATE INDEX messages_text_pending ON messages (folder_id, date) WHERE text_state = 0;

-- FTS5 tables cannot gain a column, so the index is rebuilt with one for attachment names.
DROP TRIGGER messages_fts_delete;
CREATE TEMP TABLE fts_copy AS
    SELECT rowid AS id, subject, sender, recipients, preview, body FROM messages_fts;
DROP TABLE messages_fts;
CREATE VIRTUAL TABLE messages_fts USING fts5 (
    subject,
    sender,
    recipients,
    preview,
    body,
    attachments,
    tokenize = 'unicode61 remove_diacritics 2'
);
INSERT INTO messages_fts (rowid, subject, sender, recipients, preview, body, attachments)
    SELECT f.id, f.subject, f.sender, f.recipients, f.preview, f.body,
        COALESCE((
            SELECT group_concat(json_extract(j.value, '$.filename'), ' ')
            FROM bodies b, json_each(b.attachments_json) j
            WHERE b.message_id = f.id AND NOT json_extract(j.value, '$.inline')
        ), '')
    FROM fts_copy f;
DROP TABLE fts_copy;
CREATE TRIGGER messages_fts_delete AFTER DELETE ON messages BEGIN
    DELETE FROM messages_fts WHERE rowid = old.id;
END;
