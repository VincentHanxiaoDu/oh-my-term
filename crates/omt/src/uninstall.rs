//! Removing omt's own footprint.
//!
//! `docs/architecture/19-onboarding.md` §8. A tool that cannot cleanly
//! uninstall does not get installed, so this is a feature with tests rather
//! than a paragraph in a README.
//!
//! Everything here is driven by the setup manifest, which records every
//! outward-facing write with a hash of what omt put there. That hash is the
//! whole design. It is what separates "omt wrote this line and can take it
//! back" from "somebody has since edited this line", and those two cases must
//! not be treated alike: **deleting a line a user has customised is the sin
//! this check exists to prevent.** Where the hash disagrees, omt comments the
//! block out, prints it, and says so — it does not delete it.
//!
//! Anything absent from the manifest is never touched. Not "usually not
//! touched": an uninstall that reasons about what it *probably* put somewhere
//! is one that eventually removes somebody else's configuration.

use std::path::{Path, PathBuf};

use omt_catalog::{CapabilityError, CapabilityHandler};
use serde::{Deserialize, Serialize};

/// What kind of thing omt wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Wrote {
    /// A block inside a file omt does not own, such as a shell rc.
    Block,
    /// A whole file omt created.
    File,
    /// A directory omt owns outright.
    Directory,
}

/// One outward-facing write, as the manifest records it.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Entry {
    /// Where it went.
    pub path: String,
    /// What kind of write it was.
    pub wrote: Wrote,
    /// A hash of exactly what omt put there.
    pub hash: String,
    /// Whether removing it is held back by `--keep-config`.
    #[serde(default)]
    pub config: bool,
    /// Whether removing it is held back by `--keep-data`.
    #[serde(default)]
    pub data: bool,
}

/// Everything omt wrote outside its own binary.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Manifest {
    /// Every write, in the order they were made.
    pub entries: Vec<Entry>,
}

/// The markers a shell integration block is delimited by.
pub const BLOCK_START: &str = "# >>> omt shell integration >>>";
/// Likewise.
pub const BLOCK_END: &str = "# <<< omt shell integration <<<";

/// A stable hash of a block's contents.
///
/// Not cryptographic and not meant to be: the question is "has this been
/// edited", and the adversary is a text editor, not a forger.
#[must_use]
pub fn hash(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        h ^= u64::from(*byte);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// Find omt's block inside a file, as a line range.
#[must_use]
pub fn find_block(contents: &str) -> Option<(usize, usize)> {
    let lines: Vec<&str> = contents.lines().collect();
    let start = lines.iter().position(|l| l.trim() == BLOCK_START)?;
    let end = lines
        .iter()
        .skip(start)
        .position(|l| l.trim() == BLOCK_END)?
        + start;
    Some((start, end))
}

/// What will happen to one entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Delete it.
    Remove,
    /// Comment it out and say so, because it has been edited since omt wrote it.
    CommentOut,
    /// Leave it, because `--keep-config` or `--keep-data` said so.
    Kept,
    /// It is already gone.
    Absent,
}

/// One line of the plan.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct Step {
    /// What it concerns.
    pub path: String,
    /// What will be done.
    pub action: Action,
    /// Why, in words somebody can act on.
    pub reason: String,
}

/// Input to `uninstall.plan`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct PlanIn {
    /// Leave `~/.config/omt/` alone.
    #[serde(default)]
    pub keep_config: bool,
    /// Leave `~/.local/state/omt/` alone.
    #[serde(default)]
    pub keep_data: bool,
}

/// What `uninstall.plan` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct PlanOut {
    /// Every step, in the order it would run.
    pub steps: Vec<Step>,
    /// A hash of this plan, which `uninstall.apply` must quote back.
    pub plan: String,
    /// How to remove the binary, which omt will not do itself.
    ///
    /// The binary is the package manager's business. A program that deletes
    /// its own executable mid-run leaves a half-finished uninstall with
    /// nothing left to finish it.
    pub binary: String,
}

/// Build the plan from a manifest and what is actually on disk.
#[must_use]
pub fn plan(manifest: &Manifest, keep_config: bool, keep_data: bool, root: &Path) -> Vec<Step> {
    let mut steps = Vec::new();
    for entry in &manifest.entries {
        let path = root.join(entry.path.trim_start_matches('/'));
        let step = |action, reason: &str| Step {
            path: entry.path.clone(),
            action,
            reason: reason.to_owned(),
        };
        if entry.config && keep_config {
            steps.push(step(Action::Kept, "--keep-config"));
            continue;
        }
        if entry.data && keep_data {
            steps.push(step(Action::Kept, "--keep-data"));
            continue;
        }
        if !path.exists() {
            steps.push(step(Action::Absent, "already gone"));
            continue;
        }
        match entry.wrote {
            Wrote::Block => {
                let contents = std::fs::read_to_string(&path).unwrap_or_default();
                match find_block(&contents) {
                    None => steps.push(step(Action::Absent, "the block is no longer there")),
                    Some((start, end)) => {
                        let lines: Vec<&str> = contents.lines().collect();
                        let block = lines[start..=end].join("\n");
                        if hash(&block) == entry.hash {
                            steps.push(step(Action::Remove, "unchanged since omt wrote it"));
                        } else {
                            // The sin this whole check exists to prevent.
                            steps.push(step(
                                Action::CommentOut,
                                "edited since omt wrote it — commented out rather than deleted, so nothing you added is lost",
                            ));
                        }
                    }
                }
            }
            Wrote::File => {
                let contents = std::fs::read_to_string(&path).unwrap_or_default();
                if hash(&contents) == entry.hash {
                    steps.push(step(Action::Remove, "unchanged since omt wrote it"));
                } else {
                    steps.push(step(
                        Action::CommentOut,
                        "edited since omt wrote it — left in place",
                    ));
                }
            }
            Wrote::Directory => steps.push(step(Action::Remove, "omt owns this directory")),
        }
    }
    steps
}

/// A hash of the plan, so apply cannot run against a different one.
#[must_use]
pub fn plan_hash(steps: &[Step]) -> String {
    let joined: String = steps
        .iter()
        .map(|s| format!("{}:{:?}", s.path, s.action))
        .collect::<Vec<_>>()
        .join("\n");
    hash(&joined)
}

omt_catalog::capability! {
    /// Everything an uninstall would do, doing none of it.
    pub struct UninstallPlan;
    input  = PlanIn,
    output = PlanOut,
    decl = omt_catalog::Decl {
        name: "uninstall.plan",
        group: "uninstall",
        verb: "plan",
        title: "Preview an uninstall",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Admin,
        effects: omt_catalog::Effects::READS_FS,
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Driven by the setup manifest. Anything absent from it is never touched — an uninstall that reasons about what it probably wrote eventually removes somebody else's configuration.",
    },
}

/// Where the manifest and the things it names live.
pub struct Roots {
    /// The manifest file.
    pub manifest: PathBuf,
    /// What paths in the manifest are relative to. `/` in production; a
    /// temporary directory under test, which is the only way to test an
    /// uninstall without uninstalling something.
    pub root: PathBuf,
}

impl Roots {
    /// Where omt's manifest and footprint live on this machine.
    ///
    /// `OMT_UNINSTALL_ROOT` relocates both. That exists for the tests: an
    /// uninstall is the one capability that cannot be exercised against the
    /// real machine, and one that is only ever tested in pieces is one whose
    /// wiring is untested — which is precisely where this project keeps
    /// finding its bugs.
    #[must_use]
    pub fn from_environment() -> Self {
        let root =
            std::env::var("OMT_UNINSTALL_ROOT").map_or_else(|_| PathBuf::from("/"), PathBuf::from);
        let manifest = root.join(".local/state/omt/setup-manifest.json");
        Self { manifest, root }
    }
}

/// Serves the uninstall capabilities.
pub struct UninstallHandler(pub std::sync::Arc<Roots>);

impl UninstallHandler {
    /// Read the manifest, treating an absent one as an empty one.
    ///
    /// Absent is not an error: omt that never wrote anything outward has
    /// nothing to take back, and failing here would make a clean install the
    /// one case that cannot be uninstalled.
    fn manifest(&self) -> Result<Manifest, CapabilityError> {
        match std::fs::read_to_string(&self.0.manifest) {
            Err(_) => Ok(Manifest::default()),
            Ok(text) => serde_json::from_str(&text).map_err(|e| {
                CapabilityError::internal(format!(
                    "the setup manifest is unreadable: {e}. Refusing to guess what to remove."
                ))
            }),
        }
    }
}

impl CapabilityHandler<UninstallPlan> for UninstallHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: PlanIn,
    ) -> Result<PlanOut, CapabilityError> {
        let manifest = self.manifest()?;
        let steps = plan(&manifest, input.keep_config, input.keep_data, &self.0.root);
        Ok(PlanOut {
            plan: plan_hash(&steps),
            steps,
            binary: "omt does not delete its own binary. Remove it with your package manager, e.g. `brew uninstall omt`.".to_owned(),
        })
    }
}

/// Input to `uninstall.apply`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct ApplyIn {
    /// The hash of the plan being accepted.
    pub plan: String,
    /// Explicit confirmation. Nothing happens without it.
    pub accept: bool,
    /// As on the plan.
    #[serde(default)]
    pub keep_config: bool,
    /// Likewise.
    #[serde(default)]
    pub keep_data: bool,
}

/// What `uninstall.apply` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ApplyOut {
    /// What was done, step by step.
    pub steps: Vec<Step>,
    /// How many things were removed.
    pub removed: usize,
    /// How many were commented out because they had been edited.
    pub preserved: usize,
}

omt_catalog::capability! {
    /// Carry out an uninstall that was planned.
    pub struct UninstallApply;
    input  = ApplyIn,
    output = ApplyOut,
    decl = omt_catalog::Decl {
        name: "uninstall.apply",
        group: "uninstall",
        verb: "apply",
        title: "Uninstall omt",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Command,
        role: omt_types::Role::Admin,
        effects: omt_catalog::Effects::WRITES_FS.union(omt_catalog::Effects::DESTRUCTIVE),
        intent: Some(omt_catalog::Intent::Cas),
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Refuses unless it is given the hash of a plan that still matches the disk. A confirmation against a plan that has since changed is a confirmation of something the user never saw.",
    },
}

impl CapabilityHandler<UninstallApply> for UninstallHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: ApplyIn,
    ) -> Result<ApplyOut, CapabilityError> {
        if !input.accept {
            return Err(CapabilityError::invalid_input(
                "uninstall does nothing without an explicit accept",
            ));
        }
        let manifest = self.manifest()?;
        let steps = plan(&manifest, input.keep_config, input.keep_data, &self.0.root);
        // Re-planned and re-hashed rather than trusting the caller's copy: the
        // disk may have moved since, and a confirmation against a plan that no
        // longer holds is a confirmation of something nobody saw.
        if plan_hash(&steps) != input.plan {
            return Err(CapabilityError::precondition_failed(
                "the machine changed since that plan was made. Run uninstall.plan again and look at it.",
            ));
        }

        let mut removed = 0;
        let mut preserved = 0;
        for step in &steps {
            let path = self.0.root.join(step.path.trim_start_matches('/'));
            match step.action {
                Action::Remove => {
                    let ok = if path.is_dir() {
                        std::fs::remove_dir_all(&path).is_ok()
                    } else if find_block(&std::fs::read_to_string(&path).unwrap_or_default())
                        .is_some()
                    {
                        remove_block(&path).is_ok()
                    } else {
                        std::fs::remove_file(&path).is_ok()
                    };
                    if ok {
                        removed += 1;
                    }
                }
                Action::CommentOut => {
                    if comment_out(&path).is_ok() {
                        preserved += 1;
                    }
                }
                Action::Kept | Action::Absent => {}
            }
        }
        Ok(ApplyOut {
            steps,
            removed,
            preserved,
        })
    }
}

/// Take omt's block out of a file, leaving every other byte alone.
fn remove_block(path: &Path) -> std::io::Result<()> {
    let contents = std::fs::read_to_string(path)?;
    let Some((start, end)) = find_block(&contents) else {
        return Ok(());
    };
    let lines: Vec<&str> = contents.lines().collect();
    let mut kept: Vec<&str> = Vec::new();
    kept.extend_from_slice(&lines[..start]);
    kept.extend_from_slice(&lines[end + 1..]);
    let mut out = kept.join("\n");
    if contents.ends_with('\n') && !out.is_empty() {
        out.push('\n');
    }
    std::fs::write(path, out)
}

/// Comment omt's block out instead of deleting it, and say why in the file.
fn comment_out(path: &Path) -> std::io::Result<()> {
    let contents = std::fs::read_to_string(path)?;
    let Some((start, end)) = find_block(&contents) else {
        return Ok(());
    };
    let lines: Vec<&str> = contents.lines().collect();
    let mut out: Vec<String> = lines[..start].iter().map(|l| (*l).to_owned()).collect();
    out.push("# omt: this block was edited after omt wrote it, so uninstall commented".to_owned());
    out.push(
        "# it out rather than deleting your changes. Remove it when you are ready.".to_owned(),
    );
    for line in &lines[start..=end] {
        if line.trim_start().starts_with('#') {
            out.push((*line).to_owned());
        } else {
            out.push(format!("# {line}"));
        }
    }
    out.extend(lines[end + 1..].iter().map(|l| (*l).to_owned()));
    let mut text = out.join("\n");
    if contents.ends_with('\n') {
        text.push('\n');
    }
    std::fs::write(path, text)
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    const BLOCK: &str = "# >>> omt shell integration >>>\nsource ~/.config/omt/init.sh\n# <<< omt shell integration <<<";

    fn rc(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("rc");
        std::fs::write(&path, body).expect("write");
        path
    }

    fn manifest_of(entries: Vec<Entry>) -> Manifest {
        Manifest { entries }
    }

    #[test]
    fn a_block_is_found_between_its_markers() {
        let text = format!("before\n{BLOCK}\nafter\n");
        let (start, end) = find_block(&text).expect("found");
        assert_eq!(start, 1);
        assert_eq!(end, 3);
    }

    #[test]
    fn an_unchanged_block_is_removed() {
        let dir = tempfile::tempdir().expect("tempdir");
        rc(dir.path(), &format!("before\n{BLOCK}\nafter\n"));
        let steps = plan(
            &manifest_of(vec![Entry {
                path: "rc".to_owned(),
                wrote: Wrote::Block,
                hash: hash(BLOCK),
                config: false,
                data: false,
            }]),
            false,
            false,
            dir.path(),
        );
        assert_eq!(steps[0].action, Action::Remove);
    }

    #[test]
    fn an_edited_block_is_commented_out_rather_than_deleted() {
        // The sin this whole check exists to prevent.
        let dir = tempfile::tempdir().expect("tempdir");
        let edited = BLOCK.replace("init.sh", "my-own-init.sh");
        rc(dir.path(), &format!("{edited}\n"));
        let steps = plan(
            &manifest_of(vec![Entry {
                path: "rc".to_owned(),
                wrote: Wrote::Block,
                hash: hash(BLOCK),
                config: false,
                data: false,
            }]),
            false,
            false,
            dir.path(),
        );
        assert_eq!(steps[0].action, Action::CommentOut);
        assert!(steps[0].reason.contains("edited"));
    }

    #[test]
    fn removing_a_block_leaves_every_other_line_alone() {
        // The neighbouring lines are somebody's shell configuration, and this
        // is the only part of an uninstall that touches a file omt does not own.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = rc(
            dir.path(),
            &format!("export EDITOR=vim\n{BLOCK}\nalias ll='ls -la'\n"),
        );
        remove_block(&path).expect("remove");
        let after = std::fs::read_to_string(&path).expect("read");
        assert_eq!(after, "export EDITOR=vim\nalias ll='ls -la'\n");
    }

    #[test]
    fn commenting_out_keeps_the_users_edit_readable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let edited = BLOCK.replace("init.sh", "my-own-init.sh");
        let path = rc(dir.path(), &format!("{edited}\n"));
        comment_out(&path).expect("comment");
        let after = std::fs::read_to_string(&path).expect("read");
        assert!(after.contains("my-own-init.sh"), "the edit survives");
        assert!(
            after.contains("# source ~/.config/omt/my-own-init.sh"),
            "and is inert: {after}"
        );
        assert!(after.contains("commented"), "and says why");
    }

    #[test]
    fn nothing_outside_the_manifest_is_ever_touched() {
        // Not "usually not touched". An uninstall that reasons about what it
        // probably wrote eventually removes somebody else's configuration.
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("someone-elses"), "keep me").expect("write");
        let steps = plan(&manifest_of(vec![]), false, false, dir.path());
        assert!(steps.is_empty());
        assert!(dir.path().join("someone-elses").exists());
    }

    #[test]
    fn keep_config_and_keep_data_hold_their_entries_back() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(dir.path().join("config")).expect("mkdir");
        std::fs::create_dir(dir.path().join("state")).expect("mkdir");
        let m = manifest_of(vec![
            Entry {
                path: "config".to_owned(),
                wrote: Wrote::Directory,
                hash: String::new(),
                config: true,
                data: false,
            },
            Entry {
                path: "state".to_owned(),
                wrote: Wrote::Directory,
                hash: String::new(),
                config: false,
                data: true,
            },
        ]);
        let steps = plan(&m, true, false, dir.path());
        assert_eq!(steps[0].action, Action::Kept);
        assert_eq!(steps[1].action, Action::Remove);
        let steps = plan(&m, false, true, dir.path());
        assert_eq!(steps[0].action, Action::Remove);
        assert_eq!(steps[1].action, Action::Kept);
    }

    #[test]
    fn something_already_gone_is_reported_not_failed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let steps = plan(
            &manifest_of(vec![Entry {
                path: "never-existed".to_owned(),
                wrote: Wrote::File,
                hash: String::new(),
                config: false,
                data: false,
            }]),
            false,
            false,
            dir.path(),
        );
        assert_eq!(steps[0].action, Action::Absent);
    }

    #[test]
    fn the_plan_hash_changes_when_the_plan_does() {
        // What makes accepting a plan mean anything.
        let a = vec![Step {
            path: "rc".to_owned(),
            action: Action::Remove,
            reason: String::new(),
        }];
        let b = vec![Step {
            path: "rc".to_owned(),
            action: Action::CommentOut,
            reason: String::new(),
        }];
        assert_ne!(plan_hash(&a), plan_hash(&b));
    }

    fn ctx() -> omt_catalog::CallContext {
        omt_catalog::CallContext {
            actor: omt_types::Actor::Local,
            role: omt_types::Role::Admin,
            request: omt_catalog::RequestId {
                device: omt_types::DeviceId::new(),
                n: 1,
            },
            intent: Some(omt_types::IntentId::new()),
        }
    }

    fn handler(dir: &Path, manifest: &Manifest) -> UninstallHandler {
        let path = dir.join("manifest.json");
        std::fs::write(&path, serde_json::to_string(manifest).expect("json")).expect("write");
        UninstallHandler(std::sync::Arc::new(Roots {
            manifest: path,
            root: dir.to_path_buf(),
        }))
    }

    #[test]
    fn apply_does_nothing_without_an_explicit_accept() {
        let dir = tempfile::tempdir().expect("tempdir");
        rc(dir.path(), &format!("{BLOCK}\n"));
        let h = handler(
            dir.path(),
            &manifest_of(vec![Entry {
                path: "rc".to_owned(),
                wrote: Wrote::Block,
                hash: hash(BLOCK),
                config: false,
                data: false,
            }]),
        );
        let err = CapabilityHandler::<UninstallApply>::call(
            &h,
            &ctx(),
            ApplyIn {
                plan: String::new(),
                accept: false,
                keep_config: false,
                keep_data: false,
            },
        )
        .expect_err("must refuse");
        assert!(format!("{err:?}").contains("accept"), "{err:?}");
        assert!(dir.path().join("rc").exists(), "and nothing was touched");
    }

    #[test]
    fn apply_refuses_a_plan_the_disk_no_longer_matches() {
        // A confirmation against a plan that has since changed is a
        // confirmation of something the user never saw.
        let dir = tempfile::tempdir().expect("tempdir");
        rc(dir.path(), &format!("{BLOCK}\n"));
        let h = handler(
            dir.path(),
            &manifest_of(vec![Entry {
                path: "rc".to_owned(),
                wrote: Wrote::Block,
                hash: hash(BLOCK),
                config: false,
                data: false,
            }]),
        );
        let err = CapabilityHandler::<UninstallApply>::call(
            &h,
            &ctx(),
            ApplyIn {
                plan: "not-the-plan".to_owned(),
                accept: true,
                keep_config: false,
                keep_data: false,
            },
        )
        .expect_err("must refuse");
        assert!(
            format!("{err:?}").to_lowercase().contains("changed"),
            "{err:?}"
        );
        assert!(dir.path().join("rc").exists());
    }

    #[test]
    fn a_planned_uninstall_applies_and_reports_what_it_did() {
        let dir = tempfile::tempdir().expect("tempdir");
        rc(dir.path(), &format!("keep me\n{BLOCK}\n"));
        std::fs::create_dir(dir.path().join("state")).expect("mkdir");
        let h = handler(
            dir.path(),
            &manifest_of(vec![
                Entry {
                    path: "rc".to_owned(),
                    wrote: Wrote::Block,
                    hash: hash(BLOCK),
                    config: false,
                    data: false,
                },
                Entry {
                    path: "state".to_owned(),
                    wrote: Wrote::Directory,
                    hash: String::new(),
                    config: false,
                    data: false,
                },
            ]),
        );
        let c = ctx();
        let planned = CapabilityHandler::<UninstallPlan>::call(
            &h,
            &c,
            PlanIn {
                keep_config: false,
                keep_data: false,
            },
        )
        .expect("plan");
        let out = CapabilityHandler::<UninstallApply>::call(
            &h,
            &c,
            ApplyIn {
                plan: planned.plan,
                accept: true,
                keep_config: false,
                keep_data: false,
            },
        )
        .expect("apply");
        assert_eq!(out.removed, 2);
        assert!(!dir.path().join("state").exists());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("rc")).expect("read"),
            "keep me\n",
            "the block went and the line above it stayed"
        );
    }

    #[test]
    fn a_missing_manifest_is_an_empty_one_rather_than_an_error() {
        // An omt that never wrote anything outward has nothing to take back.
        // Failing here would make a clean install the one case that cannot be
        // uninstalled.
        let dir = tempfile::tempdir().expect("tempdir");
        let h = UninstallHandler(std::sync::Arc::new(Roots {
            manifest: dir.path().join("absent.json"),
            root: dir.path().to_path_buf(),
        }));
        let out = CapabilityHandler::<UninstallPlan>::call(
            &h,
            &ctx(),
            PlanIn {
                keep_config: false,
                keep_data: false,
            },
        )
        .expect("plan");
        assert!(out.steps.is_empty());
        assert!(out.binary.contains("package manager"));
    }

    #[test]
    fn hashing_notices_an_edit() {
        assert_ne!(hash(BLOCK), hash(&BLOCK.replace("init", "other")));
    }

    #[test]
    fn a_block_with_no_closing_marker_is_not_a_block() {
        // Half a block would otherwise have omt delete from its start to the
        // end of somebody's rc file.
        assert_eq!(find_block("# >>> omt shell integration >>>\nx\n"), None);
    }
}
