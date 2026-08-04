//! The six layers, and what merging them means.

use serde::{Deserialize, Serialize};

/// Where a setting came from.
///
/// Ordered weakest to strongest; later wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layer {
    /// Compiled-in defaults.
    Builtin,
    /// `~/.config/omt/config.toml`.
    User,
    /// `<repo>/.omt/config.toml`. Arrives with a `git clone`, so it is
    /// untrusted input and restricted accordingly.
    Project,
    /// A per-instance block in `instances.toml`.
    Instance,
    /// Overrides carried in a session record.
    Session,
    /// In memory: an env var or an explicit ephemeral set. Never persisted.
    ///
    /// Above `Session` deliberately. It looks surprising until you need it: an
    /// operator saying "for this process, right now" has to beat stored state,
    /// or `OMT_SERVER__BIND=127.0.0.1:0 omt` is not a reliable escape hatch.
    Runtime,
}

impl Layer {
    /// Every layer, weakest first.
    pub const ALL: &'static [Self] = &[
        Self::Builtin,
        Self::User,
        Self::Project,
        Self::Instance,
        Self::Session,
        Self::Runtime,
    ];

    /// Whether a value in this layer can be written back to disk.
    #[must_use]
    pub const fn is_persistable(self) -> bool {
        !matches!(self, Self::Builtin | Self::Runtime)
    }
}

/// How far up the stack a setting may legitimately be set.
///
/// The reason this exists: a project config arrives with a `git clone`. A
/// repository that could set `server.bind` would be a repository that opens a
/// port on the machine of anyone who checks it out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Settable anywhere.
    Any,
    /// Ignored, with a diagnostic, if it appears in a project file.
    UserOrAbove,
    /// Only `instances.toml` or runtime — never in a shared `config.toml`,
    /// because it describes this machine rather than this user's preferences.
    DeviceLocal,
}

impl Scope {
    /// Whether a key with this scope may be honoured from a layer.
    #[must_use]
    pub const fn permits(self, layer: Layer) -> bool {
        match self {
            Self::Any => true,
            Self::UserOrAbove => !matches!(layer, Layer::Project),
            Self::DeviceLocal => matches!(layer, Layer::Instance | Layer::Runtime),
        }
    }
}

/// How arrays combine.
///
/// Replacement is the default because it is the only rule anyone predicts. Two
/// keys append, and the schema test asserts no third one gains that behaviour
/// without being documented — an array that silently accumulated across four
/// layers would be impossible to reason about from any one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ArrayMerge {
    /// The higher layer's array wins outright.
    #[default]
    Replace,
    /// The higher layer's entries are appended to the lower's.
    Append,
}

/// The keys whose arrays append rather than replace.
///
/// Exhaustive and deliberately tiny.
pub const APPENDING_ARRAYS: &[&str] = &["plugins.search_paths", "terminal.env_passthrough"];

/// The reserved value that restores whatever the layer below said.
///
/// How a project config drops a user-level keybinding without knowing what it
/// was. A plain absence cannot express this — absence means "no opinion", and
/// "no opinion" is exactly what inherits.
pub const UNSET: &str = "@unset";

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    #[test]
    fn runtime_beats_everything_including_session() {
        // The escape hatch: an operator saying "for this process, now" has to
        // win, or an env var cannot be relied on to unstick a broken instance.
        assert!(Layer::Runtime > Layer::Session);
        assert!(Layer::Runtime > Layer::User);
        assert_eq!(Layer::ALL.last(), Some(&Layer::Runtime));
        assert_eq!(Layer::ALL.first(), Some(&Layer::Builtin));
    }

    #[test]
    fn the_layers_are_ordered_weakest_first() {
        let mut sorted = Layer::ALL.to_vec();
        sorted.sort_unstable();
        assert_eq!(sorted, Layer::ALL);
    }

    #[test]
    fn defaults_and_runtime_values_are_never_written_to_disk() {
        // Persisting a builtin would freeze a default that should follow the
        // binary; persisting a runtime value would make an escape hatch
        // permanent by accident.
        assert!(!Layer::Builtin.is_persistable());
        assert!(!Layer::Runtime.is_persistable());
        assert!(Layer::User.is_persistable());
        assert!(Layer::Project.is_persistable());
    }

    #[test]
    fn a_project_file_may_not_set_a_user_or_above_key() {
        // It arrived with a git clone. A repository that could set server.bind
        // would open a port on every machine that checked it out.
        assert!(!Scope::UserOrAbove.permits(Layer::Project));
        assert!(Scope::UserOrAbove.permits(Layer::User));
        assert!(Scope::UserOrAbove.permits(Layer::Runtime));
    }

    #[test]
    fn a_device_local_key_is_confined_to_this_machine() {
        // A font family in a shared config would follow the user onto a machine
        // that does not have the font.
        assert!(Scope::DeviceLocal.permits(Layer::Instance));
        assert!(Scope::DeviceLocal.permits(Layer::Runtime));
        assert!(!Scope::DeviceLocal.permits(Layer::User));
        assert!(!Scope::DeviceLocal.permits(Layer::Project));
    }

    #[test]
    fn an_unrestricted_key_is_settable_anywhere() {
        for layer in Layer::ALL {
            assert!(Scope::Any.permits(*layer), "{layer:?}");
        }
    }

    #[test]
    fn only_two_keys_append_their_arrays() {
        // Replacement is the only rule anyone predicts. This test is the gate:
        // a third appending array has to be added here, which is where somebody
        // has to justify it.
        assert_eq!(APPENDING_ARRAYS.len(), 2);
        assert!(APPENDING_ARRAYS.contains(&"plugins.search_paths"));
        assert!(APPENDING_ARRAYS.contains(&"terminal.env_passthrough"));
    }

    #[test]
    fn arrays_replace_by_default() {
        assert_eq!(ArrayMerge::default(), ArrayMerge::Replace);
    }
}
