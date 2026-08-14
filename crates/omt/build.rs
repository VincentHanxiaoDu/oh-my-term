//! Stamp the binary with which commit it was built from.
//!
//! `omt --version` printed `omt 0.1.0` and nothing else, so a person holding a
//! binary could not tell whether it contained the fix they were told about —
//! and neither could the person who told them. That cost a round trip in a
//! real session: a fix was installed, the report came back unchanged, and the
//! session was one started before the install and still running the old code.
//!
//! A commit and a build time answer that in one line.

use std::process::Command;

fn main() {
    // Rerun when the checked-out commit changes, which is what `.git/HEAD` and
    // the ref it points at record.
    println!("cargo:rerun-if-changed=../../.git/HEAD");

    let commit = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .unwrap_or_else(|| "unknown".to_owned());

    let dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .is_some_and(|out| !out.stdout.is_empty());

    println!(
        "cargo:rustc-env=OMT_BUILD_COMMIT={commit}{}",
        if dirty { "+" } else { "" }
    );
}
