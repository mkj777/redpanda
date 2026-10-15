//! Built-in table of well-known mail providers.

use crate::model::{AuthType, Security, ServerConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Username {
    Email,
    LocalPart,
}

#[derive(Debug)]
pub struct Provider {
    pub id: &'static str,
    pub name: &'static str,
    pub domains: &'static [&'static str],
    /// Suffixes of MX hosts that identify this provider for custom domains.
    pub mx_suffixes: &'static [&'static str],
    pub imap: (&'static str, u16, Security),
    pub smtp: (&'static str, u16, Security),
    pub auth: AuthType,
    /// A password or app password can be used instead of OAuth.
    pub allow_password: bool,
    pub username: Username,
    /// i18n key of a setup hint shown in the account wizard.
    pub hint: Option<&'static str>,
    /// The server copies mail sent via SMTP into the Sent folder itself.
    pub sent_saved_by_server: bool,
    pub is_gmail: bool,
}

impl Provider {
    pub fn imap_config(&self) -> ServerConfig {
        ServerConfig {
            host: self.imap.0.into(),
            port: self.imap.1,
            security: self.imap.2,
        }
    }

    pub fn smtp_config(&self) -> ServerConfig {
        ServerConfig {
            host: self.smtp.0.into(),
            port: self.smtp.1,
            security: self.smtp.2,
        }
    }

    pub fn username_for(&self, email: &str) -> String {
        match self.username {
            Username::Email => email.to_string(),
            Username::LocalPart => email.split('@').next().unwrap_or(email).to_string(),
        }
    }
}

use Security::{StartTls, Tls};

pub static PROVIDERS: &[Provider] = &[
    Provider {
        id: "google",
        name: "Google",
        domains: &["gmail.com", "googlemail.com"],
        mx_suffixes: &["google.com", "googlemail.com"],
        imap: ("imap.gmail.com", 993, Tls),
        smtp: ("smtp.gmail.com", 465, Tls),
        auth: AuthType::OauthGoogle,
        allow_password: true,
        username: Username::Email,
        hint: Some("hint.google_app_password"),
        sent_saved_by_server: true,
        is_gmail: true,
    },
    Provider {
        id: "microsoft",
        name: "Microsoft",
        domains: &[
            "outlook.com",
            "outlook.de",
            "outlook.at",
            "outlook.fr",
            "hotmail.com",
            "hotmail.de",
            "hotmail.co.uk",
            "hotmail.fr",
            "hotmail.it",
            "hotmail.es",
            "live.com",
            "live.de",
            "live.at",
            "live.co.uk",
            "live.fr",
            "msn.com",
            "passport.com",
        ],
        mx_suffixes: &["mail.protection.outlook.com", "olc.protection.outlook.com"],
        imap: ("outlook.office365.com", 993, Tls),
        smtp: ("smtp.office365.com", 587, StartTls),
        auth: AuthType::OauthMicrosoft,
        allow_password: false,
        username: Username::Email,
        hint: None,
        sent_saved_by_server: true,
        is_gmail: false,
    },
    Provider {
        id: "gmx",
        name: "GMX",
        domains: &["gmx.de", "gmx.net", "gmx.at", "gmx.ch", "gmx.eu"],
        mx_suffixes: &["gmx.net"],
        imap: ("imap.gmx.net", 993, Tls),
        smtp: ("mail.gmx.net", 587, StartTls),
        auth: AuthType::Password,
        allow_password: true,
        username: Username::Email,
        hint: Some("hint.enable_imap_gmx"),
        sent_saved_by_server: false,
        is_gmail: false,
    },
    Provider {
        id: "gmx_com",
        name: "GMX",
        domains: &["gmx.com", "gmx.us", "gmx.co.uk", "gmx.fr", "gmx.es"],
        mx_suffixes: &["gmx.com"],
        imap: ("imap.gmx.com", 993, Tls),
        smtp: ("mail.gmx.com", 587, StartTls),
        auth: AuthType::Password,
        allow_password: true,
        username: Username::Email,
        hint: Some("hint.enable_imap_gmx"),
        sent_saved_by_server: false,
        is_gmail: false,
    },
    Provider {
        id: "webde",
        name: "WEB.DE",
        domains: &["web.de"],
        mx_suffixes: &["web.de"],
        imap: ("imap.web.de", 993, Tls),
        smtp: ("smtp.web.de", 587, StartTls),
        auth: AuthType::Password,
        allow_password: true,
        username: Username::Email,
        hint: Some("hint.enable_imap_webde"),
        sent_saved_by_server: false,
        is_gmail: false,
    },
    Provider {
        id: "icloud",
        name: "iCloud",
        domains: &["icloud.com", "me.com", "mac.com"],
        mx_suffixes: &["mail.icloud.com"],
        imap: ("imap.mail.me.com", 993, Tls),
        smtp: ("smtp.mail.me.com", 587, StartTls),
        auth: AuthType::AppPassword,
        allow_password: true,
        username: Username::Email,
        hint: Some("hint.icloud_app_password"),
        sent_saved_by_server: false,
        is_gmail: false,
    },
    Provider {
        id: "yahoo",
        name: "Yahoo",
        domains: &[
            "yahoo.com",
            "yahoo.de",
            "yahoo.co.uk",
            "yahoo.fr",
            "yahoo.it",
            "yahoo.es",
            "ymail.com",
            "rocketmail.com",
        ],
        mx_suffixes: &["yahoodns.net"],
        imap: ("imap.mail.yahoo.com", 993, Tls),
        smtp: ("smtp.mail.yahoo.com", 465, Tls),
        auth: AuthType::AppPassword,
        allow_password: true,
        username: Username::Email,
        hint: Some("hint.yahoo_app_password"),
        sent_saved_by_server: false,
        is_gmail: false,
    },
    Provider {
        id: "tonline",
        name: "Telekom",
        domains: &["t-online.de", "magenta.de"],
        mx_suffixes: &["t-online.de"],
        imap: ("secureimap.t-online.de", 993, Tls),
        smtp: ("securesmtp.t-online.de", 465, Tls),
        auth: AuthType::Password,
        allow_password: true,
        username: Username::Email,
        hint: Some("hint.tonline_mail_password"),
        sent_saved_by_server: false,
        is_gmail: false,
    },
    Provider {
        id: "posteo",
        name: "Posteo",
        domains: &[
            "posteo.de",
            "posteo.net",
            "posteo.eu",
            "posteo.at",
            "posteo.ch",
            "posteo.org",
        ],
        mx_suffixes: &["posteo.de"],
        imap: ("posteo.de", 993, Tls),
        smtp: ("posteo.de", 587, StartTls),
        auth: AuthType::Password,
        allow_password: true,
        username: Username::Email,
        hint: None,
        sent_saved_by_server: false,
        is_gmail: false,
    },
    Provider {
        id: "mailboxorg",
        name: "mailbox.org",
        domains: &["mailbox.org"],
        mx_suffixes: &["mailbox.org"],
        imap: ("imap.mailbox.org", 993, Tls),
        smtp: ("smtp.mailbox.org", 465, Tls),
        auth: AuthType::Password,
        allow_password: true,
        username: Username::Email,
        hint: None,
        sent_saved_by_server: false,
        is_gmail: false,
    },
    Provider {
        id: "ionos",
        name: "IONOS",
        domains: &["online.de", "onlinehome.de"],
        mx_suffixes: &["ionos.de", "kundenserver.de", "1and1.com"],
        imap: ("imap.ionos.de", 993, Tls),
        smtp: ("smtp.ionos.de", 465, Tls),
        auth: AuthType::Password,
        allow_password: true,
        username: Username::Email,
        hint: None,
        sent_saved_by_server: false,
        is_gmail: false,
    },
];

pub fn by_domain(domain: &str) -> Option<&'static Provider> {
    let domain = domain.to_ascii_lowercase();
    PROVIDERS.iter().find(|p| p.domains.iter().any(|d| *d == domain))
}

pub fn by_id(id: &str) -> Option<&'static Provider> {
    PROVIDERS.iter().find(|p| p.id == id)
}

/// Identifies the provider behind a custom domain from its MX hosts.
pub fn by_mx(mx_hosts: &[String]) -> Option<&'static Provider> {
    mx_hosts.iter().find_map(|host| {
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        PROVIDERS.iter().find(|p| {
            p.mx_suffixes
                .iter()
                .any(|s| host == *s || host.ends_with(&format!(".{s}")))
        })
    })
}

/// Identifies a provider from a server host name, e.g. found via autoconfig.
pub fn by_imap_host(host: &str) -> Option<&'static Provider> {
    let host = host.to_ascii_lowercase();
    match host.as_str() {
        "imap.gmail.com" | "imap.googlemail.com" => by_id("google"),
        "outlook.office365.com" | "imap-mail.outlook.com" | "outlook.office.com" => by_id("microsoft"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_lookup() {
        assert_eq!(by_domain("GMAIL.com").unwrap().id, "google");
        assert_eq!(by_domain("hotmail.de").unwrap().id, "microsoft");
        assert_eq!(by_domain("gmx.net").unwrap().id, "gmx");
        assert_eq!(by_domain("web.de").unwrap().id, "webde");
        assert!(by_domain("example.com").is_none());
    }

    #[test]
    fn mx_classification() {
        let google = vec![
            "aspmx.l.google.com.".to_string(),
            "alt1.aspmx.l.google.com.".into(),
        ];
        assert_eq!(by_mx(&google).unwrap().id, "google");
        let ms = vec!["contoso-com.mail.protection.outlook.com.".to_string()];
        assert_eq!(by_mx(&ms).unwrap().id, "microsoft");
        let ionos = vec!["mx00.ionos.de".to_string()];
        assert_eq!(by_mx(&ionos).unwrap().id, "ionos");
        let other = vec!["mail.example.com".to_string(), "notgoogle.com".into()];
        assert!(by_mx(&other).is_none());
    }

    #[test]
    fn usernames() {
        let p = by_id("google").unwrap();
        assert_eq!(p.username_for("a@gmail.com"), "a@gmail.com");
    }
}
