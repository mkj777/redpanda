pub mod discovery;
pub mod oauth;
pub mod providers;
pub mod secrets;

use crate::core::Core;
use crate::error::{AppError, Result};
use crate::model::Account;
use crate::store::now;
use oauth::OAuthProvider;
use secrets::StoredSecret;

/// What IMAP and SMTP log in with.
#[derive(Clone)]
pub enum LoginSecret {
    Password(String),
    /// A valid OAuth access token.
    XOAuth2(String),
}

/// Access tokens are refreshed when they expire within this many seconds.
const REFRESH_MARGIN: i64 = 120;

/// Returns credentials for the account, refreshing the OAuth access token if needed.
pub async fn login_secret(core: &Core, account: &Account) -> Result<LoginSecret> {
    match core.db.with(|c| core.secrets.load(c, &account.id))? {
        StoredSecret::Password { password } => return Ok(LoginSecret::Password(password)),
        StoredSecret::Oauth {
            access_token,
            expires_at,
            ..
        } if expires_at - REFRESH_MARGIN > now() => {
            return Ok(LoginSecret::XOAuth2(access_token));
        }
        StoredSecret::Oauth { .. } => {}
    }

    let _guard = core.refresh_lock.lock().await;
    // Another connection may have refreshed while we waited for the lock.
    let refresh_token = match core.db.with(|c| core.secrets.load(c, &account.id))? {
        StoredSecret::Oauth {
            access_token,
            expires_at,
            ..
        } if expires_at - REFRESH_MARGIN > now() => {
            return Ok(LoginSecret::XOAuth2(access_token));
        }
        StoredSecret::Oauth { refresh_token, .. } => refresh_token,
        StoredSecret::Password { password } => return Ok(LoginSecret::Password(password)),
    };

    let provider = OAuthProvider::from_auth_type(account.auth_type)
        .ok_or_else(|| AppError::Other("OAuth secret on a password account".into()))?;
    let config = core.oauth_config();
    let creds = config.creds(provider)?;
    let tokens = match oauth::refresh(&core.http, provider, creds, &refresh_token).await {
        Ok(t) => t,
        // invalid_grant and friends: the refresh token is dead, the user has to sign in again.
        Err(AppError::Auth(msg)) => {
            tracing::warn!(account = %account.id, "token refresh rejected: {msg}");
            return Err(AppError::AuthRequired);
        }
        Err(e) => return Err(e),
    };
    let secret = StoredSecret::Oauth {
        access_token: tokens.access_token.clone(),
        refresh_token: tokens.refresh_token.unwrap_or(refresh_token),
        expires_at: tokens.expires_at,
    };
    core.db.with(|c| core.secrets.store(c, &account.id, &secret))?;
    Ok(LoginSecret::XOAuth2(tokens.access_token))
}

/// Marks the stored access token as expired so the next login refreshes it. Used when the
/// server rejects a token that has not reached its expiry time yet.
pub fn invalidate_access_token(core: &Core, account_id: &str) -> Result<()> {
    core.db.with(|c| {
        if let StoredSecret::Oauth {
            access_token,
            refresh_token,
            ..
        } = core.secrets.load(c, account_id)?
        {
            core.secrets.store(
                c,
                account_id,
                &StoredSecret::Oauth {
                    access_token,
                    refresh_token,
                    expires_at: 0,
                },
            )?;
        }
        Ok(())
    })
}

pub const ACCOUNT_COLORS: &[&str] = &[
    "#c2410c", "#2563eb", "#059669", "#7c3aed", "#db2777", "#0891b2", "#ca8a04", "#4b5563",
];
