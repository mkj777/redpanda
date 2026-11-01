-- The lowest UID of the contiguous range of newest messages a folder has loaded. Unread
-- messages older than that are stored too, so "Unread" matches the server's count, but they do
-- not extend the range. NULL means the lowest stored UID.
ALTER TABLE folders ADD COLUMN window_start INTEGER;
