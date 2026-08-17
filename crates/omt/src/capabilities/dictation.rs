//! Dictation, Theme, Links, Configuration, Shell integration, Usage.
//!
//! Split out of one 6454-line module, which P1's own file-length rule
//! calls a design smell and which nothing measured until the check that
//! enforces it was written.

use super::*;

// ---------------------------------------------------------------------------
// Dictation
// ---------------------------------------------------------------------------

/// Input to `voice.providers`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct VoiceProvidersIn {}

/// One speech engine this instance could use.
#[derive(Serialize, schemars::JsonSchema)]
pub struct VoiceProvider {
    /// Its id, which is what configuration names.
    pub id: String,
    /// What to call it in a settings screen.
    pub label: String,
    /// Whether it needs a credential at all — a local engine does not, and
    /// demanding one would make the private option look like the hard one.
    pub needs_key: bool,
    /// Whether a key is configured.
    ///
    /// Presence, never the value. A capability that returned a key would put
    /// it in a log the first time somebody debugged a client.
    pub key_present: bool,
}

/// What `voice.providers` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct VoiceProvidersOut {
    /// Every provider registered here.
    pub providers: Vec<VoiceProvider>,
    /// Whether dictation can currently do anything.
    ///
    /// False on a fresh install, and that is the shipped state: no key, no
    /// provider, and no audio leaving the machine until the user says where it
    /// goes.
    pub usable: bool,
}

capability! {
    /// Which speech engines are available.
    pub struct VoiceProviders;
    input  = VoiceProvidersIn,
    output = VoiceProvidersOut,
    decl = Decl {
        name: "voice.providers",
        group: "voice",
        verb: "providers",
        title: "List speech engines",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Speech-to-text engines this instance can use, and whether each has a key. Presence only — a capability that returned a key would put it in the first log somebody captured.",
    },
}

pub(crate) struct VoiceProvidersHandler(pub(crate) State);

impl CapabilityHandler<VoiceProviders> for VoiceProvidersHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        _input: VoiceProvidersIn,
    ) -> Result<VoiceProvidersOut, CapabilityError> {
        let set = self.0.stt_providers()?;
        let providers: Vec<VoiceProvider> = set
            .all()
            .into_iter()
            .map(|p| VoiceProvider {
                id: p.id().to_owned(),
                label: p.label().to_owned(),
                needs_key: p.needs_key(),
                // A provider is only registered once its key was found, so a
                // registered one that needs a key has it. Reported rather than
                // inferred by the client, and never the value itself.
                key_present: true,
            })
            .collect();
        Ok(VoiceProvidersOut {
            usable: providers.iter().any(|p| p.key_present),
            providers,
        })
    }
}

/// Input to `voice.append`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct VoiceAppendIn {
    /// Which session is being dictated into.
    pub session: String,
    /// What was heard.
    pub text: String,
    /// Whether this is settled or still being revised.
    ///
    /// The distinction is the whole design: a partial result is shown and then
    /// replaced, and treating one as final puts a half-heard word into somebody's
    /// command line permanently.
    #[serde(default)]
    pub final_chunk: bool,
}

/// What `voice.append` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct VoiceAppendOut {
    /// Everything heard, settled and unsettled, for display.
    pub display: String,
    /// Only the settled part, which is what may be sent to a session.
    pub committed: String,
}

capability! {
    /// Add a piece of dictation.
    pub struct VoiceAppend;
    input  = VoiceAppendIn,
    output = VoiceAppendOut,
    decl = Decl {
        name: "voice.append",
        group: "voice",
        verb: "append",
        title: "Add dictated text",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::empty(),
        // Raw stream: dictation is never replayed. Re-sending a chunk after a
        // reconnect would duplicate a word in the middle of a sentence, and
        // the buffer's own revision handles the ordinary case.
        intent: Some(Intent::RawStream),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Append a transcript chunk to a session's dictation buffer. Partial chunks are replaced by later ones; only the settled part is offered for sending.",
    },
}

pub(crate) struct VoiceAppendHandler(pub(crate) State);

impl CapabilityHandler<VoiceAppend> for VoiceAppendHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: VoiceAppendIn,
    ) -> Result<VoiceAppendOut, CapabilityError> {
        // Validated even though the buffer is keyed by string: accepting a
        // malformed id would silently create a buffer nothing ever reads.
        let session = session_id(&input.session)?;
        let mut voice = self.0.voice()?;
        let buffer = voice.entry(session.to_wire()).or_default();
        buffer.apply(&omt_stt::Transcript {
            text: input.text,
            is_final: input.final_chunk,
            // Not carried over the wire: a client's confidence number means
            // nothing without knowing which engine produced it, and omt has no
            // decision that reads it.
            confidence: None,
        });
        Ok(VoiceAppendOut {
            display: buffer.display(),
            committed: buffer.committed().to_owned(),
        })
    }
}

/// Input to `voice.clear`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct VoiceClearIn {
    /// Which session's buffer.
    pub session: String,
}

/// What `voice.clear` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct VoiceClearOut {
    /// Whether there had been anything to clear.
    pub had_text: bool,
}

capability! {
    /// Throw away what was dictated.
    pub struct VoiceClear;
    input  = VoiceClearIn,
    output = VoiceClearOut,
    decl = Decl {
        name: "voice.clear",
        group: "voice",
        verb: "clear",
        title: "Clear dictation",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::empty(),
        intent: Some(Intent::Cas),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Discard a session's dictation buffer. Idempotent.",
    },
}

pub(crate) struct VoiceClearHandler(pub(crate) State);

impl CapabilityHandler<VoiceClear> for VoiceClearHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: VoiceClearIn,
    ) -> Result<VoiceClearOut, CapabilityError> {
        let session = session_id(&input.session)?;
        let mut voice = self.0.voice()?;
        let had_text = voice.get(&session.to_wire()).is_some_and(|b| !b.is_empty());
        voice.remove(&session.to_wire());
        Ok(VoiceClearOut { had_text })
    }
}

// ---------------------------------------------------------------------------
// Theme
// ---------------------------------------------------------------------------

/// Input to `theme.get`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct ThemeGetIn {}

/// A theme as a client needs it.
#[derive(Serialize, schemars::JsonSchema)]
pub struct ThemeOut {
    /// What it is called.
    pub name: String,
    /// Whether it is meant for a light or a dark terminal.
    ///
    /// Stated by the theme rather than guessed from the background, because a
    /// borderline theme guessed wrong propagates into every contrast decision
    /// a client then makes.
    pub appearance: String,
    /// Default text, as `#rrggbb`.
    pub foreground: String,
    /// The background.
    pub background: String,
    /// The cursor.
    pub cursor: String,
    /// Selected text.
    pub selection: String,
    /// The sixteen ANSI colours in their conventional order, 0–15.
    ///
    /// Sent as one flat list because that is the order every terminal and every
    /// client indexes them by; splitting normal from bright would make every
    /// consumer reassemble it.
    pub ansi: Vec<String>,
}

capability! {
    /// The theme in force.
    pub struct ThemeGet;
    input  = ThemeGetIn,
    output = ThemeOut,
    decl = Decl {
        name: "theme.get",
        group: "theme",
        verb: "get",
        title: "Read the theme",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "The colours in force, so a remote client renders a session in the user's own theme rather than in its own defaults.",
    },
}

pub(crate) struct ThemeGetHandler(pub(crate) State);

impl CapabilityHandler<ThemeGet> for ThemeGetHandler {
    fn call(&self, _ctx: &CallContext, _input: ThemeGetIn) -> Result<ThemeOut, CapabilityError> {
        let _ = &self.0;
        let theme = omt_theme::Theme::builtin_dark();
        Ok(ThemeOut {
            name: theme.name,
            appearance: match theme.appearance {
                omt_theme::Appearance::Dark => "dark".to_owned(),
                omt_theme::Appearance::Light => "light".to_owned(),
            },
            foreground: theme.foreground.to_hex(),
            background: theme.background.to_hex(),
            cursor: theme.cursor.to_hex(),
            selection: theme.selection.to_hex(),
            ansi: theme
                .palette
                .normal
                .iter()
                .chain(theme.palette.bright.iter())
                .map(|c| c.to_hex())
                .collect(),
        })
    }
}

// ---------------------------------------------------------------------------
// Links
// ---------------------------------------------------------------------------

/// Input to `open.recognize`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct OpenRecognizeIn {
    /// One line of terminal output.
    pub line: String,
}

/// Something on a line that can be opened.
#[derive(Serialize, schemars::JsonSchema)]
pub struct OpenMatch {
    /// What kind of thing it is.
    pub kind: String,
    /// The thing itself.
    pub target: String,
    /// Where it starts in the line, in bytes.
    pub start: usize,
    /// Where it ends.
    pub end: usize,
    /// The line number, for a path that carried one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_number: Option<u32>,
}

/// What `open.recognize` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct OpenRecognizeOut {
    /// Everything found, in the order it appears.
    pub matches: Vec<OpenMatch>,
}

capability! {
    /// Find the openable things on a line.
    pub struct OpenRecognize;
    input  = OpenRecognizeIn,
    output = OpenRecognizeOut,
    decl = Decl {
        name: "open.recognize",
        group: "open",
        verb: "recognize",
        title: "Find links and paths",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Paths, URLs and file:line references on a line of output, with their offsets — so a client can make them tappable without inventing its own pattern.",
    },
}

pub(crate) struct OpenRecognizeHandler(pub(crate) State);

impl CapabilityHandler<OpenRecognize> for OpenRecognizeHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: OpenRecognizeIn,
    ) -> Result<OpenRecognizeOut, CapabilityError> {
        let _ = &self.0;
        Ok(OpenRecognizeOut {
            matches: omt_open::recognize(&input.line)
                .into_iter()
                .map(|m| {
                    let (kind, target, line_number) = describe_target(&m.target);
                    OpenMatch {
                        kind,
                        target,
                        start: m.start,
                        end: m.end,
                        line_number,
                    }
                })
                .collect(),
        })
    }
}

pub(crate) fn describe_target(target: &omt_open::Target) -> (String, String, Option<u32>) {
    match target {
        omt_open::Target::Path { raw, line, .. } => ("path".to_owned(), raw.clone(), *line),
        omt_open::Target::Url { raw, .. } => ("url".to_owned(), raw.clone(), None),
    }
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Shell integration
// ---------------------------------------------------------------------------

/// Input to `shell.integration`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct ShellIntegrationIn {
    /// Which session to judge by. Absent asks about the shell omt was started
    /// from, which is what a fresh client with no session wants to know.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

/// What `shell.integration` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct ShellIntegrationOut {
    /// The shell, as far as omt can tell.
    pub shell: String,
    /// Whether marks have actually been seen.
    ///
    /// Observed rather than configured: a block with a command attributed to it
    /// is proof that OSC 133 arrived. A setting could be wrong; this cannot.
    pub active: bool,
    /// How many blocks have been seen at all.
    ///
    /// Reported so a caller can tell "no integration" from "nothing has
    /// happened yet" — a brand-new session has seen nothing either way, and
    /// telling somebody to install a snippet they already have is worse than
    /// saying nothing.
    pub blocks_seen: u32,
    /// The line to add, and where.
    ///
    /// Absent when integration is already active, and absent for a shell omt
    /// has no snippet for — returning bash's line for fish puts an error in
    /// somebody's terminal on every login.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<ShellSnippet>,
}

/// What to add to a shell's configuration.
#[derive(Serialize, schemars::JsonSchema)]
pub struct ShellSnippet {
    /// The file it belongs in.
    pub file: String,
    /// The line itself.
    pub line: String,
}

capability! {
    /// Whether this shell marks its commands.
    pub struct ShellIntegration;
    input  = ShellIntegrationIn,
    output = ShellIntegrationOut,
    decl = Decl {
        name: "shell.integration",
        group: "shell",
        verb: "integration",
        title: "Shell integration status",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Whether the shell emits command marks, judged by what has been seen rather than by a setting, with the line that enables them. omt installs that line once, inside a marked block, when `shell.auto_install` is on; it owns that block and nothing else in the file.",
    },
}

pub(crate) struct ShellIntegrationHandler(pub(crate) State);

impl CapabilityHandler<ShellIntegration> for ShellIntegrationHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: ShellIntegrationIn,
    ) -> Result<ShellIntegrationOut, CapabilityError> {
        let shell = current_shell();
        let instance = self.0.lock()?;

        // Judged over the named session, or over every session when none was
        // named: a client asking in general wants to know whether *anything*
        // here marks its commands.
        let blocks: Vec<&omt_term::Block> = match input.session.as_deref() {
            Some(raw) => {
                let id = session_id(raw)?;
                instance
                    .runtime(id)
                    .map(omt_daemon::SessionRuntime::terminal)
                    .or_else(|| instance.orphan_terminal(id))
                    .ok_or_else(|| CapabilityError::not_found("no such session"))?
                    .blocks()
                    .iter()
                    .collect()
            }
            None => instance
                .sessions()
                .iter()
                .filter_map(|s| instance.runtime(s.id))
                .flat_map(|r| r.terminal().blocks().iter())
                .collect(),
        };

        let active = blocks.iter().any(|b| b.attributed);
        Ok(ShellIntegrationOut {
            snippet: if active { None } else { snippet_for(&shell) },
            shell,
            active,
            blocks_seen: blocks.len() as u32,
        })
    }
}

/// The shell omt was started from.
pub(crate) fn current_shell() -> String {
    std::env::var("SHELL")
        .ok()
        .and_then(|p| {
            std::path::Path::new(&p)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "unknown".to_owned())
}

/// The line that turns on command marks, for a shell omt knows.
///
/// `None` for anything else, on purpose. A snippet for the wrong shell puts an
/// error in somebody's terminal on every login, which is worse than no snippet.
pub(crate) fn snippet_for(shell: &str) -> Option<ShellSnippet> {
    // One line each, self-contained, and emitting the marks the block tracker
    // reads. One line rather than a script, because it is a line somebody can
    // read in their own rc file afterwards and understand — omt installs it
    // inside a marked block, and a block containing a script would be a thing
    // nobody can check.
    let (file, line) = match shell {
        "bash" => (
            "~/.bashrc",
            r#"PROMPT_COMMAND='printf "\033]133;D;$?\007\033]133;A\007"'"#,
        ),
        "zsh" => (
            "~/.zshrc",
            r#"precmd() { print -Pn "\e]133;D;$?\a\e]133;A\a" }"#,
        ),
        "fish" => (
            "~/.config/fish/config.fish",
            r#"function omt_prompt --on-event fish_prompt; printf "\033]133;D;$status\007\033]133;A\007"; end"#,
        ),
        _ => return None,
    };
    Some(ShellSnippet {
        file: file.to_owned(),
        line: line.to_owned(),
    })
}

// ---------------------------------------------------------------------------
// Usage
// ---------------------------------------------------------------------------

/// Input to `usage.report`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct UsageReportIn {
    /// One session, or absent for the instance as a whole.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

/// What was spent.
#[derive(Serialize, schemars::JsonSchema)]
pub struct Spend {
    /// Input tokens.
    pub input: u64,
    /// Output tokens.
    pub output: u64,
    /// Tokens read from cache.
    pub cache_read: u64,
    /// Tokens written to cache.
    pub cache_write: u64,
    /// Cost, **only** when the agent itself stated one.
    ///
    /// Never computed. A price table in omt goes stale the week any provider
    /// changes pricing, and the estimate would sit beside counts that are
    /// right with nothing to tell them apart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
}

/// What `usage.report` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct UsageReportOut {
    /// This session's spend, when one was named.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<Spend>,
    /// Every session's, together.
    pub total: Spend,
    /// `unknown` or `reported`.
    ///
    /// `unknown` is the common case and must stay distinct from a low number:
    /// a surface that drew silence as an empty bar would invent the one figure
    /// a user acts on.
    pub headroom: String,
    /// What the agent said about its limit, when it said anything.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_status: Option<String>,
    /// When the agent said the limit resets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<String>,
    /// How many sessions have reported anything at all.
    pub sessions_reporting: u32,
}

capability! {
    /// What has been spent, and what is left.
    pub struct UsageReport;
    input  = UsageReportIn,
    output = UsageReportOut,
    decl = Decl {
        name: "usage.report",
        group: "usage",
        verb: "report",
        title: "Usage and quota",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Tokens spent per session and in total, with whatever the agent said about its rate limit. Cost appears only when the agent stated one — omt never computes it from a price table of its own.",
    },
}

pub(crate) struct UsageReportHandler(pub(crate) State);

impl CapabilityHandler<UsageReport> for UsageReportHandler {
    fn call(
        &self,
        _ctx: &CallContext,
        input: UsageReportIn,
    ) -> Result<UsageReportOut, CapabilityError> {
        let instance = self.0.lock()?;
        let ledger = &instance.usage;

        let (session_spend, headroom) = match input.session.as_deref() {
            Some(raw) => {
                let id = session_id(raw)?;
                (Some(spend(&ledger.session(id))), ledger.headroom(id))
            }
            // Without a session there is no limit to report: a rate limit
            // belongs to an account as an agent described it, and merging two
            // agents' limits into one number would be a figure neither said.
            None => (None, omt_agent::Headroom::Unknown),
        };

        let (headroom_kind, status, resets) = match headroom {
            omt_agent::Headroom::Unknown => ("unknown".to_owned(), None, None),
            omt_agent::Headroom::Reported(limit) => (
                "reported".to_owned(),
                Some(limit.status),
                limit.resets_at.map(|t| t.to_string()),
            ),
        };

        Ok(UsageReportOut {
            session: session_spend,
            total: spend(&ledger.total()),
            headroom: headroom_kind,
            limit_status: status,
            resets_at: resets,
            sessions_reporting: ledger.len() as u32,
        })
    }
}

/// Turn the ledger's own shape into the wire's.
pub(crate) fn spend(usage: &omt_agent::Usage) -> Spend {
    Spend {
        input: usage.input,
        output: usage.output,
        cache_read: usage.cache_read,
        cache_write: usage.cache_write,
        cost_usd: usage.cost_usd,
    }
}
