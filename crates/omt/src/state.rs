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
