//! Shared application services, independent of Tauri so they can be used from tests.

use std::sync::{Arc, RwLock};

use serde_json::json;

use crate::accounts::oauth::OAuthConfig;
use crate::accounts::secrets::Secrets;
use crate::model::SyncStatus;
use crate::store::Db;

pub trait EventSink: Send + Sync {
    fn emit(&self, event: &str, payload: serde_json::Value);
}

/// Discards events (tests, headless use).
pub struct NoEvents;

impl EventSink for NoEvents {
    fn emit(&self, _: &str, _: serde_json::Value) {}
}

pub struct Core {
    pub db: Db,
    pub secrets: Secrets,
    pub oauth: RwLock<OAuthConfig>,
    pub http: reqwest::Client,
    pub events: Arc<dyn EventSink>,
    /// Serializes OAuth token refreshes so parallel connections do not race.
    pub refresh_lock: tokio::sync::Mutex<()>,
}

impl Core {
    pub fn new(db: Db, secrets: Secrets, oauth: OAuthConfig, events: Arc<dyn EventSink>) -> Core {
        let http = reqwest::Client::builder()
            .user_agent(concat!("redpanda/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("HTTP client");
        Core {
            db,
            secrets,
            oauth: RwLock::new(oauth),
            http,
            events,
            refresh_lock: tokio::sync::Mutex::new(()),
        }
    }

    pub fn oauth_config(&self) -> OAuthConfig {
        self.oauth.read().map(|c| c.clone()).unwrap_or_default()
    }

    pub fn mail_changed(&self, account_id: &str, folder_id: Option<i64>) {
        self.events.emit(
            "mail:changed",
            json!({ "accountId": account_id, "folderId": folder_id }),
        );
    }

    /// Account data besides the sync status changed, for now the aliases.
    pub fn accounts_changed(&self) {
        self.events.emit("accounts:changed", json!({}));
    }

    pub fn sync_status(&self, status: &SyncStatus) {
        self.events
            .emit("sync:status", serde_json::to_value(status).unwrap_or_default());
    }

    pub fn action_failed(&self, account_id: &str, error: &crate::error::AppError) {
        self.events.emit(
            "action:failed",
            json!({ "accountId": account_id, "error": error }),
        );
    }
}
