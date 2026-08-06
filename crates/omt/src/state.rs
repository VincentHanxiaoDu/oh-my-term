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
        let layers =
            omt_config::load(&home, None).map_err(|e| CapabilityError::internal(e.to_string()))?;
        // No schema yet, so every key resolves and nothing is reported unknown.
        // Stated rather than silently true: once the schema exists this is the
        // line that starts rejecting typos.
        let resolved = std::sync::Arc::new(omt_config::merge(&layers, &[]));
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
    ) -> Result<MutexGuard<'_, std::collections::BTreeMap<String, omt_stt::TranscriptBuffer>>, CapabilityError>
    {
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
