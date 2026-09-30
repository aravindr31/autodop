//! Print the version and build stamp baked into this binary.
//!
//!     cd src-tauri && cargo run --example build_info
//!
//! These are exactly the values the app shows in Manage → **This build**, so a
//! build can be checked from a terminal instead of by opening the app.
//! `cargo run --release --example build_info` reports the release stamp.

fn main() {
    println!("version = {}", env!("CARGO_PKG_VERSION"));
    println!(
        "commit  = {}",
        option_env!("AUTODOP_BUILD_STAMP").unwrap_or("unknown")
    );
    println!("build   = {}", autodop_lib::build_stamp());
}
