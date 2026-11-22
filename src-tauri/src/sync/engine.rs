//! IMAP synchronisation of one account: folder sync, lazy bodies and server-side actions.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
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
/// Unread messages older than the loaded range, stored per folder and sync at most.
const MAX_OLD_UNREAD: usize = 500;
/// Messages missing inside the loaded range, fetched again per folder and sync at most.
const MAX_LOST: usize = 500;
/// Messages fetched per "load older" request.
pub const OLDER_BATCH: usize = 200;
/// Messages fetched per step when the background loads the rest of a folder.
const BACKFILL_BATCH: usize = 500;
/// Messages per request when reading the unsubscribe headers of already synced mail.
const UNSUBSCRIBE_BATCH: usize = 500;

const HEADER_QUERY: &str = "(UID FLAGS INTERNALDATE RFC822.SIZE BODY.PEEK[HEADER] BODY.PEEK[TEXT]<0.3072>)";
/// Fallback for servers whose partial FETCH responses do not parse (GreenMail, some appliances).
/// The preview is then filled in when the body is fetched.
const HEADER_QUERY_NO_PREVIEW: &str = "(UID FLAGS INTERNALDATE RFC822.SIZE BODY.PEEK[HEADER])";

/// The user's actions on the interactive connection of one account. A sync uses it to tell
/// whether a message missing here was dropped by mistake or by an action still on its way.
#[derive(Default)]
pub struct Actions {
    pending: AtomicUsize,
    epoch: AtomicU64,
}

impl Actions {
    pub fn queued(&self) {
        self.pending.fetch_add(1, Ordering::SeqCst);
        self.epoch.fetch_add(1, Ordering::SeqCst);
    }

    pub fn done(&self) {
        self.pending.fetch_sub(1, Ordering::SeqCst);
        self.epoch.fetch_add(1, Ordering::SeqCst);
    }

    /// Taken before reading the server. `None` while an action is pending.
    pub fn mark(&self) -> Option<u64> {
        (self.pending.load(Ordering::SeqCst) == 0).then(|| self.epoch.load(Ordering::SeqCst))
    }

    /// Whether no action was queued or finished since `mark`.
    pub fn unchanged_since(&self, mark: Option<u64>) -> bool {
        mark.is_some_and(|m| {
            self.pending.load(Ordering::SeqCst) == 0 && self.epoch.load(Ordering::SeqCst) == m
        })
    }
}

/// A lazily (re)connected IMAP session for one account.
pub struct Conn {
    pub core: Arc<Core>,
    pub account_id: String,
    pub actions: Arc<Actions>,
    session: Option<Session>,
    caps: Caps,
    selected: Option<String>,
    partial_fetch: bool,
}

impl Conn {
    pub fn new(core: Arc<Core>, account_id: &str, actions: Arc<Actions>) -> Conn {
        Conn {
            core,
            account_id: account_id.to_string(),
            actions,
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

    pub(super) async fn ensure_selected(&mut self, path: &str) -> Result<&mut Session> {
        match self.session.as_mut() {
            Some(session) if self.selected.as_deref() == Some(path) => {
                // A session only learns about messages that arrived after it selected the
                // folder when the server tells it. Until then Gmail answers a FETCH or STORE for
                // a new UID with nothing, so the message looks deleted and a flag change is lost.
                session.noop().await?;
                imap_conn::drain_unsolicited(session);
            }
            _ => {
                self.select(path).await?;
            }
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
    folders.sort_by_key(|f| sync_rank(&f.role));
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

/// The order folders are synced and filled in, inbox first.
pub(super) fn sync_rank(role: &str) -> u8 {
    match role {
        "inbox" => 0,
        "sent" => 1,
        "drafts" => 2,
        "archive" => 3,
        _ => 4,
    }
}

pub async fn sync_folder_by_id(conn: &mut Conn, folder_id: i64) -> Result<bool> {
    let folder = conn.core.db.with(|c| queries::get_folder(c, folder_id))?;
    sync_folder(conn, &folder).await
}

/// Brings one folder up to date. Returns `true` if anything changed.
pub async fn sync_folder(conn: &mut Conn, folder: &FolderRow) -> Result<bool> {
    let core = conn.core.clone();
    let account = conn.account()?;
    let actions = conn.actions.mark();
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
        core.db.with(|c| queries::set_window_start(c, folder.id, None))?;
        changed |= n > 0;
    } else if let Some(max) = known_max {
        // New messages.
        if mb.uid_next.is_none_or(|next| next > max + 1) {
            let fetches = conn.fetch_headers(format!("{}:*", max + 1), true).await?;
            changed |= store_fetches(&core, &account, folder, &fetches, Some(max + 1))? > 0;
        }
        let session = conn.session.as_mut().expect("selected");

        // Flag changes and deletions within the synced window.
        let oldest = core
            .db
            .with(|c| queries::window_start(c, folder.id))?
            .unwrap_or(1);
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
        let local: HashSet<u32> = local.into_iter().collect();
        let gone: Vec<u32> = local.iter().copied().filter(|u| !present.contains(u)).collect();
        if !gone.is_empty() {
            core.db.with(|c| queries::delete_by_uids(c, folder.id, &gone))?;
            changed = true;
        }
        // Messages inside the window that are missing here, dropped by mistake before. A
        // message the user just moved or deleted is missing too until the action reaches the
        // server, so this only runs when no action was queued while the folder was read.
        let lost: Vec<u32> = present.iter().copied().filter(|u| !local.contains(u)).collect();
        if !lost.is_empty() && conn.actions.unchanged_since(actions) {
            changed |= fetch_newest(conn, folder, lost, MAX_LOST).await?;
        }
    } else {
        // First sync: the newest messages by sequence number.
        let start = mb.exists.saturating_sub(INITIAL_WINDOW - 1).max(1);
        let fetches = conn.fetch_headers(format!("{start}:*"), false).await?;
        changed |= store_fetches(&core, &account, folder, &fetches, None)? > 0;
        let first = fetches.iter().filter_map(|f| f.uid).min();
        core.db.with(|c| queries::set_window_start(c, folder.id, first))?;
        if start == 1 {
            core.db.with(|c| queries::set_fully_loaded(c, folder.id, true))?;
        }
    }

    let session = conn.session.as_mut().expect("selected");
    let unseen = if mb.exists == 0 {
        HashSet::new()
    } else {
        session.uid_search("UNSEEN").await?
    };
    let unread = unseen.len() as i64;
    if mb.exists > 0 {
        changed |= sync_old_unread(conn, folder, &unseen).await?;
    }
    let session = conn.session.as_mut().expect("selected");
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

/// Keeps the unread messages older than the loaded range in step with the server, so the
/// unread views show what the folder's unread count says. Runs with the folder selected.
async fn sync_old_unread(conn: &mut Conn, folder: &FolderRow, unseen: &HashSet<u32>) -> Result<bool> {
    let core = conn.core.clone();
    let Some(window) = core.db.with(|c| queries::window_start(c, folder.id))? else {
        return Ok(false);
    };
    // Pin the range before anything below it is stored.
    core.db
        .with(|c| queries::set_window_start(c, folder.id, Some(window)))?;
    let stored: HashSet<u32> = core
        .db
        .with(|c| queries::uids_below(c, folder.id, window))?
        .into_iter()
        .collect();
    let mut changed = false;

    let missing: Vec<u32> = unseen
        .iter()
        .copied()
        .filter(|u| *u < window && !stored.contains(u))
        .collect();
    changed |= fetch_newest(conn, folder, missing, MAX_OLD_UNREAD).await?;

    // Stored ones that are no longer unread were read or deleted elsewhere (or read here).
    // Without a modification sequence to go by, all of their flags are read again.
    let full = folder.highestmodseq.is_none();
    let check: Vec<u32> = stored
        .into_iter()
        .filter(|u| full || !unseen.contains(u))
        .collect();
    if !check.is_empty() {
        let session = conn.session.as_mut().expect("selected");
        let fetches = collect(session.uid_fetch(uid_set(&check), "(UID FLAGS)").await?).await?;
        changed |= apply_flags(&core, folder, &fetches)?;
        let present: HashSet<u32> = fetches.iter().filter_map(|f| f.uid).collect();
        let gone: Vec<u32> = check.into_iter().filter(|u| !present.contains(u)).collect();
        if !gone.is_empty() {
            core.db.with(|c| queries::delete_by_uids(c, folder.id, &gone))?;
            changed = true;
        }
    }
    Ok(changed)
}

/// Fetches and stores the headers of the newest `cap` of `uids`. Returns `true` if any was new.
async fn fetch_newest(conn: &mut Conn, folder: &FolderRow, mut uids: Vec<u32>, cap: usize) -> Result<bool> {
    if uids.is_empty() {
        return Ok(false);
    }
    uids.sort_unstable_by(|a, b| b.cmp(a));
    uids.truncate(cap);
    let core = conn.core.clone();
    let account = conn.account()?;
    let fetches = conn.fetch_headers(uid_set(&uids), true).await?;
    if fetches.iter().filter(|f| f.uid.is_some()).count() < uids.len() {
        tracing::warn!(folder = %folder.path, asked = uids.len(), got = fetches.len(), "server left out messages");
    }
    Ok(store_fetches(&core, &account, folder, &fetches, None)? > 0)
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

/// Fetches the next `batch` older messages. Returns `true` if more remain on the server.
pub async fn load_older(conn: &mut Conn, folder_id: i64, batch: usize) -> Result<bool> {
    let core = conn.core.clone();
    let account = conn.account()?;
    let folder = core.db.with(|c| queries::get_folder(c, folder_id))?;
    if folder.fully_loaded {
        return Ok(false);
    }
    let oldest = core.db.with(|c| queries::window_start(c, folder.id))?;
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
    uids.truncate(batch);
    let more = uids.len() == batch;
    if let Some(&lowest) = uids.last() {
        let fetches = conn.fetch_headers(uid_set(&uids), true).await?;
        store_fetches(&core, &account, &folder, &fetches, None)?;
        // The batch is contiguous with the range above it, so the range now starts here. The
        // background and a scroll in the list can load the same folder at once, so the range
        // only ever grows.
        core.db
            .with(|c| queries::lower_window_start(c, folder.id, lowest))?;
        core.mail_changed(&account.id, Some(folder.id));
    }
    if !more {
        core.db.with(|c| queries::set_fully_loaded(c, folder.id, true))?;
    }
    Ok(more)
}

/// Loads the next batch of older headers into the first visible folder that still has some on
/// the server, so lists and search cover the whole mailbox and not just its newest messages.
/// Message numbers do not follow dates everywhere: in some mailboxes mail from last month sits
/// far below the newest numbers. Folders in `skip` are left out. Returns `false` once every
/// folder is complete.
pub async fn backfill_step(conn: &mut Conn, skip: &mut HashSet<i64>) -> Result<bool> {
    let account_id = conn.account_id.clone();
    let mut folders = conn.core.db.with(|c| queries::folder_rows(c, &account_id))?;
    folders.retain(|f| !f.hidden && !f.fully_loaded && !skip.contains(&f.id));
    folders.sort_by_key(|f| sync_rank(&f.role));
    let Some(folder) = folders.first() else {
        return Ok(false);
    };
    match load_older(conn, folder.id, BACKFILL_BATCH).await {
        Ok(_) => {}
        // A folder that cannot be read must not stop the others.
        Err(AppError::Imap(msg)) => {
            tracing::warn!(folder = %folder.path, "loading older mail failed: {msg}");
            skip.insert(folder.id);
            conn.reset();
        }
        Err(e) => return Err(e),
    }
    Ok(true)
}

/// One step of filling in the mailbox in the background: the headers of older mail first, then
/// the text of every message for search. Returns `false` once everything is in.
pub async fn fill_in_step(conn: &mut Conn, skip: &mut HashSet<i64>) -> Result<bool> {
    if backfill_step(conn, skip).await? {
        return Ok(true);
    }
    super::text::text_step(conn, skip).await
}

/// Messages found by a search on the server that are fetched per folder when not stored yet.
const MAX_SEARCH_FETCH: usize = 200;

/// The SEARCH criteria for what someone typed. Gmail gets its own search syntax (X-GM-RAW), so
/// the hits match the web; other servers look for every word in the text. Words with letters
/// beyond ASCII go as a literal where the server takes one without a round trip.
pub fn search_criteria(query: &str, caps: Caps) -> Option<String> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }
    let ascii = query.is_ascii();
    let string = |s: &str| {
        if !ascii && caps.literal_plus && s.len() <= 4096 {
            format!("{{{}+}}\r\n{s}", s.len())
        } else {
            format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
        }
    };
    let criteria = if caps.gmail {
        format!("X-GM-RAW {}", string(query))
    } else {
        query
            .split_whitespace()
            .map(|w| format!("TEXT {}", string(w)))
            .collect::<Vec<_>>()
            .join(" ")
    };
    Some(if ascii {
        criteria
    } else {
        format!("CHARSET UTF-8 {criteria}")
    })
}

/// Searches the visible folders of the account on the server and returns the local ids of what
/// it finds. Hits that are not stored yet (older mail still loading) are fetched first. A folder
/// the server cannot search is left out.
pub async fn search_server(conn: &mut Conn, query: &str) -> Result<Vec<i64>> {
    let core = conn.core.clone();
    let account = conn.account()?;
    conn.session().await?;
    let Some(criteria) = search_criteria(query, conn.caps()) else {
        return Ok(Vec::new());
    };
    let mut folders = core.db.with(|c| queries::folder_rows(c, &account.id))?;
    folders.retain(|f| !f.hidden);
    folders.sort_by_key(|f| sync_rank(&f.role));
    let mut ids = Vec::new();
    for folder in folders {
        let session = conn.ensure_selected(&folder.path).await?;
        let found: Vec<u32> = match session.uid_search(&criteria).await {
            Ok(found) => found.into_iter().collect(),
            Err(e) => match AppError::from(e) {
                e @ AppError::Network(_) => return Err(e),
                // The query itself is left out of the log.
                e => {
                    tracing::info!(folder = %folder.path, "search on the server failed: {e}");
                    conn.reset();
                    continue;
                }
            },
        };
        if found.is_empty() {
            continue;
        }
        let known: HashSet<u32> = core
            .db
            .with(|c| queries::uids_for(c, folder.id, &found))?
            .into_iter()
            .collect();
        let mut missing: Vec<u32> = found.iter().copied().filter(|u| !known.contains(u)).collect();
        if !missing.is_empty() {
            missing.sort_unstable_by(|a, b| b.cmp(a));
            missing.truncate(MAX_SEARCH_FETCH);
            let fetches = conn.fetch_headers(uid_set(&missing), true).await?;
            if store_fetches(&core, &account, &folder, &fetches, None)? > 0 {
                core.mail_changed(&account.id, Some(folder.id));
            }
        }
        ids.extend(core.db.with(|c| queries::ids_for_uids(c, folder.id, &found))?);
    }
    Ok(ids)
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
        // Only drop the row when the server confirms the message is gone.
        let session = conn.session.as_mut().expect("selected");
        if session
            .uid_search(format!("UID {}", msg.uid))
            .await?
            .contains(&msg.uid)
        {
            return Err(AppError::Other(format!("no body for message {message_id}")));
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_criteria_per_server() {
        let plain = Caps::default();
        let gmail = Caps {
            gmail: true,
            literal_plus: true,
            ..Caps::default()
        };
        assert_eq!(search_criteria("  ", plain), None);
        assert_eq!(
            search_criteria("macbook air", plain).unwrap(),
            r#"TEXT "macbook" TEXT "air""#
        );
        // Gmail gets the whole query in its own syntax.
        assert_eq!(
            search_criteria(r#"from:apple "air""#, gmail).unwrap(),
            r#"X-GM-RAW "from:apple \"air\"""#
        );
        // Beyond ASCII: a literal where the server takes one without a round trip.
        assert_eq!(
            search_criteria("Rückgabe", gmail).unwrap(),
            "CHARSET UTF-8 X-GM-RAW {9+}\r\nRückgabe"
        );
        assert_eq!(
            search_criteria("Rückgabe", plain).unwrap(),
            r#"CHARSET UTF-8 TEXT "Rückgabe""#
        );
    }

    #[test]
    fn actions_block_repairs_while_pending_or_after_a_change() {
        let a = Actions::default();
        let mark = a.mark();
        assert!(a.unchanged_since(mark));

        // Queued while the folder was read.
        a.queued();
        assert!(!a.unchanged_since(mark));
        // Pending when the sync starts.
        assert!(a.mark().is_none());
        assert!(!a.unchanged_since(a.mark()));
        // Finished while the folder was read.
        a.done();
        assert!(!a.unchanged_since(mark));
        assert!(a.unchanged_since(a.mark()));
    }
}
