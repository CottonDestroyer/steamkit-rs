//! Stamps version info (build date, commit SHA, commit date, dirty flag) into
//! the crate through `cargo:rustc-env`. See `src/version.rs`.

use std::process::Command;

/// Runs `git` and returns its trimmed stdout, or `None` when git is unavailable.
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let build_date = chrono::Local::now()
        .format("%Y-%m-%d %H:%M:%S %:z")
        .to_string();

    // Without git, the SHA and the commit date are left empty.
    let commit_sha = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_default();
    let commit_date = git(&["show", "-s", "--format=%ci", "HEAD"]).unwrap_or_default();

    // `git diff --quiet` exits non-zero when the working tree has changes.
    let dirty = match Command::new("git").args(["diff", "--quiet"]).status() {
        Ok(status) => !status.success(),
        Err(_) => false,
    };

    println!("cargo:rustc-env=NETHOOK_BUILD_DATE={build_date}");
    println!("cargo:rustc-env=NETHOOK_COMMIT_SHA={commit_sha}");
    println!("cargo:rustc-env=NETHOOK_COMMIT_DATE={commit_date}");
    println!(
        "cargo:rustc-env=NETHOOK_DIRTY={}",
        if dirty { "1" } else { "0" }
    );
}
