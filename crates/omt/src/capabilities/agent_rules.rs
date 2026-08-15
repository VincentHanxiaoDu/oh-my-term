//! Capturing, importing and tracing agent rules.
//!
//! Rules are already a file somebody can edit. What is here is everything
//! around that: writing one means knowing what the agent draws, and having one
//! means nobody else does.
//!
//! Nothing here runs on a timer. A rule set is a matcher that runs against
//! whatever is on somebody's screen, including their work — fetching one in the
//! background is not the same act as updating a library, and omt does not do
//! it.

use super::*;

// ---------------------------------------------------------------------------
// Capture
// ---------------------------------------------------------------------------

/// Input to `agent_rules.capture`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct CaptureIn {
    /// Which session's screen to write down.
    pub session: String,
}

/// What `agent_rules.capture` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct CaptureOut {
    /// The agent omt believes is there, where it believes anything.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// The visible screen.
    pub screen: String,
    /// What each named region of it contains.
    ///
    /// The screen alone is what somebody already has. What they cannot see is
    /// where `after_last_rule` starts, which is the single thing that decides
    /// whether their rule fires on the question or on the scrollback above it.
    pub regions: Vec<CapturedRegion>,
}

/// One region, and what is in it.
#[derive(Serialize, schemars::JsonSchema)]
pub struct CapturedRegion {
    /// Its name, as a rule would write it.
    pub name: String,
    /// What it contains right now.
    pub text: String,
}

capability! {
    /// Write down what a pane is drawing, for writing rules against.
    pub struct AgentRulesCapture;
    input  = CaptureIn,
    output = CaptureOut,
    decl = Decl {
        name: "agent_rules.capture",
        group: "agent_rules",
        verb: "capture",
        title: "Capture a screen for writing rules",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Operator,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "The screen and each named region of it. Rules written from memory are wrong in ways only a real screen finds: omt's own rule for Claude Code's permission prompt was written from memory and was wrong about the first screen anybody sees.",
    },
}

pub(crate) struct AgentRulesHandler(pub(crate) State);

impl CapabilityHandler<AgentRulesCapture> for AgentRulesHandler {
    fn call(&self, _ctx: &CallContext, input: CaptureIn) -> Result<CaptureOut, CapabilityError> {
        let id = omt_types::SessionId::from_wire(&input.session).ok_or_else(|| {
            CapabilityError::invalid_input(format!("`{}` is not a session id", input.session))
        })?;
        let instance = self.0.lock()?;
        let runtime = instance
            .runtime(id)
            .ok_or_else(|| CapabilityError::not_found("no such session"))?;
        let screen = omt_daemon::observe::screen_text(runtime.terminal().grid());
        let agent = instance
            .observed_agent(id)
            .map(|found| format!("{:?}", found.agent).to_lowercase());

        let regions = [
            ("whole", omt_agent_adapters::Region::Whole),
            ("after_last_rule", omt_agent_adapters::Region::AfterLastRule),
            ("prompt_box", omt_agent_adapters::Region::PromptBox),
            (
                "bottom_non_empty(5)",
                omt_agent_adapters::Region::BottomNonEmpty(5),
            ),
        ]
        .into_iter()
        .map(|(name, region)| CapturedRegion {
            name: name.to_owned(),
            text: region.of(&screen).to_owned(),
        })
        .collect();

        Ok(CaptureOut {
            agent,
            screen,
            regions,
        })
    }
}

// ---------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------

/// Input to `agent_rules.inspect`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct InspectIn {
    /// The rule set, as text.
    ///
    /// Text rather than a path, so the caller decides where it came from and
    /// omt never reads a file it was not handed.
    pub rules: String,
    /// Which agent it is being considered for.
    pub agent: String,
}

/// What `agent_rules.inspect` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct InspectOut {
    /// Whether it would be accepted.
    pub usable: bool,
    /// Why not, where not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refused: Option<String>,
    /// What each rule concludes and where it looks, in the order they decide.
    pub rules: Vec<InspectedRule>,
    /// How many rules the shipped set has, for comparison.
    pub shipped: usize,
}

/// One rule, as a person reads it.
#[derive(Serialize, schemars::JsonSchema)]
pub struct InspectedRule {
    /// Its id.
    pub id: String,
    /// What it concludes.
    pub state: String,
    /// Where it looks.
    pub region: String,
    /// Which rule wins when two match.
    pub priority: u16,
}

capability! {
    /// Say what a rule set would do, without installing it.
    pub struct AgentRulesInspect;
    input  = InspectIn,
    output = InspectOut,
    decl = Decl {
        name: "agent_rules.inspect",
        group: "agent_rules",
        verb: "inspect",
        title: "Inspect a rule set",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Operator,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "What it would change, before anything changes. A rule set is a matcher that runs against whatever is on the screen, so installing one is an act rather than an update.",
    },
}

impl CapabilityHandler<AgentRulesInspect> for AgentRulesHandler {
    fn call(&self, _ctx: &CallContext, input: InspectIn) -> Result<InspectOut, CapabilityError> {
        let shipped = omt_agent_adapters::builtin()
            .all()
            .iter()
            .find(|a| format!("{:?}", a.kind()).to_lowercase() == input.agent.to_lowercase())
            .map_or(0, |a| a.screen_rules().len());

        let manifest = match omt_agent_adapters::Manifest::parse(&input.rules) {
            Ok(manifest) => manifest,
            Err(e) => {
                return Ok(InspectOut {
                    usable: false,
                    refused: Some(format!("it is not a rule set: {e}")),
                    rules: Vec::new(),
                    shipped,
                });
            }
        };
        if manifest.id.to_lowercase() != input.agent.to_lowercase() {
            // The mistake somebody makes copying a file to start a new one.
            // Installed anyway it would apply one agent's rules to another and
            // be confidently wrong about both.
            return Ok(InspectOut {
                usable: false,
                refused: Some(format!(
                    "it describes `{}` and is being installed for `{}`",
                    manifest.id, input.agent
                )),
                rules: Vec::new(),
                shipped,
            });
        }
        match manifest.rules() {
            Ok(rules) => Ok(InspectOut {
                usable: true,
                refused: None,
                rules: rules
                    .iter()
                    .map(|rule| InspectedRule {
                        id: rule.id.to_owned(),
                        state: format!("{:?}", rule.state).to_lowercase(),
                        region: format!("{:?}", rule.region).to_lowercase(),
                        priority: rule.priority,
                    })
                    .collect(),
                shipped,
            }),
            Err(e) => Ok(InspectOut {
                usable: false,
                refused: Some(e.to_string()),
                rules: Vec::new(),
                shipped,
            }),
        }
    }
}

/// Input to `agent_rules.import`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct ImportIn {
    /// The rule set, as text.
    pub rules: String,
    /// Which agent to install it for.
    pub agent: String,
}

/// What `agent_rules.import` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct ImportOut {
    /// Where it was written.
    pub path: String,
    /// How many rules it has.
    pub rules: usize,
}

capability! {
    /// Install a rule set for an agent.
    pub struct AgentRulesImport;
    input  = ImportIn,
    output = ImportOut,
    decl = Decl {
        name: "agent_rules.import",
        group: "agent_rules",
        verb: "import",
        title: "Install a rule set",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::WRITES_FS,
        // Cas: the same set installed twice is the same file.
        intent: Some(Intent::Cas),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Only when asked, and only after the checks. omt never fetches, updates or applies a rule set on a timer or at startup: a matcher that runs against somebody's screen arrives because they decided, or not at all.",
    },
}

impl CapabilityHandler<AgentRulesImport> for AgentRulesHandler {
    fn call(&self, ctx: &CallContext, input: ImportIn) -> Result<ImportOut, CapabilityError> {
        // The same checks `inspect` reports, applied rather than described:
        // one path, so what was shown cannot differ from what was installed.
        let looked = <Self as CapabilityHandler<AgentRulesInspect>>::call(
            self,
            ctx,
            InspectIn {
                rules: input.rules.clone(),
                agent: input.agent.clone(),
            },
        )?;
        if !looked.usable {
            return Err(CapabilityError::invalid_input(
                looked
                    .refused
                    .unwrap_or_else(|| "the rule set cannot be used".to_owned()),
            ));
        }

        let dir = omt_agent_adapters::manifest::directory().ok_or_else(|| {
            CapabilityError::precondition_failed(
                "neither HOME nor XDG_CONFIG_HOME is set, so there is nowhere to keep rules",
            )
        })?;
        std::fs::create_dir_all(&dir)
            .map_err(|e| CapabilityError::internal(format!("creating the rules directory: {e}")))?;
        let path = dir.join(format!("{}.toml", input.agent.to_lowercase()));
        std::fs::write(&path, &input.rules)
            .map_err(|e| CapabilityError::internal(format!("writing the rule set: {e}")))?;

        Ok(ImportOut {
            path: path.display().to_string(),
            rules: looked.rules.len(),
        })
    }
}

// ---------------------------------------------------------------------------
// Trace
// ---------------------------------------------------------------------------

/// Input to `agent_rules.explain`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExplainIn {
    /// Which session.
    pub session: String,
}

/// What `agent_rules.explain` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct ExplainOut {
    /// The rule that decided, where one did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    /// What it concluded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// Which agent's rules were applied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// In words, for somebody looking at a wrong answer.
    pub summary: String,
}

capability! {
    /// Say which rule decided a pane's state.
    pub struct AgentRulesExplain;
    input  = ExplainIn,
    output = ExplainOut,
    decl = Decl {
        name: "agent_rules.explain",
        group: "agent_rules",
        verb: "explain",
        title: "Explain a screen verdict",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Operator,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Which rule won, where it looked, and what it concluded. A wrong answer that cannot be traced to a rule is one people argue about.",
    },
}

impl CapabilityHandler<AgentRulesExplain> for AgentRulesHandler {
    fn call(&self, _ctx: &CallContext, input: ExplainIn) -> Result<ExplainOut, CapabilityError> {
        let id = omt_types::SessionId::from_wire(&input.session).ok_or_else(|| {
            CapabilityError::invalid_input(format!("`{}` is not a session id", input.session))
        })?;
        let instance = self.0.lock()?;
        let runtime = instance
            .runtime(id)
            .ok_or_else(|| CapabilityError::not_found("no such session"))?;
        let screen = omt_daemon::observe::screen_text(runtime.terminal().grid());

        let Some(found) = instance.observed_agent(id) else {
            return Ok(ExplainOut {
                rule: None,
                state: None,
                agent: None,
                summary: "omt has not identified an agent in this session, so no rules were \
                          applied"
                    .to_owned(),
            });
        };
        let agent = format!("{:?}", found.agent).to_lowercase();
        let set = omt_agent_adapters::builtin();
        let rules = set
            .all()
            .iter()
            .find(|a| a.kind() == found.agent)
            .map(|a| a.rules_in_use())
            .unwrap_or_default();

        match omt_agent_adapters::read_screen(&rules, &screen) {
            Some(verdict) => {
                let region = rules.iter().find(|r| r.id == verdict.rule).map_or_else(
                    || "?".to_owned(),
                    |r| format!("{:?}", r.region).to_lowercase(),
                );
                Ok(ExplainOut {
                    rule: Some(verdict.rule.to_owned()),
                    state: Some(format!("{:?}", verdict.state).to_lowercase()),
                    agent: Some(agent.clone()),
                    summary: format!(
                        "`{}` matched in {region} and concluded {:?}, out of {} rule(s) for {agent}",
                        verdict.rule,
                        verdict.state,
                        rules.len()
                    ),
                })
            }
            None => Ok(ExplainOut {
                rule: None,
                state: None,
                agent: Some(agent.clone()),
                summary: format!(
                    "none of the {} rule(s) for {agent} matched this screen, so the floor decides",
                    rules.len()
                ),
            }),
        }
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

    fn handler() -> AgentRulesHandler {
        AgentRulesHandler(crate::state::State::default())
    }

    fn ctx() -> CallContext {
        crate::capabilities::layouts::daemon_context()
    }

    fn inspect(rules: &str, agent: &str) -> InspectOut {
        <AgentRulesHandler as CapabilityHandler<AgentRulesInspect>>::call(
            &handler(),
            &ctx(),
            InspectIn {
                rules: rules.to_owned(),
                agent: agent.to_owned(),
            },
        )
        .expect("inspect answers rather than failing")
    }

    #[test]
    fn a_usable_set_is_described_rule_by_rule() {
        // What it would change, before anything changes: a rule set is a
        // matcher that runs against whatever is on the screen, so installing
        // one is an act rather than an update.
        let looked = inspect(
            r#"
            id = "claudecode"
            [[rules]]
            id = "a question below the last rule"
            state = "blocked"
            priority = 900
            region = "after_last_rule"
            contains = ["esc to cancel"]
            "#,
            "claudecode",
        );
        assert!(looked.usable, "{:?}", looked.refused);
        assert_eq!(looked.rules.len(), 1);
        assert_eq!(looked.rules[0].state, "blocked");
        assert!(looked.rules[0].region.contains("afterlastrule"));
        // And how many it would replace, which is the number somebody needs to
        // decide with.
        assert!(looked.shipped > 0);
    }

    #[test]
    fn a_set_for_another_agent_is_refused_and_both_names_are_said() {
        // The mistake somebody makes copying a file to start a new one.
        // Installed anyway it would apply one agent's rules to another and be
        // confidently wrong about both.
        let looked = inspect("id = \"gemini\"", "claudecode");
        assert!(!looked.usable);
        let said = looked.refused.expect("a reason");
        assert!(said.contains("gemini"), "{said}");
        assert!(said.contains("claudecode"), "{said}");
    }

    #[test]
    fn a_rule_that_matches_everything_is_refused_by_name() {
        // The one mistake whose output cannot be noticed, because a rule with
        // no test matches every screen and is always confident.
        let looked = inspect(
            r#"
            id = "claudecode"
            [[rules]]
            id = "matches everything"
            state = "blocked"
            "#,
            "claudecode",
        );
        assert!(!looked.usable);
        assert!(
            looked
                .refused
                .expect("a reason")
                .contains("matches everything"),
            "the refusal did not name the rule"
        );
    }

    #[test]
    fn a_set_for_a_newer_omt_is_refused_whole() {
        // Not read partly: a set written against regions this build does not
        // have is one whose *absent* parts are the ones that would have
        // stopped a wrong answer.
        let looked = inspect(
            r#"
            id = "claudecode"
            min_engine = 99
            [[rules]]
            id = "r"
            state = "idle"
            contains = ["x"]
            "#,
            "claudecode",
        );
        assert!(!looked.usable);
        assert!(looked.rules.is_empty());
    }

    #[test]
    fn text_that_is_not_a_rule_set_says_so_with_the_parsers_message() {
        // Which names the line, and is more use than anything omt could say
        // about it.
        let looked = inspect("this is not toml {{{", "claudecode");
        assert!(!looked.usable);
        assert!(looked.refused.is_some());
    }

    #[test]
    fn importing_refuses_exactly_what_inspecting_refuses() {
        // One path, so what was shown cannot differ from what was installed.
        let bad = r#"
            id = "gemini"
            [[rules]]
            id = "r"
            state = "idle"
            contains = ["x"]
            "#;
        assert!(!inspect(bad, "claudecode").usable);
        let refused = <AgentRulesHandler as CapabilityHandler<AgentRulesImport>>::call(
            &handler(),
            &ctx(),
            ImportIn {
                rules: bad.to_owned(),
                agent: "claudecode".to_owned(),
            },
        );
        assert!(refused.is_err(), "import accepted what inspect refused");
    }
}
