//! Build tasks: the layering check and codegen.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

fn main() -> Result<()> {
    match std::env::args().nth(1).as_deref() {
        Some("layering") => layering(),
        Some("file-length") => file_length(),
        Some("licensing") => licensing(std::env::args().nth(2)),
        Some("handlers") => handlers(),
        Some("codegen") => codegen(false).and_then(|()| handlers()),
        Some("codegen-check") => codegen(true),
        Some("install") => install(),
        Some(other) => bail!("unknown task `{other}`"),
        None => {
            eprintln!(
                "usage: cargo xtask <layering|licensing|handlers|codegen|codegen-check|install>"
            );
            Ok(())
        }
    }
}

fn workspace_root() -> Result<PathBuf> {
    let mut dir = std::env::current_dir()?;
    loop {
        if dir.join("Cargo.toml").is_file() && dir.join("crates").is_dir() {
            return Ok(dir);
        }
        if !dir.pop() {
            bail!("could not find the workspace root");
        }
    }
}

/// Which layer a crate sits in. Layer *n* may depend downward only.
///
/// Encoded here rather than only in prose so a violation is a failing check
/// rather than a review comment somebody might miss.
fn layer_of(name: &str) -> Option<u8> {
    Some(match name {
        "omt-types" | "omt-util" => 0,
        "omt-catalog" | "omt-events" | "omt-proto" => 1,
        "omt-term" | "omt-pty" | "omt-agent-adapters" | "omt-transport" | "omt-auth"
        | "omt-identity" | "omt-stt" | "omt-media" | "omt-workspace-fs" | "omt-open"
        | "omt-input" | "omt-theme" => 2,
        "omt-session" | "omt-agent" | "omt-config" | "omt-store" | "omt-recall" => 3,
        "omt-daemon" => 4,
        "omt-tui" | "omt-server" | "omt-plugin-host" => 5,
        "omt" | "omt-hook" => 6,
        _ => return None,
    })
}

/// The L1 order, which layer numbers alone cannot express.
///
/// `omt-catalog` depends on nothing in L1, so the declaration machinery is
/// usable by any crate — including ones declaring event-shaped capabilities.
/// The forbidden edge is `omt-catalog -> omt-events`: it is the one that closes
/// a cycle, and a capability typed on an `Event` would create it.
fn l1_rank(name: &str) -> Option<u8> {
    Some(match name {
        "omt-catalog" => 0,
        "omt-events" => 1,
        "omt-proto" => 2,
        _ => return None,
    })
}

const L2_EXCEPTIONS: &[(&str, &str)] = &[
    ("omt-agent-adapters", "omt-pty"),
    ("omt-media", "omt-term"),
    ("omt-auth", "omt-identity"),
];

const LEAVES: &[&str] = &["omt-tui", "omt-server", "omt-plugin-host"];

fn read_deps(manifest: &Path) -> Result<BTreeSet<String>> {
    let text = std::fs::read_to_string(manifest)
        .with_context(|| format!("reading {}", manifest.display()))?;
    let mut deps = BTreeSet::new();
    let mut in_deps = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_deps = line == "[dependencies]" || line == "[dev-dependencies]";
            continue;
        }
        if !in_deps || line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((name, _)) = line.split_once('=') {
            let name = name.trim().trim_matches('"');
            if name.starts_with("omt") {
                deps.insert(name.to_owned());
            }
        }
    }
    Ok(deps)
}

/// The licensing gate: `docs/architecture/14-licensing.md` §1, §5, §6 and §7.
///
/// A licence policy that lives only in prose is one nobody notices breaking.
/// Each check here corresponds to a way this project could quietly acquire a
/// problem that is expensive to unwind later — a vendored file from a codebase
/// under a different licence, a dependency whose terms omt cannot accept, an
/// attribution that silently went stale.
fn licensing(root: Option<String>) -> Result<()> {
    // An optional root so the gate can be pointed at a fixture. A gate that
    // has only ever been observed passing is one nobody has shown to work,
    // and the check that matters most here — studied code being vendored —
    // cannot be provoked in the real repository without committing the very
    // thing it forbids.
    let root = match root {
        Some(path) => std::path::PathBuf::from(path),
        None => repo_root()?,
    };
    let mut problems: Vec<String> = Vec::new();
    let mut checked = 0usize;

    // §1. The licence omt is under, as a file rather than only a manifest
    // field. The field tells cargo; the file is what anybody actually reads.
    let license = root.join("LICENSE");
    if !license.is_file() {
        problems.push("LICENSE is missing".to_owned());
    } else {
        let text = std::fs::read_to_string(&license)?;
        if !text.contains("Apache License") {
            problems.push("LICENSE is not the Apache 2.0 text the manifests declare".to_owned());
        }
    }

    // §5. `.research/` holds other people's code, read to understand problems
    // and never to copy from. Two separate failures: it stops being ignored,
    // or something inside it gets committed. The second is the one that
    // matters, and being ignored is not proof it did not happen — a file added
    // with `-f` stays tracked forever after.
    let ignore = std::fs::read_to_string(root.join(".gitignore")).unwrap_or_default();
    if !ignore
        .lines()
        .any(|l| l.trim().trim_end_matches('/') == ".research")
    {
        problems.push(".research/ is no longer gitignored".to_owned());
    }
    let tracked = std::process::Command::new("git")
        .args(["ls-files", ".research"])
        .current_dir(&root)
        .output()?;
    let tracked = String::from_utf8_lossy(&tracked.stdout);
    let vendored: Vec<&str> = tracked.lines().filter(|l| !l.is_empty()).collect();
    if !vendored.is_empty() {
        problems.push(format!(
            "{} file(s) under .research/ are tracked by git. Studied code must never be vendored: {}",
            vendored.len(),
            vendored.iter().take(5).cloned().collect::<Vec<_>>().join(", ")
        ));
    }

    // §6. Every dependency's licence, checked against the policy rather than
    // trusted. The forbidden list is checked first and by prefix, because the
    // dangerous ones are families — `GPL-3.0-or-later` and `AGPL-3.0` are not
    // strings anybody enumerates correctly by hand — and because a dual
    // licence with one forbidden arm must not pass on the strength of its
    // acceptable arm without somebody deciding that deliberately.
    match dependency_licences(&root) {
        Err(e) => problems.push(format!("could not read dependency licences: {e}")),
        Ok(deps) => {
            checked = deps.len();
            for (name, licence) in deps {
                if let Some((_, why)) = EXCEPTIONS.iter().find(|(n, _)| *n == name) {
                    debug_assert!(
                        !why.is_empty(),
                        "an exception with no reason is a silencing"
                    );
                    continue;
                }
                let Some(expression) = licence else {
                    problems.push(format!(
                        "`{name}` declares no licence at all, which is case-by-case, not allowed by default"
                    ));
                    continue;
                };
                if let Some(bad) = FORBIDDEN.iter().find(|f| {
                    expression
                        .split(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '.')
                        .any(|part| part.starts_with(**f))
                }) {
                    problems.push(format!(
                        "`{name}` is `{expression}`, which contains the forbidden `{bad}`"
                    ));
                    continue;
                }
                if !expression_allowed(&expression) {
                    problems.push(format!(
                        "`{name}` is `{expression}`, which is not covered by the allow list"
                    ));
                }
            }
        }
    }

    // §7. NOTICE exists and says what it is for. Checked for substance rather
    // than presence: an empty NOTICE satisfies a file-exists test and no
    // obligation at all.
    let notice = root.join("NOTICE");
    if !notice.is_file() {
        problems.push("NOTICE is missing".to_owned());
    } else {
        let text = std::fs::read_to_string(&notice)?;
        if text.trim().len() < 100 {
            problems.push("NOTICE is too short to be carrying any attribution".to_owned());
        }
        if !text.contains("Apache") {
            problems.push("NOTICE does not name the licence it is required by".to_owned());
        }
    }

    // §8. The contributor rule, as a file a reviewer actually sees. A rule that
    // lives only in an architecture document is one nobody opening a pull
    // request reads, and the whole mechanism is that the question gets asked at
    // the moment the code arrives.
    let template = root.join(".github/PULL_REQUEST_TEMPLATE.md");
    if !template.is_file() {
        problems.push(
            "the pull-request template is missing, so §8's contributor rule is asked of nobody"
                .to_owned(),
        );
    } else {
        let text = std::fs::read_to_string(&template)?;
        // Each of the three §8 asks, by the thing it is about rather than by
        // its wording, so rephrasing the template does not break the gate and
        // deleting a check does.
        for (needle, what) in [
            ("GPL", "the no-copied-code declaration"),
            ("docs/research/", "the cite-your-source declaration"),
            ("deny.toml", "the new-dependency declaration"),
        ] {
            if !text.contains(needle) {
                problems.push(format!("the pull-request template has lost {what}"));
            }
        }
    }

    // §2 and §3. The classes that constrain omt are GPL and AGPL, and the claim
    // that nothing is derived from them is what lets omt be Apache-2.0. The
    // dependency scan above enforces it for code omt links; this asserts the
    // forbidden families are still in the list that scan uses, so the check
    // cannot be quietly emptied.
    for family in ["GPL", "AGPL"] {
        if !FORBIDDEN.contains(&family) {
            problems.push(format!(
                "`{family}` is no longer a forbidden licence family, which is the rule that lets omt be Apache-2.0"
            ));
        }
    }

    // §6. Every crate in the workspace declares a licence, and it is omt's.
    // A crate that inherits nothing is one that is published under no terms.
    for member in workspace_members(&root)? {
        let manifest = std::fs::read_to_string(member.join("Cargo.toml"))?;
        if !manifest.contains("license") && !manifest.contains("license.workspace") {
            problems.push(format!(
                "{} declares no licence",
                member.file_name().unwrap_or_default().to_string_lossy()
            ));
        }
    }

    if problems.is_empty() {
        println!(
            "licensing ok — LICENSE, NOTICE, .research/ unvendored, every crate licensed, {} dependencies checked ({} by exception)",
            checked,
            EXCEPTIONS.len()
        );
        Ok(())
    } else {
        for p in &problems {
            eprintln!("  {p}");
        }
        bail!("{} licensing problem(s)", problems.len())
    }
}

/// Licences allowed without review, from §6.
const ALLOWED: &[&str] = &[
    "Apache-2.0",
    "MIT",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Zlib",
    "Unicode-3.0",
    "CC0-1.0",
    "MPL-2.0",
    // Bundled with Apache-2.0 by several crates in the ecosystem; the
    // exception narrows Apache's patent grant rather than adding a term.
    "LLVM-exception",
    "Unlicense",
    "Unicode-DFS-2016",
];

/// Licence families omt cannot accept, from §6.
///
/// Prefixes, because these are families: `GPL-3.0-or-later` and `AGPL-3.0`
/// are not strings anybody enumerates correctly by hand.
const FORBIDDEN: &[&str] = &["GPL", "AGPL", "LGPL", "SSPL", "BUSL", "Elastic", "CC-BY-NC"];

/// Dependencies §6 calls case-by-case, each with its justification.
///
/// Kept here rather than in a data file: an exception is a decision, and a
/// decision with no reviewer attached is how an allow list becomes a list of
/// things somebody once silenced. In this form it cannot be added without
/// appearing in a diff next to the rule it is an exception to.
const EXCEPTIONS: &[(&str, &str)] = &[
    (
        "r-efi",
        "Triple-licensed MIT OR Apache-2.0 OR LGPL-2.1-or-later. omt takes the Apache-2.0 arm; the LGPL arm is one of three and is not exercised. Reached only as a transitive UEFI target dependency and never linked on the platforms omt ships.",
    ),
    (
        "webpki-roots",
        "CDLA-Permissive-2.0, a permissive data licence covering the Mozilla root certificate bundle rather than code. It imposes no restriction on distribution and no copyleft.",
    ),
];

/// Whether an SPDX expression is satisfied by the allow list.
///
/// `OR` needs one arm, `AND` needs all of them. Requiring every arm of an `OR`
/// was the first version of this and it rejected half the ecosystem: `Apache-2.0
/// OR MIT` is a choice the *user* of the crate makes, and omt takes the arm it
/// can accept. `/` is the same thing in the older spelling several crates still
/// use, and `WITH` attaches an exception to the licence on its left.
fn expression_allowed(expression: &str) -> bool {
    let normalised = expression.replace('/', " OR ");
    normalised.split(" OR ").any(|arm| {
        arm.split(" AND ").all(|term| {
            let term = term.trim().trim_matches(|c| c == '(' || c == ')').trim();
            let base = term.split(" WITH ").next().unwrap_or(term).trim();
            ALLOWED.contains(&base)
        })
    })
}

/// Every dependency and the licence it declares.
fn dependency_licences(root: &std::path::Path) -> Result<Vec<(String, Option<String>)>> {
    let output = std::process::Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--all-features"])
        .current_dir(root)
        .output()?;
    if !output.status.success() {
        bail!("cargo metadata failed");
    }
    let meta: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let mut out = Vec::new();
    for package in meta["packages"].as_array().into_iter().flatten() {
        let name = package["name"].as_str().unwrap_or("?").to_owned();
        // omt's own crates are covered by §1 and checked there.
        if name.starts_with("omt") || name == "xtask" {
            continue;
        }
        out.push((name, package["license"].as_str().map(str::to_owned)));
    }
    Ok(out)
}

/// Regenerate `web/src/generated/handlers.json` from the web client's own
/// handler table.
///
/// It used to be maintained by hand beside `capabilities.ts`, which meant two
/// gates checking the same property from two sources: the Rust parity test
/// reads this file and the Python one reads the TypeScript. They drifted, and
/// the drift was invisible until a capability happened to be added to one and
/// not the other. Derived from the single source, they cannot disagree.
fn handlers() -> Result<()> {
    let root = repo_root()?;
    let source = std::fs::read_to_string(root.join("web/src/capabilities.ts"))?;
    let mut names: Vec<String> = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        // A handler entry is `'name': (` at the start of a line. Comments
        // mentioning a capability in prose are indented differently and do not
        // match, which is why the shape is required rather than just the quotes.
        let Some(rest) = trimmed.strip_prefix('\'') else {
            continue;
        };
        let Some(end) = rest.find('\'') else { continue };
        let name = &rest[..end];
        if rest[end + 1..].trim_start().starts_with(':') && name.contains('.') {
            names.push(name.to_owned());
        }
    }
    names.sort();
    names.dedup();
    if names.is_empty() {
        bail!("no handlers found in web/src/capabilities.ts — the shape must have changed");
    }
    let path = root.join("web/src/generated/handlers.json");
    let mut json = String::from("[\n");
    for (i, name) in names.iter().enumerate() {
        json.push_str(&format!("  \"{name}\""));
        if i + 1 < names.len() {
            json.push(',');
        }
        json.push('\n');
    }
    json.push_str("]\n");
    std::fs::write(&path, json)?;
    println!("wrote {} ({} handlers)", path.display(), names.len());
    Ok(())
}

/// Every setting, as a page somebody can read.
///
/// Generated from the same table `config.schema` serves and the validator
/// checks against, so a setting cannot exist in one and not the others. A
/// hand-written settings page is wrong the first time a default changes, and
/// nothing fails when it drifts — which is the same argument as the capability
/// reference beside it.
fn settings_reference() -> String {
    let mut out = String::from(
        "# Settings\n\nGenerated from `omt-config`'s schema. Every setting has a default, because a feature works before it is configured.\n\n| Key | Holds | Default | Reload | What it does |\n|---|---|---|---|---|\n",
    );
    let mut settings: Vec<&omt_config::schema::Setting> =
        omt_config::schema::SETTINGS.iter().collect();
    settings.sort_by_key(|s| s.key);
    for setting in settings {
        let choices = if setting.choices.is_empty() {
            setting.kind.as_str().to_owned()
        } else {
            setting.choices.join(" \\| ")
        };
        let reload = match omt_config::schema::reload_class(setting.key) {
            omt_config::schema::Reload::Hot => "immediately",
            omt_config::schema::Reload::RestartServer => "on listener restart",
            omt_config::schema::Reload::Restart => "on restart",
        };
        out.push_str(&format!(
            "| `{}` | {} | `{}` | {} | {} |\n",
            setting.key, choices, setting.default, reload, setting.doc
        ));
    }
    out
}

/// The repository root, from this file's location rather than the cwd.
fn repo_root() -> Result<std::path::PathBuf> {
    let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    Ok(here
        .parent()
        .ok_or_else(|| anyhow::anyhow!("xtask has no parent directory"))?
        .to_path_buf())
}

/// Every crate directory in the workspace.
fn workspace_members(root: &std::path::Path) -> Result<Vec<std::path::PathBuf>> {
    let mut members = Vec::new();
    for entry in std::fs::read_dir(root.join("crates"))? {
        let path = entry?.path();
        if path.join("Cargo.toml").is_file() {
            members.push(path);
        }
    }
    Ok(members)
}

/// P1's file-length gate: warn at 1200 lines, fail at 2000.
///
/// [01 §P1](../../docs/architecture/01-principles.md) names this as the
/// enforcement for "a module over ~1500 lines is a design smell and must be
/// split before it grows further". It was the one part of P1 with no check
/// behind it, which is how a 6000-line module came to exist: the rule was
/// written down, nothing measured it, and every individual addition was small.
///
/// Tests are counted with the code they test. A file is one unit to a reader
/// however its halves are labelled, and excusing the test half is how a file
/// stays "1900 lines of code" forever.
fn file_length() -> Result<()> {
    const WARN: usize = 1200;
    const FAIL: usize = 2000;

    let root = workspace_root()?;
    let mut sources = Vec::new();
    collect_rust_files(&root.join("crates"), &mut sources)?;
    collect_rust_files(&root.join("xtask"), &mut sources)?;
    sources.sort();

    let mut warned = Vec::new();
    let mut failed = Vec::new();
    for path in &sources {
        let lines = std::fs::read_to_string(path)?.lines().count();
        let shown = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .display()
            .to_string();
        if lines >= FAIL {
            failed.push((shown, lines));
        } else if lines >= WARN {
            warned.push((shown, lines));
        }
    }

    for (path, lines) in &warned {
        println!("warning: {path} is {lines} lines (over {WARN})");
    }
    if failed.is_empty() {
        println!(
            "file length ok — {} files checked, {} over {WARN}",
            sources.len(),
            warned.len()
        );
        return Ok(());
    }
    for (path, lines) in &failed {
        println!("error: {path} is {lines} lines (over {FAIL})");
    }
    anyhow::bail!("{} file(s) over the {FAIL}-line limit (P1)", failed.len())
}

/// Every `.rs` file under a directory.
fn collect_rust_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            // `target` is build output and `.research` is studied material that
            // is never built; neither is omt's code to keep small.
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if name == "target" || name.starts_with('.') {
                continue;
            }
            collect_rust_files(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    Ok(())
}

fn layering() -> Result<()> {
    let root = workspace_root()?;
    let mut graph: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for entry in std::fs::read_dir(root.join("crates"))? {
        let dir = entry?.path();
        let manifest = dir.join("Cargo.toml");
        if !manifest.is_file() {
            continue;
        }
        let name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .context("crate directory has no name")?
            .to_owned();
        graph.insert(name, read_deps(&manifest)?);
    }

    let mut problems = Vec::new();

    for (name, deps) in &graph {
        let Some(layer) = layer_of(name) else {
            problems.push(format!("`{name}` is not in the layer map"));
            continue;
        };
        for dep in deps {
            let Some(dep_layer) = layer_of(dep) else {
                continue;
            };
            if dep_layer > layer {
                problems.push(format!(
                    "`{name}` (L{layer}) depends on `{dep}` (L{dep_layer}) — layers only go down"
                ));
            }
            // Leaves may be linked by a binary — that is what a binary is for,
            // assembling the surfaces. What the rule forbids is a *library*
            // building on top of one, which would make a surface something
            // another crate has to satisfy.
            if LEAVES.contains(&dep.as_str()) && name != dep && layer < 6 {
                problems.push(format!(
                    "`{name}` depends on `{dep}`, which is a leaf; only a binary may link a surface"
                ));
            }
            if let (Some(a), Some(b)) = (l1_rank(name), l1_rank(dep))
                && b >= a
            {
                problems.push(format!(
                    "`{name}` depends on `{dep}`, against the L1 order; the reverse edge closes a cycle"
                ));
            }
            if layer == 2
                && dep_layer == 2
                && name != dep
                && !L2_EXCEPTIONS.contains(&(name.as_str(), dep.as_str()))
            {
                problems.push(format!(
                    "`{name}` depends on `{dep}`; L2 crates are independent except for three documented exceptions"
                ));
            }
        }
    }

    if let Some(cycle) = find_cycle(&graph) {
        problems.push(format!("dependency cycle: {}", cycle.join(" -> ")));
    }

    if problems.is_empty() {
        println!("layering ok — {} crates checked", graph.len());
        Ok(())
    } else {
        for p in &problems {
            eprintln!("x {p}");
        }
        bail!("{} layering violation(s)", problems.len())
    }
}

fn find_cycle(graph: &BTreeMap<String, BTreeSet<String>>) -> Option<Vec<String>> {
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        Open,
        Done,
    }
    fn walk(
        node: &str,
        graph: &BTreeMap<String, BTreeSet<String>>,
        marks: &mut BTreeMap<String, Mark>,
        stack: &mut Vec<String>,
    ) -> Option<Vec<String>> {
        marks.insert(node.to_owned(), Mark::Open);
        stack.push(node.to_owned());
        for dep in graph.get(node).into_iter().flatten() {
            match marks.get(dep) {
                Some(Mark::Open) => {
                    let mut cycle = stack.clone();
                    cycle.push(dep.clone());
                    return Some(cycle);
                }
                Some(Mark::Done) => {}
                None => {
                    if let Some(c) = walk(dep, graph, marks, stack) {
                        return Some(c);
                    }
                }
            }
        }
        stack.pop();
        marks.insert(node.to_owned(), Mark::Done);
        None
    }
    let mut marks = BTreeMap::new();
    for node in graph.keys() {
        if !marks.contains_key(node) {
            let mut stack = Vec::new();
            if let Some(c) = walk(node, graph, &mut marks, &mut stack) {
                return Some(c);
            }
        }
    }
    None
}

fn codegen(check: bool) -> Result<()> {
    let root = workspace_root()?;

    // Build and run the binary, then read the catalog out of it: the input is
    // then byte-for-byte the list the process registers, so a declaration that
    // failed to link is absent and the diff fails, rather than the capability
    // silently vanishing.
    let status = Command::new(env!("CARGO"))
        .args(["build", "--quiet", "-p", "omt"])
        .current_dir(&root)
        .status()
        .context("building omt")?;
    if !status.success() {
        bail!("could not build omt");
    }

    let out = Command::new(root.join("target/debug/omt"))
        .args(["debug", "catalog-dump", "--json"])
        .output()
        .context("running catalog-dump")?;
    if !out.status.success() {
        bail!(
            "catalog-dump failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let dump: serde_json::Value =
        serde_json::from_slice(&out.stdout).context("parsing the catalog dump")?;
    let rendered = serde_json::to_string_pretty(&dump)? + "\n";

    let entries = dump
        .get("capabilities")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let artifacts = [
        (root.join("schemas/catalog.v1.json"), rendered),
        (
            root.join("web/src/generated/catalog.ts"),
            typescript(&entries),
        ),
        (
            root.join("docs/reference/capabilities.md"),
            reference(&entries),
        ),
        (
            root.join("docs/reference/settings.md"),
            settings_reference(),
        ),
    ];

    let mut stale = Vec::new();
    for (path, contents) in &artifacts {
        if check {
            let existing = std::fs::read_to_string(path).unwrap_or_default();
            if existing != *contents {
                stale.push(path.display().to_string());
            }
        } else {
            std::fs::create_dir_all(path.parent().context("output dir")?)?;
            std::fs::write(path, contents)?;
            println!("wrote {}", path.display());
        }
    }

    if check {
        if stale.is_empty() {
            println!("generated artifacts are current");
        } else {
            // Named individually: "something is stale" sends somebody looking
            // through three files for the one that moved.
            bail!(
                "stale, run `cargo xtask codegen`:\n  {}",
                stale.join("\n  ")
            );
        }
    }
    Ok(())
}

/// The capability names and their shapes, for the web client.
///
/// Generated because the alternative is a hand-maintained copy of the Rust
/// definition, and a copy drifts. Drift here does not fail a build — it makes a
/// client send a field the server does not read, which shows up as a feature
/// quietly not working.
fn typescript(entries: &[serde_json::Value]) -> String {
    let field = |e: &serde_json::Value, k: &str| {
        e.get(k)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned()
    };

    let mut out = [
        "// @generated by `cargo xtask codegen` — do not edit.",
        "//",
        "// The capability catalog as the binary actually registers it. Hand",
        "// editing this would let the client and the server disagree about",
        "// what exists, which surfaces as a feature quietly not working rather",
        "// than as a build failure.",
        "",
        "",
    ]
    .join(
        "
",
    );

    out.push_str("/** Every capability this build declares. */\nexport const CAPABILITIES = [\n");
    for e in entries {
        out.push_str(&format!("  '{}',\n", field(e, "name")));
    }
    out.push_str("] as const\n\n");
    out.push_str("/** A capability this build declares. */\nexport type Capability = (typeof CAPABILITIES)[number]\n\n");

    out.push_str("/** What each capability is, for a palette or a settings screen. */\n");
    out.push_str("export const CAPABILITY_INFO: Record<Capability, { title: string; group: string; kind: string; role: string; effects: readonly string[]; doc: string }> = {\n");
    for e in entries {
        // Effects are carried into the client because 12 §7's optimistic-UI
        // rule is mechanical: nothing DESTRUCTIVE may be applied before it is
        // confirmed. A client that had to keep its own list of which those are
        // would be wrong the day one is added.
        let effects = e
            .get("effects")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|i| i.as_str())
                    .map(|s| format!("'{s}'"))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        out.push_str(&format!(
            "  '{}': {{ title: {}, group: '{}', kind: '{}', role: '{}', effects: [{}], doc: {} }},\n",
            field(e, "name"),
            json_string(&field(e, "title")),
            field(e, "group"),
            e.get("kind").map_or("query".to_owned(), render_tag),
            e.get("role").map_or("viewer".to_owned(), render_tag),
            effects,
            json_string(&field(e, "doc")),
        ));
    }
    out.push_str("}\n");
    out
}

fn render_tag(v: &serde_json::Value) -> String {
    v.as_str().map_or_else(
        || v.to_string().trim_matches('"').to_owned(),
        std::borrow::ToOwned::to_owned,
    )
}

fn json_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "''".to_owned())
}

/// The capability reference, for humans.
fn reference(entries: &[serde_json::Value]) -> String {
    let field = |e: &serde_json::Value, k: &str| {
        e.get(k)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned()
    };

    let mut out = [
        "# Capability reference",
        "",
        "<!-- @generated by `cargo xtask codegen` — do not edit. -->",
        "",
        "Every capability this build registers. Generated from the binary",
        "rather than from source, so a declaration that failed to link is",
        "absent here and the diff fails — instead of the capability silently",
        "vanishing.",
        "",
        "| Capability | Kind | Role | What it does |",
        "|---|---|---|---|",
        "",
    ]
    .join(
        "
",
    );
    for e in entries {
        out.push_str(&format!(
            "| `{}` | {} | {} | {} |\n",
            field(e, "name"),
            e.get("kind").map_or("query".to_owned(), render_tag),
            e.get("role").map_or("viewer".to_owned(), render_tag),
            field(e, "doc").replace('|', "\\|"),
        ));
    }
    out.push_str(&format!("\n{} capabilities.\n", entries.len()));

    // The exemptions, listed rather than buried. `Parity::Exempt`'s own doc
    // says its reason is "printed in the generated reference, so it is argued
    // in public" — and it was not, which made an exemption a thing you could
    // add and nobody would ever read. A gate whose escape hatch is invisible
    // is a gate that erodes.
    let exempt: Vec<&serde_json::Value> = entries
        .iter()
        .filter(|e| e.get("parity").and_then(|p| p.get("surfaces")).is_some())
        .collect();
    out.push_str("\n## Parity exemptions\n\n");
    if exempt.is_empty() {
        out.push_str("None. Every capability is reachable from every surface.\n");
        return out;
    }
    out.push_str("| Capability | Surfaces excused | Why |\n|---|---|---|\n");
    for e in exempt {
        let parity = e.get("parity").expect("filtered above");
        let surfaces = parity
            .get("surfaces")
            .and_then(|s| s.as_array())
            .map(|a| a.iter().map(render_tag).collect::<Vec<_>>().join(", "))
            .unwrap_or_default();
        out.push_str(&format!(
            "| `{}` | {surfaces} | {} |\n",
            field(e, "name"),
            parity
                .get("reason")
                .and_then(|r| r.as_str())
                .unwrap_or("(no reason given)")
                .replace('|', "\\|"),
        ));
    }
    out
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod determinism_tests {
    use super::*;

    /// The generators, twice, over the same input.
    ///
    /// The gate this protects is `codegen-check`, which fails CI on a diff. If
    /// generation is not deterministic that gate fires on changes nobody made,
    /// and a gate that cries wolf is one people regenerate past without
    /// reading — which is the only way it was ever going to catch anything.
    ///
    /// The usual sources are a `HashMap` iteration order and a timestamp, and
    /// both produce a diff that looks exactly like a real change. The other
    /// half of this — that the *dump* is stable — is in `omt`'s own suite,
    /// where the binary that produces it lives.
    #[test]
    fn the_generators_are_pure_functions_of_their_input() {
        let entries: Vec<serde_json::Value> = (0..40)
            .map(|i| {
                serde_json::json!({
                    "name": format!("group{}.verb{}", i % 7, i),
                    "kind": if i % 2 == 0 { "query" } else { "command" },
                    "role": "operator",
                    "doc": format!("capability {i}"),
                    "parity": if i % 11 == 0 {
                        serde_json::json!({ "surfaces": ["web"], "reason": "reason" })
                    } else {
                        serde_json::json!("full")
                    },
                })
            })
            .collect();

        assert_eq!(reference(&entries), reference(&entries));
        assert_eq!(settings_reference(), settings_reference());
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "in a test, expect() is the assertion")]
mod reference_tests {
    use super::*;

    fn entry(name: &str, parity: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "name": name,
            "kind": "query",
            "role": "viewer",
            "doc": "does a thing",
            "parity": parity,
        })
    }

    #[test]
    fn an_exemption_is_printed_with_its_reason() {
        // `Parity::Exempt`'s own doc promises this — "printed in the generated
        // reference, so it is argued in public" — and it was not. An escape
        // hatch nobody can see is a gate that erodes.
        let out = reference(&[entry(
            "thing.do",
            serde_json::json!({ "surfaces": ["web"], "reason": "no phone can do it" }),
        )]);
        assert!(out.contains("## Parity exemptions"), "{out}");
        assert!(out.contains("`thing.do`"), "{out}");
        assert!(out.contains("no phone can do it"), "{out}");
    }

    #[test]
    fn a_catalog_with_no_exemptions_says_so_rather_than_omitting_the_section() {
        // A missing section reads as "not checked". "None" reads as "checked,
        // and there are none" — which is the claim actually being made.
        let out = reference(&[entry("thing.do", serde_json::json!("full"))]);
        assert!(out.contains("None. Every capability is reachable"), "{out}");
    }
}

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
fn install() -> Result<()> {
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
