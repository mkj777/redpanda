//! Folder list sync and role mapping (SPECIAL-USE attributes with name fallbacks).

use std::collections::{HashMap, HashSet};

use async_imap::types::{Name, NameAttribute};
use base64::Engine;

use super::imap_conn::{collect, Session};
use crate::core::Core;
use crate::error::Result;
use crate::model::Account;
use crate::store::queries::{self, FolderRow, FolderUpsert};

/// Decodes IMAP modified UTF-7 (RFC 3501 section 5.1.3), e.g. `Entw&APw-rfe` -> `Entwürfe`.
pub fn decode_utf7(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('-') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let encoded = &after[..end];
        if encoded.is_empty() {
            out.push('&');
        } else {
            let b64 = encoded.replace(',', "/");
            match base64::engine::general_purpose::STANDARD_NO_PAD.decode(b64.as_bytes()) {
                Ok(bytes) if bytes.len() % 2 == 0 => {
                    let units: Vec<u16> = bytes
                        .chunks(2)
                        .map(|c| u16::from_be_bytes([c[0], c[1]]))
                        .collect();
                    out.push_str(&String::from_utf16_lossy(&units));
                }
                _ => {
                    out.push('&');
                    out.push_str(encoded);
                    out.push('-');
                }
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

fn special_use_role(attrs: &[NameAttribute]) -> Option<&'static str> {
    attrs.iter().find_map(|a| match a {
        NameAttribute::Sent => Some("sent"),
        NameAttribute::Trash => Some("trash"),
        NameAttribute::Drafts => Some("drafts"),
        NameAttribute::Junk => Some("junk"),
        NameAttribute::Archive => Some("archive"),
        NameAttribute::All => Some("all"),
        NameAttribute::Flagged => Some("flagged"),
        NameAttribute::Extension(e) if e.eq_ignore_ascii_case("\\Important") => Some("important"),
        _ => None,
    })
}

/// Role guess from a folder's display name, for servers without SPECIAL-USE.
pub fn name_role(name: &str) -> Option<&'static str> {
    let n = name.trim().to_lowercase();
    let role = match n.as_str() {
        "sent"
        | "sent items"
        | "sent messages"
        | "sent mail"
        | "gesendet"
        | "gesendete objekte"
        | "gesendete elemente"
        | "gesendete nachrichten" => "sent",
        "trash"
        | "deleted"
        | "deleted items"
        | "deleted messages"
        | "bin"
        | "papierkorb"
        | "gelöscht"
        | "gelöschte elemente"
        | "gelöschte objekte" => "trash",
        "drafts" | "draft" | "entwürfe" | "entwurf" => "drafts",
        "junk" | "spam" | "junk e-mail" | "junk-e-mail" | "junk email" | "bulk mail" | "spamverdacht" => {
            "junk"
        }
        "archive" | "archives" | "archiv" => "archive",
        _ => return None,
    };
    Some(role)
}

fn is_hidden(role: &str) -> bool {
    // Virtual folders that only duplicate messages from other folders.
    matches!(role, "all" | "flagged" | "important")
}

pub struct ListedFolder {
    pub path: String,
    pub name: String,
    pub depth: u32,
    pub role: &'static str,
    pub hidden: bool,
}

/// Turns a LIST response into folders with roles. Pure, so it can be tested without a server.
pub fn classify(names: &[(String, Option<String>, Vec<NameAttribute<'static>>)]) -> Vec<ListedFolder> {
    let selectable: Vec<_> = names
        .iter()
        .filter(|(_, _, attrs)| {
            !attrs.iter().any(|a| {
                matches!(a, NameAttribute::NoSelect)
                    || matches!(a, NameAttribute::Extension(e) if e.eq_ignore_ascii_case("\\NonExistent"))
            })
        })
        .collect();
    let paths: HashSet<&str> = selectable.iter().map(|(p, _, _)| p.as_str()).collect();

    let mut taken: HashSet<&'static str> = HashSet::new();
    let mut roles: HashMap<&str, &'static str> = HashMap::new();
    for (path, _, attrs) in &selectable {
        if path.eq_ignore_ascii_case("INBOX") {
            roles.insert(path, "inbox");
            taken.insert("inbox");
        } else if let Some(r) = special_use_role(attrs) {
            roles.insert(path, r);
            taken.insert(r);
        }
    }

    let mut out = Vec::new();
    for (path, delim, _) in &selectable {
        let segments: Vec<&str> = match delim {
            Some(d) if !d.is_empty() => path.split(d.as_str()).collect(),
            _ => vec![path.as_str()],
        };
        let last = segments.last().copied().unwrap_or(path);
        let name = decode_utf7(last);
        let role = match roles.get(path.as_str()) {
            Some(r) => *r,
            None => match name_role(&name) {
                // Only top-level folders, and only if no SPECIAL-USE folder has the role yet.
                Some(r) if !taken.contains(r) && depth_of(&segments, delim, &paths) == 0 => {
                    taken.insert(r);
                    r
                }
                _ => "other",
            },
        };
        out.push(ListedFolder {
            depth: depth_of(&segments, delim, &paths),
            path: path.clone(),
            name: if role == "inbox" { "Inbox".into() } else { name },
            role,
            hidden: is_hidden(role),
        });
    }
    out
}

/// Number of selectable ancestors (Gmail's `[Gmail]` parent is not selectable, so its children
/// stay at depth 0).
fn depth_of(segments: &[&str], delim: &Option<String>, paths: &HashSet<&str>) -> u32 {
    let Some(d) = delim else { return 0 };
    (1..segments.len())
        .filter(|i| paths.contains(segments[..*i].join(d).as_str()))
        .count() as u32
}

pub async fn sync_folder_list(
    core: &Core,
    session: &mut Session,
    account: &Account,
) -> Result<Vec<FolderRow>> {
    let names: Vec<Name> = collect(session.list(Some(""), Some("*")).await?).await?;
    let raw: Vec<(String, Option<String>, Vec<NameAttribute<'static>>)> = names
        .iter()
        .map(|n| {
            (
                n.name().to_string(),
                n.delimiter().map(str::to_string),
                n.attributes()
                    .iter()
                    .cloned()
                    .map(NameAttribute::into_owned)
                    .collect(),
            )
        })
        .collect();
    let listed = classify(&raw);
    core.db.with(|c| {
        let tx = c.transaction()?;
        let mut keep = Vec::new();
        for f in &listed {
            keep.push(queries::upsert_folder(
                &tx,
                &account.id,
                &FolderUpsert {
                    path: &f.path,
                    name: &f.name,
                    depth: f.depth,
                    role: f.role,
                    hidden: f.hidden,
                },
            )?);
        }
        queries::delete_folders_except(&tx, &account.id, &keep)?;
        tx.commit()?;
        Ok(())
    })?;
    core.db.with(|c| queries::folder_rows(c, &account.id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(
        path: &str,
        attrs: Vec<NameAttribute<'static>>,
    ) -> (String, Option<String>, Vec<NameAttribute<'static>>) {
        (path.to_string(), Some("/".to_string()), attrs)
    }

    #[test]
    fn utf7() {
        assert_eq!(decode_utf7("Entw&APw-rfe"), "Entw\u{fc}rfe");
        assert_eq!(decode_utf7("Gel&APY-scht"), "Gel\u{f6}scht");
        assert_eq!(decode_utf7("Tom &- Jerry"), "Tom & Jerry");
        assert_eq!(decode_utf7("&ZeVnLIqe-"), "\u{65e5}\u{672c}\u{8a9e}");
        assert_eq!(decode_utf7("plain"), "plain");
        assert_eq!(decode_utf7("broken&abc"), "broken&abc");
    }

    #[test]
    fn gmail_layout() {
        let list = vec![
            n("INBOX", vec![]),
            n("[Gmail]", vec![NameAttribute::NoSelect]),
            n("[Gmail]/All Mail", vec![NameAttribute::All]),
            n("[Gmail]/Sent Mail", vec![NameAttribute::Sent]),
            n("[Gmail]/Starred", vec![NameAttribute::Flagged]),
            n(
                "[Gmail]/Important",
                vec![NameAttribute::Extension("\\Important".into())],
            ),
            n("[Gmail]/Trash", vec![NameAttribute::Trash]),
            n("Work", vec![]),
            n("Work/Projects", vec![]),
        ];
        let out = classify(&list);
        let get = |p: &str| out.iter().find(|f| f.path == p).unwrap();
        assert!(out.iter().all(|f| f.path != "[Gmail]"));
        assert_eq!(get("INBOX").role, "inbox");
        assert_eq!(get("[Gmail]/Sent Mail").role, "sent");
        assert_eq!(get("[Gmail]/Sent Mail").depth, 0);
        assert_eq!(get("[Gmail]/Sent Mail").name, "Sent Mail");
        assert!(get("[Gmail]/All Mail").hidden);
        assert!(get("[Gmail]/Starred").hidden);
        assert!(get("[Gmail]/Important").hidden);
        assert!(!get("[Gmail]/Trash").hidden);
        assert_eq!(get("Work/Projects").depth, 1);
        assert_eq!(get("Work/Projects").name, "Projects");
    }

    #[test]
    fn name_fallbacks_for_german_servers() {
        let list = vec![
            n("INBOX", vec![]),
            n("Gesendet", vec![]),
            n("Entw&APw-rfe", vec![]),
            n("Papierkorb", vec![]),
            n("Spamverdacht", vec![]),
            n("Projekte/Gesendet", vec![]),
        ];
        let out = classify(&list);
        let role = |p: &str| out.iter().find(|f| f.path == p).unwrap().role;
        assert_eq!(role("Gesendet"), "sent");
        assert_eq!(role("Entw&APw-rfe"), "drafts");
        assert_eq!(role("Papierkorb"), "trash");
        assert_eq!(role("Spamverdacht"), "junk");
        assert_eq!(role("Projekte/Gesendet"), "other");
    }

    #[test]
    fn special_use_wins_over_names() {
        let list = vec![n("Sent", vec![]), n("Sent Items", vec![NameAttribute::Sent])];
        let out = classify(&list);
        let role = |p: &str| out.iter().find(|f| f.path == p).unwrap().role;
        assert_eq!(role("Sent Items"), "sent");
        assert_eq!(role("Sent"), "other");
    }
}
