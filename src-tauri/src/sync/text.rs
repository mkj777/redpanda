//! The text of every message for search, loaded in the background once the headers are in: only
//! the readable part of a message (plain text, or HTML when there is none), never its
//! attachments, plus the names of the attachments.

use std::collections::{BTreeMap, HashMap, HashSet};

use async_imap::imap_proto::types::{
    BodyContentCommon, BodyParams, BodyStructure, ContentEncoding, SectionPath,
};
use async_imap::types::Fetch;
use percent_encoding::percent_decode_str;

use super::engine::{sync_rank, Conn};
use super::imap_conn::{collect, uid_set};
use crate::error::{AppError, Result};
use crate::mime::parse::{decode_words, part_text};
use crate::store::queries::{self, FolderRow, LoadedText};

/// Messages whose text is read per step.
const TEXT_BATCH: usize = 50;
/// Text parts larger than this are left out. They are rarely something a person wrote, and the
/// index keeps only the start of a text anyway.
const MAX_TEXT_PART: u32 = 2_000_000;

/// The body part that holds a message's text.
#[derive(Debug, Clone, PartialEq)]
pub struct TextPart {
    /// Part numbers, `[1]` for a message that is not multipart.
    pub path: Vec<u32>,
    pub subtype: String,
    pub charset: Option<String>,
    pub encoding: String,
    pub octets: u32,
}

impl TextPart {
    fn section(&self) -> String {
        self.path.iter().map(u32::to_string).collect::<Vec<_>>().join(".")
    }
}

/// What to read of a message, from its BODYSTRUCTURE.
#[derive(Debug, Default, PartialEq)]
pub struct Plan {
    pub text: Option<TextPart>,
    pub attachments: Vec<String>,
}

pub fn plan(bs: &BodyStructure) -> Plan {
    let mut plain = None;
    let mut html = None;
    let mut attachments = Vec::new();
    walk(bs, &[], &mut plain, &mut html, &mut attachments);
    Plan {
        text: plain.or(html),
        attachments,
    }
}

fn walk(
    bs: &BodyStructure,
    path: &[u32],
    plain: &mut Option<TextPart>,
    html: &mut Option<TextPart>,
    attachments: &mut Vec<String>,
) {
    match bs {
        BodyStructure::Multipart { bodies, .. } => {
            for (i, body) in bodies.iter().enumerate() {
                let mut child = path.to_vec();
                child.push(i as u32 + 1);
                walk(body, &child, plain, html, attachments);
            }
        }
        BodyStructure::Text { common, other, .. } => {
            let disposition = disposition(common);
            let name = file_name(common);
            if disposition.as_deref() == Some("attachment") || (disposition.is_none() && name.is_some()) {
                attachments.extend(name);
                return;
            }
            let slot = match common.ty.subtype.to_ascii_lowercase().as_str() {
                "plain" => plain,
                "html" => html,
                _ => return,
            };
            if slot.is_none() {
                *slot = Some(TextPart {
                    path: if path.is_empty() { vec![1] } else { path.to_vec() },
                    subtype: common.ty.subtype.to_ascii_lowercase(),
                    charset: param(&common.ty.params, "charset"),
                    encoding: encoding_name(&other.transfer_encoding),
                    octets: other.octets,
                });
            }
        }
        BodyStructure::Basic { common, other, .. } | BodyStructure::Message { common, other, .. } => {
            // An image shown inside the HTML is not an attachment.
            if disposition(common).as_deref() == Some("inline") && other.id.is_some() {
                return;
            }
            attachments.extend(file_name(common));
        }
    }
}

fn disposition(common: &BodyContentCommon) -> Option<String> {
    common.disposition.as_ref().map(|d| d.ty.to_ascii_lowercase())
}

fn file_name(common: &BodyContentCommon) -> Option<String> {
    common
        .disposition
        .as_ref()
        .and_then(|d| param(&d.params, "filename"))
        .or_else(|| param(&common.ty.params, "name"))
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
}

/// A parameter value, decoded from RFC 2047 words or RFC 2231 (`filename*=utf-8''R%C3%BCck.pdf`,
/// also split into `filename*0*`, `filename*1*`, ...).
fn param(params: &BodyParams, key: &str) -> Option<String> {
    let list = params.as_ref()?;
    if let Some((_, v)) = list.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)) {
        return Some(decode_words(v));
    }
    let mut parts: Vec<(u32, bool, &str)> = list
        .iter()
        .filter_map(|(k, v)| {
            let k = k.to_ascii_lowercase();
            let rest = k.strip_prefix(key)?.strip_prefix('*')?;
            let (n, encoded) = if rest.is_empty() {
                (0, true)
            } else {
                (rest.trim_end_matches('*').parse().ok()?, rest.ends_with('*'))
            };
            Some((n, encoded, v.as_ref()))
        })
        .collect();
    if parts.is_empty() {
        return None;
    }
    parts.sort_by_key(|p| p.0);
    let mut bytes = Vec::new();
    for (i, (_, encoded, v)) in parts.iter().enumerate() {
        if *encoded {
            // The first piece starts with charset'language'.
            let v = if i == 0 {
                v.splitn(3, '\'').nth(2).unwrap_or(v)
            } else {
                v
            };
            bytes.extend(percent_decode_str(v));
        } else {
            bytes.extend_from_slice(v.as_bytes());
        }
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn encoding_name(e: &ContentEncoding) -> String {
    match e {
        ContentEncoding::SevenBit => "7bit".into(),
        ContentEncoding::EightBit => "8bit".into(),
        ContentEncoding::Binary => "binary".into(),
        ContentEncoding::Base64 => "base64".into(),
        ContentEncoding::QuotedPrintable => "quoted-printable".into(),
        ContentEncoding::Other(s) => s.to_string(),
    }
}

/// Reads the text of the next batch of messages in the first folder that still has some.
/// Folders in `skip` are left out. Returns `false` once every text is in the index.
pub async fn text_step(conn: &mut Conn, skip: &mut HashSet<i64>) -> Result<bool> {
    let core = conn.core.clone();
    let mut folders = core.db.with(|c| queries::folder_rows(c, &conn.account_id))?;
    folders.retain(|f| !f.hidden && !skip.contains(&f.id));
    folders.sort_by_key(|f| sync_rank(&f.role));
    for folder in folders {
        let pending = core
            .db
            .with(|c| queries::pending_text(c, folder.id, TEXT_BATCH))?;
        if pending.is_empty() {
            continue;
        }
        match load_texts(conn, &folder, &pending).await {
            Ok(()) => {}
            // A folder that cannot be read must not stop the others.
            Err(AppError::Imap(msg)) => {
                tracing::warn!(folder = %folder.path, "loading mail text failed: {msg}");
                skip.insert(folder.id);
                conn.reset();
            }
            Err(e) => return Err(e),
        }
        return Ok(true);
    }
    Ok(false)
}

/// A message to turn into a `LoadedText`: its id, the bytes of its text part and attachment names.
type TextJob = (i64, Option<(Vec<u8>, TextPart)>, Vec<String>);

async fn load_texts(conn: &mut Conn, folder: &FolderRow, pending: &[(i64, u32)]) -> Result<()> {
    let uids: Vec<u32> = pending.iter().map(|(_, uid)| *uid).collect();
    let (structures, _) = fetch_each(conn, folder, &uids, "(UID BODYSTRUCTURE)").await?;
    let plans: HashMap<u32, Plan> = structures
        .iter()
        .filter_map(|f| Some((f.uid?, plan(f.bodystructure()?))))
        .collect();

    // One request per part number, most messages share theirs ("1" or "1.1").
    let mut groups: BTreeMap<String, (Vec<u32>, Vec<u32>)> = BTreeMap::new();
    for (uid, p) in &plans {
        if let Some(part) = p.text.as_ref().filter(|t| t.octets <= MAX_TEXT_PART) {
            groups
                .entry(part.section())
                .or_insert_with(|| (part.path.clone(), Vec::new()))
                .1
                .push(*uid);
        }
    }
    let mut raw: HashMap<u32, Vec<u8>> = HashMap::new();
    for (section, (path, uids)) in groups {
        let (fetches, _) = fetch_each(conn, folder, &uids, &format!("(UID BODY.PEEK[{section}])")).await?;
        let wanted = SectionPath::Part(path, None);
        for f in &fetches {
            if let (Some(uid), Some(bytes)) = (f.uid, f.section(&wanted)) {
                raw.insert(uid, bytes.to_vec());
            }
        }
    }

    let jobs: Vec<TextJob> = pending
        .iter()
        .map(|(id, uid)| {
            let p = plans.get(uid);
            let text = p.and_then(|p| Some((raw.remove(uid)?, p.text.clone()?)));
            (*id, text, p.map(|p| p.attachments.clone()).unwrap_or_default())
        })
        .collect();
    let texts = tokio::task::spawn_blocking(move || {
        jobs.into_iter()
            .map(|(message_id, text, names)| LoadedText {
                message_id,
                text: text.and_then(|(bytes, part)| {
                    part_text(&bytes, &part.subtype, part.charset.as_deref(), &part.encoding)
                }),
                attachments: queries::attachment_names(names.iter().map(String::as_str)),
            })
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))?;

    let core = conn.core.clone();
    core.db.with(|c| {
        let tx = c.transaction()?;
        queries::save_texts(&tx, &texts)?;
        tx.commit()?;
        Ok(())
    })?;
    core.mail_changed(&conn.account_id, Some(folder.id));
    Ok(())
}

/// Fetches `query` for `uids`. When the server's answer for the batch cannot be read (one odd
/// message is enough), each message is asked for on its own, so only that one is left out.
/// Returns the answers and the UIDs that failed.
async fn fetch_each(
    conn: &mut Conn,
    folder: &FolderRow,
    uids: &[u32],
    query: &str,
) -> Result<(Vec<Fetch>, Vec<u32>)> {
    let session = conn.ensure_selected(&folder.path).await?;
    let first = match session.uid_fetch(uid_set(uids), query).await {
        Ok(stream) => collect(stream).await,
        Err(e) => Err(e.into()),
    };
    match first {
        Ok(fetches) => return Ok((fetches, Vec::new())),
        Err(e @ AppError::Network(_)) => return Err(e),
        Err(_) if uids.len() == 1 => {
            conn.reset();
            return Ok((Vec::new(), uids.to_vec()));
        }
        // The stream is out of step after a parse error.
        Err(_) => conn.reset(),
    }
    let mut fetches = Vec::new();
    let mut failed = Vec::new();
    for uid in uids {
        let session = conn.ensure_selected(&folder.path).await?;
        let res = match session.uid_fetch(uid.to_string(), query).await {
            Ok(stream) => collect(stream).await,
            Err(e) => Err(e.into()),
        };
        match res {
            Ok(f) => fetches.extend(f),
            Err(e @ AppError::Network(_)) => return Err(e),
            Err(_) => {
                failed.push(*uid);
                conn.reset();
            }
        }
    }
    Ok((fetches, failed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_imap::imap_proto::types::{BodyContentSinglePart, ContentDisposition, ContentType};
    use std::borrow::Cow;

    fn common(
        ty: &str,
        sub: &str,
        params: &[(&str, &str)],
        disposition: Option<(&str, &[(&str, &str)])>,
    ) -> BodyContentCommon<'static> {
        let params_of = |p: &[(&str, &str)]| {
            (!p.is_empty()).then(|| {
                p.iter()
                    .map(|(k, v)| (Cow::Owned(k.to_string()), Cow::Owned(v.to_string())))
                    .collect()
            })
        };
        BodyContentCommon {
            ty: ContentType {
                ty: Cow::Owned(ty.into()),
                subtype: Cow::Owned(sub.into()),
                params: params_of(params),
            },
            disposition: disposition.map(|(d, p)| ContentDisposition {
                ty: Cow::Owned(d.into()),
                params: params_of(p),
            }),
            language: None,
            location: None,
        }
    }

    fn single(encoding: ContentEncoding<'static>, id: Option<&str>) -> BodyContentSinglePart<'static> {
        BodyContentSinglePart {
            id: id.map(|s| Cow::Owned(s.into())),
            md5: None,
            description: None,
            transfer_encoding: encoding,
            octets: 100,
        }
    }

    fn text(sub: &str, charset: &str) -> BodyStructure<'static> {
        BodyStructure::Text {
            common: common("text", sub, &[("charset", charset)], None),
            other: single(ContentEncoding::QuotedPrintable, None),
            lines: 3,
            extension: None,
        }
    }

    fn multipart(sub: &str, bodies: Vec<BodyStructure<'static>>) -> BodyStructure<'static> {
        BodyStructure::Multipart {
            common: common("multipart", sub, &[], None),
            bodies,
            extension: None,
        }
    }

    #[test]
    fn a_plain_message_is_part_one() {
        let p = plan(&text("plain", "utf-8"));
        let t = p.text.unwrap();
        assert_eq!(t.section(), "1");
        assert_eq!(t.charset.as_deref(), Some("utf-8"));
        assert_eq!(t.encoding, "quoted-printable");
        assert!(p.attachments.is_empty());
    }

    #[test]
    fn plain_text_wins_and_attachments_are_named() {
        let pdf = BodyStructure::Basic {
            common: common(
                "application",
                "pdf",
                &[("name", "x.pdf")],
                Some(("attachment", &[("filename", "BD85386379.pdf")])),
            ),
            other: single(ContentEncoding::Base64, None),
            extension: None,
        };
        let logo = BodyStructure::Basic {
            common: common("image", "png", &[("name", "logo.png")], Some(("inline", &[]))),
            other: single(ContentEncoding::Base64, Some("<logo@x>")),
            extension: None,
        };
        let body = multipart(
            "related",
            vec![
                multipart("alternative", vec![text("plain", "utf-8"), text("html", "utf-8")]),
                logo,
            ],
        );
        let p = plan(&multipart("mixed", vec![body, pdf]));
        let t = p.text.unwrap();
        assert_eq!(t.section(), "1.1.1");
        assert_eq!(t.subtype, "plain");
        assert_eq!(p.attachments, ["BD85386379.pdf"]);
    }

    #[test]
    fn html_when_there_is_no_plain_text() {
        let p = plan(&multipart("mixed", vec![text("html", "iso-8859-1")]));
        let t = p.text.unwrap();
        assert_eq!((t.section().as_str(), t.subtype.as_str()), ("1", "html"));
    }

    #[test]
    fn a_text_file_attachment_is_not_the_text() {
        let notes = BodyStructure::Text {
            common: common(
                "text",
                "plain",
                &[],
                Some(("attachment", &[("filename", "notes.txt")])),
            ),
            other: single(ContentEncoding::SevenBit, None),
            lines: 1,
            extension: None,
        };
        let p = plan(&multipart("mixed", vec![text("html", "utf-8"), notes]));
        assert_eq!(p.text.unwrap().subtype, "html");
        assert_eq!(p.attachments, ["notes.txt"]);
    }

    #[test]
    fn encoded_file_names() {
        let encoded = Some(vec![(
            Cow::Borrowed("filename"),
            Cow::Borrowed("=?UTF-8?Q?Rechnung_M=C3=A4rz.pdf?="),
        )]);
        assert_eq!(param(&encoded, "filename").as_deref(), Some("Rechnung März.pdf"));
        let rfc2231 = Some(vec![(
            Cow::Borrowed("filename*"),
            Cow::Borrowed("utf-8''R%C3%BCckgabe.pdf"),
        )]);
        assert_eq!(param(&rfc2231, "filename").as_deref(), Some("Rückgabe.pdf"));
        let split = Some(vec![
            (Cow::Borrowed("filename*1*"), Cow::Borrowed("%C3%A4t.pdf")),
            (Cow::Borrowed("filename*0*"), Cow::Borrowed("utf-8'de'Zertifik")),
        ]);
        assert_eq!(param(&split, "filename").as_deref(), Some("Zertifikät.pdf"));
    }

    #[test]
    fn part_text_decodes_html_and_charsets() {
        let html = part_text(
            b"<p>Dein MacBook&nbsp;Air =E4</p>",
            "html",
            Some("iso-8859-1"),
            "quoted-printable",
        )
        .unwrap();
        assert!(html.contains("MacBook"), "{html}");
        assert!(html.contains('ä'), "{html}");
        let plain = part_text(b"SGFsbG8gV2VsdA==", "plain", None, "base64").unwrap();
        assert_eq!(plain.trim(), "Hallo Welt");
    }
}
