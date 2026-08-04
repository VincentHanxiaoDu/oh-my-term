//! SSH tests against a real remote host.
//!
//! These drive an actual `sshd` in a container rather than a mock, because the
//! things that break here are the things a mock would have to assume away: what
//! `uname` prints on musl, whether one exec can both probe and connect, whether
//! a binary survives the pipe intact. Marked `#[ignore]` so `cargo test` stays
//! fast and works with no Docker; CI runs them explicitly.
//!
//! Start the fixture with `docker compose -f tests/docker/compose.yml up -d`.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]

use std::path::PathBuf;
use std::process::{Command, Stdio};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

/// The ssh arguments every test shares.
///
/// `StrictHostKeyChecking=no` and a null known-hosts file because the container
/// gets fresh host keys on every rebuild; this is a disposable fixture, not a
/// host anyone trusts.
fn ssh_args() -> Vec<String> {
    let key = repo_root().join("tests/docker/keys/id_test");
    vec![
        "-i".into(),
        key.display().to_string(),
        "-p".into(),
        "2222".into(),
        "-o".into(),
        "StrictHostKeyChecking=no".into(),
        "-o".into(),
        "UserKnownHostsFile=/dev/null".into(),
        "-o".into(),
        "LogLevel=ERROR".into(),
        "-o".into(),
        "ConnectTimeout=5".into(),
        "omtuser@127.0.0.1".into(),
    ]
}

fn ssh(remote_command: &str) -> std::process::Output {
    Command::new("ssh")
        .args(ssh_args())
        .arg("--")
        .arg(remote_command)
        .stdin(Stdio::null())
        .output()
        .expect("running ssh")
}

/// A remote path unique to one test.
///
/// The tests run in parallel and the fixture is one host, so a shared path —
/// `~/.local/bin/omt`, say — makes two tests race over the same file. A flaky
/// test is worse than a missing one: it trains people to re-run instead of
/// read.
fn scratch(name: &str) -> String {
    format!("/tmp/omt-test-{name}")
}

fn require_fixture() {
    let out = ssh("true");
    assert!(
        out.status.success(),
        "the SSH fixture is not reachable. Start it with:\n  \
         docker compose -f tests/docker/compose.yml up -d --build\n\
         stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
#[ignore = "needs the Docker SSH fixture"]
fn the_fixture_is_a_musl_host() {
    // The premise of the whole suite. If this drifts to glibc, the tests still
    // pass but stop proving the thing that matters.
    require_fixture();
    let out = ssh("uname -sm; ldd --version 2>&1 | head -1 || true");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("Linux"), "{text}");
    assert!(
        text.to_lowercase().contains("musl"),
        "the fixture must be musl, since that is the target the static binary exists for: {text}"
    );
}

#[test]
#[ignore = "needs the Docker SSH fixture"]
fn the_probe_reports_a_missing_omt_with_its_architecture() {
    // One exec answers both questions — is omt there, and if not, which binary
    // would it need. Two round trips would prompt twice on an MFA host.
    require_fixture();
    let out = ssh(
        r#"omt serve --stdio --proto 1 2>/dev/null || { printf "OMT-MISSING "; uname -sm; (ldd --version 2>&1 | head -1) || echo musl; }"#,
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.starts_with("OMT-MISSING"),
        "expected a miss, got: {text}"
    );
    assert!(
        text.contains("aarch64") || text.contains("x86_64"),
        "{text}"
    );
}

#[test]
#[ignore = "needs the Docker SSH fixture"]
fn a_binary_survives_the_ssh_pipe_intact() {
    // The bootstrap's core move: send the binary over the connection already
    // open, rather than asking the remote to download it. That is what makes a
    // host with no internet an ordinary case instead of a failure mode.
    require_fixture();

    let payload: Vec<u8> = (0..64u16).flat_map(|n| n.to_be_bytes()).collect();
    let expected = blake3::hash(&payload).to_hex().to_string();

    let mut child = Command::new("ssh")
        .args(ssh_args())
        .arg("--")
        .arg(format!("cat > {p} && wc -c < {p}", p = scratch("upload")))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn ssh");

    {
        use std::io::Write;
        let stdin = child.stdin.as_mut().expect("stdin");
        stdin.write_all(&payload).expect("write payload");
    }
    let out = child.wait_with_output().expect("wait");
    let size: usize = String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .expect("byte count");
    assert_eq!(size, payload.len(), "the upload was truncated");

    // And verify the *content*, not just the length — a pipe that mangles
    // bytes without losing them is the harder failure to notice.
    let out = ssh(&format!(
        "od -An -tx1 -v {} | tr -d ' \\n'",
        scratch("upload")
    ));
    let hex = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    let round_tripped = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex byte"))
        .collect::<Vec<_>>();
    assert_eq!(round_tripped, payload, "bytes changed in transit");
    assert_eq!(blake3::hash(&round_tripped).to_hex().to_string(), expected);

    ssh(&format!("rm -f {}", scratch("upload")));
}

#[test]
#[ignore = "needs the Docker SSH fixture"]
fn an_atomic_install_leaves_no_half_written_state() {
    // Write to a temp name, fsync, rename. A partially-installed binary is the
    // failure VS Code's untar-into-a-tree has and this design does not.
    require_fixture();
    let dir = scratch("atomic");
    let out = ssh(&format!(
        "set -e; rm -rf {dir}; mkdir -p {dir}; \
         printf '#!/bin/sh\\necho installed\\n' > {dir}/.omt.tmp; \
         chmod +x {dir}/.omt.tmp; \
         mv {dir}/.omt.tmp {dir}/omt; \
         ls -a {dir}"
    ));
    let listing = String::from_utf8_lossy(&out.stdout);
    assert!(listing.contains("omt"), "{listing}");
    assert!(
        !listing.contains(".omt.tmp"),
        "the temp name survived, so the install was not atomic: {listing}"
    );

    let out = ssh(&format!("{dir}/omt"));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "installed");

    ssh(&format!("rm -rf {dir}"));
}

#[test]
#[ignore = "needs the Docker SSH fixture"]
fn the_probe_finds_omt_once_it_is_installed() {
    // The other half of the ladder: with omt present the probe succeeds and the
    // session is tier 0, with no bootstrap and no prompt.
    require_fixture();
    let dir = scratch("probe-hit");
    ssh(&format!(
        "rm -rf {dir} && mkdir -p {dir} && printf '#!/bin/sh\\necho OMT-READY\\n' > {dir}/omt && chmod +x {dir}/omt"
    ));

    let out = ssh(&format!(
        r#"PATH="{dir}:$PATH"; omt serve --stdio --proto 1 2>/dev/null || {{ printf "OMT-MISSING "; uname -sm; }}"#
    ));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("OMT-READY"),
        "expected the installed omt to answer, got: {text}"
    );
    assert!(!text.contains("OMT-MISSING"));

    ssh(&format!("rm -rf {dir}"));
}

#[test]
#[ignore = "needs the Docker SSH fixture"]
fn stdio_carries_a_protocol_frame_over_ssh() {
    // `omt ssh` is not a new network protocol: it is the ordinary wire on the
    // ssh subprocess's stdin and stdout. This proves a framed message survives
    // that path byte-for-byte.
    require_fixture();

    let hello = serde_json::json!({
        "t": "hello",
        "proto": 1,
        "client": "test",
    });
    let body = serde_json::to_vec(&hello).expect("encode");
    let mut framed = (body.len() as u32).to_be_bytes().to_vec();
    framed.push(0); // text frame
    framed.extend_from_slice(&body);

    let mut child = Command::new("ssh")
        .args(ssh_args())
        .arg("--")
        .arg("cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn ssh");
    {
        use std::io::Write;
        child
            .stdin
            .as_mut()
            .expect("stdin")
            .write_all(&framed)
            .expect("write");
    }
    let out = child.wait_with_output().expect("wait");

    assert_eq!(
        out.stdout, framed,
        "the framed message did not survive the pipe"
    );

    let len = u32::from_be_bytes(out.stdout[..4].try_into().expect("length prefix")) as usize;
    assert_eq!(out.stdout[4], 0, "kind tag");
    let decoded: serde_json::Value =
        serde_json::from_slice(&out.stdout[5..5 + len]).expect("decode");
    assert_eq!(decoded["t"], "hello");
    assert_eq!(decoded["proto"], 1);
}
