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
