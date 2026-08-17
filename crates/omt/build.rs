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
    // Rerun when the checked-out commit changes.
    //
    // `.git/HEAD` alone is not that. On a branch it holds `ref: refs/heads/x`
    // and does not change when a commit is made — so the stamp stayed at
    // whatever commit was checked out the first time this was built, and
    // `omt --version` reported a commit the binary was not built from. That is
    // the exact confusion this file exists to prevent, which is how it went
    // unnoticed: the line was there and it was watching the wrong file.
    let git = std::path::Path::new("../../.git");
    println!("cargo:rerun-if-changed={}", git.join("HEAD").display());
    if let Ok(head) = std::fs::read_to_string(git.join("HEAD"))
        && let Some(reference) = head.strip_prefix("ref: ").map(str::trim)
    {
        // The ref file itself, which is what a commit actually writes.
        println!("cargo:rerun-if-changed={}", git.join(reference).display());
    }
    // And where refs live once they have been packed away, which is where a
    // ref file goes when it stops existing on its own.
    println!(
        "cargo:rerun-if-changed={}",
        git.join("packed-refs").display()
    );

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
