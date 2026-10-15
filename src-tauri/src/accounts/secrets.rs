//! Credentials are encrypted with ChaCha20-Poly1305. The 256-bit key lives in the Windows
//! Credential Manager; ciphertext and nonce live in SQLite (see ADR 0002).

use base64::Engine;
use chacha20poly1305::aead::{Aead, KeyInit, OsRng};
use chacha20poly1305::{AeadCore, ChaCha20Poly1305, Key, Nonce};
use rand::RngCore;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};
use crate::store::queries;

const KEYRING_SERVICE: &str = "redpanda";
const KEYRING_USER: &str = "master-key";
const AUTH_KIND: &str = "auth";

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StoredSecret {
    Password {
        password: String,
    },
    Oauth {
        access_token: String,
        refresh_token: String,
        /// Unix timestamp when the access token expires.
        expires_at: i64,
    },
}

// Never print secrets, not even in debug logs.
impl std::fmt::Debug for StoredSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoredSecret::Password { .. } => f.write_str("Password(***)"),
            StoredSecret::Oauth { expires_at, .. } => write!(f, "Oauth(expires_at={expires_at})"),
        }
    }
}

pub struct Secrets {
    cipher: ChaCha20Poly1305,
}

impl Secrets {
    /// Loads the master key from the OS keyring, creating it on first start.
    pub fn load_or_create() -> Result<Secrets> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
            .map_err(|e| AppError::Secrets(e.to_string()))?;
        let key = match entry.get_password() {
            Ok(b64) => base64::engine::general_purpose::STANDARD
                .decode(b64.trim())
                .map_err(|e| AppError::Secrets(format!("invalid master key: {e}")))?,
            Err(keyring::Error::NoEntry) => {
                let mut key = vec![0u8; 32];
                rand::thread_rng().fill_bytes(&mut key);
                entry
                    .set_password(&base64::engine::general_purpose::STANDARD.encode(&key))
                    .map_err(|e| AppError::Secrets(e.to_string()))?;
                key
            }
            Err(e) => return Err(AppError::Secrets(e.to_string())),
        };
        Self::from_key(&key)
    }

    pub fn from_key(key: &[u8]) -> Result<Secrets> {
        if key.len() != 32 {
            return Err(AppError::Secrets("master key must be 32 bytes".into()));
        }
        Ok(Secrets {
            cipher: ChaCha20Poly1305::new(Key::from_slice(key)),
        })
    }

    pub fn encrypt(&self, plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
        let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
        let ct = self
            .cipher
            .encrypt(&nonce, plaintext)
            .map_err(|_| AppError::Secrets("encryption failed".into()))?;
        Ok((ct, nonce.to_vec()))
    }

    pub fn decrypt(&self, ciphertext: &[u8], nonce: &[u8]) -> Result<Vec<u8>> {
        if nonce.len() != 12 {
            return Err(AppError::Secrets("invalid nonce".into()));
        }
        self.cipher
            .decrypt(Nonce::from_slice(nonce), ciphertext)
            .map_err(|_| AppError::Secrets("decryption failed".into()))
    }

    pub fn store(&self, c: &Connection, account_id: &str, secret: &StoredSecret) -> Result<()> {
        let json = serde_json::to_vec(secret)?;
        let (ct, nonce) = self.encrypt(&json)?;
        queries::put_secret(c, account_id, AUTH_KIND, &ct, &nonce)
    }

    pub fn load(&self, c: &Connection, account_id: &str) -> Result<StoredSecret> {
        let (ct, nonce) = queries::get_secret(c, account_id, AUTH_KIND)?.ok_or(AppError::AuthRequired)?;
        // A lost or replaced keyring entry makes old secrets unreadable: ask for a new sign-in.
        let plain = self.decrypt(&ct, &nonce).map_err(|_| AppError::AuthRequired)?;
        Ok(serde_json::from_slice(&plain)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Db;

    #[test]
    fn round_trip() {
        let s = Secrets::from_key(&[7u8; 32]).unwrap();
        let (ct, nonce) = s.encrypt(b"hunter2").unwrap();
        assert_ne!(ct, b"hunter2");
        assert_eq!(s.decrypt(&ct, &nonce).unwrap(), b"hunter2");
        let (ct2, nonce2) = s.encrypt(b"hunter2").unwrap();
        assert_ne!(nonce, nonce2, "nonces must be unique");
        assert_ne!(ct, ct2);
    }

    #[test]
    fn wrong_key_fails() {
        let a = Secrets::from_key(&[1u8; 32]).unwrap();
        let b = Secrets::from_key(&[2u8; 32]).unwrap();
        let (ct, nonce) = a.encrypt(b"x").unwrap();
        assert!(b.decrypt(&ct, &nonce).is_err());
        let mut tampered = ct.clone();
        tampered[0] ^= 1;
        assert!(a.decrypt(&tampered, &nonce).is_err());
    }

    #[test]
    fn stores_in_db() {
        let db = Db::open_in_memory().unwrap();
        let s = Secrets::from_key(&[3u8; 32]).unwrap();
        db.with(|c| {
            crate::store::queries::insert_account(c, &crate::store::queries_test_account("acc"))?;
            let secret = StoredSecret::Oauth {
                access_token: "a".repeat(3000),
                refresh_token: "r".repeat(3000),
                expires_at: 42,
            };
            s.store(c, "acc", &secret)?;
            match s.load(c, "acc")? {
                StoredSecret::Oauth {
                    refresh_token,
                    expires_at,
                    ..
                } => {
                    assert_eq!(refresh_token.len(), 3000);
                    assert_eq!(expires_at, 42);
                }
                _ => panic!("wrong variant"),
            }
            let other = Secrets::from_key(&[4u8; 32]).unwrap();
            assert!(matches!(other.load(c, "acc"), Err(AppError::AuthRequired)));
            assert_eq!(format!("{secret:?}"), "Oauth(expires_at=42)");
            Ok(())
        })
        .unwrap();
    }
}
