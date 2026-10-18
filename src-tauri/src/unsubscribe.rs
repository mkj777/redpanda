//! Unsubscribing from newsletters through the `List-Unsubscribe` header (RFC 2369) and its
//! one-click variant (RFC 8058), which Gmail and Yahoo require from bulk senders since 2024.
//!
//! The header is read during sync and stored with each message. Unsubscribing then takes the
//! best way the sender offers: a one-click HTTPS POST, a mail to the list's unsubscribe address,
//! or the unsubscribe page in the browser.

use std::net::{Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use url::{Host, Url};

use crate::error::{AppError, Result};
use crate::model::UnsubscribeInfo;

/// Fetches only the header fields read here, for messages synced before they were stored.
pub const FETCH_QUERY: &str =
    "(UID BODY.PEEK[HEADER.FIELDS (LIST-UNSUBSCRIBE LIST-UNSUBSCRIBE-POST AUTHENTICATION-RESULTS DKIM-SIGNATURE)])";

/// Longer addresses are ignored, real ones stay far below.
const MAX_URI: usize = 2048;

/// How a message can be unsubscribed from, stored as JSON in `messages.unsubscribe`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Unsubscribe {
    /// An http(s) address from `List-Unsubscribe`, https preferred.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// A `mailto:` address from `List-Unsubscribe`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mailto: Option<String>,
    /// The sender offers one-click for `url`, and a DKIM signature the receiving server
    /// verified covers both list headers, as RFC 8058 requires.
    #[serde(default)]
    pub one_click: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    /// An HTTPS POST, nothing else to do.
    OneClick,
    /// A mail to the list's unsubscribe address, sent from the receiving account.
    Mail,
    /// The unsubscribe page, opened in the browser.
    Web,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Method::OneClick => "one_click",
            Method::Mail => "mail",
            Method::Web => "web",
        }
    }
}

impl Unsubscribe {
    pub fn method(&self) -> Option<Method> {
        if self.one_click && self.url.as_deref().is_some_and(|u| u.starts_with("https://")) {
            Some(Method::OneClick)
        } else if self.mailto.is_some() {
            Some(Method::Mail)
        } else if self.url.is_some() {
            Some(Method::Web)
        } else {
            None
        }
    }

    /// What the user sees before confirming: the address for a mail, the host otherwise.
    pub fn target(&self, method: Method) -> String {
        match method {
            Method::Mail => self.mailto.as_deref().and_then(parse_mailto).map(|m| m.to),
            Method::OneClick | Method::Web => self
                .url
                .as_deref()
                .and_then(|u| Url::parse(u).ok())
                .and_then(|u| u.host_str().map(|h| h.trim_start_matches("www.").to_string())),
        }
        .unwrap_or_default()
    }

    pub fn info(&self, unsubscribed_at: Option<i64>) -> Option<UnsubscribeInfo> {
        let method = self.method()?;
        Some(UnsubscribeInfo {
            method,
            target: self.target(method),
            has_page: self.url.is_some(),
            unsubscribed_at,
        })
    }
}

/// Reads the unsubscribe options from a message header, the whole header or the fields in
/// [`FETCH_QUERY`].
pub fn from_header(header: &[u8]) -> Option<Unsubscribe> {
    // Most mail has no list headers, skip the parsing for it.
    if !header
        .windows(16)
        .any(|w| w.eq_ignore_ascii_case(b"list-unsubscribe"))
    {
        return None;
    }
    let fields = header_fields(header);
    let list = &fields.iter().find(|(n, _)| n == "list-unsubscribe")?.1;
    let (mut https, mut http, mut mailto) = (None, None, None);
    for item in bracketed(list) {
        // Folding may have put whitespace into a long address, RFC 2369 says to drop it.
        let item: String = item.chars().filter(|c| !c.is_whitespace()).collect();
        if item.len() > MAX_URI {
            continue;
        }
        if item.get(..7).is_some_and(|s| s.eq_ignore_ascii_case("mailto:")) {
            if mailto.is_none() && parse_mailto(&item).is_some() {
                mailto = Some(item);
            }
        } else if let Ok(u) = Url::parse(&item) {
            match u.scheme() {
                "https" if https.is_none() && u.host_str().is_some() => https = Some(u.to_string()),
                "http" if http.is_none() && u.host_str().is_some() => http = Some(u.to_string()),
                _ => {}
            }
        }
    }
    let url = https.or(http);
    if url.is_none() && mailto.is_none() {
        return None;
    }
    let post = fields.iter().any(|(n, v)| {
        n == "list-unsubscribe-post" && v.trim().eq_ignore_ascii_case("List-Unsubscribe=One-Click")
    });
    let one_click = post
        && url.as_deref().is_some_and(|u| u.starts_with("https://"))
        && dkim_covers_list_headers(&fields);
    Some(Unsubscribe {
        url,
        mailto,
        one_click,
    })
}

/// Unfolded header fields with lowercase names, in message order.
fn header_fields(header: &[u8]) -> Vec<(String, String)> {
    let text = String::from_utf8_lossy(header);
    let mut out: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            break;
        }
        if line.starts_with([' ', '\t']) {
            if let Some((_, value)) = out.last_mut() {
                value.push(' ');
                value.push_str(line.trim());
            }
        } else if let Some((name, value)) = line.split_once(':') {
            out.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
        }
    }
    out
}

/// The `<...>` items of a list header.
fn bracketed(value: &str) -> impl Iterator<Item = &str> {
    value
        .split('<')
        .skip(1)
        .filter_map(|s| s.split_once('>').map(|(inner, _)| inner))
}

/// RFC 8058: one-click needs a valid DKIM signature over both list headers. The verdict comes
/// from the topmost `Authentication-Results`, the one the receiving server added.
fn dkim_covers_list_headers(fields: &[(String, String)]) -> bool {
    let Some((_, results)) = fields.iter().find(|(n, _)| n == "authentication-results") else {
        return false;
    };
    let passed = dkim_passes(results);
    fields
        .iter()
        .filter(|(n, _)| n == "dkim-signature")
        .any(|(_, sig)| {
            let (Some(d), Some(h)) = (dkim_tag(sig, "d"), dkim_tag(sig, "h")) else {
                return false;
            };
            let d = d.to_ascii_lowercase();
            let signed: Vec<String> = h.split(':').map(|s| s.trim().to_ascii_lowercase()).collect();
            signed.iter().any(|s| s == "list-unsubscribe")
                && signed.iter().any(|s| s == "list-unsubscribe-post")
                && passed.iter().any(|p| *p == d || p.ends_with(&format!(".{d}")))
        })
}

/// Domains with `dkim=pass` in an `Authentication-Results` value (RFC 8601).
fn dkim_passes(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    for result in strip_comments(value).split(';') {
        let mut words = result.split_whitespace();
        if !words.next().is_some_and(|w| w.eq_ignore_ascii_case("dkim=pass")) {
            continue;
        }
        for word in words {
            let Some((key, val)) = word.split_once('=') else {
                continue;
            };
            let domain = match key.to_ascii_lowercase().as_str() {
                "header.d" => val,
                "header.i" => val.rsplit('@').next().unwrap_or_default(),
                _ => continue,
            };
            let domain = domain.trim_matches('"').to_ascii_lowercase();
            if !domain.is_empty() {
                out.push(domain);
            }
        }
    }
    out
}

fn strip_comments(value: &str) -> String {
    let mut depth = 0u32;
    value
        .chars()
        .filter(|&c| {
            match c {
                '(' => depth += 1,
                ')' if depth > 0 => {
                    depth -= 1;
                    return false;
                }
                _ => {}
            }
            depth == 0
        })
        .collect()
}

/// A tag of a `DKIM-Signature` value, without the whitespace folding leaves in it.
fn dkim_tag(sig: &str, name: &str) -> Option<String> {
    sig.split(';').find_map(|tag| {
        let (key, val) = tag.split_once('=')?;
        (key.trim().eq_ignore_ascii_case(name)).then(|| val.chars().filter(|c| !c.is_whitespace()).collect())
    })
}

/// The parts of a `mailto:` address used for the unsubscribe mail (RFC 6068).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mailto {
    pub to: String,
    pub subject: Option<String>,
    pub body: Option<String>,
}

pub fn parse_mailto(uri: &str) -> Option<Mailto> {
    if !uri.get(..7)?.eq_ignore_ascii_case("mailto:") {
        return None;
    }
    let rest = &uri[7..];
    let (addr, query) = rest.split_once('?').unwrap_or((rest, ""));
    let to = decode(addr.split(',').next()?)?.trim().to_string();
    if to.parse::<lettre::Address>().is_err() {
        return None;
    }
    let (mut subject, mut body) = (None, None);
    for pair in query.split('&') {
        let (key, val) = pair.split_once('=').unwrap_or((pair, ""));
        match key.to_ascii_lowercase().as_str() {
            "subject" => subject = decode(val).map(|s| one_line(&s)),
            "body" => body = decode(val).map(|s| s.chars().take(2000).collect()),
            _ => {}
        }
    }
    Some(Mailto { to, subject, body })
}

fn decode(s: &str) -> Option<String> {
    percent_encoding::percent_decode_str(s)
        .decode_utf8()
        .ok()
        .map(|c| c.into_owned())
}

fn one_line(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(200)
        .collect::<String>()
        .trim()
        .to_string()
}

/// The URL for the one-click POST, if it is HTTPS and not on this machine or the local network.
/// The request carries no cookies or credentials, but a mail must not make redpanda talk to
/// devices in the LAN.
pub fn public_https(url: &str) -> Option<Url> {
    let u = Url::parse(url).ok()?;
    if u.scheme() != "https" {
        return None;
    }
    let public = match u.host()? {
        Host::Domain(d) => {
            let d = d.trim_end_matches('.').to_ascii_lowercase();
            d.contains('.')
                && !["localhost", "local", "internal", "lan", "home.arpa"]
                    .iter()
                    .any(|s| d == *s || d.ends_with(&format!(".{s}")))
        }
        Host::Ipv4(ip) => public_v4(ip),
        Host::Ipv6(ip) => match ip.to_ipv4_mapped() {
            Some(v4) => public_v4(v4),
            None => public_v6(ip),
        },
    };
    public.then_some(u)
}

fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_documentation()
        // Carrier-grade NAT, 100.64.0.0/10.
        || (a == 100 && (b & 0xc0) == 64))
}

fn public_v6(ip: Ipv6Addr) -> bool {
    let first = ip.segments()[0];
    !(ip.is_loopback()
        || ip.is_unspecified()
        // Unique local fc00::/7 and link-local fe80::/10.
        || (first & 0xfe00) == 0xfc00
        || (first & 0xffc0) == 0xfe80)
}

/// Sends the one-click request of RFC 8058.
pub async fn one_click(url: &str) -> Result<()> {
    let url = public_https(url).ok_or_else(|| AppError::Invalid("unsubscribe_target".into()))?;
    let client = reqwest::Client::builder()
        .user_agent(concat!("redpanda/", env!("CARGO_PKG_VERSION")))
        // RFC 8058 rules out redirects, and a redirected POST would turn into a GET.
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(20))
        .build()?;
    let res = client
        .post(url)
        .header(reqwest::header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body("List-Unsubscribe=One-Click")
        .send()
        .await?;
    if res.status().is_success() {
        Ok(())
    } else {
        Err(AppError::Network(format!(
            "the unsubscribe request failed with HTTP {}",
            res.status().as_u16()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GMAIL: &str = "Delivered-To: lena@gmail.com\r\n\
        ARC-Authentication-Results: i=1; mx.google.com; dkim=pass header.i=@evil.example\r\n\
        Authentication-Results: mx.google.com;\r\n\
        \tdkim=pass header.i=@news.shop.example header.s=s1 header.b=AbC/12+=;\r\n\
        \tspf=pass (google.com: domain of bounce@shop.example designates 1.2.3.4 as permitted sender) smtp.mailfrom=bounce@shop.example;\r\n\
        \tdmarc=pass (p=REJECT sp=REJECT dis=NONE) header.from=shop.example\r\n\
        DKIM-Signature: v=1; a=rsa-sha256; c=relaxed/relaxed; d=shop.example; s=s1;\r\n\
        \th=from:to:subject:date:list-unsubscribe:list-unsubscribe-post:\r\n\
        \t message-id; bh=abc=; b=def=\r\n\
        From: Shop <news@shop.example>\r\n\
        List-Unsubscribe: <mailto:unsub@shop.example?subject=unsubscribe%20me>,\r\n\
        \t<https://shop.example/u/abc\r\n\
        \tdef?x=1>\r\n\
        List-Unsubscribe-Post: List-Unsubscribe=One-Click\r\n\
        Subject: Sale\r\n\
        \r\n\
        body List-Unsubscribe: <https://ignored.example>\r\n";

    #[test]
    fn reads_one_click_with_a_covering_signature() {
        let u = from_header(GMAIL.as_bytes()).unwrap();
        assert_eq!(u.url.as_deref(), Some("https://shop.example/u/abcdef?x=1"));
        assert_eq!(
            u.mailto.as_deref(),
            Some("mailto:unsub@shop.example?subject=unsubscribe%20me")
        );
        assert!(u.one_click);
        assert_eq!(u.method(), Some(Method::OneClick));
        assert_eq!(u.target(Method::OneClick), "shop.example");
        assert_eq!(u.target(Method::Mail), "unsub@shop.example");
    }

    #[test]
    fn one_click_needs_the_signature_to_cover_both_headers() {
        let h = GMAIL.replace("list-unsubscribe:list-unsubscribe-post:", "list-unsubscribe:");
        let u = from_header(h.as_bytes()).unwrap();
        assert!(!u.one_click);
        assert_eq!(u.method(), Some(Method::Mail));
    }

    #[test]
    fn one_click_needs_a_pass_for_the_signing_domain() {
        // Only the forged ARC result names evil.example, the receiving server saw a fail.
        let h = GMAIL.replace(
            "dkim=pass header.i=@news.shop.example",
            "dkim=fail header.i=@news.shop.example",
        );
        assert!(!from_header(h.as_bytes()).unwrap().one_click);
        let h = GMAIL.replace("d=shop.example; s=s1", "d=other.example; s=s1");
        assert!(!from_header(h.as_bytes()).unwrap().one_click);
    }

    #[test]
    fn only_the_topmost_authentication_results_counts() {
        // A sender can write a passing result into the message itself. The receiving server
        // adds its own verdict above it, and that one says fail.
        let h = format!(
            "Authentication-Results: mx.receiver.example; dkim=fail header.d=shop.example\r\n{GMAIL}"
        );
        assert!(!from_header(h.as_bytes()).unwrap().one_click);
    }

    #[test]
    fn reads_microsoft_results_without_an_authserv_id() {
        let h = "Authentication-Results: spf=pass (sender IP is 1.2.3.4) smtp.mailfrom=shop.example; dkim=pass (signature was verified) header.d=shop.example;dmarc=pass action=none header.from=shop.example;compauth=pass reason=100\r\n\
            DKIM-Signature: v=1; d=shop.example; h=From:Subject:List-Unsubscribe:List-Unsubscribe-Post; b=x\r\n\
            List-Unsubscribe: <https://shop.example/u>\r\n\
            List-Unsubscribe-Post: List-Unsubscribe=One-Click\r\n\r\n";
        assert!(from_header(h.as_bytes()).unwrap().one_click);
    }

    #[test]
    fn falls_back_to_mail_and_page() {
        let h = "List-Unsubscribe: <https://list.example/leave>\r\n\r\n";
        let u = from_header(h.as_bytes()).unwrap();
        assert_eq!(u.method(), Some(Method::Web));
        assert_eq!(u.target(Method::Web), "list.example");
        let h = "List-Unsubscribe: <http://www.list.example/leave>, <ftp://x.example/>\r\n\r\n";
        let u = from_header(h.as_bytes()).unwrap();
        assert_eq!(u.url.as_deref(), Some("http://www.list.example/leave"));
        assert_eq!(u.target(Method::Web), "list.example");
        // One-click is only for https.
        let h = "List-Unsubscribe: <http://list.example/leave>\r\nList-Unsubscribe-Post: List-Unsubscribe=One-Click\r\n\r\n";
        assert_eq!(from_header(h.as_bytes()).unwrap().method(), Some(Method::Web));
    }

    #[test]
    fn ignores_mail_without_usable_addresses() {
        assert_eq!(from_header(b"From: a@b.example\r\nSubject: hi\r\n\r\n"), None);
        assert_eq!(
            from_header(b"List-Unsubscribe: <javascript:alert(1)>, <mailto:not-an-address>\r\n\r\n"),
            None
        );
        assert_eq!(
            from_header(b"List-Unsubscribe: https://no-brackets.example\r\n\r\n"),
            None
        );
    }

    #[test]
    fn parses_mailto() {
        let m = parse_mailto("MAILTO:leave%2Bx@list.example,second@list.example?Subject=Stop%0D%0ABcc:%20x@y.example&body=please").unwrap();
        assert_eq!(m.to, "leave+x@list.example");
        assert_eq!(m.subject.as_deref(), Some("Stop  Bcc: x@y.example"));
        assert_eq!(m.body.as_deref(), Some("please"));
        assert_eq!(parse_mailto("mailto:"), None);
        assert_eq!(parse_mailto("mailto:a b@example.com"), None);
    }

    #[test]
    fn one_click_targets_must_be_public() {
        assert!(public_https("https://shop.example/u?x=1").is_some());
        for url in [
            "http://shop.example/u",
            "https://localhost/u",
            "https://printer.local/u",
            "https://router/u",
            "https://192.168.1.1/u",
            "https://10.0.0.8/u",
            "https://127.0.0.1/u",
            "https://100.64.1.1/u",
            "https://169.254.1.1/u",
            "https://[::1]/u",
            "https://[fd00::1]/u",
            "https://[::ffff:192.168.1.1]/u",
        ] {
            assert!(public_https(url).is_none(), "{url}");
        }
    }

    #[test]
    fn stored_json_round_trips() {
        let u = from_header(GMAIL.as_bytes()).unwrap();
        let json = serde_json::to_string(&u).unwrap();
        assert_eq!(serde_json::from_str::<Unsubscribe>(&json).unwrap(), u);
        let web: Unsubscribe = serde_json::from_str(r#"{"url":"https://a.example/"}"#).unwrap();
        assert_eq!(web.method(), Some(Method::Web));
    }
}
