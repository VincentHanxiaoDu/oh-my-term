//! The tasks that run things and check them, as against the ones that generate.
//!
//! Split out of `main.rs`, which had grown past the length limit carrying
//! eight unrelated jobs in one file. These are the ones that touch the world:
//! installing a binary, checking a test's isolation, reading the vocabulary
//! out of the source.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::workspace_root;

/// Build a release `omt` and put it on `PATH`, atomically.
///
/// A task rather than a line in a README, because the obvious line is wrong on
/// the platform this is developed on. Copying over the existing binary keeps
/// the inode and changes the contents, and macOS caches a code signature per
/// inode: the signature no longer matches, and the kernel answers the next
/// `exec` with SIGKILL. What that looks like is
///
///     $ omt
///     [1]    42352 killed     omt
///
/// — no diagnostic, no log line, and nothing wrong with the binary itself. It
/// cost a real session to work out, twice, because "reinstall it" is a step
/// nobody writes down.
///
/// So: write beside the target and rename over it. A rename is atomic and
/// gives the new file its own inode, which is the whole fix. Then run the
/// thing once, because an install that cannot say the installed binary starts
/// has not finished.
pub(crate) fn install() -> Result<()> {
    let root = workspace_root()?;
    let status = Command::new("cargo")
        .args(["build", "--release", "-p", "omt"])
        .current_dir(&root)
        .status()
        .context("running cargo build")?;
    if !status.success() {
        bail!("the release build failed");
    }

    let built = root.join("target/release/omt");
    let home = std::env::var("HOME").context("HOME is not set")?;
    let dir = PathBuf::from(home).join(".local/bin");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let target = dir.join("omt");
    let staged = dir.join(".omt.incoming");

    std::fs::copy(&built, &staged)
        .with_context(|| format!("copying {} to {}", built.display(), staged.display()))?;
    std::fs::rename(&staged, &target)
        .with_context(|| format!("renaming {} into place", staged.display()))?;

    // The check that would have caught it. `--version` is enough: a binary the
    // kernel refuses to run fails here with signal 9 rather than an exit code.
    let out = Command::new(&target)
        .arg("--version")
        .output()
        .with_context(|| format!("running {}", target.display()))?;
    if !out.status.success() {
        bail!(
            "{} was installed and will not run: {:?}. On macOS this is usually a stale code \
             signature from an in-place overwrite — remove the file and install again.",
            target.display(),
            out.status
        );
    }
    println!(
        "installed {} — {}",
        target.display(),
        String::from_utf8_lossy(&out.stdout).trim()
    );
    Ok(())
}

/// The variables that decide *where omt keeps things*.
///
/// Deliberately short, and not derived from the source. omt reads a dozen
/// variables and most of them cannot make a test lie: `OMT_HOOK_EVENT` changes
/// what a hook reports about itself, and a daemon test that inherits it is
/// unaffected. These four redirect state — a socket, a snapshot, a rules
/// directory, a blob directory — so a test that inherits one is a test sharing
/// that state with whatever else is running on the machine.
///
/// A gate that reports twenty-seven things is a gate somebody turns off. This
/// reports the ones that have actually gone wrong.
const REDIRECTS_STATE: [&str; 4] = [
    "OMT_SOCK",
    "OMT_STATE_FILE",
    "OMT_AGENT_RULES",
    "OMT_MEDIA_DIR",
];

/// A test that starts omt must not inherit omt's own state.
///
/// Four separate defects in one session came from this, each a test that
/// passed or failed on the machine it ran on rather than on the code:
///
/// - the agent probe matched the *harness's* environment, because the suite
///   runs inside an agent session, and reported an agent for a plain shell —
///   three tests green over a feature that did not work at all
/// - `two_instances_on_one_machine_do_not_share_a_snapshot` compared two paths
///   that `OMT_STATE_FILE` had already collapsed into one
/// - the crash tests isolated `XDG_STATE_HOME` and not the variable that
///   overrides it, so four daemons that each took a directory shared a file
/// - the screen suite started sixteen omts at once and failed as a test of the
///   machine rather than of omt
///
/// Four is a pattern rather than four accidents, so it is a check.
pub(crate) fn test_isolation() -> Result<()> {
    let root = workspace_root()?;
    let mut wrong: Vec<String> = Vec::new();

    for entry in walk(&root.join("crates"))? {
        let name = entry.to_string_lossy().to_string();
        if !name.contains("/tests/") || entry.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let text = std::fs::read_to_string(&entry).unwrap_or_default();
        // Only files that actually start the binary: a unit test that never
        // spawns omt cannot inherit anything omt would read.
        if !text.contains("CARGO_BIN_EXE_omt") {
            continue;
        }
        for variable in REDIRECTS_STATE {
            // Named anywhere in a file that also removes variables. Loose on
            // purpose: a harness may strip a list rather than call
            // `env_remove` per name, and this is a warning — a false positive
            // is a warning somebody learns to scroll past, which costs more
            // than the occasional miss.
            let controlled = text.contains(&format!("\"{variable}\""))
                && (text.contains("env_remove") || text.contains("env("));
            if !controlled {
                wrong.push(format!("{}: inherits {variable}", entry.display()));
            }
        }
    }

    if wrong.is_empty() {
        println!("test isolation ok — every test that starts omt controls where its state goes");
        return Ok(());
    }
    for line in &wrong {
        eprintln!("warning: {line}");
    }
    println!(
        "test isolation: {} place(s) where a test that starts omt shares state with the machine",
        wrong.len()
    );
    Ok(())
}

/// Every file under a directory.
pub(crate) fn walk(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(next) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&next) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    Ok(out)
}

/// Emit the words both surfaces use for an agent's state.
///
/// The TUI and the web client each decide what to *call* a state, and they
/// drifted: one said "blocked" and the other "waiting on you", in the same
/// product, about the same session, and a person found it rather than a test.
///
/// One implementation cannot be shared across a Rust binary and a browser, so
/// what is shared is the table. This writes it from the Rust source of truth
/// and the client's own tests assert against it — the same shape as the
/// capability parity gate, and for the same reason: two lists that have to
/// agree will not, unless one of them is generated.
pub(crate) fn vocabulary() -> Result<()> {
    let root = workspace_root()?;
    let source = std::fs::read_to_string(root.join("crates/omt-tui/src/statusbar.rs"))?;

    // Each arm of the three functions, in the order they decide.
    let long = arms(&source, "pub fn state_word(")?;
    let short = arms(&source, "pub fn state_word_short(")?;
    let ranks = arms(&source, "pub fn attention(")?;

    let mut json = String::from("{\n");
    json.push_str("  \"note\": \"Generated by `cargo xtask vocabulary`. The words both surfaces use for an agent's state, from crates/omt-tui/src/statusbar.rs. Edit that, not this.\",\n");
    for (name, rows) in [("long", &long), ("short", &short), ("attention", &ranks)] {
        json.push_str(&format!("  \"{name}\": {{\n"));
        for (i, (state, value)) in rows.iter().enumerate() {
            let comma = if i + 1 == rows.len() { "" } else { "," };
            json.push_str(&format!("    \"{state}\": {value}{comma}\n"));
        }
        json.push_str(if name == "attention" {
            "  }\n"
        } else {
            "  },\n"
        });
    }
    json.push_str("}\n");

    let out = root.join("web/src/generated/vocabulary.json");
    std::fs::write(&out, &json)?;
    println!("wrote {} ({} states)", out.display(), long.len());
    Ok(())
}

/// The `state => value` pairs out of one match in the source.
///
/// Parsed rather than linked, because xtask cannot depend on `omt-tui` without
/// inverting the layer order this workspace checks.
pub(crate) fn arms(source: &str, signature: &str) -> Result<Vec<(String, String)>> {
    let Some(start) = source.find(signature) else {
        bail!("`{signature}` is not in the status bar source; the shape must have changed");
    };
    let body = &source[start..];
    let end = body.find("\n}").unwrap_or(body.len());
    let mut out: Vec<(String, String)> = Vec::new();
    for line in body[..end].lines() {
        let line = line.trim();
        let Some((left, right)) = line.split_once("=>") else {
            continue;
        };
        if !left.contains("AgentState::") {
            continue;
        }
        let value = right.trim().trim_end_matches(',').to_owned();
        if value.is_empty() {
            continue;
        }
        // One row per state, even when an arm covers several: a table with a
        // key called `Working+Starting` is a table nothing can look a state up
        // in.
        //
        // `Idle if !seen` and `Idle` are different answers about the same
        // state, so the guard stays part of the key — it is the whole reason
        // "done" and "idle" are different words.
        for piece in left.split("AgentState::").skip(1) {
            let state = piece
                .replace(" { .. }", "")
                .replace(" | omt_types::", "")
                .replace('|', "")
                .replace("=>", "")
                .trim()
                .to_owned();
            if !state.is_empty() {
                out.push((state, value.clone()));
            }
        }
    }
    if out.is_empty() {
        bail!("no arms parsed out of `{signature}`");
    }
    Ok(out)
}
