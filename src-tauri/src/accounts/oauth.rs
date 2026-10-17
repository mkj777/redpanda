//! OAuth 2.0 authorization code flow with PKCE, using the system browser and a loopback
//! redirect on 127.0.0.1. Release builds ship the project's client IDs, local config can
//! replace them (see ADR 0004).

use std::path::PathBuf;
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use crate::error::{AppError, Result};
use crate::model::AuthType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OAuthProvider {
    Google,
    Microsoft,
}

impl OAuthProvider {
    pub fn from_auth_type(t: AuthType) -> Option<OAuthProvider> {
        match t {
            AuthType::OauthGoogle => Some(OAuthProvider::Google),
            AuthType::OauthMicrosoft => Some(OAuthProvider::Microsoft),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            OAuthProvider::Google => "Google",
            OAuthProvider::Microsoft => "Microsoft",
        }
    }

    fn auth_endpoint(self) -> &'static str {
        match self {
            OAuthProvider::Google => "https://accounts.google.com/o/oauth2/v2/auth",
            OAuthProvider::Microsoft => "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
        }
    }

    fn token_endpoint(self) -> &'static str {
        match self {
            OAuthProvider::Google => "https://oauth2.googleapis.com/token",
            OAuthProvider::Microsoft => "https://login.microsoftonline.com/common/oauth2/v2.0/token",
        }
    }

    fn scopes(self) -> &'static str {
        match self {
            // No `openid email`: Google treats that as "Sign in with Google" and mails the user a
            // profile sharing notice. The address comes from the Gmail profile instead.
            OAuthProvider::Google => "https://mail.google.com/",
            OAuthProvider::Microsoft => {
                "https://outlook.office.com/IMAP.AccessAsUser.All https://outlook.office.com/SMTP.Send offline_access openid email"
            }
        }
    }

    /// Google accepts `127.0.0.1` for desktop clients, Microsoft expects `localhost` (any port).
    fn redirect_uri(self, port: u16) -> String {
        match self {
            OAuthProvider::Google => format!("http://127.0.0.1:{port}"),
            OAuthProvider::Microsoft => format!("http://localhost:{port}"),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ClientCreds {
    pub client_id: String,
    #[serde(default)]
    pub client_secret: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct OAuthConfig {
    #[serde(default)]
    pub google: Option<ClientCreds>,
    #[serde(default)]
    pub microsoft: Option<ClientCreds>,
}

impl OAuthConfig {
    pub fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("redpanda").join("oauth.json"))
    }

    /// Client IDs compiled into release builds from CI secrets (see ADR 0004).
    fn bundled() -> OAuthConfig {
        OAuthConfig {
            google: creds(
                option_env!("REDPANDA_GOOGLE_CLIENT_ID"),
                option_env!("REDPANDA_GOOGLE_CLIENT_SECRET"),
            ),
            microsoft: creds(option_env!("REDPANDA_MICROSOFT_CLIENT_ID"), None),
        }
    }

    /// Environment variables win over `%APPDATA%\redpanda\oauth.json`, which wins over the
    /// IDs bundled at build time.
    pub fn load() -> OAuthConfig {
        let file: OAuthConfig = Self::config_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let env = |k: &str| std::env::var(k).ok();
        let runtime = OAuthConfig {
            google: creds(
                env("REDPANDA_GOOGLE_CLIENT_ID").as_deref(),
                env("REDPANDA_GOOGLE_CLIENT_SECRET").as_deref(),
            ),
            microsoft: creds(env("REDPANDA_MICROSOFT_CLIENT_ID").as_deref(), None),
        };
        Self::merge([runtime, file, Self::bundled()])
    }

    /// Takes each provider from the first source that has a real client ID.
    fn merge(sources: [OAuthConfig; 3]) -> OAuthConfig {
        let [a, b, c] = sources;
        let pick = |list: [Option<ClientCreds>; 3]| list.into_iter().flatten().find(is_real);
        OAuthConfig {
            google: pick([a.google, b.google, c.google]),
            microsoft: pick([a.microsoft, b.microsoft, c.microsoft]),
        }
    }

    pub fn creds(&self, p: OAuthProvider) -> Result<&ClientCreds> {
        match p {
            OAuthProvider::Google => self.google.as_ref(),
            OAuthProvider::Microsoft => self.microsoft.as_ref(),
        }
        .ok_or_else(|| AppError::OAuthNotConfigured(p.name().into()))
    }
}

fn creds(id: Option<&str>, secret: Option<&str>) -> Option<ClientCreds> {
    let non_empty = |v: Option<&str>| v.map(str::trim).filter(|v| !v.is_empty()).map(String::from);
    Some(ClientCreds {
        client_id: non_empty(id)?,
        client_secret: non_empty(secret),
    })
}

/// Treats empty IDs and the placeholders from oauth.example.json as missing.
fn is_real(c: &ClientCreds) -> bool {
    let id = c.client_id.trim();
    !id.is_empty() && !id.starts_with("YOUR_") && !id.starts_with("00000000-")
}

pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

pub fn pkce() -> Pkce {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let verifier = URL_SAFE_NO_PAD.encode(bytes);
    Pkce {
        challenge: pkce_challenge(&verifier),
        verifier,
    }
}

pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn random_state() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn authorization_url(
    provider: OAuthProvider,
    client_id: &str,
    redirect_uri: &str,
    challenge: &str,
    state: &str,
    login_hint: Option<&str>,
) -> String {
    let mut url = url::Url::parse(provider.auth_endpoint()).expect("static URL");
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("client_id", client_id)
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("response_type", "code")
            .append_pair("scope", provider.scopes())
            .append_pair("code_challenge", challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", state);
        match provider {
            // Without these Google does not return a refresh token on repeated sign-ins.
            OAuthProvider::Google => {
                q.append_pair("access_type", "offline")
                    .append_pair("prompt", "consent");
            }
            OAuthProvider::Microsoft => {
                q.append_pair("prompt", "select_account");
            }
        }
        if let Some(hint) = login_hint.filter(|h| !h.is_empty()) {
            q.append_pair("login_hint", hint);
        }
    }
    url.into()
}

#[derive(Debug, Clone)]
pub struct TokenSet {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: i64,
    pub email: Option<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    id_token: Option<String>,
}

#[derive(Deserialize)]
struct TokenError {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

/// Reads the email claim from an ID token. The token comes straight from the provider's token
/// endpoint over TLS, so the signature is not checked here.
pub fn id_token_email(id_token: &str) -> Option<String> {
    let payload = id_token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    ["email", "preferred_username", "upn"]
        .iter()
        .filter_map(|k| claims.get(*k).and_then(|v| v.as_str()))
        .find(|v| v.contains('@'))
        .map(|s| s.to_ascii_lowercase())
}

async fn token_request(
    http: &reqwest::Client,
    provider: OAuthProvider,
    form: &[(&str, &str)],
) -> Result<TokenSet> {
    let resp = http
        .post(provider.token_endpoint())
        .form(form)
        .timeout(Duration::from_secs(30))
        .send()
        .await?;
    let status = resp.status();
    let body = resp.text().await?;
    if !status.is_success() {
        let msg = serde_json::from_str::<TokenError>(&body)
            .map(|e| format!("{}: {}", e.error, e.error_description.unwrap_or_default()))
            .unwrap_or_else(|_| format!("HTTP {status}"));
        return Err(AppError::Auth(msg));
    }
    let t: TokenResponse = serde_json::from_str(&body)?;
    Ok(TokenSet {
        email: t.id_token.as_deref().and_then(id_token_email),
        access_token: t.access_token,
        refresh_token: t.refresh_token,
        expires_at: crate::store::now() + t.expires_in.unwrap_or(3600),
    })
}

pub async fn refresh(
    http: &reqwest::Client,
    provider: OAuthProvider,
    creds: &ClientCreds,
    refresh_token: &str,
) -> Result<TokenSet> {
    let mut form = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", creds.client_id.as_str()),
    ];
    if let Some(secret) = &creds.client_secret {
        form.push(("client_secret", secret.as_str()));
    }
    if provider == OAuthProvider::Microsoft {
        form.push(("scope", provider.scopes()));
    }
    token_request(http, provider, &form).await
}

const PAGE_HEAD: &str = r#"<!doctype html><meta charset="utf-8"><title>redpanda</title>
<style>
body{margin:0;font:15px/1.5 system-ui,sans-serif;display:grid;place-items:center;height:100vh;background:#faf8f5;color:#292524;text-align:center}
@media(prefers-color-scheme:dark){body{background:#161412;color:#e7e5e4}}
svg{width:88px;height:88px;margin-bottom:12px}
p{margin:4px 0}
</style>"#;

/// The app icon, inlined so the loopback page needs no second request.
const ICON: &str = include_str!("../../../public/icon.svg");

fn result_page(ok: bool) -> String {
    let body = if ok {
        "<p><b>Signed in.</b> You can close this tab and return to redpanda.</p><p><b>Angemeldet.</b> Du kannst diesen Tab schließen und zu redpanda zurückkehren.</p>"
    } else {
        "<p><b>Sign-in failed.</b> Please return to redpanda.</p><p><b>Anmeldung fehlgeschlagen.</b> Bitte kehre zu redpanda zurück.</p>"
    };
    format!("{PAGE_HEAD}<div>{ICON}{body}</div>")
}

/// Runs the full browser sign-in. `open` is called with the authorization URL.
pub async fn authorize(
    http: &reqwest::Client,
    config: &OAuthConfig,
    provider: OAuthProvider,
    login_hint: Option<&str>,
    open: impl FnOnce(&str) -> Result<()>,
    cancel: oneshot::Receiver<()>,
) -> Result<TokenSet> {
    let creds = config.creds(provider)?.clone();
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = listener.local_addr()?.port();
    let redirect_uri = provider.redirect_uri(port);
    let pkce = pkce();
    let state = random_state();
    let url = authorization_url(
        provider,
        &creds.client_id,
        &redirect_uri,
        &pkce.challenge,
        &state,
        login_hint,
    );
    open(&url)?;

    let code = tokio::select! {
        r = wait_for_code(&listener, &state) => r?,
        _ = cancel => return Err(AppError::Cancelled),
        _ = tokio::time::sleep(Duration::from_secs(600)) => return Err(AppError::Cancelled),
    };

    let mut form = vec![
        ("grant_type", "authorization_code"),
        ("code", code.as_str()),
        ("redirect_uri", redirect_uri.as_str()),
        ("client_id", creds.client_id.as_str()),
        ("code_verifier", pkce.verifier.as_str()),
    ];
    if let Some(secret) = &creds.client_secret {
        form.push(("client_secret", secret.as_str()));
    }
    let mut tokens = token_request(http, provider, &form).await?;
    if provider == OAuthProvider::Google && tokens.email.is_none() {
        tokens.email = gmail_address(http, &tokens.access_token).await;
    }
    if tokens.refresh_token.is_none() {
        return Err(AppError::Auth(
            "the provider did not return a refresh token".into(),
        ));
    }
    Ok(tokens)
}

/// The address of the signed-in Gmail account. Best effort: without it the typed address is used.
async fn gmail_address(http: &reqwest::Client, access_token: &str) -> Option<String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Profile {
        email_address: String,
    }
    let resp = http
        .get("https://gmail.googleapis.com/gmail/v1/users/me/profile")
        .bearer_auth(access_token)
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        tracing::warn!(status = %resp.status(), "could not read the Gmail profile");
        return None;
    }
    let profile: Profile = resp.json().await.ok()?;
    Some(profile.email_address.to_ascii_lowercase())
}

/// Accepts loopback connections until one carries the authorization response.
async fn wait_for_code(listener: &TcpListener, expected_state: &str) -> Result<String> {
    loop {
        let (mut stream, _) = listener.accept().await?;
        let mut buf = vec![0u8; 8192];
        let mut len = 0;
        // Read until the end of the request head.
        loop {
            let n = tokio::time::timeout(Duration::from_secs(10), stream.read(&mut buf[len..]))
                .await
                .unwrap_or(Ok(0))?;
            if n == 0 {
                break;
            }
            len += n;
            if buf[..len].windows(4).any(|w| w == b"\r\n\r\n") || len == buf.len() {
                break;
            }
        }
        let head = String::from_utf8_lossy(&buf[..len]);
        let target = head
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .unwrap_or("/");
        let parsed = url::Url::parse(&format!("http://localhost{target}")).ok();
        let param = |k: &str| {
            parsed.as_ref().and_then(|u| {
                u.query_pairs()
                    .find(|(key, _)| key == k)
                    .map(|(_, v)| v.into_owned())
            })
        };

        let (code, error) = (param("code"), param("error"));
        if code.is_none() && error.is_none() {
            // Favicon or other noise.
            let _ = stream
                .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await;
            continue;
        }
        let ok = code.is_some() && param("state").as_deref() == Some(expected_state);
        let page = result_page(ok);
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
            page.len()
        );
        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.shutdown().await;

        if let Some(err) = error {
            return Err(if err == "access_denied" {
                AppError::Cancelled
            } else {
                AppError::Auth(err)
            });
        }
        if !ok {
            return Err(AppError::Auth("state mismatch".into()));
        }
        return Ok(code.unwrap_or_default());
    }
}

/// SASL XOAUTH2 initial response for IMAP and SMTP.
pub fn xoauth2_payload(user: &str, access_token: &str) -> String {
    format!("user={user}\x01auth=Bearer {access_token}\x01\x01")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_matches_rfc7636_example() {
        // Appendix B of RFC 7636.
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let p = pkce();
        assert_eq!(p.verifier.len(), 43);
        assert_eq!(pkce_challenge(&p.verifier), p.challenge);
    }

    #[test]
    fn merges_sources_in_order() {
        let cfg = |g: Option<&str>, m: Option<&str>| OAuthConfig {
            google: creds(g, Some("secret")),
            microsoft: creds(m, None),
        };
        let merged = OAuthConfig::merge([
            cfg(None, Some(" ")),
            cfg(Some("YOUR_CLIENT_ID.apps.googleusercontent.com"), Some("file-ms")),
            cfg(Some("bundled-g"), Some("bundled-ms")),
        ]);
        let google = merged.google.unwrap();
        assert_eq!(google.client_id, "bundled-g");
        assert_eq!(google.client_secret.as_deref(), Some("secret"));
        assert_eq!(merged.microsoft.unwrap().client_id, "file-ms");

        let empty = OAuthConfig::merge([cfg(None, None), cfg(None, None), cfg(Some(""), None)]);
        assert!(empty.google.is_none() && empty.microsoft.is_none());
        assert!(creds(Some("id"), Some("")).unwrap().client_secret.is_none());
    }

    #[test]
    fn builds_authorization_url() {
        let url = authorization_url(
            OAuthProvider::Google,
            "cid",
            "http://127.0.0.1:5555",
            "chal",
            "st",
            Some("me@gmail.com"),
        );
        let u = url::Url::parse(&url).unwrap();
        assert_eq!(u.host_str(), Some("accounts.google.com"));
        let q: std::collections::HashMap<_, _> = u.query_pairs().into_owned().collect();
        assert_eq!(q["client_id"], "cid");
        assert_eq!(q["redirect_uri"], "http://127.0.0.1:5555");
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["scope"], "https://mail.google.com/");
        assert_eq!(q["access_type"], "offline");
        assert_eq!(q["login_hint"], "me@gmail.com");

        let ms = authorization_url(
            OAuthProvider::Microsoft,
            "c",
            "http://localhost:1",
            "x",
            "s",
            None,
        );
        assert!(ms.contains("IMAP.AccessAsUser.All"));
        assert!(ms.contains("offline_access"));
        assert!(!ms.contains("login_hint"));
    }

    #[test]
    fn reads_email_from_id_token() {
        let payload = URL_SAFE_NO_PAD.encode(br#"{"preferred_username":"Max@Outlook.com","sub":"1"}"#);
        let token = format!("eyJhbGciOiJub25lIn0.{payload}.sig");
        assert_eq!(id_token_email(&token).as_deref(), Some("max@outlook.com"));
        assert_eq!(id_token_email("garbage"), None);
    }

    #[test]
    fn xoauth2_format() {
        assert_eq!(
            xoauth2_payload("a@b.c", "tok"),
            "user=a@b.c\u{1}auth=Bearer tok\u{1}\u{1}"
        );
    }

    #[tokio::test]
    async fn loopback_receives_code_and_checks_state() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move { wait_for_code(&listener, "good").await });

        let send = |path: String| async move {
            let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            s.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
                .await
                .unwrap();
            let mut out = String::new();
            s.read_to_string(&mut out).await.unwrap();
            out
        };
        let favicon = send("/favicon.ico".into()).await;
        assert!(favicon.starts_with("HTTP/1.1 404"));
        let page = send("/?code=abc&state=good".into()).await;
        assert!(page.contains("Signed in"));
        assert_eq!(server.await.unwrap().unwrap(), "abc");

        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port2 = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move { wait_for_code(&listener, "good").await });
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port2))
            .await
            .unwrap();
        s.write_all(b"GET /?code=abc&state=evil HTTP/1.1\r\n\r\n")
            .await
            .unwrap();
        assert!(matches!(server.await.unwrap(), Err(AppError::Auth(_))));
    }
}
