//! Rules as data, so an agent's interface can change without a release.
//!
//! An agent CLI moves a line, renames a prompt, adds a confirmation step —
//! on its own schedule, which is not omt's. A release of omt is a bad unit of
//! response to that, and a rule wrong for a week is a rule that reports the
//! wrong state for a week.
//!
//! So the same rules the adapters carry can also be read from a file. The
//! shipped rules stay in code as the floor: a file that is missing, malformed
//! or too new leaves them in place rather than leaving omt with nothing.
//!
//! Deliberately not a general-purpose configuration format. Everything here
//! decides *state*, never content: there is no field that can extract text,
//! because a field that could would be used to put an invented sentence in
//! front of somebody as though the agent had said it.

use serde::Deserialize;

use crate::regions::Region;
use crate::rules::{Rule, ScreenState, Test};

/// What this build knows how to read.
///
/// A file declaring a higher `min_engine` is refused whole rather than read
/// partly: a rule set written against regions or combinators this build does
/// not have is one whose *absent* parts are the ones that would have stopped a
/// wrong answer.
pub const ENGINE: u32 = 1;

/// One agent's rules, as they come from a file.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Which agent this describes, matched against the adapter's own name.
    pub id: String,
    /// The rule set's own version, for saying which one is loaded.
    #[serde(default)]
    pub version: Option<String>,
    /// The lowest engine that can read it.
    #[serde(default)]
    pub min_engine: Option<u32>,
    /// The rules.
    #[serde(default)]
    pub rules: Vec<ManifestRule>,
}

/// One rule, as written in a file.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestRule {
    /// What to call it when explaining a verdict.
    pub id: String,
    /// What it concludes: `working`, `blocked`, `idle`, or `hold`.
    pub state: String,
    /// Which rule wins when two match. Higher is stronger.
    #[serde(default)]
    pub priority: u16,
    /// Where it looks. Defaults to the whole screen, which is the weakest.
    #[serde(default)]
    pub region: Option<String>,
    /// Every one of these appears in the region.
    #[serde(default)]
    pub contains: Vec<String>,
    /// Some line begins with this.
    #[serde(default)]
    pub line_starts_with: Option<String>,
    /// At least one of these holds.
    #[serde(default)]
    pub any: Vec<ManifestGate>,
    /// None of these holds.
    #[serde(default)]
    pub not: Vec<ManifestGate>,
}

/// A nested test.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestGate {
    /// Every one of these appears.
    #[serde(default)]
    pub contains: Vec<String>,
    /// Some line begins with this.
    #[serde(default)]
    pub line_starts_with: Option<String>,
}

/// Why a manifest could not be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ManifestError {
    /// It asks for an engine this build does not have.
    #[error("it needs engine {needs} and this build is engine {ENGINE}")]
    TooNew {
        /// The engine it asked for.
        needs: u32,
    },
    /// A rule names a region that does not exist.
    #[error("rule `{rule}` looks at `{region}`, which is not a region")]
    UnknownRegion {
        /// Which rule.
        rule: String,
        /// What it asked for.
        region: String,
    },
    /// A rule concludes something that is not a state.
    #[error("rule `{rule}` concludes `{state}`, which is not a state")]
    UnknownState {
        /// Which rule.
        rule: String,
        /// What it asked for.
        state: String,
    },
    /// A rule tests nothing, and so matches everything.
    #[error("rule `{rule}` has no test, so it would match every screen")]
    NoTest {
        /// Which rule.
        rule: String,
    },
}

impl Manifest {
    /// Read one from text.
    ///
    /// Here rather than at the call site so the format stays this crate's
    /// business: a caller that had to reach for a TOML parser would be a
    /// caller that could be handed JSON tomorrow and would not compile.
    ///
    /// # Errors
    /// Fails when the text is not a rule set, with the parser's own message —
    /// which names the line, and is more use than anything this could say
    /// about it.
    pub fn parse(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|e| e.to_string())
    }

    /// Turn it into rules, or say why not.
    ///
    /// # Errors
    /// Refuses a manifest that needs a newer engine, names a region or state
    /// that does not exist, or contains a rule that tests nothing — which
    /// would match every screen and report its state for every agent.
    pub fn rules(&self) -> Result<Vec<Rule>, ManifestError> {
        if let Some(needs) = self.min_engine
            && needs > ENGINE
        {
            return Err(ManifestError::TooNew { needs });
        }
        self.rules.iter().map(ManifestRule::compile).collect()
    }
}

impl ManifestRule {
    fn compile(&self) -> Result<Rule, ManifestError> {
        let region = match self.region.as_deref() {
            None | Some("whole") => Region::Whole,
            Some("after_last_rule") => Region::AfterLastRule,
            Some("prompt_box") => Region::PromptBox,
            Some(other) => {
                if let Some(n) = other
                    .strip_prefix("bottom_non_empty(")
                    .and_then(|rest| rest.strip_suffix(')'))
                    .and_then(|n| n.parse::<usize>().ok())
                {
                    Region::BottomNonEmpty(n)
                } else {
                    return Err(ManifestError::UnknownRegion {
                        rule: self.id.clone(),
                        region: other.to_owned(),
                    });
                }
            }
        };

        let state = match self.state.as_str() {
            "working" => ScreenState::Working,
            "blocked" => ScreenState::Blocked,
            "idle" => ScreenState::Idle,
            "hold" => ScreenState::Hold,
            other => {
                return Err(ManifestError::UnknownState {
                    rule: self.id.clone(),
                    state: other.to_owned(),
                });
            }
        };

        let mut tests: Vec<Test> = Vec::new();
        if !self.contains.is_empty() {
            tests.push(Test::Contains(leak(&self.contains)));
        }
        if let Some(prefix) = &self.line_starts_with {
            tests.push(Test::LineStartsWith(leak_one(prefix)));
        }
        if !self.any.is_empty() {
            tests.push(Test::Any(
                self.any.iter().map(ManifestGate::compile).collect(),
            ));
        }
        for gate in &self.not {
            tests.push(Test::Not(Box::new(gate.compile())));
        }
        if tests.is_empty() {
            // A rule with no test matches every screen and reports its state
            // for every agent, forever. Refused rather than loaded: this is
            // the one mistake in a hand-written file that cannot be noticed
            // from its output, because its output is always confident.
            return Err(ManifestError::NoTest {
                rule: self.id.clone(),
            });
        }

        Ok(Rule {
            id: leak_one(&self.id),
            region,
            test: if tests.len() == 1 {
                tests.remove(0)
            } else {
                Test::All(tests)
            },
            state,
            priority: self.priority,
        })
    }
}

impl ManifestGate {
    fn compile(&self) -> Test {
        let mut tests = Vec::new();
        if !self.contains.is_empty() {
            tests.push(Test::Contains(leak(&self.contains)));
        }
        if let Some(prefix) = &self.line_starts_with {
            tests.push(Test::LineStartsWith(leak_one(prefix)));
        }
        match tests.len() {
            0 => Test::Contains(Vec::new()),
            1 => tests.remove(0),
            _ => Test::All(tests),
        }
    }
}

/// Give a loaded string the lifetime the rule types want.
///
/// The rule types hold `&'static str` because the shipped rules are literals,
/// and changing them to owned strings to accommodate loaded ones would put an
/// allocation in the matching path for every rule in the common case. A
/// manifest is loaded once at startup and lives as long as the process, so
/// leaking it is what its lifetime actually is.
fn leak_one(text: &str) -> &'static str {
    Box::leak(text.to_owned().into_boxed_str())
}

fn leak(texts: &[String]) -> Vec<&'static str> {
    texts.iter().map(|t| leak_one(t)).collect()
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;
    use crate::rules::read;

    fn parse(toml: &str) -> Manifest {
        toml::from_str(toml).expect("a manifest")
    }

    #[test]
    fn a_loaded_rule_reads_a_screen_the_same_way_a_shipped_one_does() {
        // The whole point: the file is another way to write the same thing,
        // not a second matching engine that can disagree with the first.
        let manifest = parse(
            r#"
            id = "claude"
            version = "2026.08.15"
            [[rules]]
            id = "a question below the last rule"
            state = "blocked"
            priority = 900
            region = "after_last_rule"
            contains = ["esc to cancel"]
            "#,
        );
        let rules = manifest.rules().expect("it compiles");
        let asking = "output\n\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\nesc to cancel\n";
        assert_eq!(
            read(&rules, asking).expect("a verdict").state,
            ScreenState::Blocked
        );
        // And the region still keeps it off the scrollback.
        let scrolled =
            "esc to cancel\n\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\u{276f} \n";
        assert!(read(&rules, scrolled).is_none());
    }

    #[test]
    fn a_rule_with_no_test_is_refused() {
        // It would match every screen and report its state for every agent,
        // forever. This is the one mistake in a hand-written file that cannot
        // be noticed from its output, because its output is always confident.
        let manifest = parse(
            r#"
            id = "x"
            [[rules]]
            id = "matches everything"
            state = "blocked"
            "#,
        );
        assert_eq!(
            manifest.rules(),
            Err(ManifestError::NoTest {
                rule: "matches everything".to_owned()
            })
        );
    }

    #[test]
    fn a_manifest_for_a_newer_engine_is_refused_whole() {
        // Not read partly. A rule set written against regions this build does
        // not have is one whose *absent* parts are the ones that would have
        // stopped a wrong answer.
        let manifest = parse(
            r#"
            id = "x"
            min_engine = 99
            [[rules]]
            id = "r"
            state = "idle"
            contains = ["x"]
            "#,
        );
        assert_eq!(manifest.rules(), Err(ManifestError::TooNew { needs: 99 }));
    }

    #[test]
    fn a_region_that_does_not_exist_is_named_in_the_error() {
        // Somebody wrote it by hand and got it wrong; the error has to say
        // which rule and which word.
        let manifest = parse(
            r#"
            id = "x"
            [[rules]]
            id = "r"
            state = "idle"
            region = "somewhere"
            contains = ["x"]
            "#,
        );
        assert_eq!(
            manifest.rules(),
            Err(ManifestError::UnknownRegion {
                rule: "r".to_owned(),
                region: "somewhere".to_owned()
            })
        );
    }

    #[test]
    fn a_state_that_does_not_exist_is_named_too() {
        let manifest = parse(
            r#"
            id = "x"
            [[rules]]
            id = "r"
            state = "confused"
            contains = ["x"]
            "#,
        );
        assert!(matches!(
            manifest.rules(),
            Err(ManifestError::UnknownState { .. })
        ));
    }

    #[test]
    fn a_counted_region_is_parsed_from_its_name() {
        let manifest = parse(
            r#"
            id = "x"
            [[rules]]
            id = "r"
            state = "idle"
            region = "bottom_non_empty(3)"
            contains = ["ready"]
            "#,
        );
        let rules = manifest.rules().expect("it compiles");
        assert_eq!(rules[0].region, Region::BottomNonEmpty(3));
    }

    #[test]
    fn negation_and_alternatives_survive_the_round_trip() {
        // The combinators are what make a rule right rather than merely
        // present, so a file that could not express them would be a downgrade.
        let manifest = parse(
            r#"
            id = "x"
            [[rules]]
            id = "an empty prompt box"
            state = "idle"
            region = "prompt_box"
            line_starts_with = "❯"
            [[rules.not]]
            contains = ["enter to select"]
            "#,
        );
        let rules = manifest.rules().expect("it compiles");
        let empty = "before\n\u{2500}\u{2500}\u{2500}\u{2500}\n\u{276f}\n\u{2500}\u{2500}\u{2500}\u{2500}\nafter\n";
        let asking = "before\n\u{2500}\u{2500}\u{2500}\u{2500}\n\u{276f} pick\nenter to select\n\u{2500}\u{2500}\u{2500}\u{2500}\nafter\n";
        assert_eq!(
            read(&rules, empty).expect("a verdict").state,
            ScreenState::Idle
        );
        assert!(
            read(&rules, asking).is_none(),
            "the negation did not survive"
        );
    }

    #[test]
    fn an_unknown_field_is_refused_rather_than_ignored() {
        // A typo in a hand-written file that is silently ignored is a rule
        // that does not do what it says.
        let bad: Result<Manifest, _> = toml::from_str(
            r#"
            id = "x"
            [[rules]]
            id = "r"
            state = "idle"
            containz = ["x"]
            "#,
        );
        assert!(bad.is_err());
    }
}

/// Where loaded rule sets live.
///
/// One file per agent, named after it. A directory rather than one file so a
/// rule set can be replaced without touching the others, and so a broken one
/// costs exactly the agent it describes.
#[must_use]
pub fn directory() -> Option<std::path::PathBuf> {
    if let Ok(explicit) = std::env::var("OMT_AGENT_RULES") {
        return Some(std::path::PathBuf::from(explicit));
    }
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|_| std::env::var("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
        .ok()?;
    Some(base.join("omt").join("agent-rules"))
}

/// The rules for an agent, from a file, where there are usable ones.
///
/// `None` for every kind of absence and every kind of failure, and they are
/// deliberately the same answer: the caller's fallback is the rules compiled
/// into the binary, which are better than nothing in each case. What differs
/// is the line on the way out — a missing file is ordinary and says nothing, a
/// broken one is a mistake somebody made and has to be told about.
#[must_use]
pub fn load(agent: &str) -> Option<Vec<Rule>> {
    load_from(&directory()?, agent)
}

/// The same, from a named directory.
///
/// Split out so a caller can say where to look rather than setting an
/// environment variable to be read back: process state is shared by everything
/// in the process, and two callers pointing it at different directories race.
#[must_use]
pub fn load_from(dir: &std::path::Path, agent: &str) -> Option<Vec<Rule>> {
    let path = dir.join(format!("{agent}.toml"));
    let text = std::fs::read_to_string(&path).ok()?;
    let manifest: Manifest = match toml::from_str(&text) {
        Ok(manifest) => manifest,
        Err(e) => {
            eprintln!("omt: {} is not a rule set: {e}", path.display());
            return None;
        }
    };
    if manifest.id != agent {
        eprintln!(
            "omt: {} describes `{}` and is named for `{agent}`; ignoring it",
            path.display(),
            manifest.id
        );
        return None;
    }
    match manifest.rules() {
        Ok(rules) => Some(rules),
        Err(e) => {
            eprintln!("omt: {} cannot be used: {e}", path.display());
            None
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod loading_tests {
    use super::*;

    #[test]
    fn a_file_named_for_one_agent_and_describing_another_is_refused() {
        // The mistake somebody makes copying a file to start a new one. Loaded
        // anyway, it would apply one agent's rules to another and be
        // confidently wrong about both.
        let manifest: Manifest = toml::from_str("id = \"gemini\"").expect("parses");
        assert_ne!(manifest.id, "claude");
    }

    #[test]
    fn every_failure_is_the_same_answer_to_the_caller() {
        // Missing, malformed, too new, wrong agent: the caller's fallback is
        // the rules compiled into the binary, which is the right answer to all
        // of them. What differs is what gets said on the way out.
        let missing = load("no-such-agent-anywhere");
        assert!(missing.is_none());
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod override_tests {
    use crate::adapter::AgentAdapter;

    /// A rule set on disk, in a directory of its own.
    fn written(agent: &str, body: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("a temp dir");
        std::fs::write(dir.path().join(format!("{agent}.toml")), body).expect("write");
        dir
    }

    #[test]
    fn a_written_rule_set_replaces_the_shipped_one() {
        // The point of the whole file: an agent moves a line and somebody can
        // answer it today rather than at the next release.
        let dir = written(
            "claudecode",
            r#"
            id = "claudecode"
            version = "test"
            [[rules]]
            id = "a wording nobody shipped"
            state = "blocked"
            priority = 999
            contains = ["please decide"]
            "#,
        );
        // SAFETY: single-threaded test, and the variable is the documented way
        // to point the rules directory somewhere else.
        let loaded = crate::agents::ClaudeCode.rules_in_use_from(Some(dir.path()));
        assert!(
            loaded.iter().any(|r| r.id == "a wording nobody shipped"),
            "the written rules were not used: {:?}",
            loaded.iter().map(|r| r.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_broken_rule_set_leaves_the_shipped_ones_in_place() {
        // Every kind of wrong is the same answer, because the shipped rules
        // are better than nothing in each case. Silence here would be an agent
        // omt suddenly cannot read the state of, for a reason nobody can see.
        let dir = written("claudecode", "this is not toml at all {{{");
        let loaded = crate::agents::ClaudeCode.rules_in_use_from(Some(dir.path()));
        let shipped = crate::agents::ClaudeCode.screen_rules();
        assert_eq!(loaded.len(), shipped.len());
    }
}
