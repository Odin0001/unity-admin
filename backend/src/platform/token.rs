//! Opaque tokens for sessions and tenant API keys: 256 random bits,
//! base64url without padding. We only ever store the SHA-256.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use sha2::{Digest, Sha256};

/// ```
/// let token = admin::platform::token::generate();
/// assert_eq!(token.len(), 43);
/// ```
pub fn generate() -> String {
    let bytes: [u8; 32] = rand::random();
    URL_SAFE_NO_PAD.encode(bytes)
}

/// ```
/// use admin::platform::token;
///
/// assert_eq!(
///     token::hash("abc"),
///     "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
/// );
/// ```
pub fn hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_back_to_32_bytes() {
        assert_eq!(URL_SAFE_NO_PAD.decode(generate()).map(|b| b.len()), Ok(32));
    }

    #[test]
    fn never_repeats() {
        assert_ne!(generate(), generate());
    }
}
