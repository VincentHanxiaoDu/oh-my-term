//! The state capability handlers act on.
//!
//! A handler is registered as an owned value, so it can simply hold this. That
//! is worth saying plainly because it is what makes "the TUI and a phone take
//! the same path" cheap: there is no second, privileged way to reach a session,
//! because a handler and the local UI hold the same `Arc`.

use std::sync::{Arc, Mutex, MutexGuard};

use omt_catalog::CapabilityError;
use omt_daemon::Instance;

/// Shared access to the running instance.
#[derive(Clone)]
pub struct State {
    instance: Arc<Mutex<Instance>>,
    config: Arc<Mutex<Option<Arc<omt_config::Resolved>>>>,
    /// Command history and its suggestions.
    ///
    /// Held here rather than in the instance because it is a surface concern:
    /// the daemon does not need it to run a session, and putting it there would
    /// push a dependency down a layer for nothing.
    recall: Arc<Mutex<omt_recall::History>>,
    /// Installed plugins.
    ///
    /// Here and not in the daemon for a harder reason: the plugin host is L5
    /// and the daemon is L4, so the daemon *cannot* depend on it. Layering is
    /// not a style rule — `cargo xtask layering` fails the build.
    plugins: Arc<Mutex<Vec<omt_plugin_host::Installed>>>,
    /// Scheduled jobs.
    jobs: Arc<Mutex<Vec<omt_recall::Schedule>>>,
    /// What voice dictation has heard so far, per session.
    voice: Arc<Mutex<std::collections::BTreeMap<String, omt_stt::TranscriptBuffer>>>,
    /// Credentials minted for plugins.
    credentials: Arc<Mutex<omt_auth::CredentialStore>>,
    /// Speech engines this instance can use.
    stt: Arc<Mutex<omt_stt::ProviderSet>>,
    /// What each agent session has said and done.
    transcripts: Arc<Mutex<crate::transcript::Transcripts>>,
    /// What has already been reported for each session, by the agent's own
    /// ids.
    ///
    /// Two sources describe the same events — the hook as they happen, the
    /// agent's session file afterwards — and without this every tool call
    /// appears twice.
    seen: Arc<Mutex<crate::absorb::Seen>>,
    /// Where reading each agent's session file has got to.
    readings: Arc<Mutex<crate::reading::Readings>>,
    /// Where this instance is listening, once it is.
    ///
    /// Kept so a session it spawns can be told where its agent's hooks should
    /// report. The daemon is the only one that knows this — a hook has no way
    /// to work it out, and a pane started by a browser or another terminal had
    /// no socket in its environment at all, which is a hook that silently does
    /// nothing.
    socket: Arc<Mutex<Option<std::path::PathBuf>>>,
    /// Fan-outs in progress, by name.
    ///
    /// Named rather than numbered because a fan-out outlives the call that
    /// started it and somebody has to be able to ask about it later — from a
    /// different client, on a different device.
    fanouts: Arc<Mutex<std::collections::BTreeMap<String, omt_session::Fanout>>>,
}

/// Where omt keeps its configuration.
fn config_home() -> std::path::PathBuf {
    std::env::var("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::var("HOME")
                .map(|h| std::path::PathBuf::from(h).join(".config"))
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
        })
        .join("omt")
}

impl Default for State {
    fn default() -> Self {
        Self::new(Instance::new())
    }
}

impl State {
    /// Wrap an instance.
    #[must_use]
    pub fn new(instance: Instance) -> Self {
        Self {
            instance: Arc::new(Mutex::new(instance)),
            recall: Arc::new(Mutex::new(omt_recall::History::new(HISTORY_LIMIT))),
            plugins: Arc::new(Mutex::new(Vec::new())),
            jobs: Arc::new(Mutex::new(Vec::new())),
            voice: Arc::new(Mutex::new(std::collections::BTreeMap::new())),
            credentials: Arc::new(Mutex::new(omt_auth::CredentialStore::new())),
            // Populated from the environment, and empty without it. omt ships
            // no key: a provider appears only once the user has supplied one,
            // which is the whole of BYOK.
            stt: Arc::new(Mutex::new(providers_from_env())),
            socket: Arc::new(Mutex::new(None)),
            fanouts: Arc::new(Mutex::new(std::collections::BTreeMap::new())),
            transcripts: Arc::new(Mutex::new(crate::transcript::Transcripts::default())),
            seen: Arc::new(Mutex::new(crate::absorb::Seen::default())),
            readings: Arc::new(Mutex::new(crate::reading::Readings::default())),
            config: Arc::new(Mutex::new(None)),
        }
    }

    /// The resolved configuration.
    ///
    /// Loaded lazily and cached: reading four files on every `config.get`
    /// would make a settings screen that polls into a disk-bound loop.
    ///
    /// # Errors
    /// Fails if a config file exists but cannot be read or parsed.
    pub fn config(&self) -> Result<std::sync::Arc<omt_config::Resolved>, CapabilityError> {
        if let Some(cached) = self.config.lock().ok().and_then(|c| c.clone()) {
            return Ok(cached);
        }
        let home = config_home();
        // The directory omt was started in is the project layer. Passing `None`
        // here — which this did — means `<repo>/.omt/config.toml` is searched
        // for by nothing and applied by nothing: a file that arrives with a
        // `git clone`, that the documentation describes as the way a project
        // configures itself, and that silently did nothing at all.
        let workspace = std::env::current_dir().ok();
        let mut layers = omt_config::load(&home, workspace.as_deref())
            .map_err(|e| CapabilityError::internal(e.to_string()))?;
        // The environment is the runtime layer, and it outranks every file.
        // `omt_config::from_environment` existed and had no caller, so
        // `OMT_APPEARANCE__THEME=nord omt web` did nothing either.
        layers.push(omt_config::from_environment(std::env::vars()));

        // With the schema, a key that does not exist is reported rather than
        // silently ignored, and a key set from a layer that is not allowed to
        // set it is dropped *and said out loud*. A silently ignored setting is
        // indistinguishable from a broken feature.
        let resolved = std::sync::Arc::new(omt_config::merge(&layers, &omt_config::key_specs()));
        if let Ok(mut slot) = self.config.lock() {
            *slot = Some(resolved.clone());
        }
        Ok(resolved)
    }

    /// Borrow the instance.
    ///
    /// # Errors
    /// Fails only if a previous holder panicked while holding the lock. That is
    /// reported as an internal error rather than propagated as a panic: one
    /// capability handler falling over must not take the process with it.
    pub fn lock(&self) -> Result<MutexGuard<'_, Instance>, CapabilityError> {
        self.instance
            .lock()
            .map_err(|_| CapabilityError::internal("the instance lock was poisoned"))
    }
}

/// How many commands the history keeps.
///
/// Bounded because it is in memory and a long-lived instance would otherwise
/// grow without limit. Large enough that a week of ordinary work still suggests
/// what you ran on Monday.
const HISTORY_LIMIT: usize = 10_000;

impl State {
    /// Every session's transcript.
    ///
    /// # Errors
    /// Fails if another thread panicked holding the lock.
    pub fn transcripts(
        &self,
    ) -> Result<MutexGuard<'_, crate::transcript::Transcripts>, CapabilityError> {
        self.transcripts
            .lock()
            .map_err(|_| CapabilityError::internal("the transcript lock was poisoned"))
    }

    /// What has already been reported for each session.
    ///
    /// # Errors
    /// Fails if another thread panicked holding the lock.
    pub fn seen(&self) -> Result<MutexGuard<'_, crate::absorb::Seen>, CapabilityError> {
        self.seen
            .lock()
            .map_err(|_| CapabilityError::internal("the seen lock was poisoned"))
    }

    /// Where reading each session file has got to.
    ///
    /// # Errors
    /// Fails if another thread panicked holding the lock.
    pub fn readings(&self) -> Result<MutexGuard<'_, crate::reading::Readings>, CapabilityError> {
        self.readings
            .lock()
            .map_err(|_| CapabilityError::internal("the reading lock was poisoned"))
    }

    /// Say where this instance is listening.
    pub fn listening_on(&self, socket: &std::path::Path) {
        if let Ok(mut held) = self.socket.lock() {
            *held = Some(socket.to_path_buf());
        }
    }

    /// Where it is listening, if it has been told.
    #[must_use]
    pub fn socket(&self) -> Option<std::path::PathBuf> {
        self.socket.lock().ok().and_then(|held| held.clone())
    }

    /// The command history.
    ///
    /// # Errors
    /// Fails if another thread panicked holding the lock.
    pub fn recall(&self) -> Result<MutexGuard<'_, omt_recall::History>, CapabilityError> {
        self.recall
            .lock()
            .map_err(|_| CapabilityError::internal("the history lock was poisoned"))
    }

    /// The installed plugins.
    ///
    /// # Errors
    /// Fails if another thread panicked holding the lock.
    pub fn plugins(
        &self,
    ) -> Result<MutexGuard<'_, Vec<omt_plugin_host::Installed>>, CapabilityError> {
        self.plugins
            .lock()
            .map_err(|_| CapabilityError::internal("the plugin lock was poisoned"))
    }

    /// The scheduled jobs.
    ///
    /// # Errors
    /// Fails if another thread panicked holding the lock.
    pub fn jobs(&self) -> Result<MutexGuard<'_, Vec<omt_recall::Schedule>>, CapabilityError> {
        self.jobs
            .lock()
            .map_err(|_| CapabilityError::internal("the job lock was poisoned"))
    }

    /// Dictation buffers, keyed by session.
    ///
    /// # Errors
    /// Fails if another thread panicked holding the lock.
    pub fn voice(
        &self,
    ) -> Result<
        MutexGuard<'_, std::collections::BTreeMap<String, omt_stt::TranscriptBuffer>>,
        CapabilityError,
    > {
        self.voice
            .lock()
            .map_err(|_| CapabilityError::internal("the dictation lock was poisoned"))
    }
}

impl State {
    /// Mint a credential for a plugin about to be started.
    ///
    /// Held nowhere: it goes into the child's environment and is forgotten
    /// here, so a leaked state directory does not leak a plugin's authority.
    /// The role is the caller's to decide — this function does not look at the
    /// plugin, because the rule "grants decide the role, not requests" belongs
    /// with the plugin and not with the credential store.
    ///
    /// # Errors
    /// Fails if another thread panicked holding the credential lock.
    pub fn mint_plugin_token(
        &self,
        role: omt_types::Role,
        plugin: &str,
    ) -> Result<String, CapabilityError> {
        let mut store = self
            .credentials
            .lock()
            .map_err(|_| CapabilityError::internal("the credential lock was poisoned"))?;
        Ok(store
            .mint(role, &format!("plugin:{plugin}"), None, None)
            .token)
    }
}

impl State {
    /// The speech engines registered here.
    ///
    /// # Errors
    /// Fails if another thread panicked holding the lock.
    pub fn stt_providers(&self) -> Result<MutexGuard<'_, omt_stt::ProviderSet>, CapabilityError> {
        self.stt
            .lock()
            .map_err(|_| CapabilityError::internal("the speech provider lock was poisoned"))
    }
}

/// The speech engines the user's environment configures.
///
/// Read from the environment rather than from a file omt writes, because a key
/// omt persists is a key omt can leak. The user's own secret store, their
/// shell profile or their password manager puts it here; omt reads it and
/// forgets it.
fn providers_from_env() -> omt_stt::ProviderSet {
    providers_from(|name| std::env::var(name).ok())
}

/// Build the provider set from a lookup.
///
/// Takes the lookup rather than reading the environment directly so a test can
/// supply values without setting a process-wide variable — which in Rust 2024
/// is unsafe, and which this workspace refuses. The indirection is worth it for
/// that alone, and it also means a future config file feeds the same function.
#[must_use]
pub fn providers_from(look: impl Fn(&str) -> Option<String>) -> omt_stt::ProviderSet {
    let present = |names: [&str; 2]| -> Option<String> {
        names
            .iter()
            .filter_map(|n| look(n))
            .find(|v| !v.trim().is_empty())
    };
    let mut set = omt_stt::ProviderSet::new();

    if let Some(key) = present(["DEEPGRAM_API_KEY", "OMT_DEEPGRAM_KEY"]) {
        let provider = omt_stt::Deepgram::new(key);
        // An endpoint override, so a company running its own Deepgram-shaped
        // service does not have to fork omt to point at it.
        set.insert(Box::new(match present(["OMT_DEEPGRAM_ENDPOINT", ""]) {
            Some(url) => provider.at(url),
            None => provider,
        }));
    }
    if let Some(key) = present(["OPENAI_API_KEY", "OMT_OPENAI_KEY"]) {
        let provider = omt_stt::OpenAi::new(key);
        // The same override, which is also how a local whisper.cpp behind an
        // OpenAI-shaped API is used — the private option should be the easy one.
        set.insert(Box::new(match present(["OMT_OPENAI_ENDPOINT", ""]) {
            Some(url) => provider.at(url),
            None => provider,
        }));
    }
    set
}

impl State {
    /// An instance whose speech engines come from a supplied lookup.
    ///
    /// # Errors
    /// Never — the signature matches the other constructors for consistency.
    #[must_use]
    pub fn with_providers(look: impl Fn(&str) -> Option<String>) -> Self {
        let state = Self::default();
        if let Ok(mut set) = state.stt.lock() {
            *set = providers_from(look);
        }
        state
    }
}

impl State {
    /// The fan-outs in progress.
    ///
    /// # Errors
    /// Fails if another thread panicked holding the lock.
    pub fn fanouts(
        &self,
    ) -> Result<
        MutexGuard<'_, std::collections::BTreeMap<String, omt_session::Fanout>>,
        CapabilityError,
    > {
        self.fanouts
            .lock()
            .map_err(|_| CapabilityError::internal("the fan-out lock was poisoned"))
    }
}

#[cfg(test)]
mod provider_tests {
    use super::*;

    #[test]
    fn no_key_means_no_provider() {
        // The shipped state: no key, no provider, no audio leaving the machine.
        assert!(providers_from(|_| None).is_empty());
    }

    #[test]
    fn a_key_registers_the_provider_it_belongs_to() {
        let set = providers_from(|n| (n == "DEEPGRAM_API_KEY").then(|| "k".to_owned()));
        assert_eq!(set.all().len(), 1);
        assert_eq!(set.all()[0].id(), "deepgram");
    }

    #[test]
    fn an_empty_key_is_the_same_as_no_key() {
        // An exported-but-blank variable is the commonest way a shell profile
        // half-configures something, and treating it as present produces a
        // provider that fails on every request.
        assert!(providers_from(|_| Some("   ".to_owned())).is_empty());
    }

    #[test]
    fn both_providers_can_be_configured_at_once() {
        let set = providers_from(|n| {
            matches!(n, "DEEPGRAM_API_KEY" | "OPENAI_API_KEY").then(|| "k".to_owned())
        });
        assert_eq!(set.all().len(), 2);
    }
}

/// The path a workspace is identified by.
///
/// A workspace's id is a hash of its root, so two spellings of one directory
/// are two workspaces holding the same files — and on macOS the commonest
/// directory of all has two spellings, because `/tmp` is a symlink to
/// `/private/tmp`. The roster listed both, each offering its own Files and
/// Changes over identical contents, and nothing on the screen said they were
/// the same place.
///
/// The lower layers cannot do this: `omt-session` is state with no filesystem
/// under it by design, and resolving a symlink means asking the disk. So it
/// happens here, at every edge where a path first arrives.
///
/// A path that cannot be resolved — it does not exist yet, or is not readable
/// — is passed through unchanged rather than rejected. Whether a workspace may
/// be opened there is a question for the code that opens it, which can say so;
/// this only decides what to call the place.
pub fn canonical_root(path: &str) -> String {
    std::fs::canonicalize(path).map_or_else(
        |_| path.to_owned(),
        |resolved| resolved.display().to_string(),
    )
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod canonical_root_tests {
    use super::canonical_root;

    #[test]
    fn two_spellings_of_one_directory_agree() {
        // The bug as it appeared: `/tmp` and `/private/tmp` are one place on
        // macOS, and the roster showed two projects.
        let a = canonical_root("/tmp");
        let b = canonical_root(&format!("{a}/.."));
        assert_eq!(canonical_root(&a), a);
        assert_eq!(b, canonical_root(&b));
    }

    #[test]
    fn keeps_a_path_that_is_not_there() {
        // Not this function's refusal to make. The caller can say what is
        // wrong with the path; all this knows is that it cannot shorten it.
        let missing = "/no/such/directory/anywhere";
        assert_eq!(canonical_root(missing), missing);
    }

    #[test]
    fn resolves_a_relative_path_to_somewhere_absolute() {
        // A relative root would name a different directory for every process
        // that opened it.
        assert!(canonical_root(".").starts_with('/'));
    }
}
