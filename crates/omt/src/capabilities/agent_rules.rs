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
    fn an_empty_name_is_refused_rather_than_accepted() {
        // A row with nothing in it is worse than one saying `shell`, because
        // at least `shell` is true.
        for name in ["", "   ", "\t"] {
            let refused = <AgentRulesHandler as CapabilityHandler<SessionRename>>::call(
                &handler(),
                &ctx(),
                RenameIn {
                    session: "sess_0123456789abcdef0123456789abcdef".to_owned(),
                    title: name.to_owned(),
                },
            );
            assert!(refused.is_err(), "`{name:?}` was accepted as a name");
        }
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

// ---------------------------------------------------------------------------
// Naming a session
// ---------------------------------------------------------------------------

/// Input to `session.rename`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct RenameIn {
    /// Which session.
    pub session: String,
    /// What to call it.
    pub title: String,
}

/// What `session.rename` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct RenameOut {
    /// The name it now has.
    pub title: String,
}

capability! {
    /// Give a session a name.
    pub struct SessionRename;
    input  = RenameIn,
    output = RenameOut,
    decl = Decl {
        name: "session.rename",
        group: "session",
        verb: "rename",
        title: "Rename a session",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::empty(),
        // LwwFreeText: it is a name somebody typed, and the last one typed is
        // the one they meant. Two people renaming the same session is not a
        // conflict worth a merge.
        intent: Some(Intent::LwwFreeText),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "A roster of six things called `shell` is what omt shows without this. Detection answers it for agents; for everything else the answer is letting somebody say what a thing is.",
    },
}

impl CapabilityHandler<SessionRename> for AgentRulesHandler {
    fn call(&self, _ctx: &CallContext, input: RenameIn) -> Result<RenameOut, CapabilityError> {
        let title = input.title.trim().to_owned();
        if title.is_empty() {
            // Refused rather than accepted: a row with nothing in it is worse
            // than one saying `shell`, because at least `shell` is true.
            return Err(CapabilityError::invalid_input(
                "a session's name cannot be empty",
            ));
        }
        let id = omt_types::SessionId::from_wire(&input.session).ok_or_else(|| {
            CapabilityError::invalid_input(format!("`{}` is not a session id", input.session))
        })?;
        let mut instance = self.0.lock()?;
        let session = instance
            .session_mut(id)
            .ok_or_else(|| CapabilityError::not_found("no such session"))?;
        session.title.clone_from(&title);
        Ok(RenameOut { title })
    }
}

// ---------------------------------------------------------------------------
// Hooks
// ---------------------------------------------------------------------------

/// Where an agent keeps the settings its hooks live in.
///
/// One agent for now, because one agent has hooks omt knows how to write. A
/// list keyed by agent is what this becomes when a second one does.
fn hook_settings_for(agent: omt_types::AgentKind) -> Option<std::path::PathBuf> {
    let home = std::env::var("HOME").ok()?;
    match agent {
        omt_types::AgentKind::ClaudeCode => {
            Some(std::path::PathBuf::from(home).join(".claude/settings.json"))
        }
        _ => None,
    }
}

/// Input to `agent.hooks_plan`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct HooksPlanIn {
    /// Which agent's settings to plan against.
    pub agent: String,
}

/// What `agent.hooks_plan` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct HooksPlanOut {
    /// The file that would change.
    pub path: String,
    /// Whether anything would change.
    pub changes: bool,
    /// The change, as a diff somebody can read before agreeing to it.
    pub diff: String,
    /// Whether the hooks are installed and current.
    pub installed: bool,
}

capability! {
    /// Say what installing omt's hooks would change.
    pub struct AgentHooksPlan;
    input  = HooksPlanIn,
    output = HooksPlanOut,
    decl = Decl {
        name: "agent.hooks_plan",
        group: "agent",
        verb: "hooks_plan",
        title: "Plan a hook install",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Operator,
        effects: Effects::READS_FS,
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "The diff, before the write. omt edits a file the agent owns and somebody else may have written their own hooks into, so what changes is shown first.",
    },
}

impl CapabilityHandler<AgentHooksPlan> for AgentRulesHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: HooksPlanIn,
    ) -> Result<HooksPlanOut, CapabilityError> {
        let (path, plan) = hook_plan(&input.agent)?;
        Ok(HooksPlanOut {
            path: path.display().to_string(),
            changes: plan.changes,
            diff: plan.diff(),
            installed: !plan.changes,
        })
    }
}

/// Input to `agent.resume_plan`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct ResumePlanIn {
    /// Which session's agent.
    pub session: String,
}

/// What `agent.resume_plan` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct ResumePlanOut {
    /// The command that would continue the conversation, where omt knows one.
    pub argv: Vec<String>,
    /// The agent's own id for that conversation.
    pub agent_session: Option<String>,
    /// Why there is no command, when there is none.
    ///
    /// A reason rather than an empty list: "omt cannot resume this agent" and
    /// "this agent has not said what it calls its conversation yet" are
    /// different situations and lead somebody to do different things.
    pub because: Option<String>,
}

capability! {
    /// Say how a session's agent would be started again on the same conversation.
    pub struct AgentResumePlan;
    input  = ResumePlanIn,
    output = ResumePlanOut,
    decl = Decl {
        name: "agent.resume_plan",
        group: "agent",
        verb: "resume_plan",
        title: "Plan an agent resume",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "What would be run to continue an agent's conversation rather than start a new one. Reports why not, when omt cannot — a wrong flag starts a fresh conversation while claiming to continue one.",
    },
}

impl CapabilityHandler<AgentResumePlan> for AgentRulesHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: ResumePlanIn,
    ) -> Result<ResumePlanOut, CapabilityError> {
        let id = crate::capabilities::session_id(&input.session)?;
        let instance = self.0.lock()?;
        if instance.session(id).is_none() {
            return Err(CapabilityError::not_found(format!(
                "no session {}",
                input.session
            )));
        }
        let Some(detected) = instance.observed_agent(id) else {
            return Ok(ResumePlanOut {
                argv: Vec::new(),
                agent_session: None,
                because: Some("omt has not identified an agent in this session".to_owned()),
            });
        };
        // The hook's answer first, then whatever the spawn environment
        // carried: a hook is the agent naming its own conversation, and the
        // environment is what omt put there when it started it.
        let agent_session = instance
            .agent_session(id)
            .map(str::to_owned)
            .or_else(|| detected.agent_session.clone());
        let Some(agent_session) = agent_session else {
            return Ok(ResumePlanOut {
                argv: Vec::new(),
                agent_session: None,
                because: Some(
                    "this agent has not said what it calls its own conversation — \
                     which arrives with its hooks, and nothing else can supply it"
                        .to_owned(),
                ),
            });
        };
        let adapters = omt_agent_adapters::builtin();
        let argv = adapters
            .get(detected.agent)
            .and_then(|adapter| adapter.resume_argv(&agent_session));
        Ok(match argv {
            Some(argv) => ResumePlanOut {
                argv,
                agent_session: Some(agent_session),
                because: None,
            },
            None => ResumePlanOut {
                argv: Vec::new(),
                agent_session: Some(agent_session),
                because: Some(format!(
                    "omt does not know how to resume {:?}",
                    detected.agent
                )),
            },
        })
    }
}

/// Input to `agent.hooks_install`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct HooksInstallIn {
    /// Which agent.
    pub agent: String,
}

/// What `agent.hooks_install` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct HooksInstallOut {
    /// The file that was written.
    pub path: String,
    /// Whether it needed writing.
    pub changed: bool,
}

capability! {
    /// Install omt's hooks into an agent's settings.
    pub struct AgentHooksInstall;
    input  = HooksInstallIn,
    output = HooksInstallOut,
    decl = Decl {
        name: "agent.hooks_install",
        group: "agent",
        verb: "hooks_install",
        title: "Install omt's hooks",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::WRITES_FS,
        // Cas: installing twice replaces omt's own entry rather than adding a
        // second one that also fires.
        intent: Some(Intent::Cas),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Without these, an agent's structured question — the tool call that offers options — never reaches omt, and a card that could have been answered on a phone is a prompt only the terminal can see.",
    },
}

impl CapabilityHandler<AgentHooksInstall> for AgentRulesHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: HooksInstallIn,
    ) -> Result<HooksInstallOut, CapabilityError> {
        let (path, plan) = hook_plan(&input.agent)?;
        if !plan.changes {
            return Ok(HooksInstallOut {
                path: path.display().to_string(),
                changed: false,
            });
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| {
                CapabilityError::internal(format!("creating the settings directory: {e}"))
            })?;
        }
        std::fs::write(&path, &plan.after)
            .map_err(|e| CapabilityError::internal(format!("writing the settings: {e}")))?;
        Ok(HooksInstallOut {
            path: path.display().to_string(),
            changed: true,
        })
    }
}

/// The plan for one agent, and where it would go.
fn hook_plan(
    agent: &str,
) -> Result<(std::path::PathBuf, omt_agent_adapters::install::Plan), CapabilityError> {
    let kind = match agent.to_lowercase().as_str() {
        "claudecode" | "claude" | "claude_code" => omt_types::AgentKind::ClaudeCode,
        other => {
            return Err(CapabilityError::invalid_input(format!(
                "omt does not know where `{other}` keeps its hooks"
            )));
        }
    };
    let path = hook_settings_for(kind).ok_or_else(|| {
        CapabilityError::precondition_failed("HOME is not set, so there is no settings file")
    })?;
    // A missing file is an empty one: the first install creates it, which is
    // the ordinary case on a machine where the agent has never been
    // configured.
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    // The hook binary beside this one, not whatever is on PATH: a PATH that
    // changes between installing and firing is a hook that silently stops
    // working.
    let command = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("omt-hook")))
        .map_or_else(|| "omt-hook".to_owned(), |p| p.display().to_string());

    omt_agent_adapters::install::plan_install(
        &path.display().to_string(),
        &existing,
        kind,
        &command,
    )
    .map(|plan| (path, plan))
    .map_err(|e| CapabilityError::precondition_failed(e.to_string()))
}
