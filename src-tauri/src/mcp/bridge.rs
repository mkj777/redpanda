//! `redpanda --mcp-stdio`, the program Claude Desktop starts for redpanda. Claude Desktop runs
//! local MCP servers as programs it talks to over stdin and stdout, so the bridge posts each
//! message to the MCP server in the running app and writes its answers back. It reads the port
//! and the key itself, so Claude Desktop's config holds neither, and starts redpanda in the
//! notification area when nothing answers. See docs/adr/0010-claude-desktop-bridge.md.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine;
use futures::future::BoxFuture;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use reqwest::StatusCode;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, Mutex};

use crate::accounts::secrets::Secrets;
use crate::store::queries;

/// The argument Claude Desktop starts redpanda with.
pub const ARG: &str = "--mcp-stdio";

const SESSION: &str = "Mcp-Session-Id";
const PROTOCOL: &str = "MCP-Protocol-Version";
/// Where a request of the stateless protocol (2026-07-28) names its version.
const META_PROTOCOL: &str = "io.modelcontextprotocol/protocolVersion";
/// How long a redpanda the bridge started gets until its MCP server answers.
const START_TIMEOUT: Duration = Duration::from_secs(30);
/// JSON-RPC "internal error", for requests the bridge could not deliver.
const INTERNAL_ERROR: i64 = -32603;
const OFF: &str = "Access for Claude Code is switched off in redpanda. Switch it on in redpanda \
                   under Settings, Claude Code.";

/// Where the app's MCP server listens, and the key it wants.
#[derive(Clone)]
pub struct Target {
    pub url: String,
    pub key: String,
}

/// Finds the server, starting redpanda when needed. The error text reaches Claude Desktop.
pub type Locate = Arc<dyn Fn() -> BoxFuture<'static, Result<Target, String>> + Send + Sync>;

/// Runs the bridge on stdin and stdout until Claude Desktop closes them. Returns the exit code.
pub fn run(identifier: &str) -> i32 {
    init_logging();
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("could not start the bridge: {e}");
            return 1;
        }
    };
    let dir = dirs::data_local_dir().map(|d| d.join(identifier));
    let locate: Locate = Arc::new(move || {
        let dir = dir.clone();
        Box::pin(async move {
            let dir = dir.ok_or("Windows names no local app data folder")?;
            find(dir).await
        })
    });
    runtime.block_on(run_with(locate, tokio::io::stdin(), tokio::io::stdout()));
    // A read of stdin may still be waiting in the blocking pool.
    runtime.shutdown_timeout(Duration::from_millis(200));
    0
}

/// Claude Desktop keeps what a server writes to stderr in its own log, `mcp-server-redpanda.log`.
/// Like the app's log it never holds mail content or the key.
fn init_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_env("REDPANDA_LOG")
        .unwrap_or_else(|_| "redpanda_lib=info".into());
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_timer(tracing_subscriber::fmt::time::ChronoLocal::rfc_3339())
        .try_init();
}

/// Passes each line of `input` (one JSON-RPC message) to the MCP server and its answers to
/// `output`, until `input` ends.
pub async fn run_with<R, W>(locate: Locate, input: R, output: W)
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let (out, mut outgoing) = mpsc::unbounded_channel::<String>();
    tokio::spawn(async move {
        let mut output = output;
        while let Some(line) = outgoing.recv().await {
            let written = async {
                output.write_all(line.as_bytes()).await?;
                output.write_all(b"\n").await?;
                output.flush().await
            }
            .await;
            if written.is_err() {
                break;
            }
        }
    });
    // A new connection per message: cheap on loopback, and a restarted redpanda never meets one
    // its predecessor left behind.
    let http = match reqwest::Client::builder()
        .no_proxy()
        .pool_max_idle_per_host(0)
        .connect_timeout(Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("could not set up HTTP: {e}");
            return;
        }
    };
    let bridge = Arc::new(Bridge {
        http,
        locate,
        out,
        session: Mutex::default(),
    });
    tracing::info!("bridge for Claude Desktop started");
    let mut lines = BufReader::new(input).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let msg: Value = match serde_json::from_str(&line) {
            Ok(m) => m,
            Err(_) if line.trim().is_empty() => continue,
            Err(e) => {
                tracing::warn!("skipped a line that is not JSON: {e}");
                continue;
            }
        };
        // Requests can take long (sending a mail), so they run side by side. Everything else
        // keeps its order: the server must see `initialize` before its notification.
        if is_request(&msg) && !is_initialize(&msg) {
            tokio::spawn(bridge.clone().handle(msg));
        } else {
            bridge.clone().handle(msg).await;
        }
    }
    bridge.close().await;
    tracing::info!("Claude Desktop closed the bridge");
}

#[derive(Default)]
struct Session {
    target: Option<Target>,
    id: Option<String>,
    /// The version `initialize` agreed on, sent along with every later message.
    protocol: Option<String>,
    /// Claude Desktop's `initialize`, repeated when a restarted redpanda forgot the session.
    init: Option<Value>,
    /// Counts reconnects, so messages that all failed on the old connection reconnect once.
    generation: u64,
}

struct Bridge {
    http: reqwest::Client,
    locate: Locate,
    out: mpsc::UnboundedSender<String>,
    session: Mutex<Session>,
}

/// What a message is sent with.
struct Route {
    target: Target,
    session: Option<String>,
    protocol: Option<String>,
    generation: u64,
}

impl Bridge {
    async fn handle(self: Arc<Self>, msg: Value) {
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("").to_owned();
        // The requests in it that still wait for an answer.
        let mut open = request_ids(&msg);
        let result = self.deliver(&msg, &mut open).await;
        if let Err(e) = &result {
            tracing::warn!(method = %method, "could not pass a message on: {e}");
        }
        if open.is_empty() {
            return;
        }
        let text = result
            .err()
            .unwrap_or_else(|| "redpanda ended without an answer".into());
        for id in open {
            self.write(&json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": INTERNAL_ERROR, "message": text },
            }));
        }
    }

    /// Posts `msg` and passes the answers on. When redpanda restarted, its key changed or it no
    /// longer knows the session, the bridge reconnects once and tries again.
    async fn deliver(&self, msg: &Value, open: &mut Vec<Value>) -> Result<(), String> {
        let init = is_initialize(msg);
        if init {
            self.session.lock().await.init = Some(msg.clone());
        }
        let mut retried = false;
        loop {
            let route = self.route().await?;
            let (session, protocol) = if init {
                (None, None)
            } else {
                (route.session.as_deref(), route.protocol.as_deref())
            };
            let response = self.post(&route.target, msg, session, protocol).send().await;
            // None of these reached a tool, so sending again can't send a mail twice.
            let lost = match &response {
                Err(e) => e.is_connect(),
                Ok(r) => {
                    r.status() == StatusCode::UNAUTHORIZED
                        || (r.status() == StatusCode::NOT_FOUND && session.is_some())
                }
            };
            if lost && !retried {
                retried = true;
                self.reconnect(route.generation, !init).await?;
                continue;
            }
            let response = response.map_err(|e| format!("redpanda is not reachable: {e}"))?;
            let status = response.status();
            if !status.is_success() {
                // A JSON-RPC error in the body is the answer itself.
                let body = response.text().await.unwrap_or_default();
                match serde_json::from_str::<Value>(&body) {
                    Ok(answer) if !response_ids(&answer).is_empty() => {
                        self.pass_on(answer, open);
                        return Ok(());
                    }
                    _ => return Err(format!("redpanda answered with {status}")),
                }
            }
            if !init {
                return read_messages(response, |m| self.pass_on(m, open)).await;
            }
            // The session has to be known before Claude Desktop sends its next message.
            let id = session_id(&response);
            let mut answers = Vec::new();
            read_messages(response, |m| answers.push(m)).await?;
            {
                let mut s = self.session.lock().await;
                s.id = id;
                s.protocol = agreed_protocol(&answers);
                s.generation += 1;
            }
            for answer in answers {
                self.pass_on(answer, open);
            }
            return Ok(());
        }
    }

    /// The server and session to use, looking the server up first if there is none yet.
    async fn route(&self) -> Result<Route, String> {
        let mut s = self.session.lock().await;
        let target = match &s.target {
            Some(t) => t.clone(),
            None => {
                let t = (self.locate)().await?;
                s.target = Some(t.clone());
                t
            }
        };
        Ok(Route {
            target,
            session: s.id.clone(),
            protocol: s.protocol.clone(),
            generation: s.generation,
        })
    }

    /// Looks redpanda up again, starting it when needed, and repeats Claude Desktop's
    /// `initialize` there when `resume` asks for its session back.
    async fn reconnect(&self, seen: u64, resume: bool) -> Result<(), String> {
        let mut s = self.session.lock().await;
        if s.generation != seen {
            // Another message reconnected in the meantime.
            return Ok(());
        }
        s.generation += 1;
        s.id = None;
        s.target = None;
        let target = (self.locate)().await?;
        s.target = Some(target.clone());
        if let (true, Some(init)) = (resume, s.init.clone()) {
            let (id, protocol) = self.initialize(&target, init).await?;
            s.id = id;
            s.protocol = protocol;
        }
        tracing::info!("reconnected to redpanda");
        Ok(())
    }

    /// Opens a new session with Claude Desktop's earlier `initialize`. Its answer is not passed
    /// on: Claude Desktop already has one.
    async fn initialize(
        &self,
        target: &Target,
        mut init: Value,
    ) -> Result<(Option<String>, Option<String>), String> {
        init["id"] = json!("redpanda-bridge-reconnect");
        let response = self
            .post(target, &init, None, None)
            .send()
            .await
            .map_err(|e| format!("redpanda is not reachable: {e}"))?;
        if !response.status().is_success() {
            return Err(format!("redpanda refused a new session: {}", response.status()));
        }
        let id = session_id(&response);
        let mut answers = Vec::new();
        read_messages(response, |m| answers.push(m)).await?;
        let protocol = agreed_protocol(&answers);
        let initialized = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        let done = self
            .post(target, &initialized, id.as_deref(), protocol.as_deref())
            .send()
            .await
            .map_err(|e| format!("redpanda is not reachable: {e}"))?;
        if !done.status().is_success() {
            return Err(format!("redpanda refused a new session: {}", done.status()));
        }
        Ok((id, protocol))
    }

    fn post(
        &self,
        target: &Target,
        msg: &Value,
        session: Option<&str>,
        protocol: Option<&str>,
    ) -> reqwest::RequestBuilder {
        let mut request = self
            .http
            .post(&target.url)
            .bearer_auth(&target.key)
            .header(ACCEPT, "application/json, text/event-stream")
            .json(msg);
        if let Some(id) = session {
            request = request.header(SESSION, id);
        }
        if let Some(version) = meta_protocol(msg).or(protocol) {
            request = request.header(PROTOCOL, version);
        }
        for (name, value) in standard_headers(msg) {
            request = request.header(name, value);
        }
        request
    }

    /// Writes a message from the server to Claude Desktop and crosses off the requests it
    /// answers.
    fn pass_on(&self, msg: Value, open: &mut Vec<Value>) {
        for answered in response_ids(&msg) {
            open.retain(|id| *id != answered);
        }
        self.write(&msg);
    }

    fn write(&self, msg: &Value) {
        // Compact JSON has no line breaks, so one message stays one line.
        let _ = self.out.send(msg.to_string());
    }

    /// Ends the session, so redpanda can forget it at once. Skipped while a reconnect still
    /// waits for redpanda.
    async fn close(&self) {
        let Ok(s) = self.session.try_lock() else {
            return;
        };
        if let (Some(target), Some(id)) = (&s.target, &s.id) {
            let _ = self
                .http
                .delete(&target.url)
                .bearer_auth(&target.key)
                .header(SESSION, id)
                .timeout(Duration::from_secs(2))
                .send()
                .await;
        }
    }
}

/// Reads the answer to a POST, plain JSON or an event stream, and hands on each message in it.
async fn read_messages(mut response: reqwest::Response, mut each: impl FnMut(Value)) -> Result<(), String> {
    let stream = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|t| t.starts_with("text/event-stream"));
    if !stream {
        let body = response
            .bytes()
            .await
            .map_err(|e| format!("redpanda broke off its answer: {e}"))?;
        if !body.iter().all(u8::is_ascii_whitespace) {
            each(serde_json::from_slice(&body).map_err(|e| format!("redpanda sent no JSON: {e}"))?);
        }
        return Ok(());
    }
    let mut events = Events::default();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("redpanda broke off its answer: {e}"))?
    {
        for data in events.feed(&chunk) {
            match serde_json::from_str(&data) {
                Ok(m) => each(m),
                Err(e) => tracing::warn!("skipped an event that is not JSON: {e}"),
            }
        }
    }
    Ok(())
}

/// Splits a server-sent event stream into the data of its events.
#[derive(Default)]
struct Events {
    pending: Vec<u8>,
    data: Vec<String>,
}

impl Events {
    /// Takes the next piece of the stream and returns the events it completes. Events without
    /// data, like the first one that only primes a reconnect, are left out.
    fn feed(&mut self, chunk: &[u8]) -> Vec<String> {
        self.pending.extend_from_slice(chunk);
        let mut done = Vec::new();
        while let Some(end) = self.pending.iter().position(|&b| b == b'\n') {
            let raw: Vec<u8> = self.pending.drain(..=end).collect();
            let line = String::from_utf8_lossy(&raw);
            let line = line.trim_end_matches(['\n', '\r']);
            if line.is_empty() {
                let data = std::mem::take(&mut self.data).join("\n");
                if !data.trim().is_empty() {
                    done.push(data);
                }
            } else if let Some(value) = line.strip_prefix("data:") {
                self.data
                    .push(value.strip_prefix(' ').unwrap_or(value).to_owned());
            }
        }
        done
    }
}

/// The messages in a JSON-RPC message or batch.
fn messages(msg: &Value) -> std::slice::Iter<'_, Value> {
    match msg {
        Value::Array(batch) => batch.iter(),
        one => std::slice::from_ref(one).iter(),
    }
}

fn is_request(msg: &Value) -> bool {
    msg.get("method").is_some() && msg.get("id").is_some()
}

fn is_initialize(msg: &Value) -> bool {
    msg.get("method").and_then(Value::as_str) == Some("initialize")
}

/// The ids of the requests in a message that wait for an answer.
fn request_ids(msg: &Value) -> Vec<Value> {
    messages(msg)
        .filter(|m| is_request(m))
        .filter_map(|m| m.get("id").cloned())
        .collect()
}

/// The ids of the requests a message answers.
fn response_ids(msg: &Value) -> Vec<Value> {
    messages(msg)
        .filter(|m| m.get("method").is_none() && (m.get("result").is_some() || m.get("error").is_some()))
        .filter_map(|m| m.get("id").cloned())
        .collect()
}

fn session_id(response: &reqwest::Response) -> Option<String> {
    response
        .headers()
        .get(SESSION)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

fn agreed_protocol(answers: &[Value]) -> Option<String> {
    answers.iter().find_map(|m| {
        m.pointer("/result/protocolVersion")
            .and_then(Value::as_str)
            .map(str::to_owned)
    })
}

fn meta_protocol(msg: &Value) -> Option<&str> {
    msg.get("params")?.get("_meta")?.get(META_PROTOCOL)?.as_str()
}

/// The routing headers of the 2026-07-28 protocol (SEP-2243), taken from the message. Servers
/// ignore them for older versions.
fn standard_headers(msg: &Value) -> Vec<(&'static str, String)> {
    let Some(method) = msg.get("method").and_then(Value::as_str) else {
        return Vec::new();
    };
    let mut headers = vec![("Mcp-Method", method.to_owned())];
    let field = match method {
        "tools/call" | "prompts/get" => "name",
        "resources/read" | "resources/subscribe" | "resources/unsubscribe" => "uri",
        "tasks/get" | "tasks/update" | "tasks/cancel" => "taskId",
        _ => return headers,
    };
    if let Some(name) = msg
        .get("params")
        .and_then(|p| p.get(field))
        .and_then(Value::as_str)
    {
        headers.push(("Mcp-Name", header_value(name)));
    }
    headers
}

/// A header value the way SEP-2243 writes it: as is when it is plain ASCII, else in Base64.
fn header_value(value: &str) -> String {
    const PREFIX: &str = "=?base64?";
    const SUFFIX: &str = "?=";
    let plain = !value.starts_with([' ', '\t'])
        && !value.ends_with([' ', '\t'])
        && value.chars().all(|c| (' '..='~').contains(&c))
        && !(value.starts_with(PREFIX) && value.ends_with(SUFFIX));
    if plain {
        value.to_owned()
    } else {
        let encoded = base64::engine::general_purpose::STANDARD.encode(value);
        format!("{PREFIX}{encoded}{SUFFIX}")
    }
}

/// What the app's settings say about the MCP server.
struct Settings {
    enabled: bool,
    port: u16,
    key: Option<String>,
}

/// Reads port and key from redpanda's database and starts redpanda when nothing answers there.
async fn find(dir: PathBuf) -> Result<Target, String> {
    let read = {
        let dir = dir.clone();
        move || read_settings(&dir)
    };
    let mut settings = tokio::task::spawn_blocking(read.clone())
        .await
        .map_err(|e| e.to_string())??;
    if !settings.enabled {
        return Err(OFF.into());
    }
    if !reachable(settings.port).await {
        start_redpanda()?;
        let deadline = Instant::now() + START_TIMEOUT;
        while !reachable(settings.port).await {
            if Instant::now() > deadline {
                return Err(format!(
                    "redpanda did not answer within {} seconds after starting.",
                    START_TIMEOUT.as_secs()
                ));
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        // The app may have settled the key only now.
        settings = tokio::task::spawn_blocking(read)
            .await
            .map_err(|e| e.to_string())??;
    }
    let key = settings.key.ok_or(OFF)?;
    Ok(Target {
        url: super::url(settings.port),
        key,
    })
}

/// Reads the settings without changing anything: the database belongs to the app.
fn read_settings(dir: &Path) -> Result<Settings, String> {
    let path = dir.join(crate::DB_FILES[0]);
    if !path.exists() {
        // redpanda never ran, so access was never switched on.
        return Ok(Settings {
            enabled: false,
            port: super::DEFAULT_PORT,
            key: None,
        });
    }
    let failed = |e: String| format!("could not read redpanda's settings: {e}");
    let conn = rusqlite::Connection::open_with_flags(
        &path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| failed(e.to_string()))?;
    conn.busy_timeout(Duration::from_secs(5))
        .map_err(|e| failed(e.to_string()))?;
    let get = |key: &str| queries::get_setting(&conn, key).map_err(|e| failed(e.to_string()));
    let enabled = get(super::SETTING_ENABLED)?.as_deref() == Some("1");
    let port = super::port_from(get(super::SETTING_PORT)?);
    let key = match get(super::SETTING_KEY)? {
        Some(stored) if enabled => {
            let secrets = Secrets::load_existing().map_err(|e| failed(e.to_string()))?;
            Some(super::decrypt_key(&secrets, &stored).map_err(|e| failed(e.to_string()))?)
        }
        _ => None,
    };
    Ok(Settings { enabled, port, key })
}

async fn reachable(port: u16) -> bool {
    matches!(
        tokio::time::timeout(
            Duration::from_secs(1),
            tokio::net::TcpStream::connect(("127.0.0.1", port))
        )
        .await,
        Ok(Ok(_))
    )
}

/// Starts redpanda in the notification area, apart from Claude Desktop: without its pipes and,
/// where Windows allows it, outside its job, so redpanda keeps running when Claude Desktop quits.
/// It works in its own folder, not Claude Desktop's, which an update of Claude Desktop replaces.
fn start_redpanda() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("could not start redpanda: {e}"))?;
    tracing::info!("redpanda is not running, starting it");
    let spawn = |flags: u32| {
        let mut command = std::process::Command::new(&exe);
        if let Some(folder) = exe.parent() {
            command.current_dir(folder);
        }
        command
            .arg(crate::BACKGROUND_ARG)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(flags);
        }
        #[cfg(not(windows))]
        let _ = flags;
        command.spawn().map(drop)
    };
    #[cfg(windows)]
    keep_pipes_to_self();
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    let apart = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
    spawn(apart | CREATE_BREAKAWAY_FROM_JOB)
        .or_else(|e| {
            tracing::info!("could not leave Claude Desktop's job ({e}), redpanda ends with it");
            spawn(apart)
        })
        .map_err(|e| format!("could not start redpanda: {e}"))
}

/// Claude Desktop's pipes are inheritable. redpanda would get them too and keep them open after
/// the bridge ends, so Claude Desktop would wait for an end that never comes.
#[cfg(windows)]
fn keep_pipes_to_self() {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::{SetHandleInformation, HANDLE, HANDLE_FLAGS, HANDLE_FLAG_INHERIT};
    let handles = [
        std::io::stdin().as_raw_handle(),
        std::io::stdout().as_raw_handle(),
        std::io::stderr().as_raw_handle(),
    ];
    for handle in handles {
        if !handle.is_null() {
            // SAFETY: the process's own standard handles; only their inherit flag changes.
            let _ = unsafe { SetHandleInformation(HANDLE(handle), HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0)) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_survive_any_split() {
        let stream = b"id: 0\ndata:\n\nevent: message\r\ndata: {\"id\":1,\r\ndata: \"result\":{}}\r\n\r\n: comment\ndata: {\"id\":2}\n\n";
        for size in 1..stream.len() {
            let mut events = Events::default();
            let got: Vec<String> = stream.chunks(size).flat_map(|c| events.feed(c)).collect();
            assert_eq!(
                got,
                vec!["{\"id\":1,\n\"result\":{}}", "{\"id\":2}"],
                "chunks of {size}"
            );
        }
    }

    #[test]
    fn requests_and_answers_by_id() {
        let request = json!({"jsonrpc": "2.0", "id": 7, "method": "tools/list"});
        let notification = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        let answer = json!({"jsonrpc": "2.0", "id": 7, "result": {}});
        let failure = json!({"jsonrpc": "2.0", "id": "a", "error": {"code": 1, "message": "x"}});
        assert_eq!(request_ids(&request), vec![json!(7)]);
        assert!(request_ids(&notification).is_empty());
        assert!(request_ids(&answer).is_empty());
        assert_eq!(response_ids(&answer), vec![json!(7)]);
        assert_eq!(
            response_ids(&json!([answer, failure, request])),
            vec![json!(7), json!("a")]
        );
        assert!(is_request(&request) && !is_request(&notification));
    }

    #[test]
    fn routing_headers_follow_the_message() {
        let call = json!({"method": "tools/call", "params": {"name": "read_message", "_meta": {META_PROTOCOL: "2026-07-28"}}});
        assert_eq!(
            standard_headers(&call),
            vec![
                ("Mcp-Method", "tools/call".into()),
                ("Mcp-Name", "read_message".into())
            ]
        );
        assert_eq!(meta_protocol(&call), Some("2026-07-28"));
        assert_eq!(
            standard_headers(&json!({"method": "tools/list"})),
            vec![("Mcp-Method", "tools/list".into())]
        );
        assert!(standard_headers(&json!({"id": 1, "result": {}})).is_empty());
        assert_eq!(header_value("ab c"), "ab c");
        assert_eq!(header_value("Grüße"), "=?base64?R3LDvMOfZQ==?=");
        assert_eq!(header_value(" x"), "=?base64?IHg=?=");
    }
}
