//! Password hashing with Argon2id (research D4).

use crate::{Result, StoreError};
use argon2::{
    Argon2,
    password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash},
};
use secrecy::{ExposeSecret, SecretString};

/// Minimum password length accepted for a profile.
pub const MIN_PASSWORD_LEN: usize = 10;

/// Hashes a password into a PHC string. Runs on a blocking thread because Argon2 is deliberately slow.
pub async fn hash(password: &SecretString) -> Result<String> {
    if password.expose_secret().chars().count() < MIN_PASSWORD_LEN {
        return Err(StoreError::invalid(format!(
            "the password must have at least {MIN_PASSWORD_LEN} characters"
        )));
    }
    let password = password.clone();
    tokio::task::spawn_blocking(move || hash_blocking(&password))
        .await
        .map_err(|_| StoreError::Hash)?
}

fn hash_blocking(password: &SecretString) -> Result<String> {
    Argon2::default()
        .hash_password(password.expose_secret().as_bytes())
        .map(|h| h.to_string())
        .map_err(|_| StoreError::Hash)
}

/// Verifies a password against a PHC string. Returns `false` for any malformed hash.
pub async fn verify(password: &SecretString, phc: &str) -> bool {
    let password = password.clone();
    let phc = phc.to_string();
    tokio::task::spawn_blocking(move || verify_blocking(&password, &phc))
        .await
        .unwrap_or(false)
}

fn verify_blocking(password: &SecretString, phc: &str) -> bool {
    match PasswordHash::new(phc) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.expose_secret().as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

/// A valid hash, verified when the username does not exist so that sign-in takes the same time either way.
pub(crate) fn dummy_hash() -> &'static str {
    static DUMMY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    DUMMY.get_or_init(|| {
        hash_blocking(&SecretString::from("gameshelf timing equalizer".to_string())).unwrap_or_default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn hash_then_verify() {
        let pw = SecretString::from("correct horse battery".to_string());
        let phc = hash(&pw).await.expect("hash");
        assert!(phc.starts_with("$argon2id$"));
        assert!(verify(&pw, &phc).await);
        assert!(!verify(&SecretString::from("wrong password!".to_string()), &phc).await);
    }

    #[tokio::test]
    async fn short_passwords_are_refused() {
        let err = hash(&SecretString::from("short".to_string()))
            .await
            .expect_err("must fail");
        assert!(matches!(err, StoreError::Invalid(_)));
    }

    #[tokio::test]
    async fn malformed_hash_never_verifies() {
        assert!(!verify(&SecretString::from("anything at all".to_string()), "not-a-hash").await);
        assert!(!verify(&SecretString::from("anything at all".to_string()), dummy_hash()).await);
        assert!(dummy_hash().starts_with("$argon2id$"));
    }
}
