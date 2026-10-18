//! The MCP server against a local GreenMail server (plain IMAP 3143, SMTP 3025), driven by an
//! MCP client like Claude Code would:
//!
//! ```sh
//! docker run -d --rm -p 3025:3025 -p 3143:3143 greenmail/standalone:2.1.3
//! cargo test --test mcp -- --ignored --nocapture
//! ```

use std::sync::{Arc, RwLock};
use std::time::Duration;

use redpanda_lib::accounts::oauth::OAuthConfig;
use redpanda_lib::accounts::secrets::Secrets;
use redpanda_lib::core::{Core, NoEvents};
use redpanda_lib::mcp;
use redpanda_lib::model::*;
use redpanda_lib::service::{App, NewAccountInput, NewAuth};
use redpanda_lib::store::Db;
use rmcp::model::{CallToolRequestParams, ClientConfig};
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::ServiceExt;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

const KEY: &str = "rp_test_key";

fn server(port: u16) -> ServerConfig {
    ServerConfig {
        host: "127.0.0.1".into(),
        port,
        security: Security::None,
    }
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

type Client = rmcp::service::RunningService<rmcp::RoleClient, ClientConfig>;

/// Calls a tool and returns its text, or the error text with `Err`.
async fn call(client: &Client, tool: &str, args: Value) -> Result<String, String> {
    let args = serde_json::from_value(args).unwrap();
    let result = client
        .call_tool(CallToolRequestParams::new(tool.to_string()).with_arguments(args))
        .await
        .unwrap();
    let text = result
        .content
        .iter()
        .filter_map(|c| c.as_text().map(|t| t.text.clone()))
        .collect::<String>();
    if result.is_error == Some(true) {
        Err(text)
    } else {
        Ok(text)
    }
}

async fn call_json(client: &Client, tool: &str, args: Value) -> Value {
    let text = call(client, tool, args)
        .await
        .unwrap_or_else(|e| panic!("{tool}: {e}"));
    serde_json::from_str(&text).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a GreenMail container"]
async fn claude_reads_and_sends() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("redpanda_lib=info")
        .with_test_writer()
        .try_init();
    let user = format!("m{}@example.com", &uuid::Uuid::new_v4().simple().to_string()[..8]);

    let core = Arc::new(Core::new(
        Db::open_in_memory().unwrap(),
        Secrets::from_key(&[7u8; 32]).unwrap(),
        OAuthConfig::default(),
        Arc::new(NoEvents),
    ));
    let app = Arc::new(App::new(core));
    let account = app
        .add_account(
            NewAccountInput {
                email: user.clone(),
                display_name: "Test User".into(),
                provider_id: None,
                username: user.clone(),
                imap: server(3143),
                smtp: server(3025),
                auth: NewAuth::Password {
                    password: "secret".into(),
                },
            },
            |_| Ok(()),
        )
        .await
        .unwrap()
        .account;

    // A mail with a text attachment to find and read.
    let notes = std::env::temp_dir().join("redpanda-mcp-notes.txt");
    std::fs::write(&notes, "the attached notes").unwrap();
    app.send(Draft {
        account_id: account.id.clone(),
        to: vec![Address {
            name: None,
            email: user.clone(),
        }],
        subject: "Quarterly pineapple report".into(),
        body: "Numbers are up.\nSee the notes.".into(),
        attachments: vec![notes.display().to_string()],
        ..Default::default()
    })
    .await
    .unwrap();
    app.sync_now(None);
    eventually("the mail in the inbox", || {
        let page = app
            .list_messages(
                &View::Unified {
                    which: UnifiedView::Inbox,
                },
                0,
                10,
                false,
            )
            .unwrap();
        (page.items.len() == 1).then_some(())
    })
    .await;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let cancel = CancellationToken::new();
    let key = Arc::new(RwLock::new(KEY.to_string()));
    let handle = tokio::spawn(mcp::serve(app.clone(), listener, key.clone(), cancel.clone()));
    let url = format!("http://127.0.0.1:{port}/mcp");

    // Without the key, from a web page or under a foreign host name nothing gets through.
    let http = reqwest::Client::new();
    let init = json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}
    });
    let post = |auth: Option<&str>| {
        let mut r = http
            .post(&url)
            .header("Accept", "application/json, text/event-stream")
            .json(&init);
        if let Some(a) = auth {
            r = r.bearer_auth(a);
        }
        r
    };
    assert_eq!(post(None).send().await.unwrap().status(), 401);
    assert_eq!(post(Some("wrong")).send().await.unwrap().status(), 401);
    let foreign_host = post(Some(KEY))
        .header("Host", format!("attacker.example:{port}"))
        .send()
        .await
        .unwrap();
    assert!(
        foreign_host.status().is_client_error(),
        "{}",
        foreign_host.status()
    );
    let from_page = post(Some(KEY))
        .header("Origin", "https://attacker.example")
        .send()
        .await
        .unwrap();
    assert!(from_page.status().is_client_error(), "{}", from_page.status());
    assert!(post(Some(KEY)).send().await.unwrap().status().is_success());

    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(url.clone()).auth_header(KEY),
    );
    let client = ClientConfig::default().serve(transport).await.unwrap();

    let tools: Vec<String> = client
        .list_all_tools()
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.name.to_string())
        .collect();
    for name in [
        "list_accounts",
        "search_messages",
        "read_message",
        "get_attachment",
        "send_mail",
    ] {
        assert!(tools.iter().any(|t| t == name), "{name} missing in {tools:?}");
    }

    let accounts = call_json(&client, "list_accounts", json!({})).await;
    assert_eq!(accounts[0]["email"], user.as_str());
    assert_eq!(accounts[0]["senders"][0], user.as_str());

    let found = call_json(&client, "search_messages", json!({"query": "pineapple"})).await;
    let id = found["messages"][0]["id"]
        .as_i64()
        .expect("search finds the mail");

    let message = call_json(&client, "read_message", json!({"id": id})).await;
    assert_eq!(message["subject"], "Quarterly pineapple report");
    assert!(message["text"].as_str().unwrap().contains("Numbers are up."));
    assert_eq!(message["read"], false, "reading through MCP leaves it unread");
    let index = message["attachments"][0]["index"].as_u64().unwrap();
    let attachment = call_json(
        &client,
        "get_attachment",
        json!({"message_id": id, "index": index}),
    )
    .await;
    assert_eq!(attachment["text"], "the attached notes");

    // Unknown senders are refused before anything goes out.
    let refused = call(
        &client,
        "send_mail",
        json!({"from": "someone@else.example", "to": [user], "body": "x"}),
    )
    .await;
    assert!(refused.unwrap_err().contains("unknown sender"));

    // A reply lands in the same conversation.
    call(
        &client,
        "send_mail",
        json!({"from": user, "to": [format!("Me <{user}>")], "body": "Thanks!", "reply_to_message_id": id}),
    )
    .await
    .unwrap();
    app.sync_now(None);
    let reply = eventually("the reply in the inbox", || {
        let page = app
            .list_messages(
                &View::Unified {
                    which: UnifiedView::Inbox,
                },
                0,
                10,
                false,
            )
            .unwrap();
        page.items
            .into_iter()
            .find(|m| m.subject == "Re: Quarterly pineapple report")
    })
    .await;
    let detail = app.get_message(reply.id).await.unwrap();
    let original = app.get_message(id).await.unwrap();
    assert_eq!(detail.references, vec![original.message_id.clone().unwrap()]);
    assert_eq!(detail.text.as_deref().map(str::trim), Some("Thanks!"));

    // A draft shows up in redpanda's drafts.
    let draft = call_json(
        &client,
        "create_draft",
        json!({"from": user, "to": [user], "subject": "Later", "body": "Draft text"}),
    )
    .await;
    let draft_id = draft["draftId"].as_i64().unwrap();
    assert_eq!(app.get_draft(draft_id).unwrap().subject, "Later");

    // A new key locks out the old one right away.
    *key.write().unwrap() = "rp_rotated".into();
    assert_eq!(post(Some(KEY)).send().await.unwrap().status(), 401);

    let _ = client.cancel().await;
    cancel.cancel();
    handle.await.unwrap();
}
