use aes::cipher::block_padding::Pkcs7;
use aes::cipher::generic_array::GenericArray;
use aes::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use argon2::Argon2;
use base64::Engine as _;
use hmac::digest::KeyInit as _;
use hmac::{Hmac, Mac};
use sha2::Sha256;

type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;
type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;
type HmacSha256 = Hmac<Sha256>;

const VERSION: u8 = 0x80;

const HEADER_LEN: usize = 25;
const HMAC_LEN: usize = 32;
const KEY_LEN: usize = 32;
const BLOCK_LEN: usize = 16;

fn b64() -> base64::engine::general_purpose::GeneralPurpose {
    base64::engine::general_purpose::URL_SAFE
}

pub fn decrypt(key: &str, token: &str) -> Result<String, String> {
    let plaintext = decrypt_bytes(key, token)?;
    String::from_utf8(plaintext).map_err(|_| "decrypted value is not valid UTF-8".to_string())
}

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

pub fn generate_key() -> Result<String, String> {
    let mut bytes = [0u8; KEY_LEN];
    getrandom::getrandom(&mut bytes).map_err(|error| format!("no system randomness: {error}"))?;
    Ok(b64().encode(&bytes))
}

pub const SALT_LEN: usize = 16;

pub fn new_salt() -> Result<String, String> {
    let mut bytes = [0u8; SALT_LEN];
    getrandom::getrandom(&mut bytes).map_err(|error| format!("no system randomness: {error}"))?;
    Ok(b64().encode(&bytes))
}

pub fn derive_key(password: &str, salt_b64: &str) -> Result<String, String> {
    let salt = b64()
        .decode(salt_b64.trim())
        .map_err(|error| format!("salt is not valid base64url: {error}"))?;
    if salt.len() < 8 {
        return Err(format!("salt must be at least 8 bytes, got {}", salt.len()));
    }

    let mut key = [0u8; KEY_LEN];
    Argon2::default()
        .hash_password_into(password.as_bytes(), &salt, &mut key)
        .map_err(|error| format!("key derivation failed: {error}"))?;
    Ok(b64().encode(&key))
}

pub fn hash_login(password: &str) -> Result<String, String> {
    let mut salt_bytes = [0u8; SALT_LEN];
    getrandom::getrandom(&mut salt_bytes)
        .map_err(|error| format!("no system randomness: {error}"))?;

    Argon2::default()
        .hash_password_with_salt(password.as_bytes(), &salt_bytes)
        .map(|hash| hash.to_string())
        .map_err(|error| format!("hashing the login password failed: {error}"))
}

pub fn verify_login(password: &str, phc: &str) -> bool {
    match PasswordHash::new(phc) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

pub fn encrypt(key: &str, plaintext: &str) -> Result<String, String> {
    let mut iv = [0u8; BLOCK_LEN];
    getrandom::getrandom(&mut iv).map_err(|error| format!("no system randomness: {error}"))?;
    encrypt_at(key, plaintext, crate::now_secs(), &iv)
}

pub fn encrypt_at(key: &str, plaintext: &str, timestamp: u64, iv: &[u8]) -> Result<String, String> {
    let key_bytes = parse_key(key)?;
    if iv.len() != BLOCK_LEN {
        return Err(format!("IV must be {BLOCK_LEN} bytes, got {}", iv.len()));
    }
    let (signing_key, encryption_key) = key_bytes.split_at(KEY_LEN / 2);

    let mut body = Vec::with_capacity(HEADER_LEN + plaintext.len() + BLOCK_LEN);
    body.push(VERSION);
    body.extend_from_slice(&timestamp.to_be_bytes());
    body.extend_from_slice(iv);

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

    let mut mac = HmacSha256::new_from_slice(signing_key).map_err(|error| error.to_string())?;
    mac.update(&body);
    let tag = mac.finalize().into_bytes();
    body.extend_from_slice(&tag[..]);

    Ok(b64().encode(&body))
}

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
        let index = bytes.len() - HMAC_LEN - 1;
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

        let wrong = b64().encode([0u8; KEY_LEN]);
        assert!(decrypt(&wrong, token).is_err(), "wrong but valid key");
    }

    #[test]
    fn tolerates_surrounding_whitespace() {
        let (_, plaintext, token) = TEST_VECTORS[0];

        let padded = format!("  {token}\n");
        assert_eq!(decrypt(TEST_KEY, &padded).unwrap(), plaintext);
        assert_eq!(decrypt(&format!("{TEST_KEY}\n"), token).unwrap(), plaintext);
    }

    #[test]
    fn round_trips_a_fernet_key_shape() {
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

        let raw = b64().decode(&token).unwrap();
        assert_eq!(raw[0], VERSION);
        assert_eq!(&raw[1..9], &STAMP.to_be_bytes());
        assert_eq!(&raw[9..HEADER_LEN], &[7u8; BLOCK_LEN]);

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
    fn derives_a_usable_fernet_key_from_a_password() {
        let salt = new_salt().unwrap();
        let key = derive_key("hunter2", &salt).unwrap();

        assert_eq!(key.len(), 44, "got {key}");
        assert_eq!(parse_key(&key).unwrap().len(), KEY_LEN);

        let token = encrypt(&key, "the DOP password").unwrap();
        assert_eq!(decrypt(&key, &token).unwrap(), "the DOP password");
    }

    #[test]
    fn the_same_password_and_salt_derive_the_same_key() {
        let salt = new_salt().unwrap();
        assert_eq!(
            derive_key("pw", &salt).unwrap(),
            derive_key("pw", &salt).unwrap(),
            "derivation must be deterministic"
        );
    }

    #[test]
    fn a_different_password_or_salt_derives_a_different_key() {
        let salt = new_salt().unwrap();
        let other_salt = new_salt().unwrap();
        let base = derive_key("pw", &salt).unwrap();
        assert_ne!(
            base,
            derive_key("pw", &other_salt).unwrap(),
            "salt must matter"
        );
        assert_ne!(
            base,
            derive_key("pw2", &salt).unwrap(),
            "password must matter"
        );
    }

    #[test]
    fn refuses_a_bad_salt() {
        assert!(derive_key("pw", "not base64!").is_err());
        assert!(
            derive_key("pw", &b64().encode([0u8; 4])).is_err(),
            "too short"
        );
    }

    #[test]
    fn hashes_and_verifies_a_login_password() {
        let phc = hash_login("correct horse").unwrap();
        assert!(phc.starts_with("$argon2id$"), "got {phc}");
        assert!(verify_login("correct horse", &phc));
        assert!(!verify_login("wrong", &phc));
        assert!(!verify_login("correct horse", "not-a-phc-string"));

        assert_ne!(phc, hash_login("correct horse").unwrap());
    }

    #[test]
    fn a_derived_key_is_not_a_bare_digest_of_the_password() {
        use sha2::{Digest, Sha256};

        let salt = new_salt().unwrap();
        let key = derive_key("pw", &salt).unwrap();
        let fast = b64().encode(Sha256::digest(b"pw"));
        assert_ne!(
            key, fast,
            "the key must not be a plain SHA-256 of the login password"
        );
    }

    #[test]
    fn generates_keys_python_would_accept() {
        let key = generate_key().unwrap();

        assert_eq!(key.len(), 44, "got {key}");
        assert!(key.ends_with('='));
        assert_eq!(parse_key(&key).unwrap().len(), KEY_LEN);

        let token = encrypt(&key, "first run").unwrap();
        assert_eq!(decrypt(&key, &token).unwrap(), "first run");

        assert_ne!(key, generate_key().unwrap());
    }
}
