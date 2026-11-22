-- Attachments saved to the Downloads folder, newest first in the download list. The file itself
-- belongs to the user: removing an entry or the message never touches it.
CREATE TABLE downloads (
    id          INTEGER PRIMARY KEY,
    message_id  INTEGER NOT NULL,
    att_index   INTEGER NOT NULL,
    path        TEXT NOT NULL,
    size        INTEGER NOT NULL,
    created_at  INTEGER NOT NULL
);
