//! Data types shared between the store, the sync engine and the frontend.

use serde::{Deserialize, Serialize};

use crate::unsubscribe::Method as UnsubscribeMethod;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Security {
    /// Implicit TLS (IMAP 993, SMTP 465).
    Tls,
    /// Plain connection upgraded with STARTTLS (IMAP 143, SMTP 587).
    StartTls,
    /// No encryption. Only for local bridges and tests.
    None,
}

impl Security {
    pub fn as_str(self) -> &'static str {
        match self {
            Security::Tls => "tls",
            Security::StartTls => "start_tls",
            Security::None => "none",
        }
    }

    pub fn parse(s: &str) -> Security {
        match s {
            "start_tls" => Security::StartTls,
            "none" => Security::None,
            _ => Security::Tls,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthType {
    Password,
    /// Password accounts where the provider requires an app-specific password.
    AppPassword,
    OauthGoogle,
    OauthMicrosoft,
}

impl AuthType {
    pub fn as_str(self) -> &'static str {
        match self {
            AuthType::Password => "password",
            AuthType::AppPassword => "app_password",
            AuthType::OauthGoogle => "oauth_google",
            AuthType::OauthMicrosoft => "oauth_microsoft",
        }
    }

    pub fn parse(s: &str) -> AuthType {
        match s {
            "app_password" => AuthType::AppPassword,
            "oauth_google" => AuthType::OauthGoogle,
            "oauth_microsoft" => AuthType::OauthMicrosoft,
            _ => AuthType::Password,
        }
    }

    pub fn is_oauth(self) -> bool {
        matches!(self, AuthType::OauthGoogle | AuthType::OauthMicrosoft)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub security: Security,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub provider: String,
    pub auth_type: AuthType,
    pub username: String,
    pub imap: ServerConfig,
    pub smtp: ServerConfig,
    pub color: String,
    pub sent_saved_by_server: bool,
    pub is_gmail: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct Address {
    pub name: Option<String>,
    pub email: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub id: i64,
    pub account_id: String,
    pub path: String,
    pub name: String,
    pub depth: u32,
    pub role: String,
    pub unread_count: i64,
    pub total_count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSummary {
    pub id: i64,
    pub account_id: String,
    pub folder_id: i64,
    pub thread_key: String,
    pub from: Option<Address>,
    pub to: Vec<Address>,
    pub subject: String,
    pub date: i64,
    pub seen: bool,
    pub flagged: bool,
    pub answered: bool,
    pub has_attachments: bool,
    pub preview: String,
    /// Number of messages in the conversation when the list is grouped by thread.
    pub thread_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentInfo {
    pub index: u32,
    pub filename: String,
    pub mime_type: String,
    pub size: u64,
    pub inline: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageDetail {
    #[serde(flatten)]
    pub summary: MessageSummary,
    pub cc: Vec<Address>,
    pub reply_to: Vec<Address>,
    pub message_id: Option<String>,
    pub references: Vec<String>,
    pub text: Option<String>,
    pub html: Option<String>,
    pub has_remote_content: bool,
    pub remote_allowed: bool,
    pub attachments: Vec<AttachmentInfo>,
    /// How to leave the newsletter this message came from. Never set in the spam folder, where
    /// unsubscribing would only confirm the address to the sender.
    pub unsubscribe: Option<UnsubscribeInfo>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnsubscribeInfo {
    pub method: UnsubscribeMethod,
    /// The mail address or the host of the page, shown before the user confirms.
    pub target: String,
    /// There is an unsubscribe page to fall back to when one-click or the mail fails.
    pub has_page: bool,
    pub unsubscribed_at: Option<i64>,
}

/// A newsletter sender in one account, for managing subscriptions.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    pub account_id: String,
    pub sender: Address,
    /// The newest message, the one unsubscribing goes through.
    pub latest_id: i64,
    pub last_date: i64,
    pub total: i64,
    /// Mails in the last 30 days.
    pub recent: i64,
    /// Mails that arrived after unsubscribing.
    pub since_unsubscribed: i64,
    #[serde(flatten)]
    pub unsubscribe: UnsubscribeInfo,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePage {
    pub items: Vec<MessageSummary>,
    pub has_more: bool,
}

/// What the message list shows.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum View {
    Folder {
        #[serde(rename = "folderId")]
        folder_id: i64,
    },
    Unified {
        which: UnifiedView,
    },
    Search {
        query: String,
    },
    /// Mail from one sender in one account, for the subscriptions view.
    Sender {
        #[serde(rename = "accountId")]
        account_id: String,
        email: String,
    },
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum UnifiedView {
    Inbox,
    Unread,
    Starred,
    Sent,
    Junk,
    Trash,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UnifiedCounts {
    pub inbox_unread: i64,
    pub unread: i64,
    pub starred: i64,
    pub drafts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    #[serde(default)]
    pub id: Option<i64>,
    pub account_id: String,
    /// One of the account's aliases to send from. `None` is the account's own address.
    #[serde(default)]
    pub from_email: Option<String>,
    #[serde(default)]
    pub to: Vec<Address>,
    #[serde(default)]
    pub cc: Vec<Address>,
    #[serde(default)]
    pub bcc: Vec<Address>,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub in_reply_to: Option<String>,
    #[serde(default)]
    pub references: Vec<String>,
    #[serde(default)]
    pub attachments: Vec<String>,
    /// Local message id this draft replies to (gets `\Answered` after sending).
    #[serde(default)]
    pub reply_to_message_id: Option<i64>,
    /// Local message id whose attachments are forwarded.
    #[serde(default)]
    pub forward_from_message_id: Option<i64>,
    #[serde(default)]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SyncState {
    Idle,
    Syncing,
    Error,
    AuthRequired,
    Offline,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub account_id: String,
    pub state: SyncState,
    pub error: Option<String>,
}
