//! Build tasks: the layering check and codegen.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

fn main() -> Result<()> {
    match std::env::args().nth(1).as_deref() {
        Some("layering") => layering(),
        Some("codegen") => codegen(false),
        Some("codegen-check") => codegen(true),
        Some(other) => bail!("unknown task `{other}`"),
        None => {
            eprintln!("usage: cargo xtask <layering|codegen|codegen-check>");
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
        | "omt-input" => 2,
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
    out.push_str("export const CAPABILITY_INFO: Record<Capability, { title: string; group: string; kind: string; role: string; doc: string }> = {\n");
    for e in entries {
        out.push_str(&format!(
            "  '{}': {{ title: {}, group: '{}', kind: '{}', role: '{}', doc: {} }},\n",
            field(e, "name"),
            json_string(&field(e, "title")),
            field(e, "group"),
            e.get("kind").map_or("query".to_owned(), render_tag),
            e.get("role").map_or("viewer".to_owned(), render_tag),
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
    out
}
