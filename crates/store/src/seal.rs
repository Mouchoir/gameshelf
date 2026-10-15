//! Sealing of platform credentials and notification secrets (research D5, FR-041).

use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit},
};
use secrecy::{ExposeSecret, SecretBox};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

/// Nonce length of XChaCha20-Poly1305.
pub const NONCE_LEN: usize = 24;

#[derive(Debug, thiserror::Error)]
pub enum SealError {
    #[error("sealing failed")]
    Seal,
    #[error("the secret cannot be opened with the configured key (key {expected} expected, {found} found)")]
    WrongKey { expected: String, found: String },
    #[error("the secret is corrupted or was sealed with another key")]
    Open,
    #[error("no randomness available")]
    Random,
}

/// A sealed value as stored in the database.
#[derive(Clone, PartialEq, Eq)]
pub struct Sealed {
    pub ciphertext: Vec<u8>,
    pub nonce: Vec<u8>,
    pub key_id: String,
}

impl std::fmt::Debug for Sealed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sealed")
            .field("key_id", &self.key_id)
            .field("len", &self.ciphertext.len())
            .finish()
    }
}

/// Seals and opens secrets with the operator key.
#[derive(Clone)]
pub struct Sealer {
    cipher: XChaCha20Poly1305,
    key_id: String,
}

impl std::fmt::Debug for Sealer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sealer")
            .field("key_id", &self.key_id)
            .finish_non_exhaustive()
    }
}

impl Sealer {
    /// Builds a sealer from the 32-byte operator key.
    pub fn new(key: &SecretBox<[u8; 32]>) -> Self {
        let raw = key.expose_secret();
        let cipher = XChaCha20Poly1305::new(raw.into());
        let digest = Sha256::digest(raw);
        let key_id = hex(&digest[..4]);
        Self { cipher, key_id }
    }

    /// Short public identifier of the key, stored next to each ciphertext to support rotation.
    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    pub fn seal(&self, plaintext: &[u8]) -> Result<Sealed, SealError> {
        let mut nonce = [0u8; NONCE_LEN];
        getrandom::fill(&mut nonce).map_err(|_| SealError::Random)?;
        let ciphertext = self
            .cipher
            .encrypt(&XNonce::from(nonce), plaintext)
            .map_err(|_| SealError::Seal)?;
        Ok(Sealed {
            ciphertext,
            nonce: nonce.to_vec(),
            key_id: self.key_id.clone(),
        })
    }

    pub fn open(&self, sealed: &Sealed) -> Result<Zeroizing<Vec<u8>>, SealError> {
        if sealed.key_id != self.key_id {
            return Err(SealError::WrongKey {
                expected: self.key_id.clone(),
                found: sealed.key_id.clone(),
            });
        }
        let nonce: [u8; NONCE_LEN] = sealed.nonce.as_slice().try_into().map_err(|_| SealError::Open)?;
        self.cipher
            .decrypt(&XNonce::from(nonce), sealed.ciphertext.as_slice())
            .map(Zeroizing::new)
            .map_err(|_| SealError::Open)
    }

    /// Seals a JSON-serializable value.
    pub fn seal_json<T: serde::Serialize>(&self, value: &T) -> Result<Sealed, SealError> {
        let bytes = Zeroizing::new(serde_json::to_vec(value).map_err(|_| SealError::Seal)?);
        self.seal(&bytes)
    }

    /// Opens a sealed JSON value.
    pub fn open_json<T: serde::de::DeserializeOwned>(&self, sealed: &Sealed) -> Result<T, SealError> {
        let bytes = self.open(sealed)?;
        serde_json::from_slice(&bytes).map_err(|_| SealError::Open)
    }
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(DIGITS[(b >> 4) as usize] as char);
        s.push(DIGITS[(b & 0x0f) as usize] as char);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(byte: u8) -> SecretBox<[u8; 32]> {
        SecretBox::new(Box::new([byte; 32]))
    }

    #[test]
    fn round_trip() {
        let sealer = Sealer::new(&key(7));
        let sealed = sealer.seal(b"refresh-token-value").expect("seal");
        assert_ne!(sealed.ciphertext, b"refresh-token-value");
        assert_eq!(sealed.nonce.len(), NONCE_LEN);
        let opened = sealer.open(&sealed).expect("open");
        assert_eq!(opened.as_slice(), b"refresh-token-value");
    }

    #[test]
    fn nonces_differ_between_seals() {
        let sealer = Sealer::new(&key(7));
        let a = sealer.seal(b"x").expect("seal");
        let b = sealer.seal(b"x").expect("seal");
        assert_ne!(a.nonce, b.nonce);
        assert_ne!(a.ciphertext, b.ciphertext);
    }

    #[test]
    fn another_key_cannot_open() {
        let sealed = Sealer::new(&key(1)).seal(b"secret").expect("seal");
        let other = Sealer::new(&key(2));
        assert!(matches!(other.open(&sealed), Err(SealError::WrongKey { .. })));
    }

    #[test]
    fn tampering_is_detected() {
        let sealer = Sealer::new(&key(3));
        let mut sealed = sealer.seal(b"secret").expect("seal");
        sealed.ciphertext[0] ^= 0xff;
        assert!(matches!(sealer.open(&sealed), Err(SealError::Open)));
    }

    #[test]
    fn debug_never_shows_content() {
        let sealer = Sealer::new(&key(4));
        let sealed = sealer.seal(b"super-secret").expect("seal");
        let text = format!("{sealed:?} {sealer:?}");
        assert!(!text.contains("super-secret"));
    }

    #[test]
    fn json_round_trip() {
        let sealer = Sealer::new(&key(5));
        let sealed = sealer
            .seal_json(&serde_json::json!({"api_key": "abc"}))
            .expect("seal");
        let value: serde_json::Value = sealer.open_json(&sealed).expect("open");
        assert_eq!(value["api_key"], "abc");
    }
}
