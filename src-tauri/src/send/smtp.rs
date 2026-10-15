//! SMTP delivery through lettre with password or XOAUTH2.

use std::time::Duration;

use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::accounts::{self, LoginSecret};
use crate::core::Core;
use crate::error::{AppError, Result};
use crate::model::{Account, Security, ServerConfig};

pub fn transport(
    cfg: &ServerConfig,
    user: &str,
    secret: &LoginSecret,
) -> Result<AsyncSmtpTransport<Tokio1Executor>> {
    let builder = match cfg.security {
        Security::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(&cfg.host)?,
        Security::StartTls => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&cfg.host)?,
        Security::None => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&cfg.host),
    };
    let (pass, mechanisms) = match secret {
        LoginSecret::Password(p) => (p.clone(), vec![Mechanism::Plain, Mechanism::Login]),
        LoginSecret::XOAuth2(t) => (t.clone(), vec![Mechanism::Xoauth2]),
    };
    Ok(builder
        .port(cfg.port)
        .timeout(Some(Duration::from_secs(60)))
        .credentials(Credentials::new(user.to_string(), pass))
        .authentication(mechanisms)
        .build())
}

/// Connects and authenticates without sending anything.
pub async fn verify_login(cfg: &ServerConfig, user: &str, secret: &LoginSecret) -> Result<()> {
    let ok = transport(cfg, user, secret)?.test_connection().await?;
    if ok {
        Ok(())
    } else {
        Err(AppError::Smtp("server did not accept the connection".into()))
    }
}

pub async fn send(core: &Core, account: &Account, message: &Message) -> Result<()> {
    let secret = accounts::login_secret(core, account).await?;
    let first = transport(&account.smtp, &account.username, &secret)?
        .send(message.clone())
        .await;
    match first {
        Ok(_) => Ok(()),
        Err(e) => {
            let err = AppError::from(e);
            if err.is_auth_error() && matches!(secret, LoginSecret::XOAuth2(_)) {
                accounts::invalidate_access_token(core, &account.id)?;
                let secret = accounts::login_secret(core, account).await?;
                transport(&account.smtp, &account.username, &secret)?
                    .send(message.clone())
                    .await?;
                Ok(())
            } else {
                Err(err)
            }
        }
    }
}
