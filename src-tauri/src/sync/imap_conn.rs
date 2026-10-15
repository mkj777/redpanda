//! Connecting and logging in to IMAP servers over TLS, STARTTLS or (local only) plain TCP.

use std::fmt::Debug;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use async_imap::types::Mailbox;
use futures::TryStreamExt;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use tokio_rustls::TlsConnector;

use crate::accounts::{self, LoginSecret};
use crate::core::Core;
use crate::error::{AppError, Result};
use crate::model::{Account, Security, ServerConfig};

pub trait ImapIo: AsyncRead + AsyncWrite + Unpin + Send + Debug {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + Debug> ImapIo for T {}

pub type Stream = Box<dyn ImapIo>;
pub type Session = async_imap::Session<Stream>;
pub type Client = async_imap::Client<Stream>;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

pub fn tls_config() -> Arc<ClientConfig> {
    static CONFIG: OnceLock<Arc<ClientConfig>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let roots = RootCertStore {
                roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
            };
            Arc::new(
                ClientConfig::builder_with_provider(Arc::new(
                    tokio_rustls::rustls::crypto::ring::default_provider(),
                ))
                .with_safe_default_protocol_versions()
                .expect("TLS versions")
                .with_root_certificates(roots)
                .with_no_client_auth(),
            )
        })
        .clone()
}

async fn tls_wrap(tcp: TcpStream, host: &str) -> Result<Stream> {
    let name = ServerName::try_from(host.to_string())
        .map_err(|_| AppError::Invalid(format!("invalid host name: {host}")))?;
    let tls = timeout(
        CONNECT_TIMEOUT,
        TlsConnector::from(tls_config()).connect(name, tcp),
    )
    .await?
    .map_err(|e| AppError::Network(format!("TLS: {e}")))?;
    Ok(Box::new(tls))
}

async fn read_greeting<T: ImapIo>(client: &mut async_imap::Client<T>) -> Result<()> {
    match timeout(CONNECT_TIMEOUT, client.read_response()).await? {
        Some(Ok(_)) => Ok(()),
        Some(Err(e)) => Err(e.into()),
        None => Err(AppError::Network("server closed the connection".into())),
    }
}

pub async fn connect(cfg: &ServerConfig) -> Result<Client> {
    let tcp = timeout(CONNECT_TIMEOUT, TcpStream::connect((cfg.host.as_str(), cfg.port))).await??;
    tcp.set_nodelay(true).ok();
    match cfg.security {
        Security::Tls => {
            let mut client = Client::new(tls_wrap(tcp, &cfg.host).await?);
            read_greeting(&mut client).await?;
            Ok(client)
        }
        Security::StartTls => {
            let mut plain = async_imap::Client::new(tcp);
            read_greeting(&mut plain).await?;
            plain.run_command_and_check_ok("STARTTLS", None).await?;
            // The server sends no new greeting after the handshake.
            Ok(Client::new(tls_wrap(plain.into_inner(), &cfg.host).await?))
        }
        Security::None => {
            let mut client = Client::new(Box::new(tcp));
            read_greeting(&mut client).await?;
            Ok(client)
        }
    }
}

struct XOAuth2 {
    payload: String,
    calls: u32,
}

impl async_imap::Authenticator for XOAuth2 {
    type Response = String;

    fn process(&mut self, _challenge: &[u8]) -> String {
        self.calls += 1;
        // A second challenge carries the error details; answering with an empty line makes
        // the server finish with NO instead of waiting.
        if self.calls == 1 {
            self.payload.clone()
        } else {
            String::new()
        }
    }
}

fn login_error(e: async_imap::error::Error) -> AppError {
    use async_imap::error::Error as E;
    match e {
        E::No(msg) | E::Bad(msg) => AppError::Auth(msg),
        other => other.into(),
    }
}

pub async fn login(client: Client, user: &str, secret: &LoginSecret) -> Result<Session> {
    let fut = async {
        match secret {
            LoginSecret::Password(p) => client.login(user, p).await.map_err(|(e, _)| login_error(e)),
            LoginSecret::XOAuth2(token) => client
                .authenticate(
                    "XOAUTH2",
                    XOAuth2 {
                        payload: accounts::oauth::xoauth2_payload(user, token),
                        calls: 0,
                    },
                )
                .await
                .map_err(|(e, _)| login_error(e)),
        }
    };
    timeout(CONNECT_TIMEOUT, fut).await?
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Caps {
    pub idle: bool,
    pub condstore: bool,
    pub move_: bool,
    pub uidplus: bool,
}

pub async fn capabilities(session: &mut Session) -> Result<Caps> {
    let caps = session.capabilities().await?;
    Ok(Caps {
        idle: caps.has_str("IDLE"),
        condstore: caps.has_str("CONDSTORE") || caps.has_str("QRESYNC"),
        move_: caps.has_str("MOVE"),
        uidplus: caps.has_str("UIDPLUS"),
    })
}

/// Connects and logs in with the stored credentials. A rejected OAuth token is refreshed once.
pub async fn open_session(core: &Core, account: &Account) -> Result<(Session, Caps)> {
    let secret = accounts::login_secret(core, account).await?;
    let client = connect(&account.imap).await?;
    let mut session = match login(client, &account.username, &secret).await {
        Ok(s) => s,
        Err(AppError::Auth(msg)) if matches!(secret, LoginSecret::XOAuth2(_)) => {
            tracing::info!(account = %account.id, "token rejected, refreshing: {msg}");
            accounts::invalidate_access_token(core, &account.id)?;
            let secret = accounts::login_secret(core, account).await?;
            let client = connect(&account.imap).await?;
            login(client, &account.username, &secret).await.map_err(|e| {
                if e.is_auth_error() {
                    AppError::AuthRequired
                } else {
                    e
                }
            })?
        }
        Err(e) => return Err(e),
    };
    let caps = capabilities(&mut session).await?;
    Ok((session, caps))
}

/// Logs in with explicit credentials to check them before an account is saved.
pub async fn verify_login(cfg: &ServerConfig, user: &str, secret: &LoginSecret) -> Result<()> {
    let client = connect(cfg).await?;
    let mut session = login(client, user, secret).await?;
    let _ = session.logout().await;
    Ok(())
}

pub async fn select(session: &mut Session, path: &str, condstore: bool) -> Result<Mailbox> {
    let mb = if condstore {
        session.select_condstore(path).await?
    } else {
        session.select(path).await?
    };
    drain_unsolicited(session);
    Ok(mb)
}

pub fn drain_unsolicited(session: &mut Session) {
    while session.unsolicited_responses.try_recv().is_ok() {}
}

/// Collects a FETCH/STORE response stream fully so the connection is ready for the next command.
pub async fn collect<S, T>(stream: S) -> Result<Vec<T>>
where
    S: futures::Stream<Item = async_imap::error::Result<T>>,
{
    Ok(stream.try_collect::<Vec<T>>().await?)
}

/// Formats UIDs as a compact IMAP sequence set, e.g. `1:3,7,9:12`.
pub fn uid_set(uids: &[u32]) -> String {
    let mut sorted: Vec<u32> = uids.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let mut parts = Vec::new();
    let mut i = 0;
    while i < sorted.len() {
        let start = sorted[i];
        let mut end = start;
        while i + 1 < sorted.len() && sorted[i + 1] == end + 1 {
            i += 1;
            end = sorted[i];
        }
        parts.push(if start == end {
            start.to_string()
        } else {
            format!("{start}:{end}")
        });
        i += 1;
    }
    parts.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_uid_sets() {
        assert_eq!(uid_set(&[3, 1, 2, 7, 9, 10, 11, 11]), "1:3,7,9:11");
        assert_eq!(uid_set(&[5]), "5");
        assert_eq!(uid_set(&[]), "");
    }
}
