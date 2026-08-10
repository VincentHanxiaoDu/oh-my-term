//! First run, and the help that stops it needing a manual.
//!
//! `docs/architecture/19-onboarding.md` §1, §2 and §4. Two things live here.
//!
//! The hints, under §1.2's rule: **at most one hint is ever offered**. That
//! rule is the whole feature. A first run that fires six tips teaches nobody
//! six things — it teaches them that this program interrupts, and they dismiss
//! the lot. Which is why `hint_state` returns a single `next` rather than a
//! list and lets the client render it: given a list, every client would show
//! all of them, and the rule would live in three places and hold in none.
//!
//! The help, under §4.2, is *generated from the catalog* rather than written
//! beside it. Hand-written help is wrong the first time a capability is renamed
//! and stays wrong, because nothing fails when it drifts. Deriving it means the
//! only way to have an undocumented topic is to have an undocumented
//! capability, and the parity gate already refuses that.

use std::collections::BTreeSet;

use omt_catalog::{CapabilityError, CapabilityHandler};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Hints
// ---------------------------------------------------------------------------

/// One thing worth telling somebody once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    /// How it is referred to when dismissed.
    pub id: &'static str,
    /// What it says.
    pub text: &'static str,
    /// How many runs must have happened before it is worth saying.
    ///
    /// Staged, because §1 is three different moments: the first run should say
    /// the one thing needed to not be stuck, and the tenth should be silent
    /// unless it has something the first run had no room for.
    pub after_runs: u32,
}

/// Every hint omt knows, in the order they become useful.
///
/// Ordered rather than scored. A score would need tuning against a population
/// nobody has, and the honest version of "which is most useful" here is "the
/// earliest one they have not seen".
pub const HINTS: &[Hint] = &[
    Hint {
        id: "prefix",
        text: "Ctrl-A is omt's prefix. Ctrl-A ? lists every key.",
        after_runs: 0,
    },
    Hint {
        id: "sessions",
        text: "Ctrl-A w shows every session, here and on your other machines.",
        after_runs: 1,
    },
    Hint {
        id: "agents",
        text: "Ctrl-A a jumps to whichever agent is waiting on you.",
        after_runs: 2,
    },
    Hint {
        id: "phone",
        text: "`omt web` prints a QR code. Your phone becomes a client.",
        after_runs: 4,
    },
    Hint {
        id: "config",
        text: "`omt config edit` — every default here is a setting.",
        after_runs: 9,
    },
];

/// What omt remembers about what it has told somebody.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Hints {
    /// How many times omt has been started.
    pub runs: u32,
    /// Which hints have been shown or dismissed, by id.
    pub seen: BTreeSet<String>,
    /// Whether hints were turned off wholesale.
    pub silenced: bool,
}

impl Hints {
    /// The one hint to offer now, if any.
    ///
    /// One. §1.2's rule is enforced by the return type rather than by asking
    /// every client to slice a list, because a rule that depends on three
    /// clients each remembering it is not a rule.
    #[must_use]
    pub fn next(&self) -> Option<&'static Hint> {
        if self.silenced {
            return None;
        }
        HINTS
            .iter()
            .find(|h| h.after_runs <= self.runs && !self.seen.contains(h.id))
    }

    /// Note that a hint was put in front of somebody.
    ///
    /// Shown counts as seen. A hint that reappears until it is explicitly
    /// dismissed is a hint people learn to ignore in place, which costs the
    /// attention that every later hint depends on.
    pub fn mark_shown(&mut self, id: &str) {
        self.seen.insert(id.to_owned());
    }

    /// Turn the lot off, permanently.
    pub fn silence(&mut self) {
        self.silenced = true;
    }

    /// Record a start.
    pub fn started(&mut self) {
        self.runs = self.runs.saturating_add(1);
    }

    /// How many hints remain to be offered.
    #[must_use]
    pub fn remaining(&self) -> usize {
        if self.silenced {
            return 0;
        }
        HINTS.iter().filter(|h| !self.seen.contains(h.id)).count()
    }
}

/// Input to `onboarding.hint_state`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct HintStateIn {}

/// The hint to show, if there is one.
#[derive(Serialize, schemars::JsonSchema)]
pub struct HintOut {
    /// Its id, which dismissing names.
    pub id: String,
    /// What it says.
    pub text: String,
}

/// What `onboarding.hint_state` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct HintStateOut {
    /// The single hint to offer, or nothing.
    pub next: Option<HintOut>,
    /// How many are left, so a client can say "3 more tips" honestly.
    pub remaining: usize,
    /// How many times omt has run.
    pub runs: u32,
    /// Whether hints were turned off.
    pub silenced: bool,
}

omt_catalog::capability! {
    /// The one hint worth showing, and how many remain.
    pub struct HintState;
    input  = HintStateIn,
    output = HintStateOut,
    decl = omt_catalog::Decl {
        name: "onboarding.hint_state",
        group: "onboarding",
        verb: "hint_state",
        title: "Show the next tip",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "At most one hint is ever offered. Six tips on a first run teach nobody six things; they teach that omt interrupts.",
    },
}

/// Serves the onboarding capabilities.
pub struct HintsHandler(pub std::sync::Arc<std::sync::Mutex<Hints>>);

impl CapabilityHandler<HintState> for HintsHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        _input: HintStateIn,
    ) -> Result<HintStateOut, CapabilityError> {
        let mut hints = self
            .0
            .lock()
            .map_err(|_| CapabilityError::internal("the hint lock was poisoned"))?;
        let next = hints.next().map(|h| HintOut {
            id: h.id.to_owned(),
            text: h.text.to_owned(),
        });
        // Asking for it is showing it. Otherwise the same hint comes back on
        // every poll and the one-at-a-time rule buys nothing.
        if let Some(shown) = &next {
            hints.mark_shown(&shown.id);
        }
        Ok(HintStateOut {
            remaining: hints.remaining(),
            runs: hints.runs,
            silenced: hints.silenced,
            next,
        })
    }
}

/// Input to `onboarding.dismiss_hints`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct DismissIn {
    /// One hint by id, or nothing to turn them all off.
    #[serde(default)]
    pub hint: Option<String>,
}

/// What `onboarding.dismiss_hints` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct DismissOut {
    /// How many are left.
    pub remaining: usize,
    /// Whether the lot were turned off.
    pub silenced: bool,
}

omt_catalog::capability! {
    /// Stop offering a hint, or all of them.
    pub struct DismissHints;
    input  = DismissIn,
    output = DismissOut,
    decl = omt_catalog::Decl {
        name: "onboarding.dismiss_hints",
        group: "onboarding",
        verb: "dismiss_hints",
        title: "Stop showing tips",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Command,
        role: omt_types::Role::Operator,
        effects: omt_catalog::Effects::WRITES_FS,
        // Cas: dismissing is idempotent, so a repeat after a dropped
        // acknowledgement returns the original result rather than being
        // refused. Nobody should have to dismiss a tip twice because their
        // connection blinked.
        intent: Some(omt_catalog::Intent::Cas),
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Permanent. Somebody who turns tips off and is asked again next week has been told their answer did not count.",
    },
}

impl CapabilityHandler<DismissHints> for HintsHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: DismissIn,
    ) -> Result<DismissOut, CapabilityError> {
        let mut hints = self
            .0
            .lock()
            .map_err(|_| CapabilityError::internal("the hint lock was poisoned"))?;
        match input.hint {
            Some(id) => {
                if !HINTS.iter().any(|h| h.id == id) {
                    return Err(CapabilityError::not_found(format!("no hint `{id}`")));
                }
                hints.mark_shown(&id);
            }
            None => hints.silence(),
        }
        Ok(DismissOut {
            remaining: hints.remaining(),
            silenced: hints.silenced,
        })
    }
}

// ---------------------------------------------------------------------------
// Help
// ---------------------------------------------------------------------------

/// Input to `help.topics`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct TopicsIn {}

/// One topic: a capability group, and what is in it.
#[derive(Serialize, schemars::JsonSchema)]
pub struct Topic {
    /// The group name, which is the topic name.
    pub name: String,
    /// The titles of what it holds, so a tree can be drawn without a second call.
    pub entries: Vec<String>,
}

/// What `help.topics` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct TopicsOut {
    /// Every topic, alphabetically.
    pub topics: Vec<Topic>,
}

omt_catalog::capability! {
    /// The help tree, generated from the catalog.
    pub struct HelpTopics;
    input  = TopicsIn,
    output = TopicsOut,
    decl = omt_catalog::Decl {
        name: "help.topics",
        group: "help",
        verb: "topics",
        title: "Browse help",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Derived from the catalog, not written beside it. Hand-written help is wrong the first time something is renamed and stays wrong, because nothing fails when it drifts.",
    },
}

/// Input to `help.render`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct RenderIn {
    /// A topic name, or a capability name for the specific one.
    pub topic: String,
}

/// One entry of a rendered help page.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct HelpEntry {
    /// The capability's name.
    pub name: String,
    /// Its palette title.
    pub title: String,
    /// Its documentation.
    pub doc: String,
    /// Whether calling it changes anything.
    pub command: bool,
    /// What it needs to be allowed to do.
    pub role: String,
}

/// What `help.render` reports: the overlay's model, not a rendered page.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct RenderOut {
    /// What was asked for.
    pub topic: String,
    /// What is in it.
    pub entries: Vec<HelpEntry>,
}

omt_catalog::capability! {
    /// One help page, as a model the surface renders.
    pub struct HelpRender;
    input  = RenderIn,
    output = RenderOut,
    decl = omt_catalog::Decl {
        name: "help.render",
        group: "help",
        verb: "render",
        title: "Show help for a topic",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "A model rather than formatted text, so a phone and a terminal can render the same help differently without one of them parsing the other's layout.",
    },
}

/// Serves the help capabilities off the catalog.
pub struct HelpHandler;

/// Every declaration, which is where help comes from.
fn declarations() -> Vec<&'static omt_catalog::Decl> {
    omt_catalog::linked_decls()
        .into_iter()
        .filter(|d| !d.hidden)
        .collect()
}

impl CapabilityHandler<HelpTopics> for HelpHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        _input: TopicsIn,
    ) -> Result<TopicsOut, CapabilityError> {
        let mut groups: std::collections::BTreeMap<&str, Vec<String>> =
            std::collections::BTreeMap::new();
        for d in declarations() {
            groups.entry(d.group).or_default().push(d.title.to_owned());
        }
        Ok(TopicsOut {
            topics: groups
                .into_iter()
                .map(|(name, mut entries)| {
                    entries.sort();
                    Topic {
                        name: name.to_owned(),
                        entries,
                    }
                })
                .collect(),
        })
    }
}

impl CapabilityHandler<HelpRender> for HelpHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: RenderIn,
    ) -> Result<RenderOut, CapabilityError> {
        let wanted = input.topic.trim();
        if wanted.is_empty() {
            return Err(CapabilityError::invalid_input("a topic is needed"));
        }
        // A group or a single capability, because somebody who typed a
        // capability name into help meant that capability, and answering
        // "no such topic" to a name omt itself printed is a poor showing.
        let entries: Vec<HelpEntry> = declarations()
            .into_iter()
            .filter(|d| d.group == wanted || d.name == wanted)
            .map(|d| HelpEntry {
                name: d.name.to_owned(),
                title: d.title.to_owned(),
                doc: d.doc.to_owned(),
                command: matches!(d.kind, omt_catalog::Kind::Command),
                role: format!("{:?}", d.role).to_lowercase(),
            })
            .collect();
        if entries.is_empty() {
            return Err(CapabilityError::not_found(format!(
                "no help topic `{wanted}`"
            )));
        }
        Ok(RenderOut {
            topic: wanted.to_owned(),
            entries,
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

    /// A caller with nothing special about them: help is for everybody.
    fn ctx() -> omt_catalog::CallContext {
        omt_catalog::CallContext {
            actor: omt_types::Actor::Local,
            role: omt_types::Role::Viewer,
            request: omt_catalog::RequestId {
                device: omt_types::DeviceId::new(),
                n: 1,
            },
            intent: None,
        }
    }

    #[test]
    fn a_first_run_offers_exactly_one_hint() {
        // The rule the whole feature exists for. Six tips teach nobody six
        // things; they teach that omt interrupts.
        let hints = Hints::default();
        assert_eq!(hints.next().map(|h| h.id), Some("prefix"));
    }

    #[test]
    fn the_first_hint_is_the_one_that_stops_somebody_being_stuck() {
        // Not the most impressive feature — the prefix key. Somebody who cannot
        // reach omt at all has no use for what omt can do.
        assert_eq!(HINTS.first().map(|h| h.id), Some("prefix"));
        assert!(HINTS[0].text.contains("Ctrl-A"));
    }

    #[test]
    fn a_hint_does_not_come_back_once_it_has_been_shown() {
        let mut hints = Hints::default();
        hints.mark_shown("prefix");
        assert_ne!(hints.next().map(|h| h.id), Some("prefix"));
    }

    #[test]
    fn later_hints_wait_for_later_runs() {
        // The tenth run is a different moment from the first, and saying
        // everything on day one is how the later ones go unread.
        let mut hints = Hints::default();
        for h in HINTS {
            hints.mark_shown(h.id);
        }
        hints.seen.remove("config");
        assert_eq!(hints.next(), None, "not on a first run");
        hints.runs = 9;
        assert_eq!(hints.next().map(|h| h.id), Some("config"));
    }

    #[test]
    fn silencing_stops_everything_and_says_nothing_remains() {
        let mut hints = Hints::default();
        hints.silence();
        assert_eq!(hints.next(), None);
        assert_eq!(
            hints.remaining(),
            0,
            "`3 more tips` under a silence somebody asked for is a lie"
        );
    }

    #[test]
    fn remaining_counts_down_as_hints_are_seen() {
        let mut hints = Hints::default();
        let before = hints.remaining();
        hints.mark_shown("prefix");
        assert_eq!(hints.remaining(), before - 1);
    }

    #[test]
    fn runs_are_counted_and_do_not_overflow() {
        let mut hints = Hints {
            runs: u32::MAX,
            ..Hints::default()
        };
        hints.started();
        assert_eq!(hints.runs, u32::MAX, "a long-lived install must not panic");
    }

    #[test]
    fn every_hint_has_a_distinct_id() {
        // Two sharing one makes dismissing either dismiss both, silently.
        let ids: BTreeSet<&str> = HINTS.iter().map(|h| h.id).collect();
        assert_eq!(ids.len(), HINTS.len());
    }

    #[test]
    fn the_hints_are_ordered_by_when_they_become_useful() {
        // `next` takes the first match, so an out-of-order list would offer a
        // tenth-run hint on a first run the moment the earlier ones were seen.
        let mut runs: Vec<u32> = HINTS.iter().map(|h| h.after_runs).collect();
        let sorted = {
            let mut c = runs.clone();
            c.sort_unstable();
            c
        };
        runs.dedup();
        assert_eq!(
            HINTS.iter().map(|h| h.after_runs).collect::<Vec<_>>(),
            sorted
        );
    }

    #[test]
    fn help_topics_come_from_the_catalog() {
        let out = CapabilityHandler::<HelpTopics>::call(
            &HelpHandler,
            &ctx(),
            TopicsIn {},
        )
        .expect("topics");
        assert!(
            out.topics.iter().any(|t| t.name == "session"),
            "sessions are a topic because they are a capability group"
        );
        assert!(out.topics.iter().all(|t| !t.entries.is_empty()));
    }

    #[test]
    fn help_renders_a_group_and_a_single_capability_alike() {
        // Answering "no such topic" to a name omt itself printed is a poor
        // showing.
        let ctx = ctx();
        let group = CapabilityHandler::<HelpRender>::call(
            &HelpHandler,
            &ctx,
            RenderIn {
                topic: "help".to_owned(),
            },
        )
        .expect("group");
        assert!(group.entries.len() >= 2);
        let one = CapabilityHandler::<HelpRender>::call(
            &HelpHandler,
            &ctx,
            RenderIn {
                topic: "help.render".to_owned(),
            },
        )
        .expect("one");
        assert_eq!(one.entries.len(), 1);
    }

    #[test]
    fn an_unknown_topic_is_not_found_rather_than_empty() {
        let err = CapabilityHandler::<HelpRender>::call(
            &HelpHandler,
            &ctx(),
            RenderIn {
                topic: "nonsense".to_owned(),
            },
        )
        .expect_err("must refuse");
        assert!(format!("{err:?}").to_lowercase().contains("not"), "{err:?}");
    }

    #[test]
    fn help_says_which_entries_change_something() {
        // The one thing somebody reading help most needs before they try it.
        let out = CapabilityHandler::<HelpRender>::call(
            &HelpHandler,
            &ctx(),
            RenderIn {
                topic: "onboarding".to_owned(),
            },
        )
        .expect("render");
        assert!(out.entries.iter().any(|e| e.command));
        assert!(out.entries.iter().any(|e| !e.command));
    }
}
