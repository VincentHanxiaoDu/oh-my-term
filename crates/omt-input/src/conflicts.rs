//! Knowing which keys the program in the pane already uses.
//!
//! `docs/architecture/16-input-and-keymap.md` §5, which the doc calls the part
//! that is actually novel — and it is the reason omt can have bindings at all
//! without stealing input from the program underneath.
//!
//! Every multiplexer's real failure is the same: it binds a chord, somebody
//! runs an editor that already used that chord, and the editor stops working in
//! a way that looks like the editor's fault. The user's remedy is to stop using
//! the multiplexer.
//!
//! omt ships a **data** registry of known programs' keymaps rather than a match
//! arm, so a new agent CLI is a file rather than a release. What this module
//! does with that registry is answer one question honestly: if omt binds this
//! chord, what breaks, and how badly.
//!
//! Severity is the load-bearing part. "Conflicts with something" is not
//! actionable — every chord conflicts with something somewhere. What a person
//! needs to know is whether they are about to lose the key that opens their
//! editor's command palette or the one that toggles a status bar nobody uses.

use serde::{Deserialize, Serialize};

use crate::key::Chord;

/// How much it matters that a chord is taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// The program is unusable without it.
    Critical,
    /// A frequently used thing becomes unreachable.
    Important,
    /// Something obscure or duplicated elsewhere.
    Minor,
}

/// One key an inner program uses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InnerKey {
    /// The chord, as written.
    pub chord: String,
    /// What it does there, in that program's own words.
    pub does: String,
    /// How much losing it matters.
    pub severity: Severity,
}

/// A known program's keymap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InnerKeymap {
    /// Its id.
    pub id: String,
    /// What to call it in a message.
    pub display: String,
    /// The version this file was checked against.
    ///
    /// Recorded because a keymap is a claim about somebody else's software at
    /// a point in time. Without it, a stale file is indistinguishable from a
    /// current one, and omt would be confidently warning about a binding that
    /// moved two releases ago.
    pub verified_against: String,
    /// The keys it uses.
    pub keys: Vec<InnerKey>,
}

/// A conflict between an omt binding and a program underneath.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Conflict {
    /// Which program.
    pub program: String,
    /// The chord.
    pub chord: String,
    /// What it does there.
    pub does: String,
    /// How much it matters.
    pub severity: Severity,
    /// What that file was checked against, so a stale warning is visible.
    pub verified_against: String,
}

/// Every registered inner keymap.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    programs: Vec<InnerKeymap>,
}

impl Registry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a program's keymap.
    pub fn insert(&mut self, keymap: InnerKeymap) {
        // Replaced rather than appended, so a user file shadowing a shipped one
        // does not produce the same conflict reported twice under one name.
        self.programs.retain(|p| p.id != keymap.id);
        self.programs.push(keymap);
    }

    /// How many programs are known.
    #[must_use]
    pub fn len(&self) -> usize {
        self.programs.len()
    }

    /// Whether anything is known.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.programs.is_empty()
    }

    /// What binding this chord would break, worst first.
    ///
    /// Compared on the canonical form rather than the written one, so
    /// `Ctrl-O`, `ctrl-o` and `C-o` are one chord. Comparing written strings
    /// would silently miss most conflicts and report a clean bill of health,
    /// which is worse than not checking.
    #[must_use]
    pub fn conflicts(&self, chord: &Chord) -> Vec<Conflict> {
        let wanted = chord.canonical();
        let mut out: Vec<Conflict> = Vec::new();
        for program in &self.programs {
            for key in &program.keys {
                let Ok(parsed) = Chord::parse(&key.chord) else {
                    // An unparseable chord in a data file is skipped rather
                    // than guessed at. Guessing would attribute a conflict to
                    // a key the program may not use at all.
                    continue;
                };
                if parsed.canonical() == wanted {
                    out.push(Conflict {
                        program: program.display.clone(),
                        chord: parsed.canonical(),
                        does: key.does.clone(),
                        severity: key.severity,
                        verified_against: program.verified_against.clone(),
                    });
                }
            }
        }
        out.sort_by(|a, b| a.severity.cmp(&b.severity).then(a.program.cmp(&b.program)));
        out
    }

    /// The worst thing this chord would break, if anything.
    #[must_use]
    pub fn worst(&self, chord: &Chord) -> Option<Severity> {
        self.conflicts(chord).first().map(|c| c.severity)
    }

    /// Whether omt should refuse to take this chord by default.
    ///
    /// Only `Critical`. Refusing on anything less would leave omt with almost
    /// no bindings, since every chord collides with something somewhere — and
    /// a tool that cannot bind anything has solved the conflict problem by
    /// having no features.
    #[must_use]
    pub fn should_refuse(&self, chord: &Chord) -> bool {
        self.worst(chord) == Some(Severity::Critical)
    }
}

/// Parse a registry from the TOML-ish data files, given their decoded form.
///
/// Taking already-decoded values rather than raw text keeps this crate free of
/// a TOML parser it would otherwise need only here, and it means the same
/// function serves a plugin handing over a structure it built itself.
#[must_use]
pub fn registry_from(keymaps: Vec<InnerKeymap>) -> Registry {
    let mut r = Registry::new();
    for keymap in keymaps {
        r.insert(keymap);
    }
    r
}

/// The keymaps omt ships.
///
/// A starting set, not a claim to completeness. Each entry names the version it
/// was checked against, because a keymap is an assertion about somebody else's
/// software at a point in time — and the honest failure mode for this feature
/// is a stale file, not a missing one.
#[must_use]
pub fn builtin() -> Registry {
    registry_from(vec![
        InnerKeymap {
            id: "claude_code".to_owned(),
            display: "Claude Code".to_owned(),
            verified_against: "2.x".to_owned(),
            keys: vec![
                InnerKey {
                    chord: "ctrl-c".to_owned(),
                    does: "interrupt the current turn".to_owned(),
                    severity: Severity::Critical,
                },
                InnerKey {
                    chord: "ctrl-d".to_owned(),
                    does: "exit".to_owned(),
                    severity: Severity::Critical,
                },
                InnerKey {
                    chord: "ctrl-r".to_owned(),
                    does: "expand the previous output".to_owned(),
                    severity: Severity::Important,
                },
            ],
        },
        InnerKeymap {
            id: "vim".to_owned(),
            display: "vim / neovim".to_owned(),
            verified_against: "9.x / 0.10.x".to_owned(),
            keys: vec![
                InnerKey {
                    chord: "ctrl-w".to_owned(),
                    does: "the window prefix — every split and pane command".to_owned(),
                    severity: Severity::Critical,
                },
                InnerKey {
                    chord: "ctrl-o".to_owned(),
                    does: "jump back in the jump list".to_owned(),
                    severity: Severity::Important,
                },
                InnerKey {
                    chord: "ctrl-v".to_owned(),
                    does: "visual block mode".to_owned(),
                    severity: Severity::Critical,
                },
            ],
        },
        InnerKeymap {
            id: "emacs".to_owned(),
            display: "emacs".to_owned(),
            verified_against: "29.x".to_owned(),
            keys: vec![
                InnerKey {
                    chord: "ctrl-x".to_owned(),
                    does: "the primary prefix — almost every command begins here".to_owned(),
                    severity: Severity::Critical,
                },
                InnerKey {
                    chord: "ctrl-a".to_owned(),
                    does: "move to the beginning of the line".to_owned(),
                    severity: Severity::Important,
                },
            ],
        },
        InnerKeymap {
            id: "lazygit".to_owned(),
            display: "lazygit".to_owned(),
            verified_against: "0.4x".to_owned(),
            keys: vec![InnerKey {
                chord: "ctrl-o".to_owned(),
                does: "copy the commit to the clipboard".to_owned(),
                severity: Severity::Minor,
            }],
        },
    ])
}

/// Which of a keymap's own bindings would break a program underneath.
///
/// The cross-check the two halves of §5 exist for: a registry nobody runs the
/// defaults against is a registry that documents a problem instead of
/// preventing one.
///
/// Global bindings only. Overlay and copy-mode bindings apply when omt owns the
/// screen and no inner program is reading input, so they cannot take anything
/// from anybody — treating them as conflicts would produce warnings nobody can
/// act on and teach people to ignore the real ones.
#[must_use]
pub fn audit(keymap: &crate::keymap::Keymap, registry: &Registry) -> Vec<Conflict> {
    let mut out = Vec::new();
    for (mode, chord, _) in keymap.bindings() {
        if mode != crate::keymap::Mode::Global {
            continue;
        }
        out.extend(registry.conflicts(chord));
    }
    out.sort_by(|a, b| {
        a.severity
            .cmp(&b.severity)
            .then(a.chord.cmp(&b.chord))
            .then(a.program.cmp(&b.program))
    });
    out.dedup();
    out
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn claude() -> InnerKeymap {
        InnerKeymap {
            id: "claude_code".to_owned(),
            display: "Claude Code".to_owned(),
            verified_against: "2.1.x".to_owned(),
            keys: vec![
                InnerKey {
                    chord: "ctrl-o".to_owned(),
                    does: "open the command palette".to_owned(),
                    severity: Severity::Critical,
                },
                InnerKey {
                    chord: "ctrl-t".to_owned(),
                    does: "toggle the todo list".to_owned(),
                    severity: Severity::Minor,
                },
            ],
        }
    }

    fn registry() -> Registry {
        registry_from(vec![claude()])
    }

    fn chord(spec: &str) -> Chord {
        Chord::parse(spec).expect("a chord")
    }

    #[test]
    fn a_chord_the_inner_program_needs_is_reported_with_what_it_does() {
        // "Conflicts with something" is not actionable. What somebody needs to
        // know is whether they are about to lose their editor's palette.
        let found = registry().conflicts(&chord("ctrl-o"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].does, "open the command palette");
        assert_eq!(found[0].severity, Severity::Critical);
    }

    #[test]
    fn spelling_does_not_change_whether_a_conflict_is_found() {
        // Comparing written strings would silently miss most conflicts and
        // report a clean bill of health, which is worse than not checking.
        for spelling in ["ctrl-o", "Ctrl-O", "CTRL-o"] {
            assert_eq!(
                registry().conflicts(&chord(spelling)).len(),
                1,
                "`{spelling}` found nothing"
            );
        }
    }

    #[test]
    fn a_free_chord_conflicts_with_nothing() {
        assert!(registry().conflicts(&chord("ctrl-alt-f9")).is_empty());
        assert_eq!(registry().worst(&chord("ctrl-alt-f9")), None);
    }

    #[test]
    fn only_a_critical_conflict_refuses_the_binding() {
        // Refusing on anything less leaves omt with almost no bindings, since
        // every chord collides with something somewhere — and a tool that
        // cannot bind anything has solved the problem by having no features.
        assert!(registry().should_refuse(&chord("ctrl-o")));
        assert!(!registry().should_refuse(&chord("ctrl-t")));
        assert!(!registry().should_refuse(&chord("ctrl-alt-f9")));
    }

    #[test]
    fn conflicts_come_back_worst_first() {
        let mut r = registry();
        r.insert(InnerKeymap {
            id: "other".to_owned(),
            display: "Other".to_owned(),
            verified_against: "1.0".to_owned(),
            keys: vec![InnerKey {
                chord: "ctrl-o".to_owned(),
                does: "something minor".to_owned(),
                severity: Severity::Minor,
            }],
        });
        let found = r.conflicts(&chord("ctrl-o"));
        assert_eq!(found[0].severity, Severity::Critical);
        assert_eq!(found[1].severity, Severity::Minor);
    }

    #[test]
    fn a_user_file_replaces_a_shipped_one_rather_than_doubling_it() {
        // Otherwise the same conflict is reported twice under one name, and
        // the count somebody sees is wrong.
        let mut r = registry();
        let mut updated = claude();
        updated.verified_against = "3.0.x".to_owned();
        r.insert(updated);
        assert_eq!(r.len(), 1);
        assert_eq!(r.conflicts(&chord("ctrl-o"))[0].verified_against, "3.0.x");
    }

    #[test]
    fn every_conflict_says_what_it_was_checked_against() {
        // A keymap is a claim about somebody else's software at a point in
        // time. Without the version, a stale file is indistinguishable from a
        // current one and omt warns confidently about a binding that moved.
        for conflict in registry().conflicts(&chord("ctrl-o")) {
            assert!(!conflict.verified_against.is_empty());
        }
    }

    #[test]
    fn an_unparseable_chord_in_a_data_file_is_skipped_not_guessed() {
        // Guessing would attribute a conflict to a key the program may not use
        // at all, and the file is user-editable.
        let r = registry_from(vec![InnerKeymap {
            id: "broken".to_owned(),
            display: "Broken".to_owned(),
            verified_against: "1.0".to_owned(),
            keys: vec![InnerKey {
                chord: "not a chord at all".to_owned(),
                does: "who knows".to_owned(),
                severity: Severity::Critical,
            }],
        }]);
        assert!(r.conflicts(&chord("ctrl-o")).is_empty());
        assert_eq!(r.len(), 1, "the program is still known");
    }

    #[test]
    fn an_empty_registry_refuses_nothing() {
        // Knowing about no programs must not mean claiming every chord is
        // free — but it also cannot block everything. Reporting nothing is the
        // honest answer, and §5 pairs it with the version-stamped files above.
        let r = Registry::new();
        assert!(r.is_empty());
        assert!(!r.should_refuse(&chord("ctrl-o")));
        assert!(r.conflicts(&chord("ctrl-o")).is_empty());
    }

    #[test]
    fn the_shipped_registry_knows_the_programs_people_actually_run() {
        let r = builtin();
        assert!(r.len() >= 4);
        // The one that matters most: omt's own default leader must not be a
        // key an editor cannot live without.
        assert!(r.should_refuse(&chord("ctrl-w")), "vim's window prefix");
        assert!(r.should_refuse(&chord("ctrl-x")), "emacs' primary prefix");
    }

    #[test]
    fn every_shipped_entry_names_a_version_and_explains_each_key() {
        // A keymap with no version is a claim nobody can date, and one with no
        // explanation produces a warning somebody cannot act on.
        for chord_spec in ["ctrl-c", "ctrl-w", "ctrl-x", "ctrl-o"] {
            for conflict in builtin().conflicts(&chord(chord_spec)) {
                assert!(!conflict.verified_against.is_empty(), "{chord_spec}");
                assert!(!conflict.does.is_empty(), "{chord_spec}");
            }
        }
    }

    #[test]
    fn omts_own_defaults_take_nothing_critical_from_anybody() {
        // The cross-check both halves of §5 exist for. A registry nobody runs
        // the defaults against documents a problem instead of preventing one,
        // and this is the test that would fail the day somebody adds a
        // convenient default that breaks vim.
        let critical: Vec<Conflict> = audit(&crate::keymap::defaults(), &builtin())
            .into_iter()
            .filter(|c| c.severity == Severity::Critical)
            .collect();
        assert!(
            critical.is_empty(),
            "omt's defaults would break: {critical:?}"
        );
    }

    #[test]
    fn the_audit_looks_at_the_bindings_that_can_actually_take_something() {
        // Overlay and copy-mode bindings apply when omt owns the screen and no
        // inner program is reading input. Reporting those would produce
        // warnings nobody can act on, and teach people to ignore the real ones.
        let mut km = crate::keymap::Keymap::new();
        let _ = km.bind(
            crate::keymap::Mode::Overlay,
            chord("ctrl-w"),
            crate::keymap::Action::Capability {
                name: "ui.close".to_owned(),
            },
        );
        assert!(audit(&km, &builtin()).is_empty());
    }

    #[test]
    fn the_audit_does_find_a_conflict_when_there_is_one() {
        // Otherwise the test above passes because the audit sees nothing at
        // all, which is the same shape as the parity gate that was checking an
        // empty list.
        let mut km = crate::keymap::Keymap::new();
        let _ = km.bind(
            crate::keymap::Mode::Global,
            chord("ctrl-w"),
            crate::keymap::Action::Capability {
                name: "pane.close".to_owned(),
            },
        );
        let found = audit(&km, &builtin());
        assert!(!found.is_empty());
        assert_eq!(found[0].severity, Severity::Critical);
    }

    #[test]
    fn severity_orders_worst_first_so_comparisons_read_as_they_mean() {
        assert!(Severity::Critical < Severity::Important);
        assert!(Severity::Important < Severity::Minor);
    }
}
