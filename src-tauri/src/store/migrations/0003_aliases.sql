-- Further addresses an account may send from, besides its own. For Gmail they come from the
-- "Send mail as" settings (accounts/aliases.rs) and are replaced on every refresh.
CREATE TABLE aliases (
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    email      TEXT NOT NULL COLLATE NOCASE,
    name       TEXT,
    sort_order INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (account_id, email)
);

-- The address a draft is sent from. NULL is the account's own address.
ALTER TABLE drafts ADD COLUMN from_email TEXT;
