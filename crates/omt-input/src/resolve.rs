//! Compiling binding files into a map, and matching input against it.
//!
//! `docs/architecture/16-input-and-keymap.md` §2.3 owns the order and this
//! module implements it: pending chord, modal context, specificity, layer,
//! passthrough. Rule 5 is the invariant the whole crate rests on — a key that
//! matches nothing reaches the program in the pane, byte for byte — so it is
//! the default here rather than a fallback branch at the end.
//!
//! Compilation happens once per config load and the result is immutable. That
//! is not an optimisation: a map that could change under a pending chord would
//! make the second key of a sequence mean something the first key did not
//! promise.

use crate::bindings::{
    Binding, BindingAction, BindingFile, BindingProblem, Source, Step, codes, predicate_of, steps,
};
use crate::context::{ContextSet, Predicate};
use crate::key::Chord;
use crate::keymap::RESERVED_GLOBAL;

/// What this machine and this terminal can offer a binding.
///
/// Passed in rather than read here: a gate answered by inspecting the process
/// would make the compiled map depend on where it was compiled, and the same
/// file has to compile the same way when a test checks it.
#[derive(Debug, Clone, Default)]
pub struct Gates {
    /// The platform name a binding's `platform` list is matched against.
    pub platform: String,
    /// The terminal capabilities a binding's `requires` is matched against.
    pub terminal: Vec<String>,
    /// Whether the active keymap is modal, which is what `modes` needs.
    pub modal: bool,
}

/// One installed binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compiled {
    /// The trigger as written, for anything that reports back to a human.
    pub trigger: String,
    /// The steps it matches, `<leader>` already expanded.
    pub steps: Vec<Step>,
    /// When it applies.
    pub when: Predicate,
    /// What it does.
    pub action: BindingAction,
    /// How many context terms `when` names. 16 §2.3 rule 3.
    pub specificity: u16,
    /// Which layer it came from. 16 §2.3 rule 4.
    pub source: Source,
}

/// The outcome of matching input against the map.
///
/// Named `ChordResolution` rather than `Resolution` because the bare name
/// belongs to semantic open's target resolution, and the two are unrelated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChordResolution<'a> {
    /// Call this binding. The key is consumed.
    Dispatch(&'a Compiled),
    /// A prefix matched: hold the input and show the pending-chord hint.
    Pending,
    /// Nothing matched. The original bytes go to the program, unmodified.
    Passthrough,
}

/// Every installed binding, in the order they are considered.
#[derive(Debug, Clone, Default)]
pub struct Resolver {
    compiled: Vec<Compiled>,
    pending: Vec<Chord>,
}

/// Compile binding files into a resolver.
///
/// Files are given in layer order, lowest first. Everything that could not be
/// installed is returned rather than dropped: a binding that vanishes silently
/// is the defect this whole module was written to end.
#[must_use]
pub fn compile(
    files: &[(Source, BindingFile)],
    gates: &Gates,
    capabilities: &[String],
) -> (Resolver, Vec<BindingProblem>) {
    let mut compiled = Vec::new();
    let mut problems = Vec::new();

    for (source, file) in files {
        for binding in &file.bindings {
            match compile_one(binding, *source, &file.leader, gates, capabilities) {
                Ok(one) => compiled.push(one),
                Err(problem) => problems.push(problem),
            }
        }
    }

    // Specificity first, then layer: 16 §2.3 rules 3 and 4, in that order,
    // decided once here rather than at every keypress.
    compiled.sort_by(|a, b| {
        b.specificity
            .cmp(&a.specificity)
            .then(b.source.cmp(&a.source))
    });

    (
        Resolver {
            compiled,
            pending: Vec::new(),
        },
        problems,
    )
}

fn compile_one(
    binding: &Binding,
    source: Source,
    leader: &str,
    gates: &Gates,
    capabilities: &[String],
) -> Result<Compiled, BindingProblem> {
    let steps = steps(&binding.trigger, leader).map_err(|err| BindingProblem {
        code: codes::BAD_TRIGGER,
        trigger: binding.trigger.clone(),
        message: err.to_string(),
        help: "Write it like `ctrl-shift-p`, `<leader> c`, or `shift-mouse1`.".to_owned(),
    })?;

    let when = predicate_of(binding).map_err(|err| BindingProblem {
        code: codes::BAD_ACTION,
        trigger: binding.trigger.clone(),
        message: err.to_string(),
        help: "The context names are in `omt keys explain`; a `when` may use `&&`, `||`, `!` and brackets."
            .to_owned(),
    })?;

    if let BindingAction::Capability { name, .. } = &binding.action
        && !capabilities.is_empty()
        && !capabilities.iter().any(|c| c == name)
    {
        return Err(BindingProblem {
            code: codes::BAD_ACTION,
            trigger: binding.trigger.clone(),
            message: format!("`{name}` is not a capability this instance has"),
            // Suggested rather than merely refused: the usual cause is a near
            // miss and the usual fix is one character. Further than two edits
            // gets no guess, because a wrong suggestion is worse than none —
            // somebody takes it.
            help: nearest(name, capabilities).map_or_else(
                || "Check `omt keys list` for what can be bound.".to_owned(),
                |near| format!("Did you mean `{near}`?"),
            ),
        });
    }

    if !binding.modes.is_empty() && !gates.modal {
        return Err(BindingProblem {
            code: codes::MODES_WITHOUT_MODAL,
            trigger: binding.trigger.clone(),
            message: "`modes` needs a modal keymap".to_owned(),
            help: "Set `keymap = \"vim\"` or `\"emacs\"`, or drop `modes`.".to_owned(),
        });
    }

    if !binding.platform.is_empty() && !binding.platform.contains(&gates.platform) {
        return Err(BindingProblem {
            code: codes::GATE_UNMET,
            trigger: binding.trigger.clone(),
            message: format!(
                "this binding is for {} and this is {}",
                binding.platform.join(", "),
                gates.platform
            ),
            help: "It is not installed here. Remove `platform` to bind it everywhere.".to_owned(),
        });
    }

    if let Some(required) = &binding.requires
        && !gates.terminal.iter().any(|c| c == required)
    {
        return Err(BindingProblem {
            code: codes::GATE_UNMET,
            trigger: binding.trigger.clone(),
            message: format!("this terminal does not offer `{required}`"),
            help: "It is not installed here. A terminal that negotiates it will take it."
                .to_owned(),
        });
    }

    // A reserved chord may only be taken deliberately, and only in a scoped
    // binding or with `force`. A terminal where Ctrl+C cannot interrupt is one
    // nobody can rescue, and the user finds out during the emergency.
    if binding.when.is_none()
        && !binding.force
        && binding.action != BindingAction::Unbind
        && steps.len() == 1
        && let Some(Step::Key(chord)) = steps.first()
        && let Some((_, reason)) = RESERVED_GLOBAL
            .iter()
            .find(|(spelling, _)| *spelling == chord.canonical())
    {
        return Err(BindingProblem {
            code: codes::RESERVED,
            trigger: binding.trigger.clone(),
            message: format!(
                "omt refuses to bind `{}` globally: {reason}",
                chord.canonical()
            ),
            help: "Scope it with `when`, or set `force = true` if you mean it.".to_owned(),
        });
    }

    Ok(Compiled {
        trigger: binding.trigger.clone(),
        specificity: when.terms(),
        when,
        action: binding.action.clone(),
        steps,
        source,
    })
}

impl Resolver {
    /// Every installed binding, most specific first.
    #[must_use]
    pub fn bindings(&self) -> &[Compiled] {
        &self.compiled
    }

    /// Whether a chord sequence is part-way through.
    #[must_use]
    pub fn is_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Forget any chord in flight.
    ///
    /// §3.4's escape hatch: a pending chord that could never be finished would
    /// leave the keyboard feeling stuck, which is worse than doing nothing.
    pub fn cancel(&mut self) {
        self.pending.clear();
    }

    /// Match one key press.
    ///
    /// Rule 1 first: while a chord is in flight only its continuations are
    /// considered, because the alternative is a second key meaning something
    /// the first did not promise.
    pub fn press(&mut self, chord: &Chord, contexts: ContextSet) -> ChordResolution<'_> {
        let mut sequence = self.pending.clone();
        sequence.push(chord.clone());

        let matched = self
            .compiled
            .iter()
            .position(|b| applies(b, contexts) && key_steps(b).is_some_and(|s| s == sequence));
        if let Some(index) = matched {
            self.pending.clear();
            // `none` stops here rather than revealing whatever the layer below
            // had bound. Falling through would make removing a default reveal
            // a binding the user has never seen, which is not what "unbind"
            // means to the person writing it.
            if self.compiled[index].action == BindingAction::Unbind {
                return ChordResolution::Passthrough;
            }
            return ChordResolution::Dispatch(&self.compiled[index]);
        }

        let prefixes = self.compiled.iter().any(|b| {
            applies(b, contexts)
                && key_steps(b)
                    .is_some_and(|s| s.len() > sequence.len() && s.starts_with(&sequence))
        });
        if prefixes {
            self.pending = sequence;
            return ChordResolution::Pending;
        }

        // Rule 5. A key that matches nothing belongs to the program in the
        // pane, and a half-finished sequence that turned out to match nothing
        // is not a reason to eat it.
        self.pending.clear();
        ChordResolution::Passthrough
    }
}

/// Whether a binding is a candidate for these contexts.
///
/// An `Unbind` is a candidate: it has to be *found* to stop resolution. It is
/// the caller of this that decides what finding one means.
/// The closest capability name, when one is close enough to suggest.
fn nearest<'a>(wanted: &str, capabilities: &'a [String]) -> Option<&'a str> {
    capabilities
        .iter()
        .map(|c| (distance(wanted, c), c.as_str()))
        .filter(|(d, _)| *d <= 2)
        .min_by_key(|(d, _)| *d)
        .map(|(_, c)| c)
}

/// Levenshtein distance.
fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut current = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            current.push(
                (current[j] + 1)
                    .min(previous[j + 1] + 1)
                    .min(previous[j] + cost),
            );
        }
        previous = current;
    }
    previous[b.len()]
}

fn applies(binding: &Compiled, contexts: ContextSet) -> bool {
    binding.when.holds(contexts)
}

/// The key steps of a binding, or `None` when it is a mouse trigger.
fn key_steps(binding: &Compiled) -> Option<Vec<Chord>> {
    binding
        .steps
        .iter()
        .map(|s| match s {
            Step::Key(chord) => Some(chord.clone()),
            Step::Mouse(_) => None,
        })
        .collect()
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::{ChordResolution, Gates, compile};
    use crate::bindings::{Binding, BindingFile, Source, codes};
    use crate::context::{Context, ContextSet};
    use crate::key::Chord;

    fn file(bindings: Vec<Binding>) -> BindingFile {
        BindingFile {
            bindings,
            ..BindingFile::default()
        }
    }

    fn gates() -> Gates {
        Gates {
            platform: "macos".to_owned(),
            terminal: vec!["kitty_keyboard".to_owned()],
            modal: false,
        }
    }

    fn when(trigger: &str, capability: &str, predicate: &str) -> Binding {
        Binding {
            when: Some(predicate.to_owned()),
            ..Binding::simple(trigger, capability)
        }
    }

    #[test]
    fn a_more_specific_binding_wins() {
        // 16 §2.3 rule 3, which is the rule that lets a general binding exist
        // at all: without it, adding a scoped binding would be shadowed by
        // whichever unscoped one happened to sort first.
        let (resolver, problems) = compile(
            &[(
                Source::User,
                file(vec![
                    when("ctrl-p", "palette.open", "terminal_focused"),
                    when(
                        "ctrl-p",
                        "explorer.pick",
                        "explorer_focused && !search_active",
                    ),
                ]),
            )],
            &gates(),
            &[],
        );
        assert_eq!(problems, Vec::new());
        let mut resolver = resolver;
        let contexts = ContextSet::of(&[Context::ExplorerFocused, Context::TerminalFocused]);
        match resolver.press(&Chord::ctrl('p'), contexts) {
            ChordResolution::Dispatch(b) => assert_eq!(b.specificity, 2),
            other => panic!("wrong outcome: {other:?}"),
        }
    }

    #[test]
    fn a_higher_layer_breaks_a_tie() {
        let (mut resolver, _) = compile(
            &[
                (
                    Source::Builtin,
                    file(vec![Binding::simple("ctrl-g", "a.b")]),
                ),
                (Source::User, file(vec![Binding::simple("ctrl-g", "c.d")])),
            ],
            &gates(),
            &[],
        );
        match resolver.press(&Chord::ctrl('g'), ContextSet::EMPTY) {
            ChordResolution::Dispatch(b) => assert_eq!(b.source, Source::User),
            other => panic!("wrong outcome: {other:?}"),
        }
    }

    #[test]
    fn an_unmatched_key_passes_through() {
        // Rule 5, the invariant. Everything else in this module is allowed to
        // be wrong in ways this is not.
        let (mut resolver, _) = compile(
            &[(Source::User, file(vec![Binding::simple("ctrl-g", "a.b")]))],
            &gates(),
            &[],
        );
        assert_eq!(
            resolver.press(&Chord::ctrl('x'), ContextSet::EMPTY),
            ChordResolution::Passthrough
        );
    }

    #[test]
    fn a_sequence_holds_until_it_finishes() {
        let (mut resolver, _) = compile(
            &[(
                Source::User,
                file(vec![Binding::simple("<leader> c", "session.create")]),
            )],
            &gates(),
            &[],
        );
        assert_eq!(
            resolver.press(&Chord::ctrl('b'), ContextSet::EMPTY),
            ChordResolution::Pending
        );
        assert!(resolver.is_pending());
        match resolver.press(&Chord::plain(crate::Key::Char('c')), ContextSet::EMPTY) {
            ChordResolution::Dispatch(b) => assert_eq!(b.trigger, "<leader> c"),
            other => panic!("wrong outcome: {other:?}"),
        }
        assert!(!resolver.is_pending());
    }

    #[test]
    fn a_sequence_that_goes_nowhere_gives_the_keys_back() {
        let (mut resolver, _) = compile(
            &[(
                Source::User,
                file(vec![Binding::simple("<leader> c", "session.create")]),
            )],
            &gates(),
            &[],
        );
        assert_eq!(
            resolver.press(&Chord::ctrl('b'), ContextSet::EMPTY),
            ChordResolution::Pending
        );
        assert_eq!(
            resolver.press(&Chord::plain(crate::Key::Char('q')), ContextSet::EMPTY),
            ChordResolution::Passthrough
        );
        assert!(!resolver.is_pending());
    }

    #[test]
    fn an_unbound_default_stops_resolving_rather_than_falling_to_the_layer_below() {
        let (mut resolver, _) = compile(
            &[
                (
                    Source::Builtin,
                    file(vec![Binding::simple("ctrl-g", "palette.open")]),
                ),
                (Source::User, file(vec![Binding::simple("ctrl-g", "none")])),
            ],
            &gates(),
            &[],
        );
        assert_eq!(
            resolver.press(&Chord::ctrl('g'), ContextSet::EMPTY),
            ChordResolution::Passthrough,
            "`none` removes the default rather than revealing the one under it"
        );
    }

    #[test]
    fn a_reserved_chord_is_refused_unless_it_is_meant() {
        let (_, problems) = compile(
            &[(
                Source::User,
                file(vec![Binding::simple("ctrl-c", "session.close")]),
            )],
            &gates(),
            &[],
        );
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].code, codes::RESERVED);

        let forced = Binding {
            force: true,
            ..Binding::simple("ctrl-c", "session.close")
        };
        let (_, none) = compile(&[(Source::User, file(vec![forced]))], &gates(), &[]);
        assert_eq!(none, Vec::new(), "`force` is how a user says they mean it");
    }

    #[test]
    fn an_unmet_gate_is_reported_rather_than_dropped() {
        let elsewhere = Binding {
            platform: vec!["windows".to_owned()],
            ..Binding::simple("ctrl-g", "a.b")
        };
        let unsupported = Binding {
            requires: Some("modify_other_keys".to_owned()),
            ..Binding::simple("ctrl-j", "a.b")
        };
        let (resolver, problems) = compile(
            &[(Source::User, file(vec![elsewhere, unsupported]))],
            &gates(),
            &[],
        );
        assert_eq!(resolver.bindings().len(), 0);
        assert_eq!(problems.len(), 2);
        assert!(problems.iter().all(|p| p.code == codes::GATE_UNMET));
    }

    #[test]
    fn modes_without_a_modal_keymap_is_an_error() {
        let modal_only = Binding {
            modes: vec!["normal".to_owned()],
            ..Binding::simple("d d", "explorer.delete")
        };
        let (_, problems) = compile(
            &[(Source::User, file(vec![modal_only.clone()]))],
            &gates(),
            &[],
        );
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].code, codes::MODES_WITHOUT_MODAL);

        let (_, none) = compile(
            &[(Source::User, file(vec![modal_only]))],
            &Gates {
                modal: true,
                ..gates()
            },
            &[],
        );
        assert_eq!(none, Vec::new());
    }

    #[test]
    fn a_capability_that_does_not_exist_is_named() {
        let (_, problems) = compile(
            &[(
                Source::User,
                file(vec![Binding::simple("ctrl-g", "pane.spilt")]),
            )],
            &gates(),
            &["pane.split".to_owned()],
        );
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].code, codes::BAD_ACTION);
        assert!(
            problems[0].help.contains("pane.split"),
            "a near miss is worth suggesting: {}",
            problems[0].help
        );
    }

    #[test]
    fn something_that_resembles_nothing_gets_no_guess() {
        // A wrong suggestion is worse than none, because somebody takes it.
        let (_, problems) = compile(
            &[(
                Source::User,
                file(vec![Binding::simple("ctrl-g", "totally.unrelated")]),
            )],
            &gates(),
            &["pane.split".to_owned()],
        );
        assert!(
            problems[0].help.contains("keys list"),
            "{}",
            problems[0].help
        );
    }

    #[test]
    fn every_problem_carries_a_code_and_help() {
        // A refusal without either is a sentence nobody can act on, and a code
        // nobody can grep for.
        let (_, problems) = compile(
            &[(
                Source::User,
                file(vec![
                    Binding::simple("not a chord", "pane.split"),
                    Binding::simple("ctrl-g", "pane.spilt"),
                    when("ctrl-j", "pane.split", "copy_mod"),
                ]),
            )],
            &gates(),
            &["pane.split".to_owned()],
        );
        assert_eq!(problems.len(), 3);
        for problem in &problems {
            assert!(!problem.help.is_empty(), "{}", problem.trigger);
            assert!(problem.code.starts_with("OMT-C"), "{}", problem.code);
        }
    }
}
