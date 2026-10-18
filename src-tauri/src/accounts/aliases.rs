//! Further addresses a Gmail account may send from. Gmail lists them under "Send mail as":
//! addresses the user verified there and aliases a Workspace admin added. The Gmail API returns
//! them for the same token IMAP and SMTP use (see ADR 0007).

use std::time::Duration;

use serde::Deserialize;

use super::{invalidate_access_token, login_secret, LoginSecret};
use crate::core::Core;
use crate::error::{AppError, Result};
use crate::model::{Account, Address, AuthType};
use crate::store::queries;

const SEND_AS_URL: &str = "https://gmail.googleapis.com/gmail/v1/users/me/settings/sendAs";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendAsList {
    #[serde(default)]
    send_as: Vec<SendAs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendAs {
    send_as_email: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    is_primary: bool,
    /// Set for addresses the user added under "Send mail as": "accepted" once the user
    /// confirmed the code Gmail mailed there, "pending" until then. Absent for domain aliases.
    #[serde(default)]
    verification_status: Option<String>,
}

/// The usable addresses from a sendAs response, without the account's own.
fn parse(own: &str, body: &str) -> Result<Vec<Address>> {
    let list: SendAsList = serde_json::from_str(body)?;
    let mut out: Vec<Address> = Vec::new();
    for s in list.send_as {
        let email = s.send_as_email.trim().to_ascii_lowercase();
        let verified = s.verification_status.as_deref().is_none_or(|v| v == "accepted");
        if s.is_primary
            || !verified
            || email.eq_ignore_ascii_case(own)
            || email.parse::<lettre::Address>().is_err()
            || out.iter().any(|a| a.email == email)
        {
            continue;
        }
        let name: String = s.display_name.chars().filter(|c| !c.is_control()).collect();
        let name = name.trim();
        out.push(Address {
            name: (!name.is_empty()).then(|| name.to_string()),
            email,
        });
    }
    Ok(out)
}

async fn fetch(core: &Core, account: &Account) -> Result<Vec<Address>> {
    let mut retried = false;
    loop {
        let LoginSecret::XOAuth2(token) = login_secret(core, account).await? else {
            return Ok(vec![]);
        };
        let resp = core
            .http
            .get(SEND_AS_URL)
            .bearer_auth(&token)
            .timeout(Duration::from_secs(20))
            .send()
            .await?;
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED && !retried {
            // Revoked before its expiry time: refresh once.
            invalidate_access_token(core, &account.id)?;
            retried = true;
            continue;
        }
        if !status.is_success() {
            return Err(AppError::Other(format!("Gmail send-as settings: HTTP {status}")));
        }
        return parse(&account.email, &resp.text().await?);
    }
}

/// Reads the aliases of an account signed in with Google and stores them. Other accounts have
/// none. On failure the stored ones stay.
pub async fn refresh(core: &Core, account: &Account) -> Result<()> {
    if account.auth_type != AuthType::OauthGoogle {
        return Ok(());
    }
    let list = fetch(core, account).await?;
    if core
        .db
        .with(|c| queries::replace_aliases(c, &account.id, &list))?
    {
        tracing::info!(account = %account.id, count = list.len(), "send-as addresses changed");
        core.accounts_changed();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_verified_addresses_besides_the_own() {
        let body = r#"{"sendAs":[
            {"sendAsEmail":"kontakt@own.example","displayName":"","isPrimary":true,"isDefault":true},
            {"sendAsEmail":"Max.Old@gmail.com","displayName":"Max K","verificationStatus":"accepted",
             "smtpMsa":{"host":"smtp.gmail.com","port":587,"securityMode":"starttls"}},
            {"sendAsEmail":"info@own.example","displayName":"  "},
            {"sendAsEmail":"new@else.example","displayName":"New","verificationStatus":"pending"},
            {"sendAsEmail":"max.old@gmail.com","verificationStatus":"accepted"},
            {"sendAsEmail":"not an address","verificationStatus":"accepted"},
            {"sendAsEmail":"evil@else.example","displayName":"A\r\nBcc: x@y.z","verificationStatus":"accepted"}
        ]}"#;
        let list = parse("kontakt@own.example", body).unwrap();
        let emails: Vec<_> = list.iter().map(|a| a.email.as_str()).collect();
        assert_eq!(
            emails,
            ["max.old@gmail.com", "info@own.example", "evil@else.example"]
        );
        assert_eq!(list[0].name.as_deref(), Some("Max K"));
        assert_eq!(list[1].name, None, "a blank name falls back to the account's");
        assert_eq!(
            list[2].name.as_deref(),
            Some("ABcc: x@y.z"),
            "no line breaks in the name"
        );

        assert!(parse("a@b.c", "{}").unwrap().is_empty());
        assert!(parse("a@b.c", "<html>").is_err());
    }
}
