use std::collections::HashMap;

use base64::Engine;
use mail_parser::{HeaderValue, Message, MessageParser, MimeHeaders, PartType};

use super::sanitize::sanitize_html;
use crate::model::{Address, AttachmentInfo};

/// Header data of a message, parsed from `BODY[HEADER]` plus the first bytes of `BODY[TEXT]`.
#[derive(Debug, Clone, Default)]
pub struct ParsedHeader {
    pub message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub from: Option<Address>,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub reply_to: Vec<Address>,
    pub subject: String,
    pub date: Option<i64>,
    pub preview: String,
    pub has_attachments: bool,
}

const PREVIEW_CHARS: usize = 200;

pub fn parse_header(header: &[u8], partial_text: Option<&[u8]>) -> ParsedHeader {
    let mut raw = Vec::with_capacity(header.len() + partial_text.map_or(0, |t| t.len()));
    raw.extend_from_slice(header);
    if let Some(t) = partial_text {
        raw.extend_from_slice(t);
    }
    let Some(msg) = MessageParser::default().parse(&raw) else {
        return ParsedHeader::default();
    };
    let preview = if partial_text.is_some() {
        msg.body_preview(PREVIEW_CHARS * 2)
            .map(|p| clean_preview(&p))
            .unwrap_or_default()
    } else {
        String::new()
    };
    ParsedHeader {
        message_id: msg.message_id().map(str::to_string),
        in_reply_to: ids(msg.in_reply_to()).into_iter().next(),
        references: ids(msg.references()),
        from: msg.from().and_then(|a| addresses(Some(a)).into_iter().next()),
        to: addresses(msg.to()),
        cc: addresses(msg.cc()),
        reply_to: addresses(msg.reply_to()),
        subject: msg.subject().unwrap_or_default().trim().to_string(),
        date: msg.date().map(|d| d.to_timestamp()),
        preview,
        has_attachments: has_attachments(&msg),
    }
}

fn has_attachments(msg: &Message) -> bool {
    let root_mixed = msg.root_part().content_type().is_some_and(|ct| {
        ct.ctype().eq_ignore_ascii_case("multipart")
            && ct.subtype().is_some_and(|s| s.eq_ignore_ascii_case("mixed"))
    });
    root_mixed || msg.attachments().any(|p| !is_inline(p))
}

fn is_inline(part: &mail_parser::MessagePart) -> bool {
    part.content_disposition()
        .is_some_and(|d| d.ctype().eq_ignore_ascii_case("inline"))
        && part.content_id().is_some()
}

/// Collapses whitespace, drops quoted lines and cuts the preview to a fixed length.
pub fn clean_preview(s: &str) -> String {
    let joined = s
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('>'))
        .collect::<Vec<_>>()
        .join(" ");
    let collapsed = joined.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out: String = collapsed.chars().take(PREVIEW_CHARS).collect();
    if collapsed.chars().count() > PREVIEW_CHARS {
        out.push('\u{2026}');
    }
    out
}

fn ids(v: &HeaderValue) -> Vec<String> {
    match v {
        HeaderValue::Text(t) => vec![t.trim().to_string()],
        HeaderValue::TextList(list) => list.iter().map(|t| t.trim().to_string()).collect(),
        _ => vec![],
    }
    .into_iter()
    .filter(|s| !s.is_empty())
    .collect()
}

fn addresses(a: Option<&mail_parser::Address>) -> Vec<Address> {
    let Some(a) = a else { return vec![] };
    a.iter()
        .filter_map(|addr| {
            let email = addr.address()?.trim().to_string();
            if email.is_empty() {
                return None;
            }
            let name = addr
                .name()
                .map(|n| n.trim().trim_matches('"').trim().to_string())
                .filter(|n| !n.is_empty() && !n.eq_ignore_ascii_case(&email));
            Some(Address { name, email })
        })
        .collect()
}

/// The conversation a message belongs to: the root of its `References` chain, falling back to
/// `In-Reply-To` and then its own `Message-ID`.
pub fn thread_key(
    message_id: Option<&str>,
    in_reply_to: Option<&str>,
    references: &[String],
    fallback: &str,
) -> String {
    references
        .first()
        .map(String::as_str)
        .or(in_reply_to)
        .or(message_id)
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_else(|| fallback.to_string())
}

/// Full body of a message, ready for the reading pane.
#[derive(Debug, Clone, Default)]
pub struct ParsedBody {
    pub text: Option<String>,
    pub html: Option<String>,
    pub has_remote: bool,
    pub attachments: Vec<AttachmentInfo>,
}

/// Inline images above this size are not embedded as data URLs.
const MAX_INLINE_IMAGE: usize = 8 * 1024 * 1024;

pub fn parse_body(raw: &[u8]) -> ParsedBody {
    let Some(msg) = MessageParser::default().parse(raw) else {
        return ParsedBody {
            text: Some(String::from_utf8_lossy(raw).into_owned()),
            ..Default::default()
        };
    };

    let html_source = msg.html_part(0).and_then(|p| match &p.body {
        PartType::Html(h) => Some(h.to_string()),
        _ => None,
    });

    // Content-ID -> data URL for inline images.
    let mut cid_map = HashMap::new();
    for part in msg.attachments() {
        if let (Some(cid), Some(ct)) = (part.content_id(), part.content_type()) {
            if ct.ctype().eq_ignore_ascii_case("image") && part.contents().len() <= MAX_INLINE_IMAGE {
                let mime = format!("image/{}", ct.subtype().unwrap_or("png"));
                let data = base64::engine::general_purpose::STANDARD.encode(part.contents());
                cid_map.insert(
                    cid.trim_matches(['<', '>']).to_string(),
                    format!("data:{mime};base64,{data}"),
                );
            }
        }
    }

    let (html, has_remote) = match &html_source {
        Some(h) => {
            let s = sanitize_html(h, &cid_map);
            (Some(s.html), s.has_remote)
        }
        None => (None, false),
    };

    let referenced_cids: Vec<&String> = cid_map
        .keys()
        .filter(|cid| html_source.as_deref().is_some_and(|h| h.contains(cid.as_str())))
        .collect();

    let attachments = msg
        .attachments()
        .enumerate()
        .map(|(i, part)| {
            let mime_type = part
                .content_type()
                .map(|ct| match ct.subtype() {
                    Some(sub) => format!("{}/{}", ct.ctype(), sub),
                    None => ct.ctype().to_string(),
                })
                .unwrap_or_else(|| "application/octet-stream".into())
                .to_ascii_lowercase();
            let inline = part
                .content_id()
                .map(|cid| {
                    referenced_cids
                        .iter()
                        .any(|r| r.as_str() == cid.trim_matches(['<', '>']))
                })
                .unwrap_or(false);
            AttachmentInfo {
                index: i as u32,
                filename: attachment_name(part, i, &mime_type),
                mime_type,
                size: part.contents().len() as u64,
                inline,
            }
        })
        .collect();

    ParsedBody {
        text: msg.body_text(0).map(|t| t.into_owned()),
        html,
        has_remote,
        attachments,
    }
}

fn attachment_name(part: &mail_parser::MessagePart, index: usize, mime: &str) -> String {
    if let Some(name) = part.attachment_name() {
        let cleaned: String = name
            .chars()
            .map(|c| {
                if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                    '_'
                } else {
                    c
                }
            })
            .collect();
        if !cleaned.trim().is_empty() {
            return cleaned.trim().to_string();
        }
    }
    if matches!(part.body, PartType::Message(_)) {
        return format!("message-{}.eml", index + 1);
    }
    let ext = mime_guess::get_mime_extensions_str(mime)
        .and_then(|e| e.first())
        .copied()
        .unwrap_or("bin");
    format!("attachment-{}.{ext}", index + 1)
}

/// Returns file name and content of attachment `index`.
pub fn attachment(raw: &[u8], index: u32) -> Option<(String, String, Vec<u8>)> {
    let msg = MessageParser::default().parse(raw)?;
    let part = msg.attachment(index)?;
    let mime = part
        .content_type()
        .map(|ct| match ct.subtype() {
            Some(sub) => format!("{}/{}", ct.ctype(), sub),
            None => ct.ctype().to_string(),
        })
        .unwrap_or_else(|| "application/octet-stream".into())
        .to_ascii_lowercase();
    let name = attachment_name(part, index as usize, &mime);
    let bytes = match &part.body {
        PartType::Message(m) => m.raw_message().to_vec(),
        _ => part.contents().to_vec(),
    };
    Some((name, mime, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIMPLE: &[u8] = b"From: \"Anna Schmidt\" <anna@example.org>\r\n\
To: Max <max@example.com>, team@example.com\r\n\
Cc: =?UTF-8?Q?J=C3=BCrgen?= <juergen@example.com>\r\n\
Subject: =?UTF-8?B?UXVhcnRhbHNiZXJpY2h0IMOcYmVyc2ljaHQ=?=\r\n\
Message-ID: <m3@example.org>\r\n\
In-Reply-To: <m2@example.org>\r\n\
References: <m1@example.org> <m2@example.org>\r\n\
Date: Tue, 3 Mar 2026 10:42:00 +0100\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n";

    #[test]
    fn parses_headers_and_preview() {
        let h = parse_header(
            SIMPLE,
            Some(b"Hi Max,\r\n\r\nhere is the report.\r\n> quoted line\r\n"),
        );
        assert_eq!(h.subject, "Quartalsbericht \u{dc}bersicht");
        assert_eq!(h.from.as_ref().unwrap().name.as_deref(), Some("Anna Schmidt"));
        assert_eq!(h.to.len(), 2);
        assert_eq!(h.to[1].name, None);
        assert_eq!(h.cc[0].name.as_deref(), Some("J\u{fc}rgen"));
        assert_eq!(h.message_id.as_deref(), Some("m3@example.org"));
        assert_eq!(h.references, vec!["m1@example.org", "m2@example.org"]);
        assert_eq!(h.in_reply_to.as_deref(), Some("m2@example.org"));
        assert_eq!(h.preview, "Hi Max, here is the report.");
        assert_eq!(h.date, Some(1772530920));
        assert!(!h.has_attachments);
    }

    #[test]
    fn thread_key_prefers_root_reference() {
        let refs = vec!["Root@x".to_string(), "mid@x".to_string()];
        assert_eq!(thread_key(Some("c@x"), Some("mid@x"), &refs, "f"), "root@x");
        assert_eq!(thread_key(Some("c@x"), Some("p@x"), &[], "f"), "p@x");
        assert_eq!(thread_key(Some("C@x"), None, &[], "f"), "c@x");
        assert_eq!(thread_key(None, None, &[], "fallback"), "fallback");
    }

    #[test]
    fn broken_encoding_does_not_panic() {
        let raw = b"Subject: =?unknown-charset?Q?bad?=\r\nFrom: nobody\r\nContent-Type: text/plain; charset=x-unknown\r\nContent-Transfer-Encoding: base64\r\n\r\n%%%not base64%%%\r\n";
        let h = parse_header(raw, Some(&raw[..0]));
        assert!(h.from.is_none());
        let b = parse_body(raw);
        assert!(b.html.is_none());
    }

    const MULTIPART: &str = "From: a@example.org\r\n\
To: b@example.org\r\n\
Subject: Newsletter\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=\"outer\"\r\n\
\r\n\
--outer\r\n\
Content-Type: multipart/related; boundary=\"rel\"\r\n\
\r\n\
--rel\r\n\
Content-Type: text/html; charset=utf-8\r\n\
\r\n\
<html><body onload=\"evil()\"><script>alert(1)</script><p style=\"color:red\">Hello</p>\
<img src=\"cid:logo@x\"><img src=\"https://tracker.example/pixel.gif\" width=\"1\">\
<form action=\"https://x\"><input name=\"q\"></form><a href=\"https://example.org\" onclick=\"x()\">link</a>\
<a href=\"javascript:alert(1)\">bad</a></body></html>\r\n\
--rel\r\n\
Content-Type: image/png\r\n\
Content-ID: <logo@x>\r\n\
Content-Disposition: inline\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
iVBORw0KGgo=\r\n\
--rel--\r\n\
--outer\r\n\
Content-Type: application/pdf; name=\"report.pdf\"\r\n\
Content-Disposition: attachment; filename=\"report.pdf\"\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
JVBERi0xLjQK\r\n\
--outer--\r\n";

    #[test]
    fn parses_multipart_with_inline_images_and_attachments() {
        let b = parse_body(MULTIPART.as_bytes());
        let html = b.html.unwrap();
        assert!(!html.contains("<script"), "{html}");
        assert!(!html.contains("onload"), "{html}");
        assert!(!html.contains("onclick"), "{html}");
        assert!(!html.contains("<form"), "{html}");
        assert!(!html.contains("<input"), "{html}");
        assert!(!html.contains("javascript:"), "{html}");
        assert!(html.contains("data:image/png;base64,iVBORw0KGgo="), "{html}");
        assert!(html.contains("style=\"color:red\""), "{html}");
        assert!(html.contains("https://tracker.example/pixel.gif"));
        assert!(b.has_remote);

        let pdf = b.attachments.iter().find(|a| a.filename == "report.pdf").unwrap();
        assert_eq!(pdf.mime_type, "application/pdf");
        assert!(!pdf.inline);
        let logo = b.attachments.iter().find(|a| a.mime_type == "image/png").unwrap();
        assert!(logo.inline);

        let (name, mime, bytes) = attachment(MULTIPART.as_bytes(), pdf.index).unwrap();
        assert_eq!(name, "report.pdf");
        assert_eq!(mime, "application/pdf");
        assert_eq!(bytes, b"%PDF-1.4\n");

        let h = parse_header(MULTIPART.as_bytes(), None);
        assert!(h.has_attachments);
    }
}
