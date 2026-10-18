//! End-to-end test against a local GreenMail server (plain IMAP 3143, SMTP 3025):
//!
//! ```sh
//! docker run -d --rm -p 3025:3025 -p 3143:3143 greenmail/standalone:2.1.3
//! cargo test --test greenmail -- --ignored --nocapture
//! ```
//!
//! GreenMail creates users on first login, so every run uses a fresh random user.

use std::sync::Arc;
use std::time::Duration;

use redpanda_lib::accounts::oauth::OAuthConfig;
use redpanda_lib::accounts::secrets::Secrets;
use redpanda_lib::accounts::LoginSecret;
use redpanda_lib::core::{Core, NoEvents};
use redpanda_lib::error::AppError;
use redpanda_lib::model::*;
use redpanda_lib::service::{App, FlagKind, NewAccountInput, NewAuth};
use redpanda_lib::store::{queries, Db};
use redpanda_lib::sync::imap_conn;

const HOST: &str = "127.0.0.1";

fn imap_cfg() -> ServerConfig {
    ServerConfig {
        host: HOST.into(),
        port: 3143,
        security: Security::None,
    }
}

fn smtp_cfg() -> ServerConfig {
    ServerConfig {
        host: HOST.into(),
        port: 3025,
        security: Security::None,
    }
}

async fn raw_session(user: &str) -> imap_conn::Session {
    let client = imap_conn::connect(&imap_cfg())
        .await
        .expect("GreenMail running on 3143?");
    imap_conn::login(client, user, &LoginSecret::Password("secret".into()))
        .await
        .unwrap()
}

/// UIDs and flags of a folder, straight from the server.
async fn server_state(user: &str, folder: &str) -> Vec<(u32, bool)> {
    let mut s = raw_session(user).await;
    let mb = s.select(folder).await.unwrap();
    if mb.exists == 0 {
        return vec![];
    }
    let fetches = imap_conn::collect(s.fetch("1:*", "(UID FLAGS)").await.unwrap())
        .await
        .unwrap();
    let mut out: Vec<(u32, bool)> = fetches
        .iter()
        .map(|f| {
            let seen = f.flags().any(|fl| matches!(fl, async_imap::types::Flag::Seen));
            (f.uid.unwrap(), seen)
        })
        .collect();
    out.sort();
    let _ = s.logout().await;
    out
}

async fn eventually<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    for _ in 0..100 {
        if let Some(v) = f() {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("timed out waiting for {what}");
}

async fn eventually_async<T, F: std::future::Future<Output = Option<T>>>(
    what: &str,
    mut f: impl FnMut() -> F,
) -> T {
    for _ in 0..60 {
        if let Some(v) = f().await {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    panic!("timed out waiting for {what}");
}

fn inbox(app: &App) -> Vec<MessageSummary> {
    app.list_messages(
        &View::Unified {
            which: UnifiedView::Inbox,
        },
        0,
        100,
        false,
    )
    .unwrap()
    .items
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a GreenMail container"]
async fn full_cycle() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("redpanda_lib=debug")
        .with_test_writer()
        .try_init();
    let user = format!("u{}@example.com", &uuid::Uuid::new_v4().simple().to_string()[..8]);

    // Server side folders the account will find.
    {
        let mut s = raw_session(&user).await;
        s.create("Sent").await.unwrap();
        s.create("Trash").await.unwrap();
        let _ = s.logout().await;
    }

    let core = Arc::new(Core::new(
        Db::open_in_memory().unwrap(),
        Secrets::from_key(&[9u8; 32]).unwrap(),
        OAuthConfig::default(),
        Arc::new(NoEvents),
    ));
    let app = App::new(core.clone());

    // Wrong password is reported as an auth error... GreenMail accepts any password for new
    // users, so only check that a bad port is a network error.
    let bad = app
        .add_account(
            NewAccountInput {
                email: user.clone(),
                display_name: "Test".into(),
                provider_id: None,
                username: user.clone(),
                imap: ServerConfig {
                    port: 1,
                    ..imap_cfg()
                },
                smtp: smtp_cfg(),
                auth: NewAuth::Password {
                    password: "secret".into(),
                },
            },
            |_| Ok(()),
        )
        .await;
    assert!(matches!(bad, Err(AppError::Network(_))), "{bad:?}");

    let account = app
        .add_account(
            NewAccountInput {
                email: user.clone(),
                display_name: "Test User".into(),
                provider_id: None,
                username: user.clone(),
                imap: imap_cfg(),
                smtp: smtp_cfg(),
                auth: NewAuth::Password {
                    password: "secret".into(),
                },
            },
            |_| Ok(()),
        )
        .await
        .unwrap()
        .account;

    // Folders appear after the first sync.
    let folders = eventually("folders", || {
        let f = app.list_folders().unwrap().folders;
        (f.len() >= 3).then_some(f)
    })
    .await;
    let role = |r: &str| folders.iter().find(|f| f.role == r).unwrap().clone();
    let (inbox_f, sent_f, trash_f) = (role("inbox"), role("sent"), role("trash"));

    // Send three mails to ourselves, one with an attachment.
    let attachment = std::env::temp_dir().join("redpanda-test-notes.txt");
    std::fs::write(&attachment, "attached text").unwrap();
    for i in 1..=3 {
        app.send(Draft {
            account_id: account.id.clone(),
            to: vec![Address {
                name: Some("Me".into()),
                email: user.clone(),
            }],
            subject: format!("Hello {i}"),
            body: format!("Body number {i}\nsecond line"),
            attachments: if i == 3 {
                vec![attachment.display().to_string()]
            } else {
                vec![]
            },
            ..Default::default()
        })
        .await
        .unwrap();
    }

    app.sync_now(None);
    let msgs = eventually("three messages in the inbox", || {
        let m = inbox(&app);
        (m.len() == 3).then_some(m)
    })
    .await;
    assert_eq!(msgs[0].subject, "Hello 3", "newest first");
    assert!(msgs.iter().all(|m| !m.seen));
    assert!(msgs[0].has_attachments);

    // The Sent copies were appended because GreenMail does not file them itself.
    eventually_async("sent copies", || {
        let user = user.clone();
        async move { (server_state(&user, "Sent").await.len() == 3).then_some(()) }
    })
    .await;
    app.sync_now(None);
    eventually("sent folder synced", || {
        let page = app
            .list_messages(&View::Folder { folder_id: sent_f.id }, 0, 10, false)
            .unwrap();
        (page.items.len() == 3).then_some(())
    })
    .await;

    // Body fetch, parsing and attachments.
    let detail = app.get_message(msgs[0].id).await.unwrap();
    assert!(detail.text.as_deref().unwrap().contains("Body number 3"));
    assert_eq!(detail.attachments.len(), 1);
    assert_eq!(detail.attachments[0].filename, "redpanda-test-notes.txt");
    let out = std::env::temp_dir().join("redpanda-test-saved.txt");
    app.save_attachment(msgs[0].id, 0, out.to_str().unwrap()).unwrap();
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "attached text");

    // The preview is there (from the partial fetch, or from the body on servers like GreenMail
    // that only get headers).
    let listed = inbox(&app);
    assert!(
        listed[0].preview.starts_with("Body number 3"),
        "{}",
        listed[0].preview
    );

    // Search over the fetched body.
    let found = app
        .list_messages(
            &View::Search {
                query: "second line".into(),
            },
            0,
            10,
            false,
        )
        .unwrap();
    assert!(found.items.iter().any(|m| m.id == msgs[0].id));

    // Mark read reaches the server.
    app.set_flags(&[msgs[0].id], FlagKind::Seen, true, false).unwrap();
    eventually_async("seen flag on server", || {
        let user = user.clone();
        async move {
            let state = server_state(&user, "INBOX").await;
            (state.iter().filter(|(_, seen)| *seen).count() == 1).then_some(())
        }
    })
    .await;

    // Delete moves to Trash.
    app.delete_messages(&[msgs[1].id], false).unwrap();
    assert_eq!(inbox(&app).len(), 2, "removed locally right away");
    eventually_async("message in trash", || {
        let user = user.clone();
        async move {
            let (i, t) = (
                server_state(&user, "INBOX").await,
                server_state(&user, "Trash").await,
            );
            (i.len() == 2 && t.len() == 1).then_some(())
        }
    })
    .await;

    // Delete inside Trash expunges for good.
    app.sync_now(None);
    let trashed = eventually("trash synced", || {
        let page = app
            .list_messages(
                &View::Folder {
                    folder_id: trash_f.id,
                },
                0,
                10,
                false,
            )
            .unwrap();
        page.items.first().cloned()
    })
    .await;
    app.delete_messages(&[trashed.id], false).unwrap();
    eventually_async("trash empty", || {
        let user = user.clone();
        async move { server_state(&user, "Trash").await.is_empty().then_some(()) }
    })
    .await;

    // Archive needs an archive folder; create one and archive.
    let remaining = inbox(&app);
    assert!(matches!(
        app.archive_messages(&[remaining[0].id], false),
        Err(AppError::NoArchiveFolder)
    ));
    app.create_archive_folder(&account.id).await.unwrap();
    app.archive_messages(&[remaining[0].id], false).unwrap();
    eventually_async("archived on server", || {
        let user = user.clone();
        async move { (server_state(&user, "Archive").await.len() == 1).then_some(()) }
    })
    .await;
    assert_eq!(inbox(&app).len(), 1);

    // Reply sets \Answered on the original.
    let original = inbox(&app)[0].clone();
    app.send(Draft {
        account_id: account.id.clone(),
        to: vec![Address {
            name: None,
            email: user.clone(),
        }],
        subject: format!("Re: {}", original.subject),
        body: "Reply".into(),
        reply_to_message_id: Some(original.id),
        ..Default::default()
    })
    .await
    .unwrap();
    let summary = core.db.with(|c| queries::get_summary(c, original.id)).unwrap();
    assert!(summary.answered);

    app.remove_account(&account.id).unwrap();
    assert!(inbox(&app).is_empty());
    assert_eq!(inbox_f.role, "inbox");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a GreenMail container"]
async fn newsletters() {
    let id = &uuid::Uuid::new_v4().simple().to_string()[..8];
    let user = format!("u{id}@example.com");
    let list = format!("leave{id}@example.com");
    // GreenMail creates both users on their first login.
    let _ = raw_session(&list).await.logout().await;
    {
        let mut s = raw_session(&user).await;
        let raw = [
            "From: Shop News <news@shop.example>".to_string(),
            format!("To: {user}"),
            "Subject: Autumn sale".into(),
            format!("Message-ID: <sale-{id}@shop.example>"),
            "Date: Sat, 17 Oct 2026 09:00:00 +0000".into(),
            format!("List-Unsubscribe: <mailto:{list}?subject=leave%20list>,"),
            format!(" <https://shop.example/u?id={id}>"),
            String::new(),
            "Twenty percent off.".into(),
        ]
        .join("\r\n");
        s.append("INBOX", None, None, raw.as_bytes()).await.unwrap();
        let _ = s.logout().await;
    }

    let core = Arc::new(Core::new(
        Db::open_in_memory().unwrap(),
        Secrets::from_key(&[9u8; 32]).unwrap(),
        OAuthConfig::default(),
        Arc::new(NoEvents),
    ));
    let app = App::new(core.clone());
    let account = app
        .add_account(
            NewAccountInput {
                email: user.clone(),
                display_name: "Test User".into(),
                provider_id: None,
                username: user.clone(),
                imap: imap_cfg(),
                smtp: smtp_cfg(),
                auth: NewAuth::Password {
                    password: "secret".into(),
                },
            },
            |_| Ok(()),
        )
        .await
        .unwrap()
        .account;

    let subs = eventually("the newsletter", || {
        let s = app.list_subscriptions().unwrap();
        (!s.is_empty()).then_some(s)
    })
    .await;
    assert_eq!(subs[0].sender.email, "news@shop.example");
    assert_eq!(
        subs[0].unsubscribe.method,
        redpanda_lib::unsubscribe::Method::Mail
    );
    assert_eq!(subs[0].unsubscribe.target, list);

    // Mail synced before redpanda stored the header gets it with the next sync.
    core.db
        .with(|c| {
            c.execute("UPDATE messages SET unsubscribe = NULL", [])?;
            c.execute("UPDATE folders SET unsub_scanned = 0", [])?;
            Ok(())
        })
        .unwrap();
    assert!(app.list_subscriptions().unwrap().is_empty());
    app.sync_now(None);
    let subs = eventually("the newsletter after the backfill", || {
        let s = app.list_subscriptions().unwrap();
        (!s.is_empty()).then_some(s)
    })
    .await;
    let latest = subs[0].latest_id;
    let detail = app.get_message(latest).await.unwrap();
    assert_eq!(detail.unsubscribe.unwrap().target, list);

    // The unsubscribe mail goes to the list address, from the receiving account.
    let method = app
        .unsubscribe(latest, false, |_| panic!("no page for a mail"))
        .await
        .unwrap();
    assert_eq!(method, redpanda_lib::unsubscribe::Method::Mail);
    eventually_async("the unsubscribe mail", || {
        let list = list.clone();
        async move { (server_state(&list, "INBOX").await.len() == 1).then_some(()) }
    })
    .await;
    let mut s = raw_session(&list).await;
    s.select("INBOX").await.unwrap();
    let fetched = imap_conn::collect(s.fetch("1", "BODY.PEEK[HEADER]").await.unwrap())
        .await
        .unwrap();
    let header = String::from_utf8_lossy(fetched[0].header().unwrap()).into_owned();
    assert!(header.contains("Subject: leave list"), "{header}");
    assert!(
        header
            .lines()
            .any(|l| l.starts_with("From:") && l.contains(&user)),
        "{header}"
    );
    let _ = s.logout().await;
    let subs = app.list_subscriptions().unwrap();
    assert!(subs[0].unsubscribe.unsubscribed_at.is_some());

    // The page is the fallback and opens in the browser.
    let mut opened = None;
    let method = app
        .unsubscribe(latest, true, |url| {
            opened = Some(url.to_string());
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(method, redpanda_lib::unsubscribe::Method::Web);
    assert_eq!(opened, Some(format!("https://shop.example/u?id={id}")));

    app.remove_account(&account.id).unwrap();
}

/// The From header of the only message in `user`'s inbox.
async fn received_from(user: &str) -> String {
    eventually_async("the mail", || {
        let user = user.to_string();
        async move { (server_state(&user, "INBOX").await.len() == 1).then_some(()) }
    })
    .await;
    let mut s = raw_session(user).await;
    s.select("INBOX").await.unwrap();
    let fetched = imap_conn::collect(s.fetch("1", "BODY.PEEK[HEADER]").await.unwrap())
        .await
        .unwrap();
    let header = String::from_utf8_lossy(fetched[0].header().unwrap()).into_owned();
    let _ = s.logout().await;
    header
        .lines()
        .find(|l| l.starts_with("From:"))
        .unwrap_or_default()
        .to_string()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a GreenMail container"]
async fn send_as_alias() {
    let id = &uuid::Uuid::new_v4().simple().to_string()[..8];
    let user = format!("u{id}@example.com");
    let alias = format!("old{id}@example.net");
    let (friend, list) = (format!("f{id}@example.com"), format!("leave{id}@example.com"));
    for u in [&friend, &list] {
        let _ = raw_session(u).await.logout().await;
    }
    {
        let mut s = raw_session(&user).await;
        let raw = [
            "From: Shop News <news@shop.example>".to_string(),
            format!("To: {alias}"),
            "Subject: Autumn sale".into(),
            "Date: Sat, 17 Oct 2026 09:00:00 +0000".into(),
            format!("List-Unsubscribe: <mailto:{list}>"),
            String::new(),
            "Twenty percent off.".into(),
        ]
        .join("\r\n");
        s.append("INBOX", None, None, raw.as_bytes()).await.unwrap();
        let _ = s.logout().await;
    }

    let core = Arc::new(Core::new(
        Db::open_in_memory().unwrap(),
        Secrets::from_key(&[9u8; 32]).unwrap(),
        OAuthConfig::default(),
        Arc::new(NoEvents),
    ));
    let app = App::new(core.clone());
    let account = app
        .add_account(
            NewAccountInput {
                email: user.clone(),
                display_name: "Test User".into(),
                provider_id: None,
                username: user.clone(),
                imap: imap_cfg(),
                smtp: smtp_cfg(),
                auth: NewAuth::Password {
                    password: "secret".into(),
                },
            },
            |_| Ok(()),
        )
        .await
        .unwrap()
        .account;
    // What a Gmail account reads from its send-as settings.
    core.db
        .with(|c| {
            queries::replace_aliases(
                c,
                &account.id,
                &[Address {
                    name: Some("Old Me".into()),
                    email: alias.clone(),
                }],
            )
        })
        .unwrap();
    let view = app.list_accounts().unwrap();
    assert_eq!(view[0].aliases[0].email, alias);

    let draft = |from: &str| Draft {
        account_id: account.id.clone(),
        from_email: Some(from.to_string()),
        to: vec![Address {
            name: None,
            email: friend.clone(),
        }],
        subject: "Hello".into(),
        body: "Hi".into(),
        ..Default::default()
    };
    let refused = app.send(draft("someone@else.example")).await;
    assert!(
        matches!(&refused, Err(AppError::Invalid(m)) if m == "unknown_sender"),
        "{refused:?}"
    );
    app.send(draft(&alias.to_uppercase())).await.unwrap();
    let from = received_from(&friend).await;
    assert!(
        from.contains(&format!("\"Old Me\" <{alias}>")) || from.contains(&format!("Old Me <{alias}>")),
        "{from}"
    );

    // The unsubscribe mail leaves from the address the newsletter went to.
    let subs = eventually("the newsletter", || {
        let s = app.list_subscriptions().unwrap();
        (!s.is_empty()).then_some(s)
    })
    .await;
    app.unsubscribe(subs[0].latest_id, false, |_| panic!("no page"))
        .await
        .unwrap();
    let from = received_from(&list).await;
    assert!(from.contains(&alias), "{from}");

    app.remove_account(&account.id).unwrap();
}
