//! Fernet token decryption.
//!
//! The old Streamlit app encrypted the DOP portal credentials with Python's
//! `cryptography.fernet.Fernet` (`main.py` → `settings.decrypt_dop_passwd`), so
//! the ciphertext sitting in the `users` collection can only be read by a
//! Fernet-compatible decryptor — no other cipher can open it.
//!
//! The `fernet` crate would do this, but it links OpenSSL; this binary already
//! uses rustls and is meant to build on macOS and Windows, so instead the small
//! and fully specified Fernet framing runs on top of the standard RustCrypto
//! primitives (`aes`, `cbc`, `hmac`, `sha2`).
//!
//! Format (github.com/fernet/spec):
//!
//! ```text
//! base64url( 0x80 | timestamp:u64be | iv:16 | AES128-CBC(plaintext) | HMAC-SHA256:32 )
//! ```
//!
//! The 32-byte key splits into a signing key (first 16 bytes) and an encryption
//! key (last 16). No TTL is enforced, matching Python's `Fernet.decrypt`, whose
//! default is `ttl=None` — stored credentials of any age must keep working.

use aes::cipher::block_padding::Pkcs7;
use aes::cipher::generic_array::GenericArray;
use aes::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use base64::Engine as _;
use hmac::digest::KeyInit as _; // brings `new_from_slice`; aliased to avoid the cipher `KeyInit`
use hmac::{Hmac, Mac};
use sha2::Sha256;

type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;
type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;
type HmacSha256 = Hmac<Sha256>;

/// Version byte every Fernet token starts with.
const VERSION: u8 = 0x80;
/// version (1) + timestamp (8) + IV (16).
const HEADER_LEN: usize = 25;
const HMAC_LEN: usize = 32;
const KEY_LEN: usize = 32;
const BLOCK_LEN: usize = 16;

fn b64() -> base64::engine::general_purpose::GeneralPurpose {
    base64::engine::general_purpose::URL_SAFE
}

/// Decrypt a Fernet token into a UTF-8 string.
pub fn decrypt(key: &str, token: &str) -> Result<String, String> {
    let plaintext = decrypt_bytes(key, token)?;
    String::from_utf8(plaintext).map_err(|_| "decrypted value is not valid UTF-8".to_string())
}

/// Split a Fernet key into the raw 32 bytes it must be.
fn parse_key(key: &str) -> Result<Vec<u8>, String> {
    let key_bytes = b64()
        .decode(key.trim())
        .map_err(|error| format!("Fernet key is not valid base64url: {error}"))?;
    if key_bytes.len() != KEY_LEN {
        return Err(format!(
            "Fernet key must be {KEY_LEN} bytes, got {}",
            key_bytes.len()
        ));
    }
    Ok(key_bytes)
}

/// A fresh Fernet key: the same 44-character base64url shape, padding included,
/// that Python's `Fernet.generate_key()` produces.
pub fn generate_key() -> Result<String, String> {
    let mut bytes = [0u8; KEY_LEN];
    getrandom::getrandom(&mut bytes).map_err(|error| format!("no system randomness: {error}"))?;
    Ok(b64().encode(&bytes))
}

/// Encrypt `plaintext` into a Fernet token, stamped with the current time.
pub fn encrypt(key: &str, plaintext: &str) -> Result<String, String> {
    let mut iv = [0u8; BLOCK_LEN];
    getrandom::getrandom(&mut iv).map_err(|error| format!("no system randomness: {error}"))?;
    encrypt_at(key, plaintext, crate::now_secs(), &iv)
}

/// Encrypt with an explicit timestamp and IV, so tests can pin the output.
pub fn encrypt_at(key: &str, plaintext: &str, timestamp: u64, iv: &[u8]) -> Result<String, String> {
    let key_bytes = parse_key(key)?;
    if iv.len() != BLOCK_LEN {
        return Err(format!("IV must be {BLOCK_LEN} bytes, got {}", iv.len()));
    }
    let (signing_key, encryption_key) = key_bytes.split_at(KEY_LEN / 2);

    // version || timestamp || IV, then the AES-CBC ciphertext.
    let mut body = Vec::with_capacity(HEADER_LEN + plaintext.len() + BLOCK_LEN);
    body.push(VERSION);
    body.extend_from_slice(&timestamp.to_be_bytes());
    body.extend_from_slice(iv);

    // `encrypt_padded_mut` writes in place, so start from an over-sized buffer.
    let plain = plaintext.as_bytes();
    let mut buffer = vec![0u8; plain.len() + BLOCK_LEN];
    buffer[..plain.len()].copy_from_slice(plain);
    let ciphertext = Aes128CbcEnc::new(
        GenericArray::from_slice(encryption_key),
        GenericArray::from_slice(iv),
    )
    .encrypt_padded_mut::<Pkcs7>(&mut buffer, plain.len())
    .map_err(|_| "AES-CBC encryption failed".to_string())?;
    body.extend_from_slice(ciphertext);

    // The tag covers everything written so far, so tampering is detectable.
    let mut mac = HmacSha256::new_from_slice(signing_key).map_err(|error| error.to_string())?;
    mac.update(&body);
    let tag = mac.finalize().into_bytes();
    body.extend_from_slice(&tag[..]);

    Ok(b64().encode(&body))
}

/// Decrypt a Fernet token into raw bytes.
pub fn decrypt_bytes(key: &str, token: &str) -> Result<Vec<u8>, String> {
    let key_bytes = parse_key(key)?;

    let raw = b64()
        .decode(token.trim())
        .map_err(|error| format!("token is not valid base64url: {error}"))?;
    if raw.len() < HEADER_LEN + HMAC_LEN {
        return Err("token is too short to be a Fernet token".to_string());
    }
    if raw[0] != VERSION {
        return Err(format!("unsupported Fernet version 0x{:02x}", raw[0]));
    }

    let (signing_key, encryption_key) = key_bytes.split_at(KEY_LEN / 2);
    let (body, tag) = raw.split_at(raw.len() - HMAC_LEN);

    // Authenticate before decrypting, exactly as the spec requires.
    let mut mac = HmacSha256::new_from_slice(signing_key).map_err(|error| error.to_string())?;
    mac.update(body);
    mac.verify_slice(tag)
        .map_err(|_| "HMAC check failed — wrong key, or the token was altered".to_string())?;

    let iv = &body[1 + 8..HEADER_LEN];
    let ciphertext = &body[HEADER_LEN..];
    if ciphertext.is_empty() || ciphertext.len() % BLOCK_LEN != 0 {
        return Err("ciphertext is not a whole number of AES blocks".to_string());
    }

    let mut buffer = ciphertext.to_vec();
    let plaintext = Aes128CbcDec::new(
        GenericArray::from_slice(encryption_key),
        GenericArray::from_slice(iv),
    )
    .decrypt_padded_mut::<Pkcs7>(&mut buffer)
    .map_err(|_| "AES-CBC decryption failed (bad padding)".to_string())?;
    Ok(plaintext.to_vec())
}

#[cfg(test)]
mod tests {
    // Test vectors generated by Python's `cryptography` — see the script's header.
    include!("crypt_vectors.rs");

    use super::*;

    #[test]
    fn decrypts_python_generated_tokens() {
        assert_eq!(TEST_VECTORS.len(), 5, "expected all vectors");
        for (name, plaintext, token) in TEST_VECTORS {
            let got = match decrypt(TEST_KEY, token) {
                Ok(value) => value,
                Err(error) => panic!("vector {name} failed: {error}"),
            };
            assert_eq!(&got, plaintext, "vector {name} decrypted wrongly");
        }
    }

    #[test]
    fn rejects_a_token_encrypted_under_another_key() {
        let error = decrypt(TEST_KEY, FOREIGN_TOKEN).unwrap_err();
        assert!(error.contains("HMAC"), "unexpected error: {error}");
    }

    #[test]
    fn rejects_a_tampered_token() {
        let (_, _, token) = TEST_VECTORS[0];
        let mut bytes = b64().decode(token).unwrap();
        let index = bytes.len() - HMAC_LEN - 1; // inside the ciphertext
        bytes[index] ^= 0x01;
        let tampered = b64().encode(&bytes);
        let error = decrypt(TEST_KEY, &tampered).unwrap_err();
        assert!(error.contains("HMAC"), "unexpected error: {error}");
    }

    #[test]
    fn rejects_garbage_keys_and_tokens() {
        let (_, _, token) = TEST_VECTORS[0];
        assert!(decrypt("not-a-valid-key", token).is_err(), "bad key");
        assert!(decrypt(TEST_KEY, "!!!not base64!!!").is_err(), "bad token");
        assert!(decrypt(TEST_KEY, "").is_err(), "empty token");
        assert!(decrypt(TEST_KEY, "gAAAAA").is_err(), "truncated token");
        // A valid 32-byte key that is simply the wrong one.
        let wrong = b64().encode([0u8; KEY_LEN]);
        assert!(decrypt(&wrong, token).is_err(), "wrong but valid key");
    }

    #[test]
    fn tolerates_surrounding_whitespace() {
        let (_, plaintext, token) = TEST_VECTORS[0];
        // Values pasted into .env or read from a config file often carry these.
        let padded = format!("  {token}\n");
        assert_eq!(decrypt(TEST_KEY, &padded).unwrap(), plaintext);
        assert_eq!(decrypt(&format!("{TEST_KEY}\n"), token).unwrap(), plaintext);
    }

    #[test]
    fn round_trips_a_fernet_key_shape() {
        // The configured key must look like what Python's Fernet.generate_key emits.
        assert_eq!(TEST_KEY.len(), 44, "fernet keys are 44 base64 chars");
        assert!(TEST_KEY.ends_with('='), "fernet keys are padded");
        assert_eq!(b64().decode(TEST_KEY).unwrap().len(), KEY_LEN);
    }

    #[test]
    fn round_trips_its_own_tokens() {
        for plaintext in ["short", "", "a-14-char-pass", "åéîøü — unicode"] {
            let token = encrypt(TEST_KEY, plaintext).unwrap();
            assert_eq!(
                decrypt(TEST_KEY, &token).unwrap(),
                plaintext,
                "failed round trip for {plaintext:?}"
            );
        }
    }

    #[test]
    fn builds_the_byte_layout_python_expects() {
        const STAMP: u64 = 1_758_468_000;
        let token = encrypt_at(TEST_KEY, "abc", STAMP, &[7u8; BLOCK_LEN]).unwrap();
        assert_eq!(decrypt(TEST_KEY, &token).unwrap(), "abc");

        // Same layout as the Python vectors: 0x80, big-endian timestamp, IV.
        let raw = b64().decode(&token).unwrap();
        assert_eq!(raw[0], VERSION);
        assert_eq!(&raw[1..9], &STAMP.to_be_bytes());
        assert_eq!(&raw[9..HEADER_LEN], &[7u8; BLOCK_LEN]);
        // 3 bytes of plaintext pad to one block, plus the 32-byte tag.
        assert_eq!(raw.len(), HEADER_LEN + BLOCK_LEN + HMAC_LEN);
    }

    #[test]
    fn stamps_a_fresh_iv_on_every_call() {
        let first = encrypt(TEST_KEY, "same input").unwrap();
        let second = encrypt(TEST_KEY, "same input").unwrap();
        assert_ne!(first, second, "the IV must not repeat");
        assert_eq!(decrypt(TEST_KEY, &first).unwrap(), "same input");
        assert_eq!(decrypt(TEST_KEY, &second).unwrap(), "same input");
    }

    #[test]
    fn refuses_to_encrypt_with_a_bad_key_or_iv() {
        assert!(encrypt("not-a-key", "x").is_err(), "bad key");
        let wrong_len = b64().encode([0u8; KEY_LEN - 1]);
        assert!(encrypt(&wrong_len, "x").is_err(), "short key");
        assert!(encrypt_at(TEST_KEY, "x", 0, &[0u8; 8]).is_err(), "short iv");
    }

    #[test]
    fn generates_keys_python_would_accept() {
        let key = generate_key().unwrap();
        // Same shape as Fernet.generate_key(): 44 base64url chars, padded.
        assert_eq!(key.len(), 44, "got {key}");
        assert!(key.ends_with('='));
        assert_eq!(parse_key(&key).unwrap().len(), KEY_LEN);

        // And a generated key immediately works for a round trip.
        let token = encrypt(&key, "first run").unwrap();
        assert_eq!(decrypt(&key, &token).unwrap(), "first run");

        // Two calls must not produce the same key.
        assert_ne!(key, generate_key().unwrap());
    }
}
