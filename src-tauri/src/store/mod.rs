//! SQLite storage. One connection behind a mutex; every query is short, so the lock is held only
//! for the duration of a single statement or transaction.

mod migrations;
pub mod queries;

use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::error::{AppError, Result};

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::Other(e.to_string()))?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Db> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Db> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        migrations::apply(&mut conn)?;
        Ok(Db {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Runs `f` with exclusive access to the connection.
    pub fn with<T>(&self, f: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T> {
        let mut guard = self
            .conn
            .lock()
            .map_err(|_| AppError::Other("database lock poisoned".into()))?;
        f(&mut guard)
    }
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

#[cfg(test)]
pub fn queries_test_account(id: &str) -> crate::model::Account {
    use crate::model::*;
    Account {
        id: id.into(),
        email: format!("{id}@example.com"),
        display_name: "Test".into(),
        provider: "custom".into(),
        auth_type: AuthType::Password,
        username: format!("{id}@example.com"),
        imap: ServerConfig {
            host: "imap.example.com".into(),
            port: 993,
            security: Security::Tls,
        },
        smtp: ServerConfig {
            host: "smtp.example.com".into(),
            port: 465,
            security: Security::Tls,
        },
        color: "#c2410c".into(),
        sent_saved_by_server: false,
        is_gmail: false,
    }
}
