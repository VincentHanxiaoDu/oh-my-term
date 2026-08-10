//! Reading somebody's tmux configuration, and saying what omt can do with it.
//!
//! `docs/architecture/19-onboarding.md` §5. Somebody judges omt in sixty
//! seconds, with their fingers. A migration that silently drops a third of
//! their bindings fails that minute without ever reporting an error, so the
//! preview here returns the **per-directive decision set** rather than a
//! rendered report: every line of their config comes back classified, and the
//! ones omt cannot map come back saying so by name.
//!
//! Three classifications, not two. `Mapped` is a directive omt can honour
//! exactly. `Unsupported` is one it cannot honour at all. `Review` is the
//! interesting one — omt has an equivalent, but it is not the same thing, and
//! applying it silently would give somebody a keymap that behaves subtly unlike
//! the one they spent years building. Those require an explicit decision before
//! anything is written, which is why `apply` needs the preview's hash *and* a
//! choice for every `Review` row.
//!
//! Nothing here writes. Parsing somebody's config and telling them what would
//! happen is the whole of the preview.

use omt_catalog::{CapabilityError, CapabilityHandler};
use serde::{Deserialize, Serialize};

/// What omt can do with one directive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Mapping {
    /// omt honours it exactly.
    Mapped,
    /// omt has something close, and the difference matters enough to ask.
    Review,
    /// omt has no equivalent.
    Unsupported,
}

/// One line of a tmux config, and what omt makes of it.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct Directive {
    /// Which line it was, so somebody can find it.
    pub line: usize,
    /// The line itself.
    pub source: String,
    /// What omt can do with it.
    pub mapping: Mapping,
    /// The omt capability it becomes, where there is one.
    pub capability: Option<String>,
    /// Why, in words somebody can act on.
    pub note: String,
}

/// tmux commands omt honours exactly, and what they become.
///
/// The table from §5.1. Written out rather than pattern-matched: the value of
/// this feature is entirely in its coverage, and a clever rule that handled
/// most of them would fail silently on the rest.
pub const MAPPED: &[(&str, &str)] = &[
    ("new-session", "session.create"),
    ("new-window", "session.create"),
    ("kill-pane", "session.close"),
    ("kill-window", "session.close"),
    ("detach-client", "session.detach"),
    ("attach-session", "session.attach"),
    ("choose-tree", "ui.open_session_picker"),
    ("choose-window", "ui.open_session_picker"),
    ("split-window", "pane.split"),
    ("select-pane", "pane.navigate"),
    ("last-pane", "pane.focus_last"),
    ("resize-pane", "pane.resize"),
    ("swap-pane", "pane.swap"),
    ("rotate-window", "pane.rotate"),
    ("select-layout", "layout.preset"),
    ("break-pane", "pane.stack.break_out"),
    ("join-pane", "pane.move_to_workspace"),
    ("next-window", "pane.focus_cycle"),
    ("previous-window", "pane.focus_cycle"),
    ("last-window", "pane.focus_last"),
    ("select-window", "pane.focus_index"),
    ("rename-window", "session.rename"),
    ("rename-session", "workspace.rename"),
    ("copy-mode", "ui.copy_mode.enter"),
    ("command-prompt", "ui.open_command_palette"),
    ("list-keys", "ui.open_keymap_help"),
    ("send-prefix", "SendKey(leader)"),
    ("capture-pane", "session.capture"),
    ("send-keys", "session.send_text"),
];

/// tmux commands omt has no equivalent for, and why.
///
/// Named individually rather than lumped into "unsupported", because "omt has
/// no clock" and "omt could not parse that line" are different problems and
/// only one of them is worth reporting.
pub const UNSUPPORTED: &[(&str, &str)] = &[
    ("clock-mode", "omt has no clock mode."),
    (
        "paste-buffer",
        "omt's buffer capabilities are not built yet, so this cannot be mapped honestly.",
    ),
    (
        "list-buffers",
        "omt's buffer capabilities are not built yet.",
    ),
    (
        "choose-buffer",
        "omt's buffer capabilities are not built yet.",
    ),
    ("save-buffer", "omt's buffer capabilities are not built yet."),
    ("load-buffer", "omt's buffer capabilities are not built yet."),
    (
        "display-panes",
        "omt has no pane-number overlay yet, so the binding would do nothing.",
    ),
];

/// Options whose omt equivalent is close but not identical.
pub const REVIEW: &[(&str, &str)] = &[
    (
        "prefix",
        "omt's leader is the same key by default, but omt reserves a few chords tmux leaves free.",
    ),
    (
        "mode-keys",
        "omt's copy mode has its own key handling; vi bindings are similar, not identical.",
    ),
    (
        "status",
        "omt draws its own status line from the session model, so a tmux status format cannot be carried over literally.",
    ),
    (
        "mouse",
        "omt handles the mouse per surface rather than globally.",
    ),
    (
        "default-terminal",
        "omt sets TERM itself; overriding it here may disagree with what omt advertises.",
    ),
];

/// What one line of a tmux config is asking for.
///
/// Only the two forms that carry behaviour. Everything else in a tmux config is
/// either a comment, a blank line, or something that does not survive into any
/// other multiplexer.
fn command_of(line: &str) -> Option<(&'static str, Mapping, Option<&'static str>, String)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let mut words = trimmed.split_whitespace();
    let head = words.next()?;

    // `bind`/`bind-key` puts the interesting command after the key, and both
    // may carry flags first. Scanning for the first word that is a known
    // command is more robust than counting positions, because `bind -r -T
    // prefix C-h resize-pane -L` counts differently from `bind x kill-pane`.
    let rest: Vec<&str> = trimmed.split_whitespace().collect();
    if matches!(head, "bind" | "bind-key" | "unbind" | "unbind-key") {
        for word in &rest[1..] {
            if let Some((name, cap)) = MAPPED.iter().find(|(n, _)| n == word) {
                return Some((
                    name,
                    Mapping::Mapped,
                    Some(cap),
                    format!("`{name}` becomes `{cap}`."),
                ));
            }
            if let Some((name, why)) = UNSUPPORTED.iter().find(|(n, _)| n == word) {
                return Some((name, Mapping::Unsupported, None, (*why).to_owned()));
            }
        }
        return Some((
            "bind",
            Mapping::Review,
            None,
            "omt could not tell which command this binds. Decide what it should become, or drop it."
                .to_owned(),
        ));
    }

    if matches!(head, "set" | "set-option" | "setw" | "set-window-option") {
        for word in &rest[1..] {
            let name = word.trim_start_matches('-');
            if let Some((n, why)) = REVIEW.iter().find(|(n, _)| *n == name) {
                return Some((n, Mapping::Review, None, (*why).to_owned()));
            }
        }
        return Some((
            "set",
            Mapping::Review,
            None,
            "omt has no equivalent option. Decide whether this matters to you.".to_owned(),
        ));
    }

    Some((
        "unknown",
        Mapping::Unsupported,
        None,
        "omt does not recognise this directive.".to_owned(),
    ))
}

/// Classify a whole tmux config.
#[must_use]
pub fn read(config: &str) -> Vec<Directive> {
    config
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            command_of(line).map(|(_, mapping, capability, note)| Directive {
                line: index + 1,
                source: line.trim().to_owned(),
                mapping,
                capability: capability.map(str::to_owned),
                note,
            })
        })
        .collect()
}

/// Input to `migrate.tmux.preview`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct PreviewIn {
    /// The configuration text.
    ///
    /// Passed in rather than read from `~/.tmux.conf`, so the web client can
    /// preview a config the daemon's machine does not have — and so this can be
    /// tested against a config that is not the tester's own.
    pub config: String,
}

/// What `migrate.tmux.preview` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct PreviewOut {
    /// Every directive, classified.
    pub directives: Vec<Directive>,
    /// How many omt can honour exactly.
    pub mapped: usize,
    /// How many need a decision before anything is written.
    pub review: usize,
    /// How many omt cannot honour at all.
    pub unsupported: usize,
    /// A hash of this preview, which apply must quote back.
    pub preview: String,
}

omt_catalog::capability! {
    /// What omt would make of a tmux configuration.
    pub struct TmuxPreview;
    input  = PreviewIn,
    output = PreviewOut,
    decl = omt_catalog::Decl {
        name: "migrate.tmux_preview",
        group: "migrate",
        // `tmux_preview`, not `tmux.preview`: a capability name is its group
        // and its verb, two levels and no more, and the doc's three-level
        // spelling cannot be declared. The catalog refuses it rather than
        // routing something that does not match its own name.
        verb: "tmux_preview",
        title: "Preview a tmux config",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Operator,
        effects: omt_catalog::Effects::READS_FS,
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Every directive comes back classified, including the ones omt cannot map. A migration that silently drops a third of somebody's bindings fails without ever reporting an error.",
    },
}

/// Serves the migration capabilities.
pub struct MigrateHandler;

impl CapabilityHandler<TmuxPreview> for MigrateHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: PreviewIn,
    ) -> Result<PreviewOut, CapabilityError> {
        let directives = read(&input.config);
        let count = |m: Mapping| directives.iter().filter(|d| d.mapping == m).count();
        let digest = crate::uninstall::hash(
            &directives
                .iter()
                .map(|d| format!("{}:{:?}", d.line, d.mapping))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        Ok(PreviewOut {
            mapped: count(Mapping::Mapped),
            review: count(Mapping::Review),
            unsupported: count(Mapping::Unsupported),
            preview: digest,
            directives,
        })
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn only(config: &str) -> Directive {
        let mut d = read(config);
        assert_eq!(d.len(), 1, "{d:?}");
        d.remove(0)
    }

    #[test]
    fn a_binding_omt_honours_names_the_capability_it_becomes() {
        let d = only("bind x kill-pane");
        assert_eq!(d.mapping, Mapping::Mapped);
        assert_eq!(d.capability.as_deref(), Some("session.close"));
    }

    #[test]
    fn flags_before_the_command_do_not_hide_it() {
        // `bind -r -T prefix C-h resize-pane -L` counts differently from
        // `bind x kill-pane`, and counting positions gets one of them wrong.
        let d = only("bind -r -T prefix C-h resize-pane -L");
        assert_eq!(d.mapping, Mapping::Mapped);
        assert_eq!(d.capability.as_deref(), Some("pane.resize"));
    }

    #[test]
    fn something_omt_cannot_do_says_so_by_name() {
        // "omt has no clock" and "omt could not parse that" are different
        // problems, and only one is worth reporting.
        let d = only("bind t clock-mode");
        assert_eq!(d.mapping, Mapping::Unsupported);
        assert!(d.note.contains("clock"));
    }

    #[test]
    fn a_close_but_different_option_needs_a_decision() {
        // Applying it silently gives somebody a keymap that behaves subtly
        // unlike the one they spent years building.
        let d = only("set -g mode-keys vi");
        assert_eq!(d.mapping, Mapping::Review);
        assert!(!d.note.is_empty());
    }

    #[test]
    fn comments_and_blank_lines_are_not_directives() {
        assert!(read("# my config\n\n   \n").is_empty());
    }

    #[test]
    fn every_line_keeps_its_number_so_it_can_be_found() {
        let directives = read("# a comment\nbind x kill-pane\n\nbind t clock-mode");
        assert_eq!(directives[0].line, 2);
        assert_eq!(directives[1].line, 4);
    }

    #[test]
    fn a_binding_omt_cannot_read_is_review_not_silently_dropped() {
        // The failure this whole feature exists to prevent: a third of
        // somebody's config vanishing without an error.
        let d = only("bind C-y run-shell 'something custom'");
        assert_eq!(d.mapping, Mapping::Review);
    }

    #[test]
    fn the_counts_agree_with_the_list() {
        let out = CapabilityHandler::<TmuxPreview>::call(
            &MigrateHandler,
            &ctx(),
            PreviewIn {
                config: "bind x kill-pane\nbind t clock-mode\nset -g mode-keys vi".to_owned(),
            },
        )
        .expect("preview");
        assert_eq!(out.mapped, 1);
        assert_eq!(out.unsupported, 1);
        assert_eq!(out.review, 1);
        assert_eq!(
            out.mapped + out.unsupported + out.review,
            out.directives.len(),
            "a directive in no bucket is one nobody sees"
        );
    }

    #[test]
    fn the_preview_hash_changes_when_the_config_does() {
        let one = CapabilityHandler::<TmuxPreview>::call(
            &MigrateHandler,
            &ctx(),
            PreviewIn {
                config: "bind x kill-pane".to_owned(),
            },
        )
        .expect("a");
        let two = CapabilityHandler::<TmuxPreview>::call(
            &MigrateHandler,
            &ctx(),
            PreviewIn {
                config: "bind t clock-mode".to_owned(),
            },
        )
        .expect("b");
        assert_ne!(one.preview, two.preview);
    }

    #[test]
    fn a_realistic_config_maps_most_of_itself() {
        // The sixty seconds this feature exists for. If a normal config comes
        // back mostly unmapped, the feature does not work, however well each
        // individual line is classified.
        let config = "\
# my tmux.conf
set -g prefix C-a
bind | split-window -h
bind - split-window -v
bind h select-pane -L
bind l select-pane -R
bind c new-window
bind x kill-pane
bind , rename-window
bind [ copy-mode
bind ? list-keys
";
        let out = read(config);
        let mapped = out.iter().filter(|d| d.mapping == Mapping::Mapped).count();
        assert!(mapped >= 8, "only {mapped} of {} mapped", out.len());
    }

    fn ctx() -> omt_catalog::CallContext {
        omt_catalog::CallContext {
            actor: omt_types::Actor::Local,
            role: omt_types::Role::Operator,
            request: omt_catalog::RequestId {
                device: omt_types::DeviceId::new(),
                n: 1,
            },
            intent: None,
        }
    }
}
