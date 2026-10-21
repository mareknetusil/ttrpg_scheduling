// Build script: runs before compiling the crate.
fn main() {
    // `sqlx::migrate!()` embeds migrations/, so rebuild when it changes.
    println!("cargo:rerun-if-changed=migrations");

    // The version shown in the footer and /healthz: the Cargo version plus the git commit, when
    // building from a git checkout ("0.1.0+4adafb9", "-dirty" with uncommitted changes).
    let commit = std::process::Command::new("git")
        .args(["describe", "--always", "--dirty", "--exclude", "*"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string());
    let version = match commit {
        Some(c) if !c.is_empty() => format!("{}+{c}", env!("CARGO_PKG_VERSION")),
        _ => env!("CARGO_PKG_VERSION").to_string(),
    };
    println!("cargo:rustc-env=APP_VERSION={version}");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");
}
