//! A Model Context Protocol server inside the running app, so Claude Code can read and send mail
//! through redpanda without ever seeing a password or token. See docs/adr/0008-mcp-server.md.
//!
//! It listens on 127.0.0.1 only, wants the key from the settings as a bearer token, accepts only
//! loopback `Host` headers and refuses requests that carry an `Origin`, so web pages can't reach it.

pub mod tools;

use std::sync::{Arc, Mutex, OnceLock, RwLock};

use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use base64::Engine;
use rand::RngCore;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use serde::Serialize;
use subtle::ConstantTimeEq;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, Result};
use crate::service::App;
use crate::store::queries;

pub const DEFAULT_PORT: u16 = 47821;
const SETTING_ENABLED: &str = "mcp_enabled";
const SETTING_PORT: &str = "mcp_port";
const SETTING_KEY: &str = "mcp_key";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpStatus {
    pub enabled: bool,
    pub port: u16,
    pub running: bool,
    /// `port_in_use` or another message when the server could not start.
    pub error: Option<String>,
    /// The key with all but its last characters hidden, once one exists.
    pub key_masked: Option<String>,
    /// Whether the `claude` command is installed, for the button that registers redpanda.
    pub claude_available: bool,
    pub url: String,
}

struct Running {
    port: u16,
    cancel: CancellationToken,
}

pub struct Mcp {
    app: Arc<App>,
    /// Shared with the running server, so a new key applies to the next request.
    key: Arc<RwLock<String>>,
    running: tokio::sync::Mutex<Option<Running>>,
    error: Mutex<Option<String>>,
}

impl Mcp {
    pub fn new(app: Arc<App>) -> Arc<Mcp> {
        Arc::new(Mcp {
            app,
            key: Arc::new(RwLock::new(String::new())),
            running: tokio::sync::Mutex::new(None),
            error: Mutex::new(None),
        })
    }

    fn setting(&self, key: &str) -> Result<Option<String>> {
        self.app.core.db.with(|c| queries::get_setting(c, key))
    }

    fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.app.core.db.with(|c| queries::set_setting(c, key, value))
    }

    pub fn enabled(&self) -> bool {
        matches!(self.setting(SETTING_ENABLED), Ok(Some(v)) if v == "1")
    }

    pub fn port(&self) -> u16 {
        self.setting(SETTING_PORT)
            .ok()
            .flatten()
            .and_then(|p| p.parse().ok())
            .unwrap_or(DEFAULT_PORT)
    }

    /// The stored key, encrypted like the account secrets. `None` before the first start.
    fn stored_key(&self) -> Result<Option<String>> {
        let Some(stored) = self.setting(SETTING_KEY)? else {
            return Ok(None);
        };
        let b64 = base64::engine::general_purpose::STANDARD;
        let (nonce, ciphertext) = stored
            .split_once(':')
            .ok_or_else(|| AppError::Secrets("bad MCP key".into()))?;
        let decode = |s: &str| b64.decode(s).map_err(|e| AppError::Secrets(e.to_string()));
        let plain = self
            .app
            .core
            .secrets
            .decrypt(&decode(ciphertext)?, &decode(nonce)?)?;
        String::from_utf8(plain)
            .map(Some)
            .map_err(|e| AppError::Secrets(e.to_string()))
    }

    fn new_key(&self) -> Result<String> {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        let key = format!(
            "rp_{}",
            bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
        );
        let (ciphertext, nonce) = self.app.core.secrets.encrypt(key.as_bytes())?;
        let b64 = base64::engine::general_purpose::STANDARD;
        self.set_setting(
            SETTING_KEY,
            &format!("{}:{}", b64.encode(nonce), b64.encode(ciphertext)),
        )?;
        Ok(key)
    }

    /// The key, created on first use.
    pub fn key(&self) -> Result<String> {
        let key = match self.stored_key()? {
            Some(k) => k,
            None => self.new_key()?,
        };
        if let Ok(mut k) = self.key.write() {
            k.clone_from(&key);
        }
        Ok(key)
    }

    /// Replaces the key. The old one stops working at once.
    pub fn rotate_key(&self) -> Result<()> {
        let key = self.new_key()?;
        if let Ok(mut k) = self.key.write() {
            *k = key;
        }
        self.changed();
        Ok(())
    }

    pub async fn status(&self) -> McpStatus {
        let port = self.port();
        let running = self.running.lock().await.is_some();
        McpStatus {
            enabled: self.enabled(),
            port,
            running,
            error: self.error.lock().ok().and_then(|e| e.clone()),
            key_masked: self.stored_key().ok().flatten().map(|k| mask(&k)),
            claude_available: claude_command().is_some(),
            url: url(port),
        }
    }

    /// Starts the server when it is switched on and not running. Errors end up in the status.
    pub async fn start(&self) {
        let mut running = self.running.lock().await;
        if running.is_some() || !self.enabled() {
            return;
        }
        let port = self.port();
        let result = async {
            self.key()?;
            let listener = TcpListener::bind(("127.0.0.1", port)).await.map_err(|e| {
                if e.kind() == std::io::ErrorKind::AddrInUse {
                    AppError::Other("port_in_use".into())
                } else {
                    AppError::Other(e.to_string())
                }
            })?;
            let cancel = CancellationToken::new();
            tokio::spawn(serve(
                self.app.clone(),
                listener,
                self.key.clone(),
                cancel.clone(),
            ));
            Ok::<_, AppError>(cancel)
        }
        .await;
        let error = match result {
            Ok(cancel) => {
                tracing::info!(port, "MCP server listening");
                *running = Some(Running { port, cancel });
                None
            }
            Err(e) => {
                tracing::warn!(port, "MCP server did not start: {e}");
                Some(e.to_string())
            }
        };
        if let Ok(mut slot) = self.error.lock() {
            *slot = error;
        }
        drop(running);
        self.changed();
    }

    pub async fn stop(&self) {
        if let Some(r) = self.running.lock().await.take() {
            r.cancel.cancel();
            tracing::info!(port = r.port, "MCP server stopped");
        }
        if let Ok(mut slot) = self.error.lock() {
            *slot = None;
        }
        self.changed();
    }

    pub async fn set_enabled(&self, value: bool) -> Result<()> {
        self.set_setting(SETTING_ENABLED, if value { "1" } else { "0" })?;
        if value {
            self.start().await;
        } else {
            self.stop().await;
        }
        Ok(())
    }

    pub async fn set_port(&self, port: u16) -> Result<()> {
        if port < 1024 {
            return Err(AppError::Invalid("port must be 1024 or higher".into()));
        }
        self.set_setting(SETTING_PORT, &port.to_string())?;
        if self.running.lock().await.is_some() {
            self.stop().await;
        }
        self.start().await;
        Ok(())
    }

    /// The command that registers redpanda in Claude Code, for this port and key.
    pub fn claude_command_line(&self) -> Result<String> {
        Ok(format!(
            "claude mcp add --scope user --transport http redpanda {} --header \"Authorization: Bearer {}\"",
            url(self.port()),
            self.key()?
        ))
    }

    /// Registers redpanda in Claude Code for the user, replacing an earlier entry.
    pub async fn connect_claude(&self) -> Result<()> {
        let claude = claude_command().ok_or_else(|| AppError::Invalid("claude_not_found".into()))?;
        let key = self.key()?;
        let url = url(self.port());
        tokio::task::spawn_blocking(move || {
            // An older entry may point at another port or key.
            let _ = run_claude(&claude, &["mcp", "remove", "--scope", "user", "redpanda"]);
            run_claude(
                &claude,
                &[
                    "mcp",
                    "add",
                    "--scope",
                    "user",
                    "--transport",
                    "http",
                    "redpanda",
                    &url,
                    "--header",
                    &format!("Authorization: Bearer {key}"),
                ],
            )
        })
        .await
        .map_err(|e| AppError::Other(e.to_string()))?
    }

    fn changed(&self) {
        let enabled = self.enabled();
        self.app
            .core
            .events
            .emit("mcp:changed", serde_json::json!({ "enabled": enabled }));
    }
}

fn url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/mcp")
}

fn mask(key: &str) -> String {
    let tail: String = key
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("rp_••••••••••••{tail}")
}

/// Serves the MCP endpoint on `listener` until `cancel` fires. Public for the integration tests.
pub async fn serve(
    app: Arc<App>,
    listener: TcpListener,
    key: Arc<RwLock<String>>,
    cancel: CancellationToken,
) {
    let port = match listener.local_addr() {
        Ok(a) => a.port(),
        Err(e) => {
            tracing::warn!("MCP listener has no address: {e}");
            return;
        }
    };
    let config = StreamableHttpServerConfig::default()
        .with_allowed_hosts([format!("127.0.0.1:{port}"), format!("localhost:{port}")])
        .enforce_origin_validation()
        .with_cancellation_token(cancel.child_token());
    let service: StreamableHttpService<tools::MailTools, LocalSessionManager> = StreamableHttpService::new(
        move || Ok(tools::MailTools::new(app.clone())),
        Default::default(),
        config,
    );
    let router = axum::Router::new()
        .nest_service("/mcp", service)
        .layer(middleware::from_fn_with_state(key, authorize));
    if let Err(e) = axum::serve(listener, router)
        .with_graceful_shutdown(async move { cancel.cancelled_owned().await })
        .await
    {
        tracing::warn!("MCP server ended: {e}");
    }
}

async fn authorize(
    State(key): State<Arc<RwLock<String>>>,
    request: Request,
    next: Next,
) -> std::result::Result<Response, StatusCode> {
    let given = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    let ok = key
        .read()
        .map(|k| !k.is_empty() && bool::from(k.as_bytes().ct_eq(given.as_bytes())))
        .unwrap_or(false);
    if !ok {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(next.run(request).await)
}

/// Where the `claude` command lives, looked up once.
fn claude_command() -> Option<std::path::PathBuf> {
    static FOUND: OnceLock<Option<std::path::PathBuf>> = OnceLock::new();
    FOUND
        .get_or_init(|| {
            let finder = if cfg!(windows) { "where" } else { "which" };
            let out = hidden(std::process::Command::new(finder).arg("claude"))
                .output()
                .ok()?;
            let text = String::from_utf8_lossy(&out.stdout).into_owned();
            let mut found: Vec<_> = text
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(std::path::PathBuf::from)
                .collect();
            // `where` also lists the extensionless npm shim, which Windows can't start.
            found.sort_by_key(|p| match p.extension().and_then(|e| e.to_str()) {
                Some("exe") => 0,
                Some("cmd") => 1,
                _ => 2,
            });
            found.into_iter().next()
        })
        .clone()
}

fn run_claude(claude: &std::path::Path, args: &[&str]) -> Result<()> {
    let is_script = claude
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
    let mut command = if is_script {
        let mut c = std::process::Command::new("cmd");
        c.arg("/C").arg(claude);
        c
    } else {
        std::process::Command::new(claude)
    };
    let out = hidden(command.args(args))
        .output()
        .map_err(|e| AppError::Other(e.to_string()))?;
    if out.status.success() {
        Ok(())
    } else {
        // The output can echo the command, so it is not passed on: it contains the key.
        Err(AppError::Other(format!("claude mcp failed with {}", out.status)))
    }
}

/// No console window flashes up for the helper processes.
fn hidden(command: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_all_but_the_end() {
        assert_eq!(mask("rp_0123456789abcdef"), "rp_••••••••••••cdef");
    }
}
