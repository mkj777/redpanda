//! Per-account sync workers.
//!
//! Every account runs three tasks, each with its own IMAP connection:
//! - **interactive**: body fetches and user actions, so they never wait behind a long sync
//! - **background**: full sync on start, every 5 minutes and when IDLE reports changes
//! - **idle**: IDLE on INBOX (or polling if the server has no IDLE)

pub mod engine;
pub mod folders;
pub mod imap_conn;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::core::Core;
use crate::error::{AppError, Result};
use crate::model::{SyncState, SyncStatus};
use crate::store::queries;
use engine::Conn;

const POLL_INTERVAL: Duration = Duration::from_secs(5 * 60);
const IDLE_TIMEOUT: Duration = Duration::from_secs(25 * 60);
const NO_IDLE_POLL: Duration = Duration::from_secs(2 * 60);
const MAX_BACKOFF: Duration = Duration::from_secs(5 * 60);

#[derive(Debug)]
pub enum Job {
    FetchBody {
        message_id: i64,
    },
    SetFlag {
        folder_id: i64,
        uids: Vec<u32>,
        flag: &'static str,
        add: bool,
    },
    Move {
        folder_id: i64,
        uids: Vec<u32>,
        target_path: String,
    },
    Expunge {
        folder_id: i64,
        uids: Vec<u32>,
    },
    LoadOlder {
        folder_id: i64,
    },
    SyncFolder {
        folder_id: i64,
    },
    CreateFolder {
        path: String,
    },
    Append {
        folder_path: String,
        raw: Vec<u8>,
        flags: String,
    },
}

struct JobMsg {
    job: Job,
    reply: Option<oneshot::Sender<Result<bool>>>,
}

#[derive(Debug)]
enum BgCmd {
    SyncAll,
    SyncFolder(i64),
}

struct Workers {
    jobs: mpsc::UnboundedSender<JobMsg>,
    bg: mpsc::UnboundedSender<BgCmd>,
    tasks: Vec<JoinHandle<()>>,
}

impl Drop for Workers {
    fn drop(&mut self) {
        for t in &self.tasks {
            t.abort();
        }
    }
}

pub struct SyncManager {
    core: Arc<Core>,
    workers: Mutex<HashMap<String, Workers>>,
    statuses: Arc<Mutex<HashMap<String, SyncStatus>>>,
}

impl SyncManager {
    pub fn new(core: Arc<Core>) -> SyncManager {
        SyncManager {
            core,
            workers: Mutex::new(HashMap::new()),
            statuses: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn start_all(&self) -> Result<()> {
        let accounts = self.core.db.with(|c| queries::list_accounts(c))?;
        for a in accounts {
            self.start(&a.id);
        }
        Ok(())
    }

    /// Starts (or restarts) the workers of an account. Must run inside a Tokio runtime.
    pub fn start(&self, account_id: &str) {
        let (jobs_tx, jobs_rx) = mpsc::unbounded_channel();
        let (bg_tx, bg_rx) = mpsc::unbounded_channel();
        let status = StatusHandle {
            core: self.core.clone(),
            statuses: self.statuses.clone(),
            account_id: account_id.to_string(),
        };
        let tasks = vec![
            tokio::spawn(interactive_worker(
                self.core.clone(),
                account_id.to_string(),
                jobs_rx,
                bg_tx.clone(),
                status.clone(),
            )),
            tokio::spawn(background_worker(
                self.core.clone(),
                account_id.to_string(),
                bg_rx,
                status.clone(),
            )),
            tokio::spawn(idle_worker(
                self.core.clone(),
                account_id.to_string(),
                bg_tx.clone(),
            )),
        ];
        let workers = Workers {
            jobs: jobs_tx,
            bg: bg_tx,
            tasks,
        };
        if let Ok(mut map) = self.workers.lock() {
            map.insert(account_id.to_string(), workers);
        }
    }

    pub fn stop(&self, account_id: &str) {
        if let Ok(mut map) = self.workers.lock() {
            map.remove(account_id);
        }
        if let Ok(mut s) = self.statuses.lock() {
            s.remove(account_id);
        }
    }

    pub fn status(&self, account_id: &str) -> Option<SyncStatus> {
        self.statuses.lock().ok()?.get(account_id).cloned()
    }

    pub fn sync_all(&self, account_id: Option<&str>) {
        if let Ok(map) = self.workers.lock() {
            for (id, w) in map.iter() {
                if account_id.is_none_or(|a| a == id) {
                    let _ = w.bg.send(BgCmd::SyncAll);
                }
            }
        }
    }

    /// Queues a job without waiting for the result. Failures are reported via `action:failed`.
    pub fn enqueue(&self, account_id: &str, job: Job) {
        if let Some(tx) = self.jobs_tx(account_id) {
            let _ = tx.send(JobMsg { job, reply: None });
        }
    }

    /// Runs a job on the interactive connection and waits for it.
    pub async fn run(&self, account_id: &str, job: Job) -> Result<bool> {
        let tx = self
            .jobs_tx(account_id)
            .ok_or_else(|| AppError::NotFound(format!("account {account_id}")))?;
        let (reply, rx) = oneshot::channel();
        tx.send(JobMsg {
            job,
            reply: Some(reply),
        })
        .map_err(|_| AppError::Other("sync worker stopped".into()))?;
        tokio::time::timeout(Duration::from_secs(120), rx)
            .await?
            .map_err(|_| AppError::Other("sync worker stopped".into()))?
    }

    /// A handle for queueing jobs from a spawned task.
    pub fn sender(&self, account_id: &str) -> Option<impl Fn(Job) + Send + 'static> {
        let tx = self.jobs_tx(account_id)?;
        Some(move |job| {
            let _ = tx.send(JobMsg { job, reply: None });
        })
    }

    fn jobs_tx(&self, account_id: &str) -> Option<mpsc::UnboundedSender<JobMsg>> {
        self.workers.lock().ok()?.get(account_id).map(|w| w.jobs.clone())
    }
}

#[derive(Clone)]
struct StatusHandle {
    core: Arc<Core>,
    statuses: Arc<Mutex<HashMap<String, SyncStatus>>>,
    account_id: String,
}

impl StatusHandle {
    fn set(&self, state: SyncState, error: Option<String>) {
        let status = SyncStatus {
            account_id: self.account_id.clone(),
            state,
            error,
        };
        if let Ok(mut map) = self.statuses.lock() {
            map.insert(self.account_id.clone(), status.clone());
        }
        self.core.sync_status(&status);
    }

    fn set_error(&self, e: &AppError) {
        match e {
            AppError::Auth(_) | AppError::AuthRequired | AppError::OAuthNotConfigured(_) => {
                self.set(SyncState::AuthRequired, Some(e.to_string()))
            }
            AppError::Network(_) => self.set(SyncState::Offline, Some(e.to_string())),
            _ => self.set(SyncState::Error, Some(e.to_string())),
        }
    }
}

async fn run_job(conn: &mut Conn, job: &Job) -> Result<bool> {
    match job {
        Job::FetchBody { message_id } => engine::fetch_body(conn, *message_id).await.map(|_| true),
        Job::SetFlag {
            folder_id,
            uids,
            flag,
            add,
        } => engine::store_flag(conn, *folder_id, uids, flag, *add)
            .await
            .map(|_| true),
        Job::Move {
            folder_id,
            uids,
            target_path,
        } => engine::move_uids(conn, *folder_id, uids, target_path)
            .await
            .map(|_| true),
        Job::Expunge { folder_id, uids } => engine::expunge_uids(conn, *folder_id, uids).await.map(|_| true),
        Job::LoadOlder { folder_id } => engine::load_older(conn, *folder_id).await,
        Job::SyncFolder { folder_id } => engine::sync_folder_by_id(conn, *folder_id).await,
        Job::CreateFolder { path } => engine::create_folder(conn, path).await.map(|_| true),
        Job::Append {
            folder_path,
            raw,
            flags,
        } => engine::append(conn, folder_path, raw, flags).await.map(|_| true),
    }
}

async fn interactive_worker(
    core: Arc<Core>,
    account_id: String,
    mut rx: mpsc::UnboundedReceiver<JobMsg>,
    bg: mpsc::UnboundedSender<BgCmd>,
    status: StatusHandle,
) {
    let mut conn = Conn::new(core.clone(), &account_id);
    while let Some(JobMsg { job, reply }) = rx.recv().await {
        let mut res = run_job(&mut conn, &job).await;
        if res.as_ref().is_err_and(AppError::is_connection_error) {
            // Stale connection (server timeout): reconnect once.
            conn.reset();
            res = run_job(&mut conn, &job).await;
        }
        if let Err(e) = &res {
            tracing::warn!(account = %account_id, ?job, "job failed: {e}");
            if e.is_auth_error() {
                status.set_error(e);
            }
            if reply.is_none() {
                core.action_failed(&account_id, e);
                // Undo the optimistic local change.
                match &job {
                    Job::Move { folder_id, uids, .. } | Job::Expunge { folder_id, uids } => {
                        let _ = engine::refetch_uids(&mut conn, *folder_id, uids).await;
                    }
                    Job::SetFlag { folder_id, .. } => {
                        let _ = bg.send(BgCmd::SyncFolder(*folder_id));
                    }
                    _ => {}
                }
            }
        }
        if let Some(reply) = reply {
            let _ = reply.send(res);
        }
    }
    conn.logout().await;
}

async fn background_worker(
    core: Arc<Core>,
    account_id: String,
    mut rx: mpsc::UnboundedReceiver<BgCmd>,
    status: StatusHandle,
) {
    let mut conn = Conn::new(core.clone(), &account_id);
    let mut backoff = Duration::from_secs(5);
    let mut next = Some(BgCmd::SyncAll);
    loop {
        let cmd = match next.take() {
            Some(c) => c,
            None => match tokio::time::timeout(POLL_INTERVAL, rx.recv()).await {
                Ok(Some(c)) => c,
                Ok(None) => break,
                Err(_) => BgCmd::SyncAll,
            },
        };
        // Collapse a burst of queued commands into one run.
        let mut full = matches!(cmd, BgCmd::SyncAll);
        let mut folders = match cmd {
            BgCmd::SyncFolder(id) => vec![id],
            BgCmd::SyncAll => vec![],
        };
        while let Ok(c) = rx.try_recv() {
            match c {
                BgCmd::SyncAll => full = true,
                BgCmd::SyncFolder(id) if !folders.contains(&id) => folders.push(id),
                BgCmd::SyncFolder(_) => {}
            }
        }

        status.set(SyncState::Syncing, None);
        let mut res = run_bg(&mut conn, full, &folders).await;
        if res.as_ref().is_err_and(AppError::is_connection_error) {
            conn.reset();
            res = run_bg(&mut conn, full, &folders).await;
        }
        match res {
            Ok(()) => {
                status.set(SyncState::Idle, None);
                backoff = Duration::from_secs(5);
            }
            Err(e) => {
                tracing::warn!(account = %account_id, "sync failed: {e}");
                status.set_error(&e);
                conn.reset();
                if e.is_auth_error() {
                    // Wait for an explicit trigger (re-login restarts the workers anyway).
                    match rx.recv().await {
                        Some(c) => next = Some(c),
                        None => break,
                    }
                } else {
                    tokio::select! {
                        _ = tokio::time::sleep(backoff) => next = Some(BgCmd::SyncAll),
                        c = rx.recv() => match c {
                            Some(_) => next = Some(BgCmd::SyncAll),
                            None => break,
                        },
                    }
                    backoff = (backoff * 2).min(MAX_BACKOFF);
                }
            }
        }
    }
    conn.logout().await;
}

async fn run_bg(conn: &mut Conn, full: bool, folders: &[i64]) -> Result<()> {
    if full {
        return engine::sync_account(conn).await;
    }
    for id in folders {
        match engine::sync_folder_by_id(conn, *id).await {
            Ok(_) | Err(AppError::NotFound(_)) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

async fn idle_worker(core: Arc<Core>, account_id: String, bg: mpsc::UnboundedSender<BgCmd>) {
    let mut backoff = Duration::from_secs(10);
    loop {
        match idle_session(&core, &account_id, &bg).await {
            Ok(()) => return,
            Err(e) => {
                tracing::debug!(account = %account_id, "idle connection ended: {e}");
                let wait = if e.is_auth_error() { MAX_BACKOFF } else { backoff };
                tokio::time::sleep(wait).await;
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        }
        if bg.is_closed() {
            return;
        }
    }
}

async fn idle_session(core: &Arc<Core>, account_id: &str, bg: &mpsc::UnboundedSender<BgCmd>) -> Result<()> {
    // Wait for the first full sync to create the inbox row.
    let inbox = loop {
        if let Some(f) = core
            .db
            .with(|c| queries::folder_by_role(c, account_id, "inbox"))?
        {
            break f;
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
        if bg.is_closed() {
            return Ok(());
        }
    };
    let account = core.db.with(|c| queries::get_account(c, account_id))?;
    let (mut session, caps) = imap_conn::open_session(core, &account).await?;
    if !caps.idle {
        let _ = session.logout().await;
        loop {
            tokio::time::sleep(NO_IDLE_POLL).await;
            if bg.send(BgCmd::SyncFolder(inbox.id)).is_err() {
                return Ok(());
            }
        }
    }
    imap_conn::select(&mut session, &inbox.path, false).await?;
    loop {
        let mut idle = session.idle();
        idle.init().await?;
        let (wait, stop) = idle.wait_with_timeout(IDLE_TIMEOUT);
        let response = wait.await;
        // Dropping the stop source early would interrupt the wait, so keep it alive until here.
        drop(stop);
        let response = response?;
        session = idle.done().await?;
        imap_conn::drain_unsolicited(&mut session);
        if let async_imap::extensions::idle::IdleResponse::NewData(_) = response {
            // Let a burst of changes settle before syncing.
            tokio::time::sleep(Duration::from_millis(500)).await;
            if bg.send(BgCmd::SyncFolder(inbox.id)).is_err() {
                return Ok(());
            }
        } else if session.noop().await.is_err() {
            return Err(AppError::Network("idle keepalive failed".into()));
        }
    }
}
