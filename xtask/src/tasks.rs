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
    // Both, always. `agent.hooks_install` writes `omt-hook` into an agent's own
    // configuration by absolute path, and a path pointing at a binary nobody
    // installed is a hook that never runs — which looks exactly like a hook
    // that was never installed, from every surface. That was the state of this
    // machine: the settings file named it and it was not there.
    let status = Command::new("cargo")
        .args(["build", "--release", "-p", "omt", "-p", "omt-hook"])
        .current_dir(&root)
        .status()
        .context("running cargo build")?;
    if !status.success() {
        bail!("the release build failed");
    }

    let home = std::env::var("HOME").context("HOME is not set")?;
    let dir = PathBuf::from(home).join(".local/bin");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    for binary in ["omt", "omt-hook"] {
        install_one(&root.join("target/release").join(binary), &dir, binary)?;
    }
    Ok(())
}

/// Put one built binary in place and prove it runs.
fn install_one(built: &std::path::Path, dir: &std::path::Path, name: &str) -> Result<()> {
    let target = dir.join(name);
    let staged = dir.join(format!(".{name}.incoming"));

    // Copied beside and renamed, never written over: macOS caches a code
    // signature per inode, and an in-place overwrite leaves the kernel
    // refusing to run what it thinks it already knows — which it reports as
    // signal 9 and nothing else.
    std::fs::copy(built, &staged)
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
    // The hook binary answers every invocation, `--version` included, with its
    // agent's empty document — that is the fail-open contract and not a fault.
    // So the line says what was proved: that the kernel runs it.
    let said = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    let note = if said.is_empty() || said == "{}" {
        "runs".to_owned()
    } else {
        said
    };
    println!("installed {} — {note}", target.display());
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
        let mut text = std::fs::read_to_string(&entry).unwrap_or_default();
        // A test file's harness may live beside it in `tests/common/`, and the
        // isolation it establishes is established there. Reading the file
        // alone once made this gate fire on a suite that *was* isolated —
        // which is the worst kind of gate, because the fix for a false alarm
        // is to weaken it.
        //
        // A harness split into a module directory (`tests/screen/main.rs`)
        // points at it with `#[path]`, and the path is followed here: it is
        // the same discovery rule, not a different one. The screen suite uses
        // it to stay one test binary while keeping files under the line
        // limit.
        if let Some(dir) = harness_dir(&text, entry.parent()) {
            for shared in walk(&dir).unwrap_or_default() {
                if shared.extension().is_some_and(|e| e == "rs") {
                    text.push_str(&std::fs::read_to_string(&shared).unwrap_or_default());
                }
            }
        }
        // Only files that actually start the binary: a unit test that never
        // spawns omt cannot inherit anything omt would read.
        if !text.contains("CARGO_BIN_EXE_omt") {
            continue;
        }
        // Attached is the default, so a test that starts omt and says nothing
        // joins whichever instance is running on the machine it runs on —
        // somebody's real one, with their work in it. It must either ask for
        // the local path or point at a runtime directory of its own.
        //
        // A failure rather than a warning: the others cost a confusing test
        // run, and this one costs somebody else's sessions.
        if !text.contains("OMT_LOCAL") && !text.contains("XDG_RUNTIME_DIR") {
            bail!(
                "{}: starts omt without asking for the local path or a runtime \
                 directory of its own, so it would attach to whatever instance \
                 is running on this machine",
                entry.display()
            );
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
        println!(
            "test isolation ok — every test that starts omt controls where its state goes, \
             and none of them would join somebody else's instance"
        );
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

/// Where a test file's harness lives, when it has one.
///
/// `tests/<name>.rs` declares `mod common;`, resolving to
/// `tests/<name>/common/` — wait, to `tests/common.rs`/`tests/common/` beside
/// the file. A module directory test (`tests/screen/main.rs`) declares
/// `#[path = "../common/mod.rs"] mod common;`, and the path is relative to
/// the file's own directory.
#[must_use]
fn harness_dir(text: &str, file_dir: Option<&Path>) -> Option<PathBuf> {
    let dir = file_dir?;
    if text.contains("mod common;") {
        // `#[path = "..."]` above the declaration, resolved against the file's
        // directory; without it, `tests/<name>/common/`.
        let path_attr = text.lines().rev().find_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("#[path") {
                trimmed
                    .split('=')
                    .nth(1)
                    // `]` comes off before the quotes: the line ends
                    // `..."...]`, and quoting is what protects a path with a
                    // bracket in it.
                    .map(|p| p.trim().trim_matches(']').trim_matches('"').trim())
            } else {
                None
            }
        });
        return Some(path_attr.map_or_else(
            || dir.join("common"),
            |p| {
                // The attribute names a file (`../common/mod.rs`); the walk
                // below takes directories, so the file's parent is what the
                // harness actually lives in.
                let joined = dir.join(p);
                let is_mod = joined.file_name().is_some_and(|n| n == "mod.rs");
                if is_mod {
                    joined
                        .parent()
                        .map_or_else(|| joined.clone(), Path::to_path_buf)
                } else {
                    joined
                }
            },
        ));
    }
    None
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

/// Every setting either has a reader or says why it has none.
///
/// The gate for the defect this repository keeps meeting: a vocabulary that
/// exists everywhere and a producer — here, a *consumer* — that exists
/// nowhere. Fifty-nine of the sixty-six settings parsed, validated and
/// reported their provenance while changing nothing at all, and from outside
/// that is indistinguishable from a setting that works.
///
/// It fails in both directions on purpose. A setting nothing reads and nothing
/// declares is a silent promise; an entry in `UNWIRED` for a setting that has
/// since been wired up is a stale excuse, and stale excuses are how a list
/// like this stops meaning anything.
pub(crate) fn settings_readers() -> Result<()> {
    let root = workspace_root()?;

    let schema = std::fs::read_to_string(root.join("crates/omt-config/src/schema.rs"))
        .context("reading the settings schema")?;
    let keys = declared(&schema, "key: \"");
    let unwired = unwired_entries(&schema);

    // Everything that could read a setting, which is everything except the
    // crate that defines and validates them: naming a key there is declaring
    // it, not acting on it.
    let mut sources = String::new();
    for dir in [
        root.join("crates/omt/src"),
        root.join("crates/omt-server/src"),
        root.join("crates/omt-daemon/src"),
        root.join("web/src"),
    ] {
        for file in walk(&dir)? {
            let path = file.to_string_lossy().to_string();
            if file
                .extension()
                .is_none_or(|e| e != "rs" && e != "ts" && e != "tsx")
            {
                continue;
            }
            // Tests are excluded: a fixture that mentions a key proves
            // somebody typed it, not that anything acts on it.
            if path.contains("/tests/") {
                continue;
            }
            sources.push_str(&std::fs::read_to_string(&file).unwrap_or_default());
            sources.push('\n');
        }
    }

    let mut problems: Vec<String> = Vec::new();
    for key in &keys {
        let read = reads(&sources, key);
        let declared = unwired.iter().any(|(k, _)| k == key);
        match (read, declared) {
            (false, false) => problems.push(format!(
                "`{key}` is a setting nothing reads. Wire it up, or add it to \
                 `UNWIRED` in omt-config's schema with the reason."
            )),
            (true, true) => problems.push(format!(
                "`{key}` has a reader and is still listed in `UNWIRED`. Remove \
                 the entry: a stale excuse is how the list stops meaning anything."
            )),
            _ => {}
        }
    }
    for (key, why) in &unwired {
        if !keys.contains(key) {
            problems.push(format!("`{key}` is in `UNWIRED` and is not a setting"));
        }
        if why.trim().is_empty() {
            problems.push(format!("`{key}` is unwired without a reason"));
        }
    }

    if problems.is_empty() {
        println!(
            "settings readers ok — {} settings, {} of them declared unwired with a reason",
            keys.len(),
            unwired.len()
        );
        return Ok(());
    }
    for problem in &problems {
        eprintln!("  {problem}");
    }
    bail!("{} setting(s) without an honest answer", problems.len())
}

/// Whether anything actually asks the configuration for a key.
///
/// The key must appear *inside a lookup*, not merely somewhere in a file. A
/// bare string match called `terminal.background` read: the name collides with
/// a VS Code theme key that `omt-theme` maps, and a gate that counted that
/// would report a setting as wired because two unrelated things are spelled
/// the same.
fn reads(sources: &str, key: &str) -> bool {
    // Both quote styles: the Rust consumers write `"key"` and the web client
    // writes `'key'`, and a gate that read one and not the other reported
    // eleven wired settings as unwired — a false alarm in exactly the
    // direction a gate must not lie.
    let quoted_double = format!("\"{key}\"");
    let quoted_single = format!("'{key}'");
    reads_quoted(sources, &quoted_double) || reads_quoted(sources, &quoted_single)
}

/// Whether a quoted key appears inside a lookup call.
fn reads_quoted(sources: &str, quoted: &str) -> bool {
    let mut from = 0;
    while let Some(at) = sources[from..].find(quoted) {
        let at = from + at;
        // A window around the key rather than the line it sits on: rustfmt
        // splits a long call across lines, and a gate that depends on
        // formatting fails for the wrong reason — which is how this one
        // failed twice already.
        let start = at.saturating_sub(120);
        let end = (at + quoted.len() + 200).min(sources.len());
        let window = &sources[start..end];
        // A reader is a lookup: a call in Rust (`get(`, `setting(`,
        // `sound_file(`) or a bracket access in the web client
        // (`values[...]`, `settings[...]`). A bare mention is not a reader.
        if window.contains("get(")
            || window.contains("setting(")
            || window.contains("_file(")
            || window.contains("values[")
            || window.contains("settings[")
        {
            return true;
        }
        from = at + quoted.len();
    }
    false
}

/// The quoted words following a marker, in file order.
fn declared(text: &str, marker: &str) -> Vec<String> {
    text.match_indices(marker)
        .filter_map(|(at, _)| {
            let rest = &text[at + marker.len()..];
            rest.find('"').map(|end| rest[..end].to_owned())
        })
        .collect()
}

/// The `UNWIRED` table, as pairs.
fn unwired_entries(schema: &str) -> Vec<(String, String)> {
    let Some(start) = schema.find("pub const UNWIRED") else {
        return Vec::new();
    };
    let body = &schema[start..];
    let Some(end) = body.find("\n];") else {
        return Vec::new();
    };
    // Every quoted string in the block, paired up. Reading it line by line
    // broke the moment rustfmt wrapped an entry across three lines — a gate
    // that depends on formatting is a gate that fails for the wrong reason.
    let quoted: Vec<String> = body[..end]
        .split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect();
    quoted
        .chunks_exact(2)
        .map(|pair| (pair[0].clone(), pair[1].clone()))
        .collect()
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

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod settings_reader_tests {
    use super::{reads, unwired_entries};

    #[test]
    fn a_name_collision_is_not_a_reader() {
        // `omt-theme` maps a VS Code key spelled `terminal.background`. A gate
        // that counted that would report a setting as wired because two
        // unrelated things are spelled the same — which is the exact shape of
        // defect it exists to catch.
        let collision = "    let background = get(\"terminal.background\")";
        assert!(reads(collision, "terminal.background"));
        let mention = "    // terminal.background is documented in 10 §3";
        assert!(!reads(mention, "terminal.background"));
    }

    #[test]
    fn a_lookup_is_what_counts_as_reading() {
        let lookup = "let when = When::parse(&setting(&state, \"notifications.when\"));";
        assert!(reads(lookup, "notifications.when"));
        let listed = "    pub key: \"notifications.when\",";
        assert!(!reads(listed, "notifications.when"));
    }

    #[test]
    fn a_type_script_key_is_read_too() {
        // The web client reads settings with single quotes, and the gate once
        // reported eleven wired settings as unwired because it only looked
        // for double-quoted keys.
        let ts = "      values['appearance.font.family'] = entry.value";
        assert!(reads(ts, "appearance.font.family"));
    }

    #[test]
    fn a_call_rustfmt_split_across_lines_still_counts() {
        // The gate once reported a setting as unread because the call and the
        // key sat on different lines after formatting — a gate failing for
        // the wrong reason.
        let split = "let cwd = canonical_root(&match terminal_bits::setting(\n        &state,\n        \"terminal.default_cwd\",\n    )";
        assert!(reads(split, "terminal.default_cwd"));
    }

    #[test]
    fn the_unwired_table_survives_being_reformatted() {
        // rustfmt wraps a long entry across three lines. A gate that read the
        // table line by line failed the moment it did, which is a gate failing
        // for the wrong reason.
        let wrapped = "pub const UNWIRED: &[(&str, &str)] = &[\n    (\n        \"a.b\",\n        \"why\",\n    ),\n];";
        assert_eq!(
            unwired_entries(wrapped),
            vec![("a.b".to_owned(), "why".to_owned())]
        );
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod isolation_tests {
    /// The shape the gate looks for, so the rule is checked rather than
    /// believed: a file that starts omt and names neither the local path nor a
    /// runtime directory of its own.
    #[test]
    fn the_gate_refuses_a_test_that_would_join_the_machines_instance() {
        let careless = "let exe = env!(\"CARGO_BIN_EXE_omt\");";
        assert!(careless.contains("CARGO_BIN_EXE_omt"));
        assert!(!careless.contains("OMT_LOCAL"));
        assert!(!careless.contains("XDG_RUNTIME_DIR"));
    }
}
