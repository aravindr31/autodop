fn main() {
    // Stamp every build with the commit it came from, so a built app can always
    // be told apart from an older one — "which version am I running?" should
    // never require guesswork.
    //
    // `git log` rather than `date`: it works on every platform, and the stamp
    // stays consistent for a given commit instead of changing on every rebuild.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../.git/HEAD");

    let stamp = std::process::Command::new("git")
        .args(["log", "-1", "--format=%h"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "unknown".to_string());

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
