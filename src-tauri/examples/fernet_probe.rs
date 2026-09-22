//! Round-trip a value through the app's Fernet implementation.
//!
//! Prints the encrypted token this app would write, then decrypts it again. The
//! token can also be handed to Python's `cryptography` — the format the DOP
//! password already lives in — to prove the two implementations interoperate.
//! Never touches the database, never prints the key.
//!
//!     cd src-tauri && cargo run --example fernet_probe -- "some-value"

use autodop_lib::crypt;
use autodop_lib::db::fernet_key;

fn main() {
    let value = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "probe-value".to_string());

    let Some(key) = fernet_key(None) else {
        eprintln!("NO_FERNET_KEY: set FERNET_KEY in src-tauri/.env");
        std::process::exit(1);
    };

    let token = match crypt::encrypt(&key, &value) {
        Ok(token) => token,
        Err(error) => {
            eprintln!("ENCRYPT_FAILED: {error}");
            std::process::exit(1);
        }
    };

    let back = match crypt::decrypt(&key, &token) {
        Ok(back) => back,
        Err(error) => {
            eprintln!("DECRYPT_FAILED: {error}");
            std::process::exit(1);
        }
    };

    println!("token={token}");
    println!("roundtrip_ok={}", back == value);
}
