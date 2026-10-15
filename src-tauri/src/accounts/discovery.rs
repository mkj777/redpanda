//! Finds server settings for an email address. The chain stops at the first hit:
//! provider table, MX lookup, Mozilla ISPDB / autoconfig, RFC 6186 SRV records, guessing.

use std::time::Duration;

use hickory_resolver::proto::rr::RecordType;
use hickory_resolver::TokioResolver;
use quick_xml::events::Event;
use quick_xml::Reader;
use serde::Serialize;
use tokio::net::TcpStream;
use tokio::time::timeout;

use super::providers::{self, Provider};
use crate::model::{AuthType, Security, ServerConfig};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Provider,
    Mx,
    Autoconfig,
    Srv,
    Guess,
    None,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryResult {
    pub source: Source,
    pub provider_id: Option<String>,
    pub provider_name: Option<String>,
    pub auth_type: AuthType,
    pub allow_password: bool,
    pub imap: Option<ServerConfig>,
    pub smtp: Option<ServerConfig>,
    pub username: String,
    pub hint: Option<String>,
    pub sent_saved_by_server: bool,
    pub is_gmail: bool,
}

impl DiscoveryResult {
    fn from_provider(p: &Provider, email: &str, source: Source) -> Self {
        DiscoveryResult {
            source,
            provider_id: Some(p.id.into()),
            provider_name: Some(p.name.into()),
            auth_type: p.auth,
            allow_password: p.allow_password,
            imap: Some(p.imap_config()),
            smtp: Some(p.smtp_config()),
            username: p.username_for(email),
            hint: p.hint.map(str::to_string),
            sent_saved_by_server: p.sent_saved_by_server,
            is_gmail: p.is_gmail,
        }
    }

    fn servers(source: Source, imap: ServerConfig, smtp: Option<ServerConfig>, username: String) -> Self {
        DiscoveryResult {
            source,
            provider_id: None,
            provider_name: None,
            auth_type: AuthType::Password,
            allow_password: true,
            imap: Some(imap),
            smtp,
            username,
            hint: None,
            sent_saved_by_server: false,
            is_gmail: false,
        }
    }

    fn none(email: &str) -> Self {
        DiscoveryResult {
            source: Source::None,
            provider_id: None,
            provider_name: None,
            auth_type: AuthType::Password,
            allow_password: true,
            imap: None,
            smtp: None,
            username: email.to_string(),
            hint: None,
            sent_saved_by_server: false,
            is_gmail: false,
        }
    }
}

pub fn split_email(email: &str) -> Option<(&str, &str)> {
    let email = email.trim();
    let (local, domain) = email.rsplit_once('@')?;
    if local.is_empty() || domain.is_empty() || !domain.contains('.') {
        return None;
    }
    Some((local, domain))
}

pub async fn discover(http: &reqwest::Client, email: &str) -> DiscoveryResult {
    let email = email.trim();
    let Some((_, domain)) = split_email(email) else {
        return DiscoveryResult::none(email);
    };
    let domain = domain.to_ascii_lowercase();

    if let Some(p) = providers::by_domain(&domain) {
        return DiscoveryResult::from_provider(p, email, Source::Provider);
    }

    let resolver = resolver();
    if let Some(resolver) = &resolver {
        let mx = mx_hosts(resolver, &domain).await;
        if let Some(p) = providers::by_mx(&mx) {
            return DiscoveryResult::from_provider(p, email, Source::Mx);
        }
    }

    if let Some(found) = autoconfig(http, &domain, email).await {
        if let Some(p) = found.imap.as_ref().and_then(|i| providers::by_imap_host(&i.host)) {
            return DiscoveryResult::from_provider(p, email, Source::Autoconfig);
        }
        return found;
    }

    if let Some(resolver) = &resolver {
        if let Some(found) = srv(resolver, &domain, email).await {
            return found;
        }
    }

    if let Some(found) = guess(&domain, email).await {
        return found;
    }

    DiscoveryResult::none(email)
}

fn resolver() -> Option<TokioResolver> {
    let mut builder = TokioResolver::builder_tokio().ok()?;
    builder.options_mut().timeout = Duration::from_secs(3);
    builder.options_mut().attempts = 1;
    Some(builder.build())
}

async fn mx_hosts(resolver: &TokioResolver, domain: &str) -> Vec<String> {
    match timeout(Duration::from_secs(5), resolver.mx_lookup(domain)).await {
        Ok(Ok(lookup)) => {
            let mut records: Vec<_> = lookup
                .iter()
                .map(|mx| (mx.preference(), mx.exchange().to_utf8()))
                .collect();
            records.sort();
            records.into_iter().map(|(_, h)| h).collect()
        }
        _ => vec![],
    }
}

async fn autoconfig(http: &reqwest::Client, domain: &str, email: &str) -> Option<DiscoveryResult> {
    let urls = [
        format!("https://autoconfig.thunderbird.net/v1.1/{domain}"),
        format!("https://autoconfig.{domain}/mail/config-v1.1.xml?emailaddress={email}"),
        format!("https://{domain}/.well-known/autoconfig/mail/config-v1.1.xml"),
    ];
    for url in urls {
        let resp = match http.get(&url).timeout(Duration::from_secs(6)).send().await {
            Ok(r) if r.status().is_success() => r,
            _ => continue,
        };
        let Ok(body) = resp.text().await else { continue };
        if let Some(found) = parse_autoconfig(&body, email) {
            return Some(found);
        }
    }
    None
}

#[derive(Default, Debug)]
struct XmlServer {
    kind: String,
    hostname: String,
    port: u16,
    socket: String,
    username: String,
}

/// Parses a Thunderbird autoconfig document (`clientConfig` version 1.1).
pub fn parse_autoconfig(xml: &str, email: &str) -> Option<DiscoveryResult> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut incoming: Vec<XmlServer> = vec![];
    let mut outgoing: Vec<XmlServer> = vec![];
    let mut current: Option<(bool, XmlServer)> = None;
    let mut field = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                if name == "incomingServer" || name == "outgoingServer" {
                    let kind = e
                        .attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == b"type")
                        .map(|a| String::from_utf8_lossy(&a.value).to_string())
                        .unwrap_or_default();
                    current = Some((
                        name == "incomingServer",
                        XmlServer {
                            kind,
                            ..Default::default()
                        },
                    ));
                }
                field = name;
            }
            Ok(Event::Text(t)) => {
                if let Some((_, server)) = current.as_mut() {
                    let text = t.unescape().map(|c| c.to_string()).unwrap_or_default();
                    match field.as_str() {
                        "hostname" => server.hostname = text,
                        "port" => server.port = text.parse().unwrap_or(0),
                        "socketType" => server.socket = text,
                        "username" => server.username = text,
                        _ => {}
                    }
                }
            }
            Ok(Event::End(e)) => {
                let name = e.local_name();
                if name.as_ref() == b"incomingServer" || name.as_ref() == b"outgoingServer" {
                    if let Some((is_in, server)) = current.take() {
                        if is_in {
                            incoming.push(server);
                        } else {
                            outgoing.push(server);
                        }
                    }
                }
                field.clear();
            }
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
    }

    let pick = |list: &[XmlServer], kind: &str| -> Option<(ServerConfig, String)> {
        let mut candidates: Vec<&XmlServer> = list
            .iter()
            .filter(|s| s.kind == kind && !s.hostname.is_empty() && s.port > 0)
            .collect();
        // Prefer implicit TLS, then STARTTLS, never plain.
        candidates.sort_by_key(|s| match s.socket.to_ascii_uppercase().as_str() {
            "SSL" | "TLS" => 0,
            "STARTTLS" => 1,
            _ => 2,
        });
        let s = candidates
            .into_iter()
            .find(|s| socket_security(&s.socket).is_some())?;
        Some((
            ServerConfig {
                host: expand(&s.hostname, email),
                port: s.port,
                security: socket_security(&s.socket)?,
            },
            expand(&s.username, email),
        ))
    };

    let (imap, username) = pick(&incoming, "imap")?;
    let smtp = pick(&outgoing, "smtp").map(|(cfg, _)| cfg);
    let username = if username.is_empty() {
        email.to_string()
    } else {
        username
    };
    Some(DiscoveryResult::servers(Source::Autoconfig, imap, smtp, username))
}

fn socket_security(s: &str) -> Option<Security> {
    match s.to_ascii_uppercase().as_str() {
        "SSL" | "TLS" => Some(Security::Tls),
        "STARTTLS" => Some(Security::StartTls),
        _ => None,
    }
}

fn expand(template: &str, email: &str) -> String {
    let (local, domain) = split_email(email).unwrap_or((email, ""));
    template
        .replace("%EMAILADDRESS%", email)
        .replace("%EMAILLOCALPART%", local)
        .replace("%EMAILDOMAIN%", domain)
}

async fn srv_record(resolver: &TokioResolver, name: &str) -> Option<(String, u16)> {
    let lookup = timeout(Duration::from_secs(4), resolver.lookup(name, RecordType::SRV))
        .await
        .ok()?
        .ok()?;
    let mut records: Vec<_> = lookup
        .record_iter()
        .filter_map(|r| r.data().as_srv().cloned())
        .collect();
    records.sort_by_key(|r| (r.priority(), std::cmp::Reverse(r.weight())));
    let r = records.into_iter().next()?;
    let target = r.target().to_utf8();
    let target = target.trim_end_matches('.');
    // A target of "." means the service is explicitly not available.
    if target.is_empty() || r.port() == 0 {
        return None;
    }
    Some((target.to_string(), r.port()))
}

async fn srv(resolver: &TokioResolver, domain: &str, email: &str) -> Option<DiscoveryResult> {
    let imap = match srv_record(resolver, &format!("_imaps._tcp.{domain}.")).await {
        Some((host, port)) => ServerConfig {
            host,
            port,
            security: Security::Tls,
        },
        None => {
            let (host, port) = srv_record(resolver, &format!("_imap._tcp.{domain}.")).await?;
            ServerConfig {
                host,
                port,
                security: Security::StartTls,
            }
        }
    };
    let smtp = match srv_record(resolver, &format!("_submissions._tcp.{domain}.")).await {
        Some((host, port)) => Some(ServerConfig {
            host,
            port,
            security: Security::Tls,
        }),
        None => srv_record(resolver, &format!("_submission._tcp.{domain}."))
            .await
            .map(|(host, port)| ServerConfig {
                host,
                port,
                security: Security::StartTls,
            }),
    };
    Some(DiscoveryResult::servers(
        Source::Srv,
        imap,
        smtp,
        email.to_string(),
    ))
}

async fn reachable(host: &str, port: u16) -> bool {
    matches!(
        timeout(Duration::from_secs(3), TcpStream::connect((host, port))).await,
        Ok(Ok(_))
    )
}

async fn guess(domain: &str, email: &str) -> Option<DiscoveryResult> {
    let mut imap = None;
    for host in [format!("imap.{domain}"), format!("mail.{domain}")] {
        if reachable(&host, 993).await {
            imap = Some(ServerConfig {
                host,
                port: 993,
                security: Security::Tls,
            });
            break;
        }
    }
    let imap = imap?;
    let mut smtp = None;
    'outer: for host in [format!("smtp.{domain}"), format!("mail.{domain}")] {
        for (port, security) in [(465, Security::Tls), (587, Security::StartTls)] {
            if reachable(&host, port).await {
                smtp = Some(ServerConfig {
                    host: host.clone(),
                    port,
                    security,
                });
                break 'outer;
            }
        }
    }
    Some(DiscoveryResult::servers(
        Source::Guess,
        imap,
        smtp,
        email.to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FREENET: &str = include_str!("fixtures/autoconfig_freenet.xml");
    const LOCALPART: &str = include_str!("fixtures/autoconfig_localpart.xml");

    #[test]
    fn parses_ispdb_document() {
        let r = parse_autoconfig(FREENET, "max@freenet.de").unwrap();
        let imap = r.imap.unwrap();
        assert_eq!(imap.host, "mx.freenet.de");
        assert_eq!(imap.port, 993);
        assert_eq!(imap.security, Security::Tls);
        let smtp = r.smtp.unwrap();
        assert_eq!(smtp.host, "mx.freenet.de");
        assert_eq!(smtp.port, 587);
        assert_eq!(smtp.security, Security::StartTls);
        assert_eq!(r.username, "max@freenet.de");
        assert_eq!(r.source, Source::Autoconfig);
    }

    #[test]
    fn expands_placeholders_and_skips_plain() {
        let r = parse_autoconfig(LOCALPART, "jane@corp.example").unwrap();
        let imap = r.imap.unwrap();
        assert_eq!(imap.host, "imap.corp.example");
        assert_eq!(
            imap.security,
            Security::StartTls,
            "plain socket must not be chosen"
        );
        assert_eq!(r.username, "jane");
        assert!(r.smtp.is_none());
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_autoconfig("<html>not found</html>", "a@b.c").is_none());
        assert!(parse_autoconfig("<<<", "a@b.c").is_none());
    }

    #[test]
    fn email_split() {
        assert_eq!(split_email(" a.b@Example.org "), Some(("a.b", "Example.org")));
        assert_eq!(split_email("nope"), None);
        assert_eq!(split_email("a@localhost"), None);
    }

    #[tokio::test]
    async fn known_provider_needs_no_network() {
        let http = reqwest::Client::new();
        let r = discover(&http, "someone@gmail.com").await;
        assert_eq!(r.source, Source::Provider);
        assert_eq!(r.auth_type, AuthType::OauthGoogle);
        assert!(r.is_gmail);
        let r = discover(&http, "someone@outlook.com").await;
        assert_eq!(r.auth_type, AuthType::OauthMicrosoft);
        assert!(!r.allow_password);
    }
}
