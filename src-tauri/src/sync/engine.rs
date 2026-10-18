//! IMAP synchronisation of one account: folder sync, lazy bodies and server-side actions.

use std::collections::HashSet;
use std::sync::Arc;

use async_imap::types::{Fetch, Flag};

use super::folders;
use super::imap_conn::{self, collect, uid_set, Caps, Session};
use crate::core::Core;
use crate::error::{AppError, Result};
use crate::mime::parse::{parse_body, parse_header, thread_key};
use crate::model::Account;
use crate::store::queries::{self, BodyRow, FolderRow, FolderSyncState, NewMessage};
use crate::unsubscribe;

/// Messages fetched per folder on first sync.
pub const INITIAL_WINDOW: u32 = 500;
/// Messages fetched per "load older" request.
pub const OLDER_BATCH: usize = 200;
/// Messages per request when reading the unsubscribe headers of already synced mail.
const UNSUBSCRIBE_BATCH: usize = 500;

const HEADER_QUERY: &str = "(UID FLAGS INTERNALDATE RFC822.SIZE BODY.PEEK[HEADER] BODY.PEEK[TEXT]<0.3072>)";
/// Fallback for servers whose partial FETCH responses do not parse (GreenMail, some appliances).
/// The preview is then filled in when the body is fetched.
const HEADER_QUERY_NO_PREVIEW: &str = "(UID FLAGS INTERNALDATE RFC822.SIZE BODY.PEEK[HEADER])";

/// A lazily (re)connected IMAP session for one account.
pub struct Conn {
    pub core: Arc<Core>,
    pub account_id: String,
    session: Option<Session>,
    caps: Caps,
    selected: Option<String>,
    partial_fetch: bool,
}

impl Conn {
    pub fn new(core: Arc<Core>, account_id: &str) -> Conn {
        Conn {
            core,
            account_id: account_id.to_string(),
            session: None,
            caps: Caps::default(),
            selected: None,
            partial_fetch: true,
        }
    }

    pub fn account(&self) -> Result<Account> {
        self.core.db.with(|c| queries::get_account(c, &self.account_id))
    }

    pub async fn session(&mut self) -> Result<&mut Session> {
        if self.session.is_none() {
            let account = self.account()?;
            let (session, caps) = imap_conn::open_session(&self.core, &account).await?;
            self.session = Some(session);
            self.caps = caps;
            self.selected = None;
        }
        Ok(self.session.as_mut().expect("session set above"))
    }

    pub fn caps(&self) -> Caps {
        self.caps
    }

    /// Drops the connection; the next call reconnects.
    pub fn reset(&mut self) {
        self.session = None;
        self.selected = None;
    }

    pub async fn logout(&mut self) {
        if let Some(mut s) = self.session.take() {
            let _ = s.logout().await;
        }
    }

    fn header_query(&self) -> &'static str {
        if self.partial_fetch {
            HEADER_QUERY
        } else {
            HEADER_QUERY_NO_PREVIEW
        }
    }

    /// Fetches headers of `set` (UIDs or sequence numbers) from the selected folder.
    async fn fetch_headers(&mut self, set: String, by_uid: bool) -> Result<Vec<Fetch>> {
        let query = self.header_query();
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| AppError::Network("not connected".into()))?;
        let res = if by_uid {
            match session.uid_fetch(set, query).await {
                Ok(stream) => collect(stream).await,
                Err(e) => Err(e.into()),
            }
        } else {
            match session.fetch(set, query).await {
                Ok(stream) => collect(stream).await,
                Err(e) => Err(e.into()),
            }
        };
        if let Err(e) = &res {
            if self.partial_fetch && e.to_string().contains("during parsing") {
                tracing::info!(account = %self.account_id, "server sends unparsable partial fetches, disabling previews");
                self.partial_fetch = false;
                // The stream is out of sync after a parse error.
                self.reset();
            }
        }
        res
    }

    async fn select(&mut self, path: &str) -> Result<async_imap::types::Mailbox> {
        self.session().await?;
        let condstore = self.caps.condstore;
        let session = self.session.as_mut().expect("connected");
        let mb = imap_conn::select(session, path, condstore).await?;
        self.selected = Some(path.to_string());
        Ok(mb)
    }

    async fn ensure_selected(&mut self, path: &str) -> Result<&mut Session> {
        if self.session.is_none() || self.selected.as_deref() != Some(path) {
            self.select(path).await?;
        }
        Ok(self.session.as_mut().expect("connected"))
    }
}

// ---------------------------------------------------------------------------------------------
// Sync

/// Syncs the folder list and then every visible folder, inbox first.
pub async fn sync_account(conn: &mut Conn) -> Result<()> {
    let account = conn.account()?;
    let core = conn.core.clone();
    let mut folders = {
        let session = conn.session().await?;
        folders::sync_folder_list(&core, session, &account).await?
    };
    conn.selected = None;
    conn.core.mail_changed(&account.id, None);
    folders.retain(|f| !f.hidden);
    folders.sort_by_key(|f| match f.role.as_str() {
        "inbox" => 0,
        "sent" => 1,
        "drafts" => 2,
        "archive" => 3,
        _ => 4,
    });
    for folder in folders {
        match sync_folder(conn, &folder).await {
            Ok(_) => {}
            // A single broken folder (no permission, odd name) must not stop the rest.
            Err(AppError::Imap(msg)) => {
                tracing::warn!(folder = %folder.path, "folder sync failed: {msg}");
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

pub async fn sync_folder_by_id(conn: &mut Conn, folder_id: i64) -> Result<bool> {
    let folder = conn.core.db.with(|c| queries::get_folder(c, folder_id))?;
    sync_folder(conn, &folder).await
}

/// Brings one folder up to date. Returns `true` if anything changed.
pub async fn sync_folder(conn: &mut Conn, folder: &FolderRow) -> Result<bool> {
    let core = conn.core.clone();
    let account = conn.account()?;
    let mb = conn.select(&folder.path).await?;
    let caps = conn.caps();
    let mut changed = false;

    let uidvalidity = mb.uid_validity;
    let mut known_max = core.db.with(|c| queries::max_uid(c, folder.id))?;
    if folder.uidvalidity.is_some() && folder.uidvalidity != uidvalidity {
        tracing::info!(folder = %folder.path, "UIDVALIDITY changed, resetting folder");
        core.db.with(|c| queries::reset_folder(c, folder.id))?;
        known_max = None;
        changed = true;
    }

    if mb.exists == 0 {
        let n = core.db.with(|c| {
            c.execute("DELETE FROM messages WHERE folder_id = ?1", [folder.id])
                .map_err(AppError::from)
        })?;
        changed |= n > 0;
    } else if let Some(max) = known_max {
        // New messages.
        if mb.uid_next.is_none_or(|next| next > max + 1) {
            let fetches = conn.fetch_headers(format!("{}:*", max + 1), true).await?;
            changed |= store_fetches(&core, &account, folder, &fetches, Some(max + 1))? > 0;
        }
        let session = conn.session.as_mut().expect("selected");

        // Flag changes and deletions within the synced window.
        let oldest = core.db.with(|c| queries::min_uid(c, folder.id))?.unwrap_or(1);
        let use_condstore = caps.condstore && folder.highestmodseq.is_some() && mb.highest_modseq.is_some();
        let present: HashSet<u32> = if use_condstore {
            if mb.highest_modseq != folder.highestmodseq {
                let query = format!("(UID FLAGS) (CHANGEDSINCE {})", folder.highestmodseq.unwrap_or(0));
                let fetches = collect(session.uid_fetch(format!("{oldest}:{max}"), query).await?).await?;
                changed |= apply_flags(&core, folder, &fetches)?;
            }
            session.uid_search(format!("UID {oldest}:{max}")).await?
        } else {
            let fetches = collect(
                session
                    .uid_fetch(format!("{oldest}:{max}"), "(UID FLAGS)")
                    .await?,
            )
            .await?;
            changed |= apply_flags(&core, folder, &fetches)?;
            fetches.iter().filter_map(|f| f.uid).collect()
        };
        let local = core
            .db
            .with(|c| queries::uids_in_range(c, folder.id, oldest, max))?;
        let gone: Vec<u32> = local.into_iter().filter(|u| !present.contains(u)).collect();
        if !gone.is_empty() {
            core.db.with(|c| queries::delete_by_uids(c, folder.id, &gone))?;
            changed = true;
        }
    } else {
        // First sync: the newest messages by sequence number.
        let start = mb.exists.saturating_sub(INITIAL_WINDOW - 1).max(1);
        let fetches = conn.fetch_headers(format!("{start}:*"), false).await?;
        changed |= store_fetches(&core, &account, folder, &fetches, None)? > 0;
        if start == 1 {
            core.db.with(|c| queries::set_fully_loaded(c, folder.id, true))?;
        }
    }

    let session = conn.session.as_mut().expect("selected");
    let unread = if mb.exists == 0 {
        0
    } else {
        session.uid_search("UNSEEN").await?.len() as i64
    };
    imap_conn::drain_unsolicited(session);
    core.db.with(|c| {
        queries::update_folder_sync_state(
            c,
            folder.id,
            &FolderSyncState {
                uidvalidity,
                uidnext: mb.uid_next,
                highestmodseq: mb.highest_modseq,
                unread,
                total: mb.exists as i64,
            },
        )
    })?;
    if !folder.unsub_scanned {
        changed |= backfill_unsubscribe(conn, folder).await?;
    }
    if changed {
        core.mail_changed(&account.id, Some(folder.id));
    }
    Ok(changed)
}

fn flags_of(f: &Fetch) -> (bool, bool, bool) {
    let (mut seen, mut flagged, mut answered) = (false, false, false);
    for flag in f.flags() {
        match flag {
            Flag::Seen => seen = true,
            Flag::Flagged => flagged = true,
            Flag::Answered => answered = true,
            _ => {}
        }
    }
    (seen, flagged, answered)
}

fn apply_flags(core: &Core, folder: &FolderRow, fetches: &[Fetch]) -> Result<bool> {
    core.db.with(|c| {
        let tx = c.transaction()?;
        let mut changed = false;
        for f in fetches {
            if let Some(uid) = f.uid {
                let (seen, flagged, answered) = flags_of(f);
                changed |= queries::update_flags(&tx, folder.id, uid, seen, flagged, answered)?;
            }
        }
        tx.commit()?;
        Ok(changed)
    })
}

/// Parses header fetches and stores them. Returns the number of new messages.
fn store_fetches(
    core: &Core,
    account: &Account,
    folder: &FolderRow,
    fetches: &[Fetch],
    min_uid: Option<u32>,
) -> Result<usize> {
    let is_sent = folder.role == "sent";
    core.db.with(|c| {
        let tx = c.transaction()?;
        let mut inserted = 0;
        for f in fetches {
            let Some(uid) = f.uid else { continue };
            if min_uid.is_some_and(|min| uid < min) {
                continue;
            }
            let h = parse_header(f.header().unwrap_or_default(), f.text());
            let (seen, flagged, answered) = flags_of(f);
            let date = h
                .date
                .or_else(|| f.internal_date().map(|d| d.timestamp()))
                .unwrap_or_else(crate::store::now);
            let key = thread_key(
                h.message_id.as_deref(),
                h.in_reply_to.as_deref(),
                &h.references,
                &format!("local:{}:{}:{}", account.id, folder.id, uid),
            );
            let m = NewMessage {
                account_id: account.id.clone(),
                folder_id: folder.id,
                uid,
                message_id: h.message_id,
                in_reply_to: h.in_reply_to,
                references: h.references,
                thread_key: key,
                from: h.from,
                to: h.to,
                cc: h.cc,
                reply_to: h.reply_to,
                subject: h.subject,
                date,
                seen,
                flagged,
                answered,
                has_attachments: h.has_attachments,
                preview: h.preview,
                size: f.size.unwrap_or(0),
                unsubscribe: h.unsubscribe,
            };
            if queries::upsert_message(&tx, &m, is_sent)? {
                inserted += 1;
            }
        }
        tx.commit()?;
        Ok(inserted)
    })
}

/// Reads the unsubscribe headers of messages synced before they were stored, once per folder.
/// Returns `true` if a newsletter turned up. Runs at the end of a folder sync, with the folder
/// still selected.
async fn backfill_unsubscribe(conn: &mut Conn, folder: &FolderRow) -> Result<bool> {
    let core = conn.core.clone();
    let uids = core
        .db
        .with(|c| queries::uids_in_range(c, folder.id, 0, u32::MAX))?;
    let mut found = false;
    for chunk in uids.chunks(UNSUBSCRIBE_BATCH) {
        let session = conn.session.as_mut().expect("selected");
        let res = match session.uid_fetch(uid_set(chunk), unsubscribe::FETCH_QUERY).await {
            Ok(stream) => collect(stream).await,
            Err(e) => Err(e.into()),
        };
        let fetches = match res {
            Ok(f) => f,
            Err(e @ AppError::Network(_)) => return Err(e),
            Err(e) => {
                // A server that cannot answer this must not hold up the sync, the messages just
                // stay without unsubscribe data.
                tracing::warn!(folder = %folder.path, "reading unsubscribe headers failed: {e}");
                conn.reset();
                break;
            }
        };
        let news: Vec<_> = fetches
            .iter()
            .filter_map(|f| Some((f.uid?, unsubscribe::from_header(f.header()?)?)))
            .collect();
        found |= !news.is_empty();
        core.db.with(|c| {
            let tx = c.transaction()?;
            queries::set_unsubscribe(&tx, folder.id, &news)?;
            tx.commit()?;
            Ok(())
        })?;
    }
    if let Some(session) = conn.session.as_mut() {
        imap_conn::drain_unsolicited(session);
    }
    core.db.with(|c| queries::set_unsub_scanned(c, folder.id))?;
    Ok(found)
}

/// Fetches the next batch of older messages. Returns `true` if more remain on the server.
pub async fn load_older(conn: &mut Conn, folder_id: i64) -> Result<bool> {
    let core = conn.core.clone();
    let account = conn.account()?;
    let folder = core.db.with(|c| queries::get_folder(c, folder_id))?;
    if folder.fully_loaded {
        return Ok(false);
    }
    let oldest = core.db.with(|c| queries::min_uid(c, folder.id))?;
    let session = conn.ensure_selected(&folder.path).await?;
    let query = match oldest {
        Some(1) => {
            core.db.with(|c| queries::set_fully_loaded(c, folder.id, true))?;
            return Ok(false);
        }
        Some(min) => format!("UID 1:{}", min - 1),
        None => "ALL".to_string(),
    };
    let mut uids: Vec<u32> = session.uid_search(query).await?.into_iter().collect();
    uids.sort_unstable_by(|a, b| b.cmp(a));
    uids.truncate(OLDER_BATCH);
    let more = uids.len() == OLDER_BATCH;
    if !uids.is_empty() {
        let fetches = conn.fetch_headers(uid_set(&uids), true).await?;
        store_fetches(&core, &account, &folder, &fetches, None)?;
        core.mail_changed(&account.id, Some(folder.id));
    }
    if !more {
        core.db.with(|c| queries::set_fully_loaded(c, folder.id, true))?;
    }
    Ok(more)
}

/// Re-fetches headers for UIDs, used to undo an optimistic local change after a failed action.
pub async fn refetch_uids(conn: &mut Conn, folder_id: i64, uids: &[u32]) -> Result<()> {
    let core = conn.core.clone();
    let account = conn.account()?;
    let folder = core.db.with(|c| queries::get_folder(c, folder_id))?;
    conn.ensure_selected(&folder.path).await?;
    let fetches = conn.fetch_headers(uid_set(uids), true).await?;
    store_fetches(&core, &account, &folder, &fetches, None)?;
    core.mail_changed(&account.id, Some(folder.id));
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Bodies

pub async fn fetch_body(conn: &mut Conn, message_id: i64) -> Result<()> {
    let core = conn.core.clone();
    let msg = core
        .db
        .with(|c| queries::message_refs(c, &[message_id]))?
        .into_iter()
        .next()
        .ok_or_else(|| AppError::NotFound(format!("message {message_id}")))?;
    let folder = core.db.with(|c| queries::get_folder(c, msg.folder_id))?;
    let session = conn.ensure_selected(&folder.path).await?;
    let fetches = collect(
        session
            .uid_fetch(msg.uid.to_string(), "(UID BODY.PEEK[])")
            .await?,
    )
    .await?;
    let Some(raw) = fetches
        .iter()
        .find(|f| f.uid == Some(msg.uid))
        .and_then(|f| f.body())
        .map(<[u8]>::to_vec)
    else {
        // Deleted on the server in the meantime.
        core.db.with(|c| queries::delete_messages(c, &[message_id]))?;
        core.mail_changed(&msg.account_id, Some(msg.folder_id));
        return Err(AppError::NotFound(format!("message {message_id}")));
    };
    let parsed = tokio::task::spawn_blocking({
        let raw = raw.clone();
        move || parse_body(&raw)
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))?;
    let has_attachments = parsed.attachments.iter().any(|a| !a.inline);
    let preview = parsed.text.as_deref().map(crate::mime::parse::clean_preview);
    core.db.with(|c| {
        queries::save_body(
            c,
            message_id,
            &BodyRow {
                text: parsed.text,
                html: parsed.html,
                has_remote: parsed.has_remote,
                attachments: parsed.attachments,
            },
            &raw,
        )?;
        queries::set_has_attachments(c, message_id, has_attachments)?;
        if let Some(preview) = preview {
            queries::set_preview_if_empty(c, message_id, &preview)?;
        }
        Ok(())
    })?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Actions

pub async fn store_flag(conn: &mut Conn, folder_id: i64, uids: &[u32], flag: &str, add: bool) -> Result<()> {
    let folder = conn.core.db.with(|c| queries::get_folder(c, folder_id))?;
    let session = conn.ensure_selected(&folder.path).await?;
    let op = if add { "+FLAGS.SILENT" } else { "-FLAGS.SILENT" };
    collect(session.uid_store(uid_set(uids), format!("{op} ({flag})")).await?).await?;
    imap_conn::drain_unsolicited(session);
    Ok(())
}

pub async fn move_uids(conn: &mut Conn, folder_id: i64, uids: &[u32], target_path: &str) -> Result<()> {
    let folder = conn.core.db.with(|c| queries::get_folder(c, folder_id))?;
    let caps = {
        conn.session().await?;
        conn.caps()
    };
    let session = conn.ensure_selected(&folder.path).await?;
    let set = uid_set(uids);
    if caps.move_ {
        session.uid_mv(&set, target_path).await?;
    } else {
        session.uid_copy(&set, target_path).await?;
        collect(session.uid_store(&set, "+FLAGS.SILENT (\\Deleted)").await?).await?;
        expunge(session, &set, caps).await?;
    }
    imap_conn::drain_unsolicited(session);
    Ok(())
}

/// Permanently deletes messages (used inside Trash).
pub async fn expunge_uids(conn: &mut Conn, folder_id: i64, uids: &[u32]) -> Result<()> {
    let folder = conn.core.db.with(|c| queries::get_folder(c, folder_id))?;
    let caps = {
        conn.session().await?;
        conn.caps()
    };
    let session = conn.ensure_selected(&folder.path).await?;
    let set = uid_set(uids);
    collect(session.uid_store(&set, "+FLAGS.SILENT (\\Deleted)").await?).await?;
    expunge(session, &set, caps).await?;
    imap_conn::drain_unsolicited(session);
    Ok(())
}

async fn expunge(session: &mut Session, set: &str, caps: Caps) -> Result<()> {
    if caps.uidplus {
        collect(session.uid_expunge(set).await?).await?;
    } else {
        collect(session.expunge().await?).await?;
    }
    Ok(())
}

pub async fn append(conn: &mut Conn, folder_path: &str, raw: &[u8], flags: &str) -> Result<()> {
    let session = conn.session().await?;
    session.append(folder_path, Some(flags), None, raw).await?;
    imap_conn::drain_unsolicited(session);
    Ok(())
}

pub async fn create_folder(conn: &mut Conn, path: &str) -> Result<()> {
    let session = conn.session().await?;
    session.create(path).await?;
    let _ = session.subscribe(path).await;
    Ok(())
}
