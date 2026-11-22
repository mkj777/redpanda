//! The text of every message for search, loaded in the background once the headers are in: only
//! the readable part of a message, never its attachments, plus the names of the attachments.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{Duration, Instant};

use async_imap::imap_proto::types::{
    BodyContentCommon, BodyParams, BodyStructure, ContentEncoding, SectionPath,
};
use async_imap::types::Fetch;
use mail_parser::{MessageParser, MimeHeaders};

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
/// How often a server may leave a message out of its answer before it counts as unreadable.
const MAX_MISSES: u8 = 3;
/// The lists and the progress in the sidebar are refreshed at most this often while texts load.
/// Every refresh makes an open mail reload, so once per batch would be too much.
const NOTICE_EVERY: Duration = Duration::from_secs(5);

/// What the background fill-in keeps from one step to the next.
#[derive(Default)]
pub struct FillIn {
    /// Folders left out until the next full sync, because the server would not read them.
    pub skip: HashSet<i64>,
    /// How often the server left a message out of its answer, by message id.
    misses: HashMap<i64, u8>,
    last_notice: Option<Instant>,
    /// A folder whose loaded texts the lists have not been told about yet.
    unannounced: Option<i64>,
}

impl FillIn {
    fn notice(&mut self, conn: &Conn, folder_id: i64, force: bool) {
        if force || self.last_notice.is_none_or(|t| t.elapsed() >= NOTICE_EVERY) {
            conn.core.mail_changed(&conn.account_id, Some(folder_id));
            self.last_notice = Some(Instant::now());
            self.unannounced = None;
        } else {
            self.unannounced = Some(folder_id);
        }
    }
}

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
    let mut attachments = Vec::new();
    names(bs, &mut attachments);
    Plan {
        text: text_part(bs, &[]),
        attachments,
    }
}

/// The part a reader sees as the text: plain text over HTML only where both say the same
/// (multipart/alternative), otherwise the first text in the message. A mailing list's plain
/// footer after an HTML body is not the text.
fn text_part(bs: &BodyStructure, path: &[u32]) -> Option<TextPart> {
    match bs {
        BodyStructure::Multipart { common, bodies, .. } => {
            let children = bodies.iter().enumerate().filter_map(|(i, body)| {
                let mut child = path.to_vec();
                child.push(i as u32 + 1);
                text_part(body, &child)
            });
            if common.ty.subtype.eq_ignore_ascii_case("alternative") {
                let found: Vec<TextPart> = children.collect();
                found
                    .iter()
                    .find(|t| t.subtype == "plain")
                    .or(found.first())
                    .cloned()
            } else {
                children.into_iter().next()
            }
        }
        BodyStructure::Text { common, other, .. } => {
            if is_attachment(common) {
                return None;
            }
            let subtype = common.ty.subtype.to_ascii_lowercase();
            if subtype != "plain" && subtype != "html" {
                return None;
            }
            Some(TextPart {
                path: if path.is_empty() { vec![1] } else { path.to_vec() },
                subtype,
                charset: param(&common.ty.params, "charset"),
                encoding: encoding_name(&other.transfer_encoding),
                octets: other.octets,
            })
        }
        BodyStructure::Basic { .. } | BodyStructure::Message { .. } => None,
    }
}

fn names(bs: &BodyStructure, out: &mut Vec<String>) {
    match bs {
        BodyStructure::Multipart { bodies, .. } => bodies.iter().for_each(|b| names(b, out)),
        BodyStructure::Text { common, .. } => {
            if is_attachment(common) {
                out.extend(file_name(common));
            }
        }
        BodyStructure::Basic { common, other, .. } | BodyStructure::Message { common, other, .. } => {
            // An image shown inside the HTML is not an attachment.
            if disposition(common).as_deref() == Some("inline") && other.id.is_some() {
                return;
            }
            out.extend(file_name(common));
        }
    }
}

/// A text part sent as a file (`notes.txt`), not as the message's text.
fn is_attachment(common: &BodyContentCommon) -> bool {
    match disposition(common).as_deref() {
        Some("attachment") => true,
        None => file_name(common).is_some(),
        _ => false,
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
/// split into `filename*0*`, `filename*1*`, in any charset). The server hands the parameters over
/// as they were in the message, so they are put back into a header for mail-parser to decode.
fn param(params: &BodyParams, key: &str) -> Option<String> {
    let list = params.as_ref()?;
    let mut header = String::from("Content-Type: application/octet-stream");
    for (k, v) in list {
        let k: String = k
            .chars()
            .filter(|c| c.is_ascii_graphic() && !matches!(c, ';' | '=' | '"'))
            .collect();
        let v: String = v.chars().filter(|c| !c.is_control()).collect();
        if k.is_empty() {
            continue;
        }
        if k.ends_with('*') {
            header.push_str(&format!("; {k}={}", v.replace([' ', ';', '"'], "")));
        } else {
            header.push_str(&format!(
                "; {k}=\"{}\"",
                v.replace('\\', "\\\\").replace('"', "\\\"")
            ));
        }
    }
    header.push_str("\r\n\r\n");
    let msg = MessageParser::default().parse(header.as_bytes())?;
    let value = msg.content_type()?.attribute(key)?;
    Some(decode_words(value))
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
/// Returns `false` once every text is in the index.
pub async fn text_step(conn: &mut Conn, state: &mut FillIn) -> Result<bool> {
    let core = conn.core.clone();
    let mut folders = core.db.with(|c| queries::folder_rows(c, &conn.account_id))?;
    folders.retain(|f| !f.hidden && !state.skip.contains(&f.id));
    folders.sort_by_key(|f| sync_rank(&f.role));
    for folder in folders {
        let pending = core
            .db
            .with(|c| queries::pending_text(c, folder.id, TEXT_BATCH))?;
        if pending.is_empty() {
            continue;
        }
        match load_texts(conn, &folder, &pending, state).await {
            Ok(()) => {}
            // A folder that cannot be read must not stop the others.
            Err(AppError::Imap(msg)) => {
                tracing::warn!(folder = %folder.path, "loading mail text failed: {msg}");
                state.skip.insert(folder.id);
                conn.reset();
            }
            Err(e) => return Err(e),
        }
        return Ok(true);
    }
    if let Some(folder_id) = state.unannounced {
        state.notice(conn, folder_id, true);
    }
    Ok(false)
}

async fn load_texts(
    conn: &mut Conn,
    folder: &FolderRow,
    pending: &[(i64, u32)],
    state: &mut FillIn,
) -> Result<()> {
    let uids: Vec<u32> = pending.iter().map(|(_, uid)| *uid).collect();
    let (structures, mut unreadable) = fetch_each(conn, folder, &uids, "(UID BODYSTRUCTURE)").await?;
    let plans: HashMap<u32, Plan> = structures
        .iter()
        .filter_map(|f| Some((f.uid?, plan(f.bodystructure()?))))
        .collect();
    if plans.is_empty() && unreadable.is_empty() {
        // Not one answer: the server refuses this folder for now.
        return Err(AppError::Imap("the server returned no message structure".into()));
    }

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
        let (fetches, failed) =
            fetch_each(conn, folder, &uids, &format!("(UID BODY.PEEK[{section}])")).await?;
        unreadable.extend(failed);
        let wanted = SectionPath::Part(path, None);
        for f in &fetches {
            if let (Some(uid), Some(bytes)) = (f.uid, f.section(&wanted)) {
                raw.insert(uid, bytes.to_vec());
            }
        }
    }

    let mut jobs: Vec<TextJob> = Vec::new();
    for (id, uid) in pending {
        let plan = plans.get(uid);
        let wants_text = plan.is_some_and(|p| p.text.as_ref().is_some_and(|t| t.octets <= MAX_TEXT_PART));
        let got = raw.remove(uid);
        // Left out of the answer without an error: try again later, a few times.
        let missing = !unreadable.contains(uid) && (plan.is_none() || (wants_text && got.is_none()));
        if missing {
            let n = state.misses.entry(*id).or_default();
            *n += 1;
            if *n < MAX_MISSES {
                continue;
            }
        }
        state.misses.remove(id);
        let text = got.zip(plan.and_then(|p| p.text.clone()));
        let names = plan.map(|p| p.attachments.clone()).unwrap_or_default();
        jobs.push((*id, text, names));
    }
    if jobs.is_empty() {
        return Ok(());
    }
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

    conn.core.db.with(|c| {
        let tx = c.transaction()?;
        queries::save_texts(&tx, &texts)?;
        tx.commit()?;
        Ok(())
    })?;
    state.notice(conn, folder.id, false);
    Ok(())
}

/// A message to turn into a `LoadedText`: its id, the bytes of its text part and attachment names.
type TextJob = (i64, Option<(Vec<u8>, TextPart)>, Vec<String>);

/// Fetches `query` for `uids`. When the answer for the batch cannot be read (one odd message is
/// enough), each message is asked for on its own, so only that one is left out. Returns the
/// answers and the UIDs whose answer could not be read.
async fn fetch_each(
    conn: &mut Conn,
    folder: &FolderRow,
    uids: &[u32],
    query: &str,
) -> Result<(Vec<Fetch>, Vec<u32>)> {
    match fetch(conn, folder, &uid_set(uids), query).await {
        Ok(fetches) => return Ok((fetches, Vec::new())),
        Err(e) if !e.is_unreadable_answer() => return Err(e),
        // The connection is out of step after an answer that could not be read.
        Err(_) => conn.reset(),
    }
    let mut fetches = Vec::new();
    let mut failed = Vec::new();
    for uid in uids {
        match fetch(conn, folder, &uid.to_string(), query).await {
            Ok(f) => fetches.extend(f),
            Err(e) if !e.is_unreadable_answer() => return Err(e),
            Err(_) => {
                failed.push(*uid);
                conn.reset();
            }
        }
    }
    if !failed.is_empty() {
        tracing::info!(folder = %folder.path, count = failed.len(), "messages the server's answer could not be read for");
    }
    Ok((fetches, failed))
}

async fn fetch(conn: &mut Conn, folder: &FolderRow, set: &str, query: &str) -> Result<Vec<Fetch>> {
    let session = conn.ensure_selected(&folder.path).await?;
    match session.uid_fetch(set, query).await {
        Ok(stream) => collect(stream).await,
        Err(e) => Err(e.into()),
    }
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
                multipart("alternative", vec![text("html", "utf-8"), text("plain", "utf-8")]),
                logo,
            ],
        );
        let p = plan(&multipart("mixed", vec![body, pdf]));
        let t = p.text.unwrap();
        assert_eq!(t.section(), "1.1.2");
        assert_eq!(t.subtype, "plain");
        assert_eq!(p.attachments, ["BD85386379.pdf"]);
    }

    #[test]
    fn a_list_footer_is_not_the_text() {
        // Mailman: the HTML post, then the list's plain footer.
        let p = plan(&multipart(
            "mixed",
            vec![text("html", "utf-8"), text("plain", "us-ascii")],
        ));
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
        let p = plan(&multipart("mixed", vec![notes, text("html", "utf-8")]));
        assert_eq!(p.text.unwrap().subtype, "html");
        assert_eq!(p.attachments, ["notes.txt"]);
    }

    #[test]
    fn encoded_file_names() {
        let one = |k: &'static str, v: &'static str| Some(vec![(Cow::Borrowed(k), Cow::Borrowed(v))]);
        assert_eq!(
            param(&one("filename", "=?UTF-8?Q?Rechnung_M=C3=A4rz.pdf?="), "filename").as_deref(),
            Some("Rechnung März.pdf")
        );
        assert_eq!(
            param(&one("filename*", "utf-8''R%C3%BCckgabe.pdf"), "filename").as_deref(),
            Some("Rückgabe.pdf")
        );
        assert_eq!(
            param(&one("filename*", "iso-8859-1''R%FCckgabe.pdf"), "filename").as_deref(),
            Some("Rückgabe.pdf")
        );
        let split = Some(vec![
            (Cow::Borrowed("filename*1*"), Cow::Borrowed("%C3%A4t.pdf")),
            (Cow::Borrowed("filename*0*"), Cow::Borrowed("utf-8'de'Zertifik")),
        ]);
        assert_eq!(param(&split, "filename").as_deref(), Some("Zertifikät.pdf"));
        assert_eq!(
            param(&one("charset", "ISO-8859-1"), "charset").as_deref(),
            Some("ISO-8859-1")
        );
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
