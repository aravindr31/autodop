fn main() {
    use std::path::Path;

    println!("cargo:rerun-if-changed=build.rs");

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

    let dirty = std::process::Command::new("git")
        .args([
            "status",
            "--porcelain",
            "--untracked-files=no",
            "--",
            "src-tauri",
            "frontend",
            "scripts",
            "package.json",
            "scraper.py",
        ])
        .current_dir(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap_or(Path::new(".")),
        )
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

    let epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    println!("cargo:rustc-env=AUTODOP_BUILD_EPOCH={epoch}");

    tauri_build::build()
}
