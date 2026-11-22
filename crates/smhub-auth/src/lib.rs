//! Password hashes (decision D41), session tokens and the mosquitto password format.
//!
//! Hashes stay compatible with the database the old backend wrote: Argon2 in PHC form
//! (`argon2::Argon2::default()`, as `smhub_rs` used), and legacy bcrypt hashes that still verify
//! and are replaced at the next sign-in.

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256, Sha512};
use smhub_model::{Error, Result};

/// Fill a buffer with random bytes from the kernel.
pub fn random_bytes<const N: usize>() -> Result<[u8; N]> {
    let mut buf = [0u8; N];
    getrandom::fill(&mut buf).map_err(|e| Error::internal(format!("random: {e}")))?;
    Ok(buf)
}

/// Argon2id hash in PHC form with the library's default parameters.
pub fn hash_password(password: &str) -> Result<String> {
    let salt_bytes: [u8; 16] = random_bytes()?;
    let salt = SaltString::encode_b64(&salt_bytes).map_err(|e| Error::internal(e.to_string()))?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| Error::internal(e.to_string()))
}

/// Whether the hash is a legacy bcrypt hash that should be replaced.
pub fn is_legacy(hash: &str) -> bool {
    hash.starts_with("$2b$") || hash.starts_with("$2a$") || hash.starts_with("$2y$")
}

/// Check a password against an Argon2 or bcrypt hash.
pub fn verify_password(password: &str, hash: &str) -> bool {
    if is_legacy(hash) {
        return bcrypt::verify(password, hash).unwrap_or(false);
    }
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok(),
        Err(_) => false,
    }
}

/// A new session token for the cookie, and the hash the database keeps.
pub fn new_token() -> Result<(String, String)> {
    let bytes: [u8; 32] = random_bytes()?;
    let token = URL_SAFE_NO_PAD.encode(bytes);
    let hash = token_hash(&token);
    Ok((token, hash))
}

/// SHA-256 of a token, in hex. Only this is stored.
pub fn token_hash(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

/// Lower-case hex.
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(char::from(DIGITS[usize::from(b >> 4)]));
        s.push(char::from(DIGITS[usize::from(b & 0x0f)]));
    }
    s
}

/// A short random identifier (operations, uploads).
pub fn new_id() -> Result<String> {
    let bytes: [u8; 9] = random_bytes()?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// A random UUID (version 4), as the old backend used for user ids.
pub fn new_uuid() -> Result<String> {
    let mut b: [u8; 16] = random_bytes()?;
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h = hex(&b);
    Ok(format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32]))
}

/// A line for mosquitto's password file in its `$7$` format (PBKDF2-SHA512, 1000 rounds,
/// 12-byte salt), exactly as the old backend's `smhub_rs` wrote it.
pub fn mosquitto_entry(username: &str, password: &str) -> Result<String> {
    if username.is_empty() || username.contains([':', '\n', '\r']) {
        return Err(Error::invalid("username", "a broker user name cannot contain ':' or line breaks"));
    }
    if password.contains(['\n', '\r']) {
        return Err(Error::invalid("password", "a password cannot contain line breaks"));
    }
    let salt: [u8; 12] = random_bytes()?;
    let mut out = [0u8; 64];
    pbkdf2::pbkdf2_hmac::<Sha512>(password.as_bytes(), &salt, 1000, &mut out);
    Ok(format!("{username}:$7$1000${}${}", STANDARD.encode(salt), STANDARD.encode(out)))
}

/// A new ESPHome API encryption key: 32 random bytes in base64.
pub fn new_esphome_key() -> Result<String> {
    let bytes: [u8; 32] = random_bytes()?;
    Ok(STANDARD.encode(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify() -> Result<()> {
        let h = hash_password("correct horse")?;
        assert!(h.starts_with("$argon2id$"));
        assert!(verify_password("correct horse", &h));
        assert!(!verify_password("wrong", &h));
        assert!(!verify_password("x", "not a hash"));
        Ok(())
    }

    #[test]
    fn bcrypt_legacy() {
        let h = bcrypt::hash("pw", 4).unwrap_or_default();
        assert!(is_legacy(&h));
        assert!(verify_password("pw", &h));
    }

    #[test]
    fn tokens() -> Result<()> {
        let (t, h) = new_token()?;
        assert_eq!(token_hash(&t), h);
        assert_eq!(h.len(), 64);
        assert_eq!(new_uuid()?.len(), 36);
        Ok(())
    }

    #[test]
    fn mosquitto() -> Result<()> {
        let e = mosquitto_entry("bob", "secret")?;
        assert!(e.starts_with("bob:$7$1000$"));
        assert!(mosquitto_entry("a:b", "x").is_err());
        Ok(())
    }
}
