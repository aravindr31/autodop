fn main() {
    // Stamp every build with the commit it came from, so a built app can always
    // be told apart from an older one — "which version am I running?" should
    // never require guesswork.
    //
    // `git log` rather than `date`: it works on every platform, and the stamp
    // stays consistent for a given commit instead of changing on every rebuild.
    println!("cargo:rerun-if-changed=build.rs");
    // A commit updates the *branch ref*, not HEAD — watching only HEAD missed
    // every commit, leaving the stamp one behind the code it was built from.
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/refs/heads");
    println!("cargo:rerun-if-changed=../.git/packed-refs");

    let stamp = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "unknown".to_string());

    // Say so when the code is not a committed revision — an uncommitted build
    // must not look like a released one.
    let dirty = std::process::Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=no"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| !out.stdout.is_empty())
        .unwrap_or(false);
    let stamp = if dirty {
        format!("{stamp}+dirty")
    } else {
        stamp
    };

    println!("cargo:rustc-env=AUTODOP_BUILD_STAMP={stamp}");

    // Plus the moment of this build, so two builds of the same commit are still
    // distinguishable. Epoch seconds here, formatted as UTC on the Rust side —
    // no dependency on a platform-specific `date`.
    let epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    println!("cargo:rustc-env=AUTODOP_BUILD_EPOCH={epoch}");

    tauri_build::build()
}
