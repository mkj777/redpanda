//! Application logic behind the Tauri commands. Kept free of Tauri types so it can be driven
//! from integration tests.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use crate::accounts::discovery::{self, DiscoveryResult};
use crate::accounts::oauth::{self, OAuthConfig, OAuthProvider};
use crate::accounts::secrets::StoredSecret;
use crate::accounts::{providers, LoginSecret, ACCOUNT_COLORS};
use crate::core::Core;
use crate::error::{AppError, Result};
use crate::mime;
use crate::model::*;
use crate::send::{compose, smtp};
use crate::store::queries::{self, MessageRef};
use crate::sync::{imap_conn, Job, SyncManager};
use crate::unsubscribe::{self, Method as UnsubscribeMethod};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    #[serde(flatten)]
    pub account: Account,
    /// Further addresses the account can send from.
    pub aliases: Vec<Address>,
    pub status: SyncState,
    pub status_error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NewAuth {
    Password { password: String },
    Oauth { provider: OAuthProvider },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewAccountInput {
    pub email: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub provider_id: Option<String>,
    pub username: String,
    pub imap: ServerConfig,
    pub smtp: ServerConfig,
    pub auth: NewAuth,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderList {
    pub folders: Vec<Folder>,
    pub unified: UnifiedCounts,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthStatus {
    pub google: bool,
    pub microsoft: bool,
    pub config_path: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlagKind {
    Seen,
    Flagged,
}

/// File types that are never opened directly, only saved.
pub(crate) const BLOCKED_EXTENSIONS: &[&str] = &[
    "exe",
    "com",
    "bat",
    "cmd",
    "scr",
    "pif",
    "msi",
    "msp",
    "ps1",
    "psm1",
    "vbs",
    "vbe",
    "js",
    "jse",
    "wsf",
    "wsh",
    "hta",
    "lnk",
    "jar",
    "cpl",
    "reg",
    "scf",
    "url",
    "application",
    "appref-ms",
];

pub struct App {
    pub core: Arc<Core>,
    pub sync: SyncManager,
    oauth_cancel: Mutex<Option<oneshot::Sender<()>>>,
}

impl App {
    pub fn new(core: Arc<Core>) -> App {
        App {
            sync: SyncManager::new(core.clone()),
            core,
            oauth_cancel: Mutex::new(None),
        }
    }

    // -----------------------------------------------------------------------------------------
    // Accounts

    pub fn list_accounts(&self) -> Result<Vec<AccountView>> {
        self.core.db.with(|c| {
            let mut out = Vec::new();
            for account in queries::list_accounts(c)? {
                let status = self.sync.status(&account.id);
                out.push(AccountView {
                    aliases: queries::aliases(c, &account.id)?,
                    status: status.as_ref().map_or(SyncState::Syncing, |s| s.state),
                    status_error: status.and_then(|s| s.error),
                    account,
                });
            }
            Ok(out)
        })
    }

    pub async fn discover(&self, email: &str) -> DiscoveryResult {
        discovery::discover(&self.core.http, email).await
    }

    pub fn oauth_status(&self) -> OAuthStatus {
        // Re-read the file so users can add IDs without restarting.
        let cfg = OAuthConfig::load();
        let status = OAuthStatus {
            google: cfg.google.is_some(),
            microsoft: cfg.microsoft.is_some(),
            config_path: OAuthConfig::config_path().map(|p| p.display().to_string()),
        };
        if let Ok(mut c) = self.core.oauth.write() {
            *c = cfg;
        }
        status
    }

    pub fn cancel_oauth(&self) {
        if let Some(tx) = self.oauth_cancel.lock().ok().and_then(|mut c| c.take()) {
            let _ = tx.send(());
        }
    }

    async fn run_oauth(
        &self,
        provider: OAuthProvider,
        login_hint: Option<&str>,
        open: impl FnOnce(&str) -> Result<()>,
    ) -> Result<oauth::TokenSet> {
        let (tx, rx) = oneshot::channel();
        if let Ok(mut c) = self.oauth_cancel.lock() {
            if let Some(old) = c.replace(tx) {
                let _ = old.send(());
            }
        }
        let config = self.core.oauth_config();
        oauth::authorize(&self.core.http, &config, provider, login_hint, open, rx).await
    }

    pub async fn add_account(
        &self,
        input: NewAccountInput,
        open: impl FnOnce(&str) -> Result<()>,
    ) -> Result<AccountView> {
        let mut email = input.email.trim().to_lowercase();
        if discovery::split_email(&email).is_none() {
            return Err(AppError::Invalid("invalid email address".into()));
        }
        let provider = input
            .provider_id
            .as_deref()
            .and_then(providers::by_id)
            .or_else(|| providers::by_imap_host(&input.imap.host));
        let mut username = input.username.trim().to_string();

        let (auth_type, secret, stored) = match &input.auth {
            NewAuth::Password { password } => {
                let auth_type = match provider.map(|p| p.auth) {
                    Some(AuthType::AppPassword) => AuthType::AppPassword,
                    _ => AuthType::Password,
                };
                (
                    auth_type,
                    LoginSecret::Password(password.clone()),
                    StoredSecret::Password {
                        password: password.clone(),
                    },
                )
            }
            NewAuth::Oauth { provider: p } => {
                let tokens = self.run_oauth(*p, Some(&email), open).await?;
                // The account the user picked in the browser wins over the typed address.
                if let Some(signed_in) = tokens.email.clone() {
                    if signed_in != email {
                        email = signed_in.clone();
                        username = signed_in;
                    }
                }
                let auth_type = match p {
                    OAuthProvider::Google => AuthType::OauthGoogle,
                    OAuthProvider::Microsoft => AuthType::OauthMicrosoft,
                };
                (
                    auth_type,
                    LoginSecret::XOAuth2(tokens.access_token.clone()),
                    StoredSecret::Oauth {
                        access_token: tokens.access_token,
                        refresh_token: tokens.refresh_token.unwrap_or_default(),
                        expires_at: tokens.expires_at,
                    },
                )
            }
        };
        if username.is_empty() {
            username = email.clone();
        }
        if self
            .core
            .db
            .with(|c| queries::account_exists_for_email(c, &email))?
        {
            return Err(AppError::Invalid(format!("{email} is already set up")));
        }

        imap_conn::verify_login(&input.imap, &username, &secret).await?;
        smtp::verify_login(&input.smtp, &username, &secret).await?;

        let count = self.core.db.with(|c| queries::list_accounts(c))?.len();
        let display_name = if input.display_name.trim().is_empty() {
            email.split('@').next().unwrap_or_default().to_string()
        } else {
            input.display_name.trim().to_string()
        };
        let account = Account {
            id: uuid::Uuid::new_v4().to_string(),
            email,
            display_name,
            provider: provider.map_or("custom", |p| p.id).to_string(),
            auth_type,
            username,
            imap: input.imap,
            smtp: input.smtp,
            color: ACCOUNT_COLORS[count % ACCOUNT_COLORS.len()].to_string(),
            sent_saved_by_server: provider.is_some_and(|p| p.sent_saved_by_server),
            is_gmail: provider.is_some_and(|p| p.is_gmail),
        };
        self.core.db.with(|c| {
            let tx = c.transaction()?;
            queries::insert_account(&tx, &account)?;
            self.core.secrets.store(&tx, &account.id, &stored)?;
            tx.commit()?;
            Ok(())
        })?;
        self.sync.start(&account.id);
        Ok(AccountView {
            account,
            aliases: vec![],
            status: SyncState::Syncing,
            status_error: None,
        })
    }

    pub async fn reauth_account(
        &self,
        account_id: &str,
        password: Option<String>,
        open: impl FnOnce(&str) -> Result<()>,
    ) -> Result<()> {
        let account = self.core.db.with(|c| queries::get_account(c, account_id))?;
        let (secret, stored) = match OAuthProvider::from_auth_type(account.auth_type) {
            Some(p) => {
                let tokens = self.run_oauth(p, Some(&account.email), open).await?;
                if tokens.email.as_deref().is_some_and(|e| e != account.email) {
                    return Err(AppError::Invalid(format!(
                        "signed in as {}, expected {}",
                        tokens.email.unwrap_or_default(),
                        account.email
                    )));
                }
                (
                    LoginSecret::XOAuth2(tokens.access_token.clone()),
                    StoredSecret::Oauth {
                        access_token: tokens.access_token,
                        refresh_token: tokens.refresh_token.unwrap_or_default(),
                        expires_at: tokens.expires_at,
                    },
                )
            }
            None => {
                let password = password.ok_or_else(|| AppError::Invalid("password required".into()))?;
                (
                    LoginSecret::Password(password.clone()),
                    StoredSecret::Password { password },
                )
            }
        };
        imap_conn::verify_login(&account.imap, &account.username, &secret).await?;
        self.core
            .db
            .with(|c| self.core.secrets.store(c, &account.id, &stored))?;
        self.sync.stop(&account.id);
        self.sync.start(&account.id);
        Ok(())
    }

    pub fn update_account(&self, account_id: &str, display_name: &str, color: &str) -> Result<()> {
        self.core
            .db
            .with(|c| queries::update_account_profile(c, account_id, display_name.trim(), color))
    }

    pub fn remove_account(&self, account_id: &str) -> Result<()> {
        self.sync.stop(account_id);
        self.core.db.with(|c| queries::delete_account(c, account_id))?;
        self.core.mail_changed(account_id, None);
        Ok(())
    }

    // -----------------------------------------------------------------------------------------
    // Folders and messages

    pub fn list_folders(&self) -> Result<FolderList> {
        self.core.db.with(|c| {
            Ok(FolderList {
                folders: queries::list_folders(c)?,
                unified: queries::unified_counts(c)?,
            })
        })
    }

    pub fn list_messages(&self, view: &View, offset: u32, limit: u32, threads: bool) -> Result<MessagePage> {
        self.core
            .db
            .with(|c| queries::list_messages(c, view, offset, limit.clamp(1, 500), threads))
    }

    pub fn open_folder(&self, folder_id: i64) -> Result<()> {
        let folder = self.core.db.with(|c| queries::get_folder(c, folder_id))?;
        self.sync
            .enqueue(&folder.account_id, Job::SyncFolder { folder_id });
        Ok(())
    }

    pub fn sync_now(&self, account_id: Option<&str>) {
        self.sync.sync_all(account_id);
    }

    pub async fn load_older(&self, folder_id: i64) -> Result<bool> {
        let folder = self.core.db.with(|c| queries::get_folder(c, folder_id))?;
        self.sync
            .run(&folder.account_id, Job::LoadOlder { folder_id })
            .await
    }

    pub async fn get_message(&self, id: i64) -> Result<MessageDetail> {
        let summary = self.core.db.with(|c| queries::get_summary(c, id))?;
        if self.core.db.with(|c| queries::get_body(c, id))?.is_none() {
            self.sync
                .run(&summary.account_id, Job::FetchBody { message_id: id })
                .await?;
        }
        let summary = self.core.db.with(|c| queries::get_summary(c, id))?;
        let body = self
            .core
            .db
            .with(|c| queries::get_body(c, id))?
            .ok_or_else(|| AppError::NotFound(format!("message {id}")))?;
        let extras = self.core.db.with(|c| queries::header_extras(c, id))?;
        let remote_allowed = match &summary.from {
            Some(a) => self.core.db.with(|c| queries::is_remote_allowed(c, &a.email))?,
            None => false,
        };
        let unsubscribe = self
            .core
            .db
            .with(|c| queries::unsubscribe_row(c, id))?
            .filter(|row| row.folder_role != "junk")
            .and_then(|row| row.unsubscribe.info(row.unsubscribed_at));
        Ok(MessageDetail {
            summary,
            cc: extras.cc,
            reply_to: extras.reply_to,
            message_id: extras.message_id,
            references: extras.references,
            text: body.text,
            html: body.html,
            has_remote_content: body.has_remote,
            remote_allowed,
            attachments: body.attachments,
            unsubscribe,
        })
    }

    /// All messages of the conversation `id` belongs to, oldest first, one per Message-ID.
    pub fn get_thread(&self, id: i64) -> Result<Vec<MessageSummary>> {
        self.core.db.with(|c| {
            let m = queries::get_summary(c, id)?;
            let ids = queries::thread_messages(c, &m.account_id, &m.thread_key)?;
            let mut seen = std::collections::HashSet::new();
            let mut out = Vec::new();
            for tid in ids {
                let s = queries::get_summary(c, tid)?;
                let extras = queries::header_extras(c, tid)?;
                let key = extras.message_id.unwrap_or_else(|| format!("local:{tid}"));
                if seen.insert(key) {
                    out.push(s);
                }
            }
            Ok(out)
        })
    }

    /// With `threads`, an id stands for every message of its conversation in the same folder.
    fn refs(&self, ids: &[i64], threads: bool) -> Result<Vec<MessageRef>> {
        self.core.db.with(|c| {
            let mut all = ids.to_vec();
            if threads {
                for m in queries::message_refs(c, ids)? {
                    let s = queries::get_summary(c, m.id)?;
                    let mut stmt = c.prepare(
                        "SELECT id FROM messages WHERE account_id = ?1 AND thread_key = ?2 AND folder_id = ?3",
                    )?;
                    let more = stmt.query_map(rusqlite::params![m.account_id, s.thread_key, m.folder_id], |r| r.get::<_, i64>(0))?;
                    for id in more {
                        let id = id?;
                        if !all.contains(&id) {
                            all.push(id);
                        }
                    }
                }
            }
            queries::message_refs(c, &all)
        })
    }

    fn group(refs: Vec<MessageRef>) -> BTreeMap<(String, i64), Vec<MessageRef>> {
        let mut groups: BTreeMap<(String, i64), Vec<MessageRef>> = BTreeMap::new();
        for r in refs {
            groups
                .entry((r.account_id.clone(), r.folder_id))
                .or_default()
                .push(r);
        }
        groups
    }

    pub fn set_flags(&self, ids: &[i64], flag: FlagKind, value: bool, threads: bool) -> Result<()> {
        let refs = self.refs(ids, threads)?;
        for ((account_id, folder_id), msgs) in Self::group(refs) {
            let account = self.core.db.with(|c| queries::get_account(c, &account_id))?;
            let changed: Vec<&MessageRef> = msgs
                .iter()
                .filter(|m| match flag {
                    FlagKind::Seen => m.seen != value,
                    FlagKind::Flagged => m.flagged != value,
                })
                .collect();
            if changed.is_empty() {
                continue;
            }
            self.core.db.with(|c| {
                let tx = c.transaction()?;
                for m in &changed {
                    match flag {
                        FlagKind::Seen => queries::set_seen(&tx, m.id, value)?,
                        FlagKind::Flagged => queries::set_flagged(&tx, m.id, value)?,
                    }
                    if account.is_gmail {
                        // Gmail flags are per message, not per label: update the other copies.
                        let col = match flag {
                            FlagKind::Seen => "seen",
                            FlagKind::Flagged => "flagged",
                        };
                        tx.execute(
                            &format!(
                                "UPDATE messages SET {col} = ?3 WHERE account_id = ?1 AND message_id = \
                                 (SELECT message_id FROM messages WHERE id = ?2) AND id != ?2"
                            ),
                            rusqlite::params![account_id, m.id, value],
                        )?;
                    }
                }
                if matches!(flag, FlagKind::Seen) {
                    let delta = if value {
                        -(changed.len() as i64)
                    } else {
                        changed.len() as i64
                    };
                    queries::adjust_unread(&tx, folder_id, delta)?;
                }
                tx.commit()?;
                Ok(())
            })?;
            self.sync.enqueue(
                &account_id,
                Job::SetFlag {
                    folder_id,
                    uids: changed.iter().map(|m| m.uid).collect(),
                    flag: match flag {
                        FlagKind::Seen => "\\Seen",
                        FlagKind::Flagged => "\\Flagged",
                    },
                    add: value,
                },
            );
            self.core.mail_changed(&account_id, Some(folder_id));
        }
        Ok(())
    }

    fn remove_locally(&self, account_id: &str, folder_id: i64, msgs: &[MessageRef]) -> Result<()> {
        let unread = msgs.iter().filter(|m| !m.seen).count() as i64;
        self.core.db.with(|c| {
            let tx = c.transaction()?;
            queries::delete_messages(&tx, &msgs.iter().map(|m| m.id).collect::<Vec<_>>())?;
            queries::adjust_unread(&tx, folder_id, -unread)?;
            tx.commit()?;
            Ok(())
        })?;
        self.core.mail_changed(account_id, Some(folder_id));
        Ok(())
    }

    pub fn move_messages(&self, ids: &[i64], target_folder_id: i64, threads: bool) -> Result<()> {
        let target = self.core.db.with(|c| queries::get_folder(c, target_folder_id))?;
        let refs = self.refs(ids, threads)?;
        for ((account_id, folder_id), msgs) in Self::group(refs) {
            if account_id != target.account_id || folder_id == target.id {
                continue;
            }
            self.remove_locally(&account_id, folder_id, &msgs)?;
            self.sync.enqueue(
                &account_id,
                Job::Move {
                    folder_id,
                    uids: msgs.iter().map(|m| m.uid).collect(),
                    target_path: target.path.clone(),
                },
            );
            self.sync
                .enqueue(&account_id, Job::SyncFolder { folder_id: target.id });
        }
        Ok(())
    }

    pub fn delete_messages(&self, ids: &[i64], threads: bool) -> Result<()> {
        let refs = self.refs(ids, threads)?;
        for ((account_id, folder_id), msgs) in Self::group(refs) {
            let (folder, trash) = self.core.db.with(|c| {
                Ok((
                    queries::get_folder(c, folder_id)?,
                    queries::folder_by_role(c, &account_id, "trash")?,
                ))
            })?;
            self.remove_locally(&account_id, folder_id, &msgs)?;
            let uids = msgs.iter().map(|m| m.uid).collect();
            match trash {
                Some(t) if folder.role != "trash" => {
                    self.sync.enqueue(
                        &account_id,
                        Job::Move {
                            folder_id,
                            uids,
                            target_path: t.path.clone(),
                        },
                    );
                    self.sync
                        .enqueue(&account_id, Job::SyncFolder { folder_id: t.id });
                }
                _ => self.sync.enqueue(&account_id, Job::Expunge { folder_id, uids }),
            }
        }
        Ok(())
    }

    pub fn archive_messages(&self, ids: &[i64], threads: bool) -> Result<()> {
        let refs = self.refs(ids, threads)?;
        let groups = Self::group(refs);
        // Check every account first so nothing is half archived.
        let mut targets = BTreeMap::new();
        for (account_id, _) in groups.keys() {
            let account = self.core.db.with(|c| queries::get_account(c, account_id))?;
            let role = if account.is_gmail { "all" } else { "archive" };
            let target = self
                .core
                .db
                .with(|c| queries::folder_by_role(c, account_id, role))?
                .ok_or(AppError::NoArchiveFolder)?;
            targets.insert(account_id.clone(), target);
        }
        for ((account_id, folder_id), msgs) in groups {
            let target = &targets[&account_id];
            if target.id == folder_id {
                continue;
            }
            self.remove_locally(&account_id, folder_id, &msgs)?;
            self.sync.enqueue(
                &account_id,
                Job::Move {
                    folder_id,
                    uids: msgs.iter().map(|m| m.uid).collect(),
                    target_path: target.path.clone(),
                },
            );
            if !target.hidden {
                self.sync
                    .enqueue(&account_id, Job::SyncFolder { folder_id: target.id });
            }
        }
        Ok(())
    }

    pub async fn create_archive_folder(&self, account_id: &str) -> Result<()> {
        let path = "Archive";
        self.sync
            .run(account_id, Job::CreateFolder { path: path.into() })
            .await?;
        self.core.db.with(|c| {
            queries::upsert_folder(
                c,
                account_id,
                &queries::FolderUpsert {
                    path,
                    name: path,
                    depth: 0,
                    role: "archive",
                    hidden: false,
                },
            )
        })?;
        self.core.mail_changed(account_id, None);
        Ok(())
    }

    // -----------------------------------------------------------------------------------------
    // Reading pane helpers

    pub fn allow_remote_content(&self, sender: &str) -> Result<()> {
        self.core.db.with(|c| queries::allow_remote_content(c, sender))
    }

    /// Name, MIME type and content of an attachment, from the stored raw message.
    pub(crate) fn attachment(&self, message_id: i64, index: u32) -> Result<(String, String, Vec<u8>)> {
        let raw = self
            .core
            .db
            .with(|c| queries::get_raw(c, message_id))?
            .ok_or_else(|| AppError::NotFound("message body".into()))?;
        mime::parse::attachment(&raw, index).ok_or_else(|| AppError::NotFound("attachment".into()))
    }

    pub fn save_attachment(&self, message_id: i64, index: u32, path: &str) -> Result<()> {
        let (_, _, bytes) = self.attachment(message_id, index)?;
        std::fs::write(path, bytes).map_err(|e| AppError::Other(e.to_string()))
    }

    /// Writes the attachment to a temporary file for opening with the default app.
    pub fn attachment_temp_file(&self, message_id: i64, index: u32) -> Result<PathBuf> {
        let (name, _, bytes) = self.attachment(message_id, index)?;
        let ext = std::path::Path::new(&name)
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if BLOCKED_EXTENSIONS.contains(&ext.as_str()) {
            return Err(AppError::Invalid("blocked_file_type".into()));
        }
        let dir = std::env::temp_dir().join("redpanda").join(message_id.to_string());
        std::fs::create_dir_all(&dir).map_err(|e| AppError::Other(e.to_string()))?;
        let path = dir.join(name);
        std::fs::write(&path, bytes).map_err(|e| AppError::Other(e.to_string()))?;
        Ok(path)
    }

    // -----------------------------------------------------------------------------------------
    // Newsletters

    pub fn list_subscriptions(&self) -> Result<Vec<Subscription>> {
        let since = crate::store::now() - 30 * 86_400;
        self.core.db.with(|c| queries::list_subscriptions(c, since))
    }

    /// Unsubscribes from the newsletter `message_id` came from, the best way its sender offers.
    /// With `page` it opens the unsubscribe page instead, the fallback when the other ways fail.
    pub async fn unsubscribe(
        &self,
        message_id: i64,
        page: bool,
        open_url: impl FnOnce(&str) -> Result<()>,
    ) -> Result<UnsubscribeMethod> {
        let row = self
            .core
            .db
            .with(|c| queries::unsubscribe_row(c, message_id))?
            .ok_or_else(|| AppError::NotFound("unsubscribe".into()))?;
        // Answering spam would only confirm the address.
        if row.folder_role == "junk" {
            return Err(AppError::Invalid("unsubscribe_junk".into()));
        }
        let u = &row.unsubscribe;
        let method = if page {
            UnsubscribeMethod::Web
        } else {
            u.method()
                .ok_or_else(|| AppError::NotFound("unsubscribe".into()))?
        };
        match method {
            UnsubscribeMethod::OneClick => {
                unsubscribe::one_click(u.url.as_deref().unwrap_or_default()).await?
            }
            UnsubscribeMethod::Mail => {
                let mailto = u
                    .mailto
                    .as_deref()
                    .and_then(unsubscribe::parse_mailto)
                    .ok_or_else(|| AppError::NotFound("unsubscribe".into()))?;
                let account = self.core.db.with(|c| queries::get_account(c, &row.account_id))?;
                // From the address the newsletter went to, lists may go by the sender.
                let from = match self.core.db.with(|c| queries::get_summary(c, message_id)) {
                    Ok(m) => self.own_recipient(&account, &m.to)?,
                    Err(_) => None,
                };
                let draft = Draft {
                    account_id: account.id.clone(),
                    from_email: from,
                    to: vec![Address {
                        name: None,
                        email: mailto.to,
                    }],
                    subject: mailto
                        .subject
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| "Unsubscribe".into()),
                    body: mailto.body.unwrap_or_else(|| "Unsubscribe".into()),
                    ..Default::default()
                };
                // Sent directly: the list address belongs neither in the contacts nor in Sent.
                let sender = self.sender(&account, draft.from_email.as_deref())?;
                let message = compose::build_message(&sender, &draft, vec![])?;
                smtp::send(&self.core, &account, &message).await?;
            }
            UnsubscribeMethod::Web => {
                let url = u
                    .url
                    .as_deref()
                    .ok_or_else(|| AppError::NotFound("unsubscribe".into()))?;
                open_url(url)?;
            }
        }
        self.core
            .db
            .with(|c| queries::mark_unsubscribed(c, &row.account_id, &row.sender, method))?;
        self.core.mail_changed(&row.account_id, None);
        Ok(method)
    }

    // -----------------------------------------------------------------------------------------
    // Compose

    /// The address to send from: the account's own or one of its aliases, nothing else.
    fn sender(&self, account: &Account, from_email: Option<&str>) -> Result<Address> {
        let own = Address {
            name: Some(account.display_name.clone()),
            email: account.email.clone(),
        };
        let Some(email) = from_email
            .map(str::trim)
            .filter(|e| !e.is_empty() && !e.eq_ignore_ascii_case(&account.email))
        else {
            return Ok(own);
        };
        let alias = self
            .core
            .db
            .with(|c| queries::aliases(c, &account.id))?
            .into_iter()
            .find(|a| a.email.eq_ignore_ascii_case(email))
            .ok_or_else(|| AppError::Invalid("unknown_sender".into()))?;
        Ok(Address {
            name: alias.name.or(own.name),
            email: alias.email,
        })
    }

    /// The account `email` belongs to, and the alias to send from when it is not the account's
    /// own address. Fails for addresses that are neither.
    pub fn resolve_sender(&self, email: &str) -> Result<(String, Option<String>)> {
        let email = email.trim();
        self.core.db.with(|c| {
            for account in queries::list_accounts(c)? {
                if account.email.eq_ignore_ascii_case(email) {
                    return Ok((account.id, None));
                }
                if let Some(alias) = queries::aliases(c, &account.id)?
                    .into_iter()
                    .find(|a| a.email.eq_ignore_ascii_case(email))
                {
                    return Ok((account.id, Some(alias.email)));
                }
            }
            Err(AppError::Invalid("unknown_sender".into()))
        })
    }

    /// The first alias of the account among `recipients`. `None` for the own address or none.
    fn own_recipient(&self, account: &Account, recipients: &[Address]) -> Result<Option<String>> {
        let aliases = self.core.db.with(|c| queries::aliases(c, &account.id))?;
        Ok(recipients
            .iter()
            .find_map(|r| {
                if r.email.eq_ignore_ascii_case(&account.email) {
                    return Some(None);
                }
                aliases
                    .iter()
                    .find(|a| a.email.eq_ignore_ascii_case(&r.email))
                    .map(|a| Some(a.email.clone()))
            })
            .flatten())
    }

    pub async fn send(&self, draft: Draft) -> Result<()> {
        let account = self
            .core
            .db
            .with(|c| queries::get_account(c, &draft.account_id))?;
        let mut attachments = draft
            .attachments
            .iter()
            .map(|p| compose::read_attachment_file(p))
            .collect::<Result<Vec<_>>>()?;
        if let Some(fwd) = draft.forward_from_message_id {
            if self.core.db.with(|c| queries::get_raw(c, fwd))?.is_none() {
                let s = self.core.db.with(|c| queries::get_summary(c, fwd))?;
                self.sync
                    .run(&s.account_id, Job::FetchBody { message_id: fwd })
                    .await?;
            }
            let raw = self
                .core
                .db
                .with(|c| queries::get_raw(c, fwd))?
                .unwrap_or_default();
            let parsed = mime::parse::parse_body(&raw);
            for a in parsed.attachments.iter().filter(|a| !a.inline) {
                if let Some((filename, mime_type, bytes)) = mime::parse::attachment(&raw, a.index) {
                    attachments.push(compose::AttachmentData {
                        filename,
                        mime_type,
                        bytes,
                    });
                }
            }
        }
        let sender = self.sender(&account, draft.from_email.as_deref())?;
        let message = compose::build_message(&sender, &draft, attachments)?;
        smtp::send(&self.core, &account, &message).await?;

        let raw = message.formatted();
        let now = crate::store::now();
        let sent_folder = self.core.db.with(|c| {
            let tx = c.transaction()?;
            for a in draft.to.iter().chain(&draft.cc).chain(&draft.bcc) {
                queries::bump_contact(&tx, a, 5, now)?;
            }
            if let Some(id) = draft.id {
                queries::delete_draft(&tx, id)?;
            }
            let sent = queries::folder_by_role(&tx, &account.id, "sent")?;
            tx.commit()?;
            Ok(sent)
        })?;

        if let Some(reply_id) = draft.reply_to_message_id {
            if let Some(m) = self
                .core
                .db
                .with(|c| queries::message_refs(c, &[reply_id]))?
                .into_iter()
                .next()
            {
                self.core.db.with(|c| queries::set_answered(c, reply_id))?;
                self.sync.enqueue(
                    &m.account_id,
                    Job::SetFlag {
                        folder_id: m.folder_id,
                        uids: vec![m.uid],
                        flag: "\\Answered",
                        add: true,
                    },
                );
            }
        }

        if let Some(sent) = sent_folder {
            if !account.sent_saved_by_server {
                self.sync.enqueue(
                    &account.id,
                    Job::Append {
                        folder_path: sent.path.clone(),
                        raw,
                        flags: "(\\Seen)".into(),
                    },
                );
                self.sync
                    .enqueue(&account.id, Job::SyncFolder { folder_id: sent.id });
            } else {
                // The server files the copy itself, give it a moment before syncing.
                let core = self.core.clone();
                let account_id = account.id.clone();
                let jobs = self.sync.sender(&account.id);
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(4)).await;
                    if let Some(jobs) = jobs {
                        jobs(Job::SyncFolder { folder_id: sent.id });
                    }
                    core.mail_changed(&account_id, Some(sent.id));
                });
            }
        }
        self.core.mail_changed(&account.id, None);
        Ok(())
    }

    pub fn save_draft(&self, draft: &Draft) -> Result<i64> {
        self.core.db.with(|c| queries::save_draft(c, draft))
    }

    pub fn list_drafts(&self) -> Result<Vec<Draft>> {
        self.core.db.with(|c| queries::list_drafts(c))
    }

    pub fn get_draft(&self, id: i64) -> Result<Draft> {
        self.core.db.with(|c| queries::get_draft(c, id))
    }

    pub fn delete_draft(&self, id: i64) -> Result<()> {
        self.core.db.with(|c| queries::delete_draft(c, id))
    }

    pub fn suggest_contacts(&self, prefix: &str) -> Result<Vec<Address>> {
        if prefix.trim().is_empty() {
            return Ok(vec![]);
        }
        self.core
            .db
            .with(|c| queries::suggest_contacts(c, prefix.trim(), 8))
    }

    // -----------------------------------------------------------------------------------------
    // App settings

    pub fn app_settings(&self) -> Result<AppSettings> {
        self.core.db.with(|c| {
            Ok(AppSettings {
                close_to_tray: queries::get_setting(c, SETTING_CLOSE_TO_TRAY)?.as_deref() != Some("0"),
            })
        })
    }

    pub fn set_close_to_tray(&self, value: bool) -> Result<()> {
        self.core
            .db
            .with(|c| queries::set_setting(c, SETTING_CLOSE_TO_TRAY, if value { "1" } else { "0" }))
    }

    /// True the first time it is called, so the hint that redpanda keeps running shows once.
    pub fn take_tray_hint(&self) -> Result<bool> {
        self.core.db.with(|c| {
            let shown = queries::get_setting(c, SETTING_TRAY_HINT_SHOWN)?.is_some();
            if !shown {
                queries::set_setting(c, SETTING_TRAY_HINT_SHOWN, "1")?;
            }
            Ok(!shown)
        })
    }

    /// Unread mail in all inboxes, for the tray tooltip.
    pub fn inbox_unread(&self) -> Result<i64> {
        self.core
            .db
            .with(|c| Ok(queries::unified_counts(c)?.inbox_unread))
    }
}

const SETTING_CLOSE_TO_TRAY: &str = "close_to_tray";
const SETTING_TRAY_HINT_SHOWN: &str = "tray_hint_shown";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    /// Closing the window hides it and redpanda keeps running in the notification area.
    pub close_to_tray: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The JSON shapes below are exactly what `src/lib/ipc.ts` sends.
    #[test]
    fn frontend_payloads_deserialize() {
        let v: View = serde_json::from_str(r#"{"kind":"folder","folderId":4}"#).unwrap();
        assert!(matches!(v, View::Folder { folder_id: 4 }));
        let v: View = serde_json::from_str(r#"{"kind":"unified","which":"starred"}"#).unwrap();
        assert!(matches!(
            v,
            View::Unified {
                which: UnifiedView::Starred
            }
        ));
        let v: View = serde_json::from_str(r#"{"kind":"search","query":"x"}"#).unwrap();
        assert!(matches!(v, View::Search { .. }));
        let v: View = serde_json::from_str(r#"{"kind":"sender","accountId":"a","email":"n@x.de"}"#).unwrap();
        assert!(matches!(v, View::Sender { .. }));

        let input: NewAccountInput = serde_json::from_str(
            r#"{"email":"a@b.de","displayName":"A","providerId":null,"username":"a@b.de",
                "imap":{"host":"i","port":993,"security":"tls"},
                "smtp":{"host":"s","port":587,"security":"start_tls"},
                "auth":{"type":"oauth","provider":"microsoft"}}"#,
        )
        .unwrap();
        assert_eq!(input.smtp.security, Security::StartTls);
        assert!(matches!(
            input.auth,
            NewAuth::Oauth {
                provider: OAuthProvider::Microsoft
            }
        ));

        let d: Draft = serde_json::from_str(
            r#"{"accountId":"a","to":[{"name":null,"email":"x@y.z"}],"cc":[],"bcc":[],"subject":"s",
                "body":"b","references":[],"attachments":[],"replyToMessageId":3,"inReplyTo":"m@x"}"#,
        )
        .unwrap();
        assert_eq!(d.reply_to_message_id, Some(3));
        let f: FlagKind = serde_json::from_str(r#""flagged""#).unwrap();
        assert!(matches!(f, FlagKind::Flagged));
    }

    #[test]
    fn views_serialize_like_the_frontend_types() {
        let a = AccountView {
            account: crate::store::queries_test_account("x"),
            aliases: vec![],
            status: SyncState::AuthRequired,
            status_error: None,
        };
        let json = serde_json::to_value(&a).unwrap();
        assert_eq!(json["status"], "auth_required");
        assert_eq!(json["authType"], "password");
        assert_eq!(json["imap"]["security"], "tls");
        assert_eq!(json["sentSavedByServer"], false);

        let sub = Subscription {
            account_id: "a".into(),
            sender: Address {
                name: None,
                email: "n@x.de".into(),
            },
            latest_id: 7,
            last_date: 1,
            total: 2,
            recent: 1,
            since_unsubscribed: 0,
            unsubscribe: UnsubscribeInfo {
                method: UnsubscribeMethod::OneClick,
                target: "x.de".into(),
                has_page: true,
                unsubscribed_at: None,
            },
        };
        let json = serde_json::to_value(&sub).unwrap();
        assert_eq!(json["latestId"], 7);
        assert_eq!(json["sinceUnsubscribed"], 0);
        assert_eq!(json["method"], "one_click");
        assert_eq!(json["hasPage"], true);
        assert!(json["unsubscribedAt"].is_null());
    }
}
