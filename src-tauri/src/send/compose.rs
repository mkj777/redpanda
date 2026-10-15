//! Builds outgoing RFC 5322 messages from drafts.

use std::path::Path;

use lettre::message::header::ContentType;
use lettre::message::{Attachment, Mailbox, MultiPart, SinglePart};
use lettre::Message;

use crate::error::{AppError, Result};
use crate::model::{Account, Address, Draft};

pub struct AttachmentData {
    pub filename: String,
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

/// Maximum total attachment size. Most providers reject messages above 25 MB.
const MAX_ATTACHMENTS: usize = 25 * 1024 * 1024;

fn mailbox(a: &Address) -> Result<Mailbox> {
    let email = a
        .email
        .trim()
        .parse()
        .map_err(|_| AppError::Invalid(format!("invalid address: {}", a.email)))?;
    Ok(Mailbox::new(
        a.name.clone().filter(|n| !n.trim().is_empty()),
        email,
    ))
}

fn angle(id: &str) -> String {
    let id = id.trim().trim_start_matches('<').trim_end_matches('>');
    format!("<{id}>")
}

pub fn read_attachment_file(path: &str) -> Result<AttachmentData> {
    let p = Path::new(path);
    let bytes = std::fs::read(p).map_err(|e| AppError::Invalid(format!("{path}: {e}")))?;
    let filename = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "attachment".into());
    let mime_type = mime_guess::from_path(p).first_or_octet_stream().to_string();
    Ok(AttachmentData {
        filename,
        mime_type,
        bytes,
    })
}

pub fn build_message(account: &Account, draft: &Draft, attachments: Vec<AttachmentData>) -> Result<Message> {
    if draft.to.is_empty() && draft.cc.is_empty() && draft.bcc.is_empty() {
        return Err(AppError::Invalid("no recipients".into()));
    }
    let total: usize = attachments.iter().map(|a| a.bytes.len()).sum();
    if total > MAX_ATTACHMENTS {
        return Err(AppError::Invalid("attachments are larger than 25 MB".into()));
    }

    let from = mailbox(&Address {
        name: Some(account.display_name.clone()),
        email: account.email.clone(),
    })?;
    let domain = account.email.rsplit('@').next().unwrap_or("localhost");
    let mut builder = Message::builder()
        .from(from)
        .subject(draft.subject.trim())
        .date_now()
        .message_id(Some(format!("<{}@{}>", uuid::Uuid::new_v4(), domain)))
        .user_agent(concat!("redpanda/", env!("CARGO_PKG_VERSION")).to_string());
    for a in &draft.to {
        builder = builder.to(mailbox(a)?);
    }
    for a in &draft.cc {
        builder = builder.cc(mailbox(a)?);
    }
    for a in &draft.bcc {
        builder = builder.bcc(mailbox(a)?);
    }
    if let Some(irt) = draft.in_reply_to.as_deref().filter(|s| !s.is_empty()) {
        builder = builder.in_reply_to(angle(irt));
    }
    if !draft.references.is_empty() {
        builder = builder.references(
            draft
                .references
                .iter()
                .map(|r| angle(r))
                .collect::<Vec<_>>()
                .join(" "),
        );
    }

    let text = SinglePart::plain(draft.body.replace("\r\n", "\n").replace('\n', "\r\n"));
    let message = if attachments.is_empty() {
        builder.singlepart(text)?
    } else {
        let mut multi = MultiPart::mixed().singlepart(text);
        for a in attachments {
            let ct = ContentType::parse(&a.mime_type).unwrap_or_else(|_| {
                ContentType::parse("application/octet-stream").expect("static content type")
            });
            multi = multi.singlepart(Attachment::new(a.filename).body(a.bytes, ct));
        }
        builder.multipart(multi)?
    };
    Ok(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AuthType, Security, ServerConfig};
    use mail_parser::MessageParser;

    fn account() -> Account {
        Account {
            id: "a".into(),
            email: "max@example.com".into(),
            display_name: "Max Kielholz".into(),
            provider: "custom".into(),
            auth_type: AuthType::Password,
            username: "max@example.com".into(),
            imap: ServerConfig {
                host: "i".into(),
                port: 993,
                security: Security::Tls,
            },
            smtp: ServerConfig {
                host: "s".into(),
                port: 465,
                security: Security::Tls,
            },
            color: "#000".into(),
            sent_saved_by_server: false,
            is_gmail: false,
        }
    }

    #[test]
    fn builds_reply_with_threading_headers() {
        let draft = Draft {
            account_id: "a".into(),
            to: vec![Address {
                name: Some("Anna".into()),
                email: "anna@example.org".into(),
            }],
            bcc: vec![Address {
                name: None,
                email: "hidden@example.org".into(),
            }],
            subject: "Re: Bericht \u{dc}bersicht".into(),
            body: "Danke!\n\n> Hallo".into(),
            in_reply_to: Some("m2@example.org".into()),
            references: vec!["m1@example.org".into(), "<m2@example.org>".into()],
            ..Default::default()
        };
        let msg = build_message(&account(), &draft, vec![]).unwrap();
        let raw = msg.formatted();
        let parsed = MessageParser::default().parse(&raw).unwrap();
        assert_eq!(parsed.subject(), Some("Re: Bericht \u{dc}bersicht"));
        assert_eq!(parsed.in_reply_to().as_text(), Some("m2@example.org"));
        assert_eq!(parsed.references().as_text_list().map(|l| l.len()), Some(2));
        assert_eq!(
            parsed.from().unwrap().first().unwrap().name(),
            Some("Max Kielholz")
        );
        assert!(parsed.body_text(0).unwrap().contains("Danke!"));
        let envelope = msg.envelope();
        assert_eq!(envelope.to().len(), 2, "bcc is part of the envelope");
    }

    #[test]
    fn attachments_and_validation() {
        let mut draft = Draft {
            account_id: "a".into(),
            subject: "Files".into(),
            body: "see attached".into(),
            ..Default::default()
        };
        assert!(
            build_message(&account(), &draft, vec![]).is_err(),
            "no recipients"
        );
        draft.to.push(Address {
            name: None,
            email: "not an address".into(),
        });
        assert!(build_message(&account(), &draft, vec![]).is_err());
        draft.to[0].email = "b@example.org".into();
        let msg = build_message(
            &account(),
            &draft,
            vec![AttachmentData {
                filename: "notes.txt".into(),
                mime_type: "text/plain".into(),
                bytes: b"hello".to_vec(),
            }],
        )
        .unwrap();
        let raw = msg.formatted();
        let parsed = MessageParser::default().parse(&raw).unwrap();
        assert_eq!(parsed.attachment_count(), 1);
        assert_eq!(parsed.attachment(0).unwrap().contents(), b"hello");
    }
}
