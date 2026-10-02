fn main() {
    println!("version = {}", env!("CARGO_PKG_VERSION"));
    println!(
        "commit  = {}",
        option_env!("AUTODOP_BUILD_STAMP").unwrap_or("unknown")
    );
    println!("build   = {}", autodop_lib::build_stamp());
}
