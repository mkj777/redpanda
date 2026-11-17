//! A Model Context Protocol server inside the running app, so Claude Code can read and send mail
//! through redpanda without ever seeing a password or token. See docs/adr/0008-mcp-server.md.
//!
//! It listens on 127.0.0.1 only, wants the key from the settings as a bearer token, accepts only
//! loopback `Host` headers and refuses requests that carry an `Origin`, so web pages can't reach it.
//! Claude Desktop reaches it through [`bridge`].

pub mod bridge;
pub mod tools;

use std::path::{Path, PathBuf};
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

use crate::accounts::secrets::Secrets;
use crate::error::{AppError, Result};
use crate::service::App;
use crate::store::queries;

pub const DEFAULT_PORT: u16 = 47821;
const SETTING_ENABLED: &str = "mcp_enabled";
const SETTING_PORT: &str = "mcp_port";
const SETTING_KEY: &str = "mcp_key";
/// The file Claude Desktop reads its local MCP servers from.
const DESKTOP_CONFIG: &str = "claude_desktop_config.json";

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
    /// Whether Claude Desktop keeps its data on this computer, for the button that registers
    /// redpanda there.
    pub claude_desktop_available: bool,
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
        port_from(self.setting(SETTING_PORT).ok().flatten())
    }

    /// The stored key, encrypted like the account secrets. `None` before the first start.
    fn stored_key(&self) -> Result<Option<String>> {
        self.setting(SETTING_KEY)?
            .map(|stored| decrypt_key(&self.app.core.secrets, &stored))
            .transpose()
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
            claude_desktop_available: claude_desktop_config().is_some(),
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

    /// Registers the bridge in Claude Desktop, replacing an earlier entry. The entry holds only
    /// the path of redpanda: the bridge reads port and key itself.
    pub async fn connect_claude_desktop(&self) -> Result<()> {
        let config = claude_desktop_config()
            .ok_or_else(|| AppError::Invalid("Claude Desktop was not found".into()))?;
        let exe = std::env::current_exe().map_err(|e| AppError::Other(e.to_string()))?;
        tokio::task::spawn_blocking(move || add_to_desktop_config(&config, &exe))
            .await
            .map_err(|e| AppError::Other(e.to_string()))??;
        tracing::info!("added redpanda to Claude Desktop");
        Ok(())
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

fn port_from(setting: Option<String>) -> u16 {
    setting.and_then(|p| p.parse().ok()).unwrap_or(DEFAULT_PORT)
}

/// The key as `app_settings` keeps it: nonce and ciphertext in Base64, split by a colon.
fn decrypt_key(secrets: &Secrets, stored: &str) -> Result<String> {
    let b64 = base64::engine::general_purpose::STANDARD;
    let (nonce, ciphertext) = stored
        .split_once(':')
        .ok_or_else(|| AppError::Secrets("bad MCP key".into()))?;
    let decode = |s: &str| b64.decode(s).map_err(|e| AppError::Secrets(e.to_string()));
    let plain = secrets.decrypt(&decode(ciphertext)?, &decode(nonce)?)?;
    String::from_utf8(plain).map_err(|e| AppError::Secrets(e.to_string()))
}

/// Claude Desktop's config file in the data folder it used last. The Store version keeps its
/// data in `%LOCALAPPDATA%\Claude-Data\<package>`, the installer version in `%APPDATA%\Claude`.
fn claude_desktop_config() -> Option<PathBuf> {
    let mut folders = Vec::new();
    if let Some(Ok(entries)) = dirs::data_local_dir().map(|d| std::fs::read_dir(d.join("Claude-Data"))) {
        folders.extend(entries.flatten().map(|e| e.path()));
    }
    folders.extend(dirs::config_dir().map(|d| d.join("Claude")));
    latest_claude_folder(folders).map(|d| d.join(DESKTOP_CONFIG))
}

/// Of the folders Claude Desktop may use, the one it wrote its own settings (`config.json`) to
/// last. A folder an uninstalled version left behind has older ones.
fn latest_claude_folder(folders: Vec<PathBuf>) -> Option<PathBuf> {
    folders
        .into_iter()
        .filter_map(|d| {
            let written = std::fs::metadata(d.join("config.json")).and_then(|m| m.modified());
            Some((written.ok()?, d))
        })
        .max_by_key(|(written, _)| *written)
        .map(|(_, d)| d)
}

/// Sets the `redpanda` entry under `mcpServers` and keeps everything else in the file.
fn add_to_desktop_config(path: &Path, exe: &Path) -> Result<()> {
    let invalid = || AppError::Other(format!("{} is not a JSON object", path.display()));
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(AppError::Other(e.to_string())),
    };
    let text = text.trim_start_matches('\u{feff}');
    let mut config = if text.trim().is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(text).map_err(|_| invalid())?
    };
    let servers = config
        .as_object_mut()
        .ok_or_else(invalid)?
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(invalid)?;
    servers.insert(
        "redpanda".into(),
        serde_json::json!({ "command": exe.to_string_lossy(), "args": [bridge::ARG] }),
    );
    // Written next to it and renamed, so Claude Desktop never reads half a file.
    let partial = path.with_extension("json.partial");
    std::fs::write(&partial, serde_json::to_string_pretty(&config)?)
        .and_then(|()| std::fs::rename(&partial, path))
        .map_err(|e| AppError::Other(e.to_string()))
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

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("redpanda-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn key_round_trip() {
        let secrets = Secrets::from_key(&[3u8; 32]).unwrap();
        let (ciphertext, nonce) = secrets.encrypt(b"rp_abc").unwrap();
        let b64 = base64::engine::general_purpose::STANDARD;
        let stored = format!("{}:{}", b64.encode(nonce), b64.encode(ciphertext));
        assert_eq!(decrypt_key(&secrets, &stored).unwrap(), "rp_abc");
        assert!(decrypt_key(&secrets, "no colon").is_err());
        assert_eq!(port_from(Some("50000".into())), 50000);
        assert_eq!(port_from(Some("x".into())), DEFAULT_PORT);
        assert_eq!(port_from(None), DEFAULT_PORT);
    }

    #[test]
    fn claude_desktop_entry_keeps_the_rest_of_the_file() {
        let dir = temp_dir("desktop-config");
        let path = dir.join(DESKTOP_CONFIG);
        let exe = Path::new(r"C:\Apps\redpanda\redpanda.exe");
        let entry =
            serde_json::json!({ "command": r"C:\Apps\redpanda\redpanda.exe", "args": ["--mcp-stdio"] });

        // Missing: created with just the entry.
        add_to_desktop_config(&path, exe).unwrap();
        let written: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            written,
            serde_json::json!({ "mcpServers": { "redpanda": entry } })
        );

        // Existing, with a BOM: other servers, settings and an old entry.
        std::fs::write(
            &path,
            "\u{feff}{\"preferences\":{\"menuBarEnabled\":false},\"mcpServers\":{\"other\":{\"command\":\"npx\"},\"redpanda\":{\"command\":\"old.exe\"}}}",
        )
        .unwrap();
        add_to_desktop_config(&path, exe).unwrap();
        let written: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written["preferences"]["menuBarEnabled"], false);
        assert_eq!(written["mcpServers"]["other"]["command"], "npx");
        assert_eq!(written["mcpServers"]["redpanda"], entry);

        // A file that is not a JSON object stays as it is.
        std::fs::write(&path, "[1, 2").unwrap();
        assert!(add_to_desktop_config(&path, exe).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[1, 2");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn claude_desktop_folder_used_last_wins() {
        let root = temp_dir("desktop-folders");
        let (old, new, empty) = (root.join("old"), root.join("new"), root.join("empty"));
        for (dir, age) in [(&old, 3600), (&new, 60)] {
            std::fs::create_dir_all(dir).unwrap();
            let file = std::fs::File::create(dir.join("config.json")).unwrap();
            file.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(age))
                .unwrap();
        }
        std::fs::create_dir_all(&empty).unwrap();
        let picked = latest_claude_folder(vec![old.clone(), empty.clone(), new.clone()]);
        assert_eq!(picked, Some(new));
        assert_eq!(latest_claude_folder(vec![empty]), None);
        std::fs::remove_dir_all(root).unwrap();
    }
}
