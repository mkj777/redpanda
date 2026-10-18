//! All SQL lives here. Functions take `&Connection` so callers decide about transactions.

use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row};

use super::now;
use crate::error::{AppError, Result};
use crate::model::*;
use crate::unsubscribe::{Method, Unsubscribe};

// ---------------------------------------------------------------------------------------------
// Accounts

const ACCOUNT_COLS: &str = "id, email, display_name, provider, auth_type, username, imap_host, imap_port, \
     imap_security, smtp_host, smtp_port, smtp_security, color, sent_saved_by_server, is_gmail";

fn account_from_row(r: &Row) -> rusqlite::Result<Account> {
    Ok(Account {
        id: r.get(0)?,
        email: r.get(1)?,
        display_name: r.get(2)?,
        provider: r.get(3)?,
        auth_type: AuthType::parse(&r.get::<_, String>(4)?),
        username: r.get(5)?,
        imap: ServerConfig {
            host: r.get(6)?,
            port: r.get(7)?,
            security: Security::parse(&r.get::<_, String>(8)?),
        },
        smtp: ServerConfig {
            host: r.get(9)?,
            port: r.get(10)?,
            security: Security::parse(&r.get::<_, String>(11)?),
        },
        color: r.get(12)?,
        sent_saved_by_server: r.get(13)?,
        is_gmail: r.get(14)?,
    })
}

pub fn insert_account(c: &Connection, a: &Account) -> Result<()> {
    let order: i64 = c.query_row(
        "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM accounts",
        [],
        |r| r.get(0),
    )?;
    c.execute(
        &format!(
            "INSERT INTO accounts ({ACCOUNT_COLS}, sort_order, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)"
        ),
        params![
            a.id,
            a.email,
            a.display_name,
            a.provider,
            a.auth_type.as_str(),
            a.username,
            a.imap.host,
            a.imap.port,
            a.imap.security.as_str(),
            a.smtp.host,
            a.smtp.port,
            a.smtp.security.as_str(),
            a.color,
            a.sent_saved_by_server,
            a.is_gmail,
            order,
            now()
        ],
    )?;
    Ok(())
}

pub fn list_accounts(c: &Connection) -> Result<Vec<Account>> {
    let mut stmt = c.prepare(&format!(
        "SELECT {ACCOUNT_COLS} FROM accounts ORDER BY sort_order, created_at"
    ))?;
    let rows = stmt.query_map([], account_from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn get_account(c: &Connection, id: &str) -> Result<Account> {
    c.query_row(
        &format!("SELECT {ACCOUNT_COLS} FROM accounts WHERE id = ?1"),
        [id],
        account_from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound(format!("account {id}")))
}

pub fn account_exists_for_email(c: &Connection, email: &str) -> Result<bool> {
    Ok(c.query_row(
        "SELECT 1 FROM accounts WHERE email = ?1 COLLATE NOCASE",
        [email],
        |_| Ok(()),
    )
    .optional()?
    .is_some())
}

pub fn update_account_profile(c: &Connection, id: &str, display_name: &str, color: &str) -> Result<()> {
    c.execute(
        "UPDATE accounts SET display_name = ?2, color = ?3 WHERE id = ?1",
        params![id, display_name, color],
    )?;
    Ok(())
}

pub fn update_account_servers(c: &Connection, a: &Account) -> Result<()> {
    c.execute(
        "UPDATE accounts SET username = ?2, imap_host = ?3, imap_port = ?4, imap_security = ?5, \
         smtp_host = ?6, smtp_port = ?7, smtp_security = ?8 WHERE id = ?1",
        params![
            a.id,
            a.username,
            a.imap.host,
            a.imap.port,
            a.imap.security.as_str(),
            a.smtp.host,
            a.smtp.port,
            a.smtp.security.as_str()
        ],
    )?;
    Ok(())
}

pub fn delete_account(c: &Connection, id: &str) -> Result<()> {
    c.execute("DELETE FROM accounts WHERE id = ?1", [id])?;
    Ok(())
}

pub fn aliases(c: &Connection, account_id: &str) -> Result<Vec<Address>> {
    let mut stmt =
        c.prepare("SELECT name, email FROM aliases WHERE account_id = ?1 ORDER BY sort_order, email")?;
    let rows = stmt.query_map([account_id], |r| {
        Ok(Address {
            name: r.get(0)?,
            email: r.get(1)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Replaces the account's aliases. Returns whether anything changed.
pub fn replace_aliases(c: &mut Connection, account_id: &str, list: &[Address]) -> Result<bool> {
    if aliases(c, account_id)? == list {
        return Ok(false);
    }
    let tx = c.transaction()?;
    tx.execute("DELETE FROM aliases WHERE account_id = ?1", [account_id])?;
    for (i, a) in list.iter().enumerate() {
        tx.execute(
            "INSERT OR IGNORE INTO aliases (account_id, email, name, sort_order) VALUES (?1, ?2, ?3, ?4)",
            params![account_id, a.email, a.name, i as i64],
        )?;
    }
    tx.commit()?;
    Ok(true)
}

// ---------------------------------------------------------------------------------------------
// App settings

pub fn get_setting(c: &Connection, key: &str) -> Result<Option<String>> {
    Ok(
        c.query_row("SELECT value FROM app_settings WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()?,
    )
}

pub fn set_setting(c: &Connection, key: &str, value: &str) -> Result<()> {
    c.execute(
        "INSERT INTO app_settings (key, value) VALUES (?1, ?2) \
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        [key, value],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Secrets

pub fn put_secret(
    c: &Connection,
    account_id: &str,
    kind: &str,
    ciphertext: &[u8],
    nonce: &[u8],
) -> Result<()> {
    c.execute(
        "INSERT INTO secrets (account_id, kind, ciphertext, nonce) VALUES (?1, ?2, ?3, ?4) \
         ON CONFLICT (account_id, kind) DO UPDATE SET ciphertext = excluded.ciphertext, nonce = excluded.nonce",
        params![account_id, kind, ciphertext, nonce],
    )?;
    Ok(())
}

pub fn get_secret(c: &Connection, account_id: &str, kind: &str) -> Result<Option<(Vec<u8>, Vec<u8>)>> {
    Ok(c.query_row(
        "SELECT ciphertext, nonce FROM secrets WHERE account_id = ?1 AND kind = ?2",
        params![account_id, kind],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()?)
}

// ---------------------------------------------------------------------------------------------
// Folders

/// Folder with its sync state, used by the sync engine.
#[derive(Debug, Clone)]
pub struct FolderRow {
    pub id: i64,
    pub account_id: String,
    pub path: String,
    pub role: String,
    pub hidden: bool,
    pub uidvalidity: Option<u32>,
    pub uidnext: Option<u32>,
    pub highestmodseq: Option<u64>,
    pub fully_loaded: bool,
    /// The unsubscribe headers of all stored messages have been read.
    pub unsub_scanned: bool,
}

const FOLDER_ROW_COLS: &str =
    "id, account_id, path, role, hidden, uidvalidity, uidnext, highestmodseq, fully_loaded, unsub_scanned";

fn folder_row(r: &Row) -> rusqlite::Result<FolderRow> {
    Ok(FolderRow {
        id: r.get(0)?,
        account_id: r.get(1)?,
        path: r.get(2)?,
        role: r.get(3)?,
        hidden: r.get(4)?,
        uidvalidity: r.get::<_, Option<i64>>(5)?.map(|v| v as u32),
        uidnext: r.get::<_, Option<i64>>(6)?.map(|v| v as u32),
        highestmodseq: r.get::<_, Option<i64>>(7)?.map(|v| v as u64),
        fully_loaded: r.get(8)?,
        unsub_scanned: r.get(9)?,
    })
}

pub fn folder_rows(c: &Connection, account_id: &str) -> Result<Vec<FolderRow>> {
    let mut stmt = c.prepare(&format!(
        "SELECT {FOLDER_ROW_COLS} FROM folders WHERE account_id = ?1 ORDER BY id"
    ))?;
    let rows = stmt.query_map([account_id], folder_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn get_folder(c: &Connection, id: i64) -> Result<FolderRow> {
    c.query_row(
        &format!("SELECT {FOLDER_ROW_COLS} FROM folders WHERE id = ?1"),
        [id],
        folder_row,
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound(format!("folder {id}")))
}

pub fn folder_by_role(c: &Connection, account_id: &str, role: &str) -> Result<Option<FolderRow>> {
    Ok(c.query_row(
        &format!(
            "SELECT {FOLDER_ROW_COLS} FROM folders WHERE account_id = ?1 AND role = ?2 ORDER BY id LIMIT 1"
        ),
        params![account_id, role],
        folder_row,
    )
    .optional()?)
}

pub fn folder_by_path(c: &Connection, account_id: &str, path: &str) -> Result<Option<FolderRow>> {
    Ok(c.query_row(
        &format!("SELECT {FOLDER_ROW_COLS} FROM folders WHERE account_id = ?1 AND path = ?2"),
        params![account_id, path],
        folder_row,
    )
    .optional()?)
}

pub struct FolderUpsert<'a> {
    pub path: &'a str,
    pub name: &'a str,
    pub depth: u32,
    pub role: &'a str,
    pub hidden: bool,
}

pub fn upsert_folder(c: &Connection, account_id: &str, f: &FolderUpsert) -> Result<i64> {
    c.execute(
        "INSERT INTO folders (account_id, path, name, depth, role, hidden) VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
         ON CONFLICT (account_id, path) DO UPDATE SET name = excluded.name, depth = excluded.depth, \
         role = excluded.role, hidden = excluded.hidden",
        params![account_id, f.path, f.name, f.depth, f.role, f.hidden],
    )?;
    Ok(c.query_row(
        "SELECT id FROM folders WHERE account_id = ?1 AND path = ?2",
        params![account_id, f.path],
        |r| r.get(0),
    )?)
}

/// Removes folders that no longer exist on the server.
pub fn delete_folders_except(c: &Connection, account_id: &str, keep: &[i64]) -> Result<usize> {
    let existing: Vec<i64> = {
        let mut stmt = c.prepare("SELECT id FROM folders WHERE account_id = ?1")?;
        let rows = stmt.query_map([account_id], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    let mut removed = 0;
    for id in existing.into_iter().filter(|id| !keep.contains(id)) {
        removed += c.execute("DELETE FROM folders WHERE id = ?1", [id])?;
    }
    Ok(removed)
}

pub struct FolderSyncState {
    pub uidvalidity: Option<u32>,
    pub uidnext: Option<u32>,
    pub highestmodseq: Option<u64>,
    pub unread: i64,
    pub total: i64,
}

pub fn update_folder_sync_state(c: &Connection, id: i64, s: &FolderSyncState) -> Result<()> {
    c.execute(
        "UPDATE folders SET uidvalidity = ?2, uidnext = ?3, highestmodseq = ?4, unread_count = ?5, \
         total_count = ?6, last_sync = ?7 WHERE id = ?1",
        params![
            id,
            s.uidvalidity.map(|v| v as i64),
            s.uidnext.map(|v| v as i64),
            s.highestmodseq.map(|v| v as i64),
            s.unread,
            s.total,
            now()
        ],
    )?;
    Ok(())
}

pub fn set_fully_loaded(c: &Connection, id: i64, value: bool) -> Result<()> {
    c.execute(
        "UPDATE folders SET fully_loaded = ?2 WHERE id = ?1",
        params![id, value],
    )?;
    Ok(())
}

/// Drops all cached messages of a folder (UIDVALIDITY changed).
pub fn reset_folder(c: &Connection, id: i64) -> Result<()> {
    c.execute("DELETE FROM messages WHERE folder_id = ?1", [id])?;
    c.execute(
        "UPDATE folders SET uidvalidity = NULL, uidnext = NULL, highestmodseq = NULL, fully_loaded = 0 WHERE id = ?1",
        [id],
    )?;
    Ok(())
}

pub fn adjust_unread(c: &Connection, folder_id: i64, delta: i64) -> Result<()> {
    c.execute(
        "UPDATE folders SET unread_count = MAX(0, unread_count + ?2) WHERE id = ?1",
        params![folder_id, delta],
    )?;
    Ok(())
}

pub fn list_folders(c: &Connection) -> Result<Vec<Folder>> {
    let mut stmt = c.prepare(
        "SELECT f.id, f.account_id, f.path, f.name, f.depth, f.role, f.unread_count, f.total_count \
         FROM folders f JOIN accounts a ON a.id = f.account_id WHERE f.hidden = 0 \
         ORDER BY a.sort_order, \
           CASE f.role WHEN 'inbox' THEN 0 WHEN 'drafts' THEN 1 WHEN 'sent' THEN 2 WHEN 'archive' THEN 3 \
             WHEN 'junk' THEN 4 WHEN 'trash' THEN 5 ELSE 6 END, \
           f.path COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Folder {
            id: r.get(0)?,
            account_id: r.get(1)?,
            path: r.get(2)?,
            name: r.get(3)?,
            depth: r.get(4)?,
            role: r.get(5)?,
            unread_count: r.get(6)?,
            total_count: r.get(7)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

// ---------------------------------------------------------------------------------------------
// Messages

pub struct NewMessage {
    pub account_id: String,
    pub folder_id: i64,
    pub uid: u32,
    pub message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub thread_key: String,
    pub from: Option<Address>,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub reply_to: Vec<Address>,
    pub subject: String,
    pub date: i64,
    pub seen: bool,
    pub flagged: bool,
    pub answered: bool,
    pub has_attachments: bool,
    pub preview: String,
    pub size: u32,
    pub unsubscribe: Option<Unsubscribe>,
}

fn addresses_text(list: &[Address]) -> String {
    list.iter()
        .map(|a| match &a.name {
            Some(n) => format!("{n} {}", a.email),
            None => a.email.clone(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Inserts a message (or refreshes its flags if the UID is already known). Returns `true` when
/// a new row was created.
pub fn upsert_message(c: &Connection, m: &NewMessage, collect_recipients: bool) -> Result<bool> {
    let existing: Option<i64> = c
        .query_row(
            "SELECT id FROM messages WHERE folder_id = ?1 AND uid = ?2",
            params![m.folder_id, m.uid],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = existing {
        c.execute(
            "UPDATE messages SET seen = ?2, flagged = ?3, answered = ?4 WHERE id = ?1",
            params![id, m.seen, m.flagged, m.answered],
        )?;
        return Ok(false);
    }
    c.execute(
        "INSERT INTO messages (account_id, folder_id, uid, message_id, in_reply_to, refs, thread_key, \
         from_name, from_addr, to_json, cc_json, reply_to_json, subject, date, seen, flagged, answered, \
         has_attachments, preview, size, unsubscribe) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)",
        params![
            m.account_id,
            m.folder_id,
            m.uid,
            m.message_id,
            m.in_reply_to,
            serde_json::to_string(&m.references)?,
            m.thread_key,
            m.from.as_ref().and_then(|a| a.name.clone()),
            m.from.as_ref().map(|a| a.email.clone()),
            serde_json::to_string(&m.to)?,
            serde_json::to_string(&m.cc)?,
            serde_json::to_string(&m.reply_to)?,
            m.subject,
            m.date,
            m.seen,
            m.flagged,
            m.answered,
            m.has_attachments,
            m.preview,
            m.size,
            m.unsubscribe.as_ref().map(serde_json::to_string).transpose()?
        ],
    )?;
    let id = c.last_insert_rowid();
    let mut recipients = m.to.clone();
    recipients.extend(m.cc.iter().cloned());
    c.execute(
        "INSERT INTO messages_fts (rowid, subject, sender, recipients, preview, body) VALUES (?1, ?2, ?3, ?4, ?5, '')",
        params![
            id,
            m.subject,
            m.from.as_ref().map(|a| addresses_text(std::slice::from_ref(a))).unwrap_or_default(),
            addresses_text(&recipients),
            m.preview
        ],
    )?;
    if let Some(from) = &m.from {
        bump_contact(c, from, 1, m.date)?;
    }
    if collect_recipients {
        for a in &recipients {
            bump_contact(c, a, 3, m.date)?;
        }
    }
    Ok(true)
}

pub fn max_uid(c: &Connection, folder_id: i64) -> Result<Option<u32>> {
    Ok(c.query_row(
        "SELECT MAX(uid) FROM messages WHERE folder_id = ?1",
        [folder_id],
        |r| r.get::<_, Option<i64>>(0),
    )?
    .map(|v| v as u32))
}

pub fn min_uid(c: &Connection, folder_id: i64) -> Result<Option<u32>> {
    Ok(c.query_row(
        "SELECT MIN(uid) FROM messages WHERE folder_id = ?1",
        [folder_id],
        |r| r.get::<_, Option<i64>>(0),
    )?
    .map(|v| v as u32))
}

pub fn uids_in_range(c: &Connection, folder_id: i64, from: u32, to: u32) -> Result<Vec<u32>> {
    let mut stmt = c.prepare("SELECT uid FROM messages WHERE folder_id = ?1 AND uid BETWEEN ?2 AND ?3")?;
    let rows = stmt.query_map(params![folder_id, from, to], |r| r.get::<_, i64>(0))?;
    Ok(rows
        .map(|r| r.map(|v| v as u32))
        .collect::<rusqlite::Result<_>>()?)
}

/// Applies server flags. Returns `true` if anything changed.
pub fn update_flags(
    c: &Connection,
    folder_id: i64,
    uid: u32,
    seen: bool,
    flagged: bool,
    answered: bool,
) -> Result<bool> {
    let n = c.execute(
        "UPDATE messages SET seen = ?3, flagged = ?4, answered = ?5 \
         WHERE folder_id = ?1 AND uid = ?2 AND (seen != ?3 OR flagged != ?4 OR answered != ?5)",
        params![folder_id, uid, seen, flagged, answered],
    )?;
    Ok(n > 0)
}

pub fn delete_by_uids(c: &Connection, folder_id: i64, uids: &[u32]) -> Result<usize> {
    let mut n = 0;
    for uid in uids {
        n += c.execute(
            "DELETE FROM messages WHERE folder_id = ?1 AND uid = ?2",
            params![folder_id, uid],
        )?;
    }
    Ok(n)
}

pub fn delete_messages(c: &Connection, ids: &[i64]) -> Result<()> {
    for id in ids {
        c.execute("DELETE FROM messages WHERE id = ?1", [id])?;
    }
    Ok(())
}

/// Minimal reference to a message for actions.
#[derive(Debug, Clone)]
pub struct MessageRef {
    pub id: i64,
    pub account_id: String,
    pub folder_id: i64,
    pub uid: u32,
    pub seen: bool,
    pub flagged: bool,
    pub from_addr: Option<String>,
}

pub fn message_refs(c: &Connection, ids: &[i64]) -> Result<Vec<MessageRef>> {
    let mut out = Vec::with_capacity(ids.len());
    let mut stmt = c.prepare(
        "SELECT id, account_id, folder_id, uid, seen, flagged, from_addr FROM messages WHERE id = ?1",
    )?;
    for id in ids {
        if let Some(r) = stmt
            .query_row([id], |r| {
                Ok(MessageRef {
                    id: r.get(0)?,
                    account_id: r.get(1)?,
                    folder_id: r.get(2)?,
                    uid: r.get::<_, i64>(3)? as u32,
                    seen: r.get(4)?,
                    flagged: r.get(5)?,
                    from_addr: r.get(6)?,
                })
            })
            .optional()?
        {
            out.push(r);
        }
    }
    Ok(out)
}

pub fn set_seen(c: &Connection, id: i64, value: bool) -> Result<()> {
    c.execute("UPDATE messages SET seen = ?2 WHERE id = ?1", params![id, value])?;
    Ok(())
}

pub fn set_flagged(c: &Connection, id: i64, value: bool) -> Result<()> {
    c.execute(
        "UPDATE messages SET flagged = ?2 WHERE id = ?1",
        params![id, value],
    )?;
    Ok(())
}

pub fn set_answered(c: &Connection, id: i64) -> Result<()> {
    c.execute("UPDATE messages SET answered = 1 WHERE id = ?1", [id])?;
    Ok(())
}

pub fn set_preview_if_empty(c: &Connection, id: i64, preview: &str) -> Result<()> {
    c.execute(
        "UPDATE messages SET preview = ?2 WHERE id = ?1 AND preview = ''",
        params![id, preview],
    )?;
    c.execute(
        "UPDATE messages_fts SET preview = ?2 WHERE rowid = ?1 AND preview = ''",
        params![id, preview],
    )?;
    Ok(())
}

pub fn set_has_attachments(c: &Connection, id: i64, value: bool) -> Result<()> {
    c.execute(
        "UPDATE messages SET has_attachments = ?2 WHERE id = ?1",
        params![id, value],
    )?;
    Ok(())
}

const SUMMARY_SELECT: &str =
    "id, account_id, folder_id, thread_key, from_name, from_addr, to_json, subject, \
     date, seen, flagged, answered, has_attachments, preview";

fn summary_from_row(r: &Row, thread_count: i64) -> rusqlite::Result<MessageSummary> {
    let from_name: Option<String> = r.get(4)?;
    let from_addr: Option<String> = r.get(5)?;
    let to_json: String = r.get(6)?;
    Ok(MessageSummary {
        id: r.get(0)?,
        account_id: r.get(1)?,
        folder_id: r.get(2)?,
        thread_key: r.get(3)?,
        from: from_addr.map(|email| Address {
            name: from_name,
            email,
        }),
        to: serde_json::from_str(&to_json).unwrap_or_default(),
        subject: r.get(7)?,
        date: r.get(8)?,
        seen: r.get(9)?,
        flagged: r.get(10)?,
        answered: r.get(11)?,
        has_attachments: r.get(12)?,
        preview: r.get(13)?,
        thread_count,
    })
}

/// Builds an FTS5 query that matches every word as a prefix.
pub fn fts_query(input: &str) -> Option<String> {
    let terms: Vec<String> = input
        .split_whitespace()
        .map(|t| t.replace('"', ""))
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{t}\"*"))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" "))
    }
}

const VISIBLE_FOLDERS: &str = "SELECT id FROM folders WHERE hidden = 0 AND role NOT IN ('trash', 'junk')";

pub fn list_messages(
    c: &Connection,
    view: &View,
    offset: u32,
    limit: u32,
    threads: bool,
) -> Result<MessagePage> {
    let mut args: Vec<rusqlite::types::Value> = Vec::new();
    let filter = match view {
        View::Folder { folder_id } => {
            args.push((*folder_id).into());
            "m.folder_id = ?1".to_string()
        }
        View::Unified { which } => match which {
            UnifiedView::Inbox => "m.folder_id IN (SELECT id FROM folders WHERE role = 'inbox')".to_string(),
            UnifiedView::Unread => format!(
                "m.seen = 0 AND m.folder_id IN ({VISIBLE_FOLDERS} AND role NOT IN ('sent', 'drafts'))"
            ),
            UnifiedView::Starred => format!("m.flagged = 1 AND m.folder_id IN ({VISIBLE_FOLDERS})"),
            UnifiedView::Sent => "m.folder_id IN (SELECT id FROM folders WHERE role = 'sent')".to_string(),
            UnifiedView::Junk => "m.folder_id IN (SELECT id FROM folders WHERE role = 'junk')".to_string(),
            UnifiedView::Trash => "m.folder_id IN (SELECT id FROM folders WHERE role = 'trash')".to_string(),
        },
        View::Search { query } => {
            let Some(q) = fts_query(query) else {
                return Ok(MessagePage {
                    items: vec![],
                    has_more: false,
                });
            };
            args.push(q.into());
            "m.id IN (SELECT rowid FROM messages_fts WHERE messages_fts MATCH ?1)".to_string()
        }
        View::Sender { account_id, email } => {
            args.push(account_id.clone().into());
            args.push(email.clone().into());
            format!("m.account_id = ?1 AND m.from_addr = ?2 COLLATE NOCASE AND m.folder_id IN ({VISIBLE_FOLDERS})")
        }
    };
    let group = if threads { "account_id, thread_key" } else { "id" };
    let n = args.len();
    args.push((limit as i64 + 1).into());
    args.push((offset as i64).into());
    // Unified and search views dedupe copies of the same message (Gmail labels show up in
    // several folders), then optionally collapse conversations.
    let sql = format!(
        "WITH v AS ( \
           SELECT m.*, ROW_NUMBER() OVER ( \
             PARTITION BY m.account_id, COALESCE(m.message_id, 'local:' || m.id) ORDER BY m.id) AS dup \
           FROM messages m WHERE {filter} \
         ), g AS ( \
           SELECT v.*, \
             ROW_NUMBER() OVER (PARTITION BY {group} ORDER BY date DESC, id DESC) AS rn, \
             COUNT(*) OVER (PARTITION BY {group}) AS cnt, \
             MIN(seen) OVER (PARTITION BY {group}) AS all_seen, \
             MAX(flagged) OVER (PARTITION BY {group}) AS any_flagged \
           FROM v WHERE dup = 1 \
         ) \
         SELECT id, account_id, folder_id, thread_key, from_name, from_addr, to_json, subject, date, \
           all_seen, any_flagged, answered, has_attachments, preview, cnt \
         FROM g WHERE rn = 1 ORDER BY date DESC, id DESC LIMIT ?{} OFFSET ?{}",
        n + 1,
        n + 2
    );
    let mut stmt = c.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
        let cnt: i64 = r.get(14)?;
        summary_from_row(r, cnt)
    })?;
    let mut items: Vec<MessageSummary> = rows.collect::<rusqlite::Result<_>>()?;
    let has_more = items.len() > limit as usize;
    items.truncate(limit as usize);
    Ok(MessagePage { items, has_more })
}

pub fn get_summary(c: &Connection, id: i64) -> Result<MessageSummary> {
    c.query_row(
        &format!("SELECT {SUMMARY_SELECT} FROM messages WHERE id = ?1"),
        [id],
        |r| summary_from_row(r, 1),
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound(format!("message {id}")))
}

/// Messages of the same conversation, oldest first.
pub fn thread_messages(c: &Connection, account_id: &str, thread_key: &str) -> Result<Vec<i64>> {
    let mut stmt =
        c.prepare("SELECT id FROM messages WHERE account_id = ?1 AND thread_key = ?2 ORDER BY date, id")?;
    let rows = stmt.query_map(params![account_id, thread_key], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub struct HeaderExtras {
    pub cc: Vec<Address>,
    pub reply_to: Vec<Address>,
    pub message_id: Option<String>,
    pub references: Vec<String>,
}

pub fn header_extras(c: &Connection, id: i64) -> Result<HeaderExtras> {
    let (cc, reply_to, message_id, refs): (String, String, Option<String>, String) = c.query_row(
        "SELECT cc_json, reply_to_json, message_id, refs FROM messages WHERE id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    Ok(HeaderExtras {
        cc: serde_json::from_str(&cc).unwrap_or_default(),
        reply_to: serde_json::from_str(&reply_to).unwrap_or_default(),
        message_id,
        references: serde_json::from_str(&refs).unwrap_or_default(),
    })
}

pub fn unified_counts(c: &Connection) -> Result<UnifiedCounts> {
    let inbox_unread = c.query_row(
        "SELECT COALESCE(SUM(unread_count), 0) FROM folders WHERE role = 'inbox'",
        [],
        |r| r.get(0),
    )?;
    let unread = c.query_row(
        &format!(
            "SELECT COUNT(DISTINCT account_id || ':' || COALESCE(message_id, id)) FROM messages \
             WHERE seen = 0 AND folder_id IN ({VISIBLE_FOLDERS} AND role NOT IN ('sent', 'drafts'))"
        ),
        [],
        |r| r.get(0),
    )?;
    let starred = c.query_row(
        &format!(
            "SELECT COUNT(DISTINCT account_id || ':' || COALESCE(message_id, id)) FROM messages \
             WHERE flagged = 1 AND folder_id IN ({VISIBLE_FOLDERS})"
        ),
        [],
        |r| r.get(0),
    )?;
    let drafts = c.query_row("SELECT COUNT(*) FROM drafts", [], |r| r.get(0))?;
    Ok(UnifiedCounts {
        inbox_unread,
        unread,
        starred,
        drafts,
    })
}

// ---------------------------------------------------------------------------------------------
// Bodies

pub struct BodyRow {
    pub text: Option<String>,
    pub html: Option<String>,
    pub has_remote: bool,
    pub attachments: Vec<AttachmentInfo>,
}

pub fn save_body(c: &Connection, message_id: i64, body: &BodyRow, raw: &[u8]) -> Result<()> {
    c.execute(
        "INSERT INTO bodies (message_id, text, html, has_remote, attachments_json, raw) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
         ON CONFLICT (message_id) DO UPDATE SET text = excluded.text, html = excluded.html, \
         has_remote = excluded.has_remote, attachments_json = excluded.attachments_json, raw = excluded.raw",
        params![
            message_id,
            body.text,
            body.html,
            body.has_remote,
            serde_json::to_string(&body.attachments)?,
            raw
        ],
    )?;
    c.execute(
        "UPDATE messages_fts SET body = ?2 WHERE rowid = ?1",
        params![message_id, body.text.clone().unwrap_or_default()],
    )?;
    Ok(())
}

pub fn get_body(c: &Connection, message_id: i64) -> Result<Option<BodyRow>> {
    Ok(c.query_row(
        "SELECT text, html, has_remote, attachments_json FROM bodies WHERE message_id = ?1",
        [message_id],
        |r| {
            let att: String = r.get(3)?;
            Ok(BodyRow {
                text: r.get(0)?,
                html: r.get(1)?,
                has_remote: r.get(2)?,
                attachments: serde_json::from_str(&att).unwrap_or_default(),
            })
        },
    )
    .optional()?)
}

pub fn get_raw(c: &Connection, message_id: i64) -> Result<Option<Vec<u8>>> {
    Ok(c.query_row(
        "SELECT raw FROM bodies WHERE message_id = ?1",
        [message_id],
        |r| r.get(0),
    )
    .optional()?)
}

// ---------------------------------------------------------------------------------------------
// Newsletters

/// Stores unsubscribe data read for already synced messages, by UID.
pub fn set_unsubscribe(c: &Connection, folder_id: i64, found: &[(u32, Unsubscribe)]) -> Result<()> {
    let mut stmt = c.prepare("UPDATE messages SET unsubscribe = ?3 WHERE folder_id = ?1 AND uid = ?2")?;
    for (uid, u) in found {
        stmt.execute(params![folder_id, uid, serde_json::to_string(u)?])?;
    }
    Ok(())
}

pub fn set_unsub_scanned(c: &Connection, folder_id: i64) -> Result<()> {
    c.execute("UPDATE folders SET unsub_scanned = 1 WHERE id = ?1", [folder_id])?;
    Ok(())
}

/// A message's unsubscribe data, and whether its sender was left already.
pub struct UnsubscribeRow {
    pub account_id: String,
    pub sender: String,
    pub unsubscribe: Unsubscribe,
    pub folder_role: String,
    pub unsubscribed_at: Option<i64>,
}

pub fn unsubscribe_row(c: &Connection, message_id: i64) -> Result<Option<UnsubscribeRow>> {
    let row = c
        .query_row(
            "SELECT m.account_id, m.from_addr, m.unsubscribe, f.role, u.at FROM messages m \
             JOIN folders f ON f.id = m.folder_id \
             LEFT JOIN unsubscribed u ON u.account_id = m.account_id AND u.sender = m.from_addr \
             WHERE m.id = ?1 AND m.unsubscribe IS NOT NULL AND m.from_addr IS NOT NULL",
            [message_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<i64>>(4)?,
                ))
            },
        )
        .optional()?;
    Ok(
        row.and_then(|(account_id, sender, json, folder_role, unsubscribed_at)| {
            Some(UnsubscribeRow {
                account_id,
                sender,
                unsubscribe: serde_json::from_str(&json).ok()?,
                folder_role,
                unsubscribed_at,
            })
        }),
    )
}

pub fn mark_unsubscribed(c: &Connection, account_id: &str, sender: &str, method: Method) -> Result<()> {
    c.execute(
        "INSERT INTO unsubscribed (account_id, sender, method, at) VALUES (?1, ?2, ?3, ?4) \
         ON CONFLICT (account_id, sender) DO UPDATE SET method = excluded.method, at = excluded.at",
        params![account_id, sender, method.as_str(), now()],
    )?;
    Ok(())
}

/// Newsletter senders per account: the ones still subscribed first, the busiest of those on
/// top. `recent_since` starts the window for [`Subscription::recent`].
pub fn list_subscriptions(c: &Connection, recent_since: i64) -> Result<Vec<Subscription>> {
    // Copies of one message in several folders (Gmail labels) count once, as in the list.
    let sql = format!(
        "WITH v AS ( \
           SELECT m.id, m.account_id, m.from_name, m.from_addr, m.date, m.unsubscribe, \
             ROW_NUMBER() OVER ( \
               PARTITION BY m.account_id, COALESCE(m.message_id, 'local:' || m.id) ORDER BY m.id) AS dup \
           FROM messages m \
           WHERE m.unsubscribe IS NOT NULL AND m.from_addr IS NOT NULL \
             AND m.folder_id IN ({VISIBLE_FOLDERS} AND role NOT IN ('sent', 'drafts')) \
         ), s AS ( \
           SELECT v.*, u.at AS unsubscribed_at, \
             ROW_NUMBER() OVER (PARTITION BY v.account_id, lower(v.from_addr) ORDER BY v.date DESC, v.id DESC) AS rn, \
             COUNT(*) OVER (PARTITION BY v.account_id, lower(v.from_addr)) AS total, \
             SUM(v.date >= ?1) OVER (PARTITION BY v.account_id, lower(v.from_addr)) AS recent, \
             SUM(u.at IS NOT NULL AND v.date > u.at) OVER (PARTITION BY v.account_id, lower(v.from_addr)) AS since \
           FROM v LEFT JOIN unsubscribed u ON u.account_id = v.account_id AND u.sender = v.from_addr \
           WHERE v.dup = 1 \
         ) \
         SELECT id, account_id, from_name, from_addr, date, unsubscribe, unsubscribed_at, total, recent, since \
         FROM s WHERE rn = 1 \
         ORDER BY unsubscribed_at IS NOT NULL, recent DESC, total DESC, date DESC"
    );
    let mut stmt = c.prepare(&sql)?;
    let rows = stmt.query_map([recent_since], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, i64>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, Option<i64>>(6)?,
            r.get::<_, i64>(7)?,
            r.get::<_, i64>(8)?,
            r.get::<_, i64>(9)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (latest_id, account_id, name, email, last_date, json, at, total, recent, since) = row?;
        let Some(info) = serde_json::from_str::<Unsubscribe>(&json)
            .ok()
            .and_then(|u| u.info(at))
        else {
            continue;
        };
        out.push(Subscription {
            account_id,
            sender: Address { name, email },
            latest_id,
            last_date,
            total,
            recent,
            since_unsubscribed: since,
            unsubscribe: info,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Drafts

pub fn save_draft(c: &Connection, d: &Draft) -> Result<i64> {
    let (to, cc, bcc) = (
        serde_json::to_string(&d.to)?,
        serde_json::to_string(&d.cc)?,
        serde_json::to_string(&d.bcc)?,
    );
    let (refs, attachments) = (
        serde_json::to_string(&d.references)?,
        serde_json::to_string(&d.attachments)?,
    );
    let ts = now();
    if let Some(id) = d.id {
        let n = c.execute(
            "UPDATE drafts SET account_id = ?1, to_json = ?2, cc_json = ?3, bcc_json = ?4, subject = ?5, \
             body = ?6, in_reply_to = ?7, refs = ?8, attachments_json = ?9, reply_to_message_id = ?10, \
             forward_from_message_id = ?11, updated_at = ?12, from_email = ?14 WHERE id = ?13",
            params![
                d.account_id,
                to,
                cc,
                bcc,
                d.subject,
                d.body,
                d.in_reply_to,
                refs,
                attachments,
                d.reply_to_message_id,
                d.forward_from_message_id,
                ts,
                id,
                d.from_email
            ],
        )?;
        if n > 0 {
            return Ok(id);
        }
    }
    let values = params![
        d.account_id,
        to,
        cc,
        bcc,
        d.subject,
        d.body,
        d.in_reply_to,
        refs,
        attachments,
        d.reply_to_message_id,
        d.forward_from_message_id,
        ts,
        d.from_email,
    ];
    c.execute(
        "INSERT INTO drafts (account_id, to_json, cc_json, bcc_json, subject, body, in_reply_to, refs, \
         attachments_json, reply_to_message_id, forward_from_message_id, updated_at, from_email) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        values,
    )?;
    Ok(c.last_insert_rowid())
}

fn draft_from_row(r: &Row) -> rusqlite::Result<Draft> {
    let j = |i: usize| -> rusqlite::Result<String> { r.get(i) };
    Ok(Draft {
        id: Some(r.get(0)?),
        account_id: r.get(1)?,
        to: serde_json::from_str(&j(2)?).unwrap_or_default(),
        cc: serde_json::from_str(&j(3)?).unwrap_or_default(),
        bcc: serde_json::from_str(&j(4)?).unwrap_or_default(),
        subject: r.get(5)?,
        body: r.get(6)?,
        in_reply_to: r.get(7)?,
        references: serde_json::from_str(&j(8)?).unwrap_or_default(),
        attachments: serde_json::from_str(&j(9)?).unwrap_or_default(),
        reply_to_message_id: r.get(10)?,
        forward_from_message_id: r.get(11)?,
        updated_at: r.get(12)?,
        from_email: r.get(13)?,
    })
}

const DRAFT_COLS: &str = "id, account_id, to_json, cc_json, bcc_json, subject, body, in_reply_to, refs, \
     attachments_json, reply_to_message_id, forward_from_message_id, updated_at, from_email";

pub fn list_drafts(c: &Connection) -> Result<Vec<Draft>> {
    let mut stmt = c.prepare(&format!(
        "SELECT {DRAFT_COLS} FROM drafts ORDER BY updated_at DESC"
    ))?;
    let rows = stmt.query_map([], draft_from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn get_draft(c: &Connection, id: i64) -> Result<Draft> {
    c.query_row(
        &format!("SELECT {DRAFT_COLS} FROM drafts WHERE id = ?1"),
        [id],
        draft_from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound(format!("draft {id}")))
}

pub fn delete_draft(c: &Connection, id: i64) -> Result<()> {
    c.execute("DELETE FROM drafts WHERE id = ?1", [id])?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Contacts and remote content

pub fn bump_contact(c: &Connection, a: &Address, weight: i64, seen_at: i64) -> Result<()> {
    if a.email.is_empty() || !a.email.contains('@') {
        return Ok(());
    }
    c.execute(
        "INSERT INTO contacts (email, name, weight, last_seen) VALUES (?1, ?2, ?3, ?4) \
         ON CONFLICT (email) DO UPDATE SET name = COALESCE(excluded.name, contacts.name), \
         weight = contacts.weight + excluded.weight, last_seen = MAX(contacts.last_seen, excluded.last_seen)",
        params![a.email, a.name, weight, seen_at],
    )?;
    Ok(())
}

pub fn suggest_contacts(c: &Connection, prefix: &str, limit: u32) -> Result<Vec<Address>> {
    let pattern = format!(
        "{}%",
        prefix
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    let word = format!("% {pattern}");
    let mut stmt = c.prepare(
        "SELECT email, name FROM contacts \
         WHERE email LIKE ?1 ESCAPE '\\' OR name LIKE ?1 ESCAPE '\\' OR name LIKE ?2 ESCAPE '\\' \
         ORDER BY weight DESC, last_seen DESC LIMIT ?3",
    )?;
    let rows = stmt.query_map(params![pattern, word, limit], |r| {
        Ok(Address {
            email: r.get(0)?,
            name: r.get(1)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn allow_remote_content(c: &Connection, sender: &str) -> Result<()> {
    c.execute(
        "INSERT OR IGNORE INTO remote_content_allowed (sender) VALUES (?1)",
        [sender],
    )?;
    Ok(())
}

pub fn is_remote_allowed(c: &Connection, sender: &str) -> Result<bool> {
    Ok(c.query_row(
        "SELECT 1 FROM remote_content_allowed WHERE sender = ?1",
        [sender],
        |_| Ok(()),
    )
    .optional()?
    .is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Db;

    fn msg(account: &str, folder: i64, uid: u32, mid: &str, thread: &str, date: i64) -> NewMessage {
        NewMessage {
            account_id: account.into(),
            folder_id: folder,
            uid,
            message_id: Some(mid.into()),
            in_reply_to: None,
            references: vec![],
            thread_key: thread.into(),
            from: Some(Address {
                name: Some("Anna Schmidt".into()),
                email: "anna@example.org".into(),
            }),
            to: vec![],
            cc: vec![],
            reply_to: vec![],
            subject: format!("Subject {uid}"),
            date,
            seen: false,
            flagged: false,
            answered: false,
            has_attachments: false,
            preview: "Quarterly report attached".into(),
            size: 100,
            unsubscribe: None,
        }
    }

    fn newsletter(folder: i64, uid: u32, mid: &str, sender: &str, date: i64) -> NewMessage {
        let mut m = msg("a1", folder, uid, mid, mid, date);
        m.from = Some(Address {
            name: Some(sender.split('@').nth(1).unwrap().into()),
            email: sender.into(),
        });
        m.unsubscribe = Some(Unsubscribe {
            url: Some(format!("https://{}/u", sender.split('@').nth(1).unwrap())),
            mailto: None,
            one_click: true,
        });
        m
    }

    fn setup() -> (Db, i64, i64) {
        let db = Db::open_in_memory().unwrap();
        let ids = db
            .with(|c| {
                insert_account(c, &crate::store::queries_test_account("a1"))?;
                let inbox = upsert_folder(
                    c,
                    "a1",
                    &FolderUpsert {
                        path: "INBOX",
                        name: "Inbox",
                        depth: 0,
                        role: "inbox",
                        hidden: false,
                    },
                )?;
                let label = upsert_folder(
                    c,
                    "a1",
                    &FolderUpsert {
                        path: "Work",
                        name: "Work",
                        depth: 0,
                        role: "other",
                        hidden: false,
                    },
                )?;
                Ok((inbox, label))
            })
            .unwrap();
        (db, ids.0, ids.1)
    }

    #[test]
    fn upsert_list_and_threads() {
        let (db, inbox, label) = setup();
        db.with(|c| {
            assert!(upsert_message(c, &msg("a1", inbox, 1, "m1", "t1", 100), false)?);
            assert!(upsert_message(c, &msg("a1", inbox, 2, "m2", "t1", 200), false)?);
            assert!(upsert_message(c, &msg("a1", inbox, 3, "m3", "t2", 300), false)?);
            // Same message id in a second folder (Gmail label) must not duplicate unified views.
            assert!(upsert_message(c, &msg("a1", label, 7, "m3", "t2", 300), false)?);
            // Second insert of the same UID only refreshes flags.
            let mut again = msg("a1", inbox, 1, "m1", "t1", 100);
            again.seen = true;
            assert!(!upsert_message(c, &again, false)?);

            let flat = list_messages(c, &View::Folder { folder_id: inbox }, 0, 50, false)?;
            assert_eq!(flat.items.len(), 3);
            assert_eq!(flat.items[0].subject, "Subject 3");

            let threaded = list_messages(c, &View::Folder { folder_id: inbox }, 0, 50, true)?;
            assert_eq!(threaded.items.len(), 2);
            let t1 = threaded.items.iter().find(|m| m.thread_key == "t1").unwrap();
            assert_eq!(t1.thread_count, 2);
            assert!(!t1.seen, "thread with an unread message is unread");

            let unread = list_messages(
                c,
                &View::Unified {
                    which: UnifiedView::Unread,
                },
                0,
                50,
                false,
            )?;
            assert_eq!(unread.items.len(), 2, "m3 deduplicated, m1 is read");

            let page = list_messages(c, &View::Folder { folder_id: inbox }, 0, 2, false)?;
            assert!(page.has_more);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn search_and_contacts() {
        let (db, inbox, _) = setup();
        db.with(|c| {
            upsert_message(c, &msg("a1", inbox, 1, "m1", "t1", 100), false)?;
            let found = list_messages(
                c,
                &View::Search {
                    query: "quart".into(),
                },
                0,
                10,
                false,
            )?;
            assert_eq!(found.items.len(), 1);
            let found = list_messages(
                c,
                &View::Search {
                    query: "schmidt".into(),
                },
                0,
                10,
                false,
            )?;
            assert_eq!(found.items.len(), 1);
            let none = list_messages(
                c,
                &View::Search {
                    query: "nothing".into(),
                },
                0,
                10,
                false,
            )?;
            assert!(none.items.is_empty());

            let s = suggest_contacts(c, "sch", 5)?;
            assert_eq!(s[0].email, "anna@example.org");
            let s = suggest_contacts(c, "ann", 5)?;
            assert_eq!(s.len(), 1);

            // Deleting the message removes it from the index too.
            delete_by_uids(c, inbox, &[1])?;
            let gone = list_messages(
                c,
                &View::Search {
                    query: "quart".into(),
                },
                0,
                10,
                false,
            )?;
            assert!(gone.items.is_empty());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn subscriptions() {
        let (db, inbox, label) = setup();
        db.with(|c| {
            let junk = upsert_folder(
                c,
                "a1",
                &FolderUpsert {
                    path: "Junk",
                    name: "Junk",
                    depth: 0,
                    role: "junk",
                    hidden: false,
                },
            )?;
            let day = 86_400;
            let today = 100 * day;
            upsert_message(
                c,
                &newsletter(inbox, 1, "s1", "news@shop.example", today - 40 * day),
                false,
            )?;
            upsert_message(
                c,
                &newsletter(inbox, 2, "s2", "News@Shop.example", today - 2 * day),
                false,
            )?;
            // A Gmail label copy of s2 counts once.
            upsert_message(
                c,
                &newsletter(label, 3, "s2", "news@shop.example", today - 2 * day),
                false,
            )?;
            upsert_message(
                c,
                &newsletter(inbox, 4, "c1", "club@verein.example", today - 60 * day),
                false,
            )?;
            upsert_message(c, &newsletter(junk, 5, "j1", "deals@spam.example", today), false)?;
            upsert_message(c, &msg("a1", inbox, 6, "p1", "p1", today), false)?;

            let subs = list_subscriptions(c, today - 30 * day)?;
            let senders: Vec<_> = subs.iter().map(|s| s.sender.email.to_lowercase()).collect();
            assert_eq!(
                senders,
                ["news@shop.example", "club@verein.example"],
                "no spam, no ordinary mail"
            );
            assert_eq!((subs[0].total, subs[0].recent), (2, 1));
            assert_eq!(subs[0].unsubscribe.method, Method::OneClick);
            assert_eq!(subs[0].unsubscribe.target, "shop.example");
            let latest = subs[0].latest_id;
            assert_eq!(get_summary(c, latest)?.date, today - 2 * day);

            // Unsubscribed senders move to the end and count what still arrives.
            let row = unsubscribe_row(c, latest)?.unwrap();
            assert_eq!(row.folder_role, "inbox");
            mark_unsubscribed(c, "a1", &row.sender, Method::OneClick)?;
            c.execute("UPDATE unsubscribed SET at = ?1", [today - 3 * day])?;
            let subs = list_subscriptions(c, today - 30 * day)?;
            assert_eq!(subs[0].sender.email, "club@verein.example");
            assert_eq!(subs[1].unsubscribe.unsubscribed_at, Some(today - 3 * day));
            assert_eq!(subs[1].since_unsubscribed, 1);
            assert!(unsubscribe_row(c, latest)?.unwrap().unsubscribed_at.is_some());

            let mail = list_messages(
                c,
                &View::Sender {
                    account_id: "a1".into(),
                    email: "NEWS@shop.example".into(),
                },
                0,
                10,
                false,
            )?;
            assert_eq!(mail.items.len(), 2);

            let spam = c.query_row("SELECT id FROM messages WHERE uid = 5", [], |r| {
                r.get::<_, i64>(0)
            })?;
            assert_eq!(unsubscribe_row(c, spam)?.unwrap().folder_role, "junk");
            let plain = c.query_row("SELECT id FROM messages WHERE uid = 6", [], |r| {
                r.get::<_, i64>(0)
            })?;
            assert!(unsubscribe_row(c, plain)?.is_none());

            // The backfill for messages synced before the column existed.
            set_unsubscribe(
                c,
                inbox,
                &[(
                    6,
                    Unsubscribe {
                        url: None,
                        mailto: Some("mailto:x@y.example".into()),
                        one_click: false,
                    },
                )],
            )?;
            assert_eq!(
                unsubscribe_row(c, plain)?.unwrap().unsubscribe.method(),
                Some(Method::Mail)
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn drafts_round_trip() {
        let (db, _, _) = setup();
        db.with(|c| {
            let mut d = Draft {
                account_id: "a1".into(),
                subject: "Hi".into(),
                to: vec![Address {
                    name: None,
                    email: "x@y.z".into(),
                }],
                ..Default::default()
            };
            let id = save_draft(c, &d)?;
            d.id = Some(id);
            d.subject = "Hi again".into();
            d.from_email = Some("alias@y.z".into());
            assert_eq!(save_draft(c, &d)?, id);
            let back = get_draft(c, id)?;
            assert_eq!(back.subject, "Hi again");
            assert_eq!(back.from_email.as_deref(), Some("alias@y.z"));
            assert_eq!(back.to.len(), 1);
            assert_eq!(list_drafts(c)?.len(), 1);
            delete_draft(c, id)?;
            assert!(list_drafts(c)?.is_empty());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn aliases_are_replaced_and_report_changes() {
        let (db, _, _) = setup();
        db.with(|c| {
            let list = vec![
                Address {
                    name: Some("Max".into()),
                    email: "z@gmail.com".into(),
                },
                Address {
                    name: None,
                    email: "a@own.example".into(),
                },
            ];
            assert!(replace_aliases(c, "a1", &list)?);
            assert_eq!(aliases(c, "a1")?, list, "keeps the order it was given");
            assert!(!replace_aliases(c, "a1", &list)?);
            assert!(replace_aliases(c, "a1", &list[1..])?);
            assert_eq!(aliases(c, "a1")?.len(), 1);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn account_delete_cascades() {
        let (db, inbox, _) = setup();
        db.with(|c| {
            upsert_message(c, &msg("a1", inbox, 1, "m1", "t1", 100), false)?;
            replace_aliases(
                c,
                "a1",
                &[Address {
                    name: None,
                    email: "x@y.z".into(),
                }],
            )?;
            delete_account(c, "a1")?;
            assert!(aliases(c, "a1")?.is_empty());
            let n: i64 = c.query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))?;
            assert_eq!(n, 0);
            let n: i64 = c.query_row("SELECT COUNT(*) FROM messages_fts", [], |r| r.get(0))?;
            assert_eq!(n, 0);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn settings_are_replaced() {
        let db = Db::open_in_memory().unwrap();
        db.with(|c| {
            assert_eq!(get_setting(c, "k")?, None);
            set_setting(c, "k", "1")?;
            set_setting(c, "k", "0")?;
            assert_eq!(get_setting(c, "k")?.as_deref(), Some("0"));
            Ok(())
        })
        .unwrap();
    }
}
