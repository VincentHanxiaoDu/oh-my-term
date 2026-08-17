//! The keybinding files this instance is running with.
//!
//! `omt-config` finds them and `omt-input` says what they mean; this is the
//! part that holds the answer for the surfaces — the TUI's leader, copy mode's
//! flavour, and `keys.list`.
//!
//! It exists because none of that was connected: `keybindings.toml` was
//! specified in 10 §8.2, described to users in the guide, and opened by
//! nothing.

use omt_config::keys::LoadedBindings;
use omt_input::bindings::{BindingProblem, DEFAULT_LEADER};
use omt_input::resolve::{Gates, Resolver};

/// Every binding file that was found, lowest layer first.
#[derive(Debug, Clone, Default)]
pub struct Bindings {
    loaded: Vec<LoadedBindings>,
}

impl Bindings {
    /// Hold what was loaded.
    #[must_use]
    pub fn from_files(loaded: Vec<LoadedBindings>) -> Self {
        Self { loaded }
    }

    /// The leader chord, as the highest layer that set one spells it.
    ///
    /// A keybinding is a config value and obeys the config precedence: the
    /// project's file outranks the user's, exactly as `config.toml` does.
    #[must_use]
    pub fn leader(&self) -> String {
        self.loaded
            .last()
            .map_or_else(|| DEFAULT_LEADER.to_owned(), |l| l.file.leader.clone())
    }

    /// Which keymap flavour: `default`, `vim`, `emacs`, or a name.
    ///
    /// From the binding file, which is where 10 §8.2 puts it. It used to be
    /// read from the *configuration* stack, where the key does not exist — the
    /// merge answered `OMT-C101 unknown key`, so the flavour was always the
    /// default and a user who set it got an error for their trouble.
    #[must_use]
    pub fn keymap(&self) -> String {
        self.loaded
            .last()
            .map_or_else(|| "default".to_owned(), |l| l.file.keymap.clone())
    }

    /// Which files were read.
    #[must_use]
    pub fn files(&self) -> &[LoadedBindings] {
        &self.loaded
    }

    /// Everything in the files that could not be read, with the file it was in.
    #[must_use]
    pub fn problems(&self) -> Vec<(String, BindingProblem)> {
        self.loaded
            .iter()
            .flat_map(|l| {
                let path = l.path.display().to_string();
                l.problems.iter().map(move |p| (path.clone(), p.clone()))
            })
            .collect()
    }

    /// Compile the files into a resolver.
    ///
    /// The capability list is passed in because it is the registry's to know:
    /// a binding naming a capability that is not there parses, loads and does
    /// nothing, and the person who wrote it concludes their key is broken
    /// rather than their spelling.
    #[must_use]
    pub fn resolver(&self, capabilities: &[String]) -> (Resolver, Vec<BindingProblem>) {
        let files: Vec<_> = self
            .loaded
            .iter()
            .map(|l| (l.source, l.file.clone()))
            .collect();
        omt_input::resolve::compile(&files, &self.gates(), capabilities)
    }

    /// What this machine and this terminal offer a binding.
    ///
    /// The terminal capabilities are left empty deliberately: omt does not
    /// query the terminal mid-session, because a reply arriving in the same
    /// stream as somebody's typing is how escape text ends up in their shell.
    /// A binding that `requires` one is reported as not installed, which is the
    /// honest answer until a negotiated capability set exists to consult.
    fn gates(&self) -> Gates {
        Gates {
            platform: std::env::consts::OS.to_owned(),
            terminal: Vec::new(),
            modal: matches!(self.keymap().as_str(), "vim" | "emacs"),
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
    use super::Bindings;
    use omt_config::keys::LoadedBindings;
    use omt_input::bindings::{Binding, BindingFile, Source};

    fn loaded(
        source: Source,
        leader: &str,
        keymap: &str,
        bindings: Vec<Binding>,
    ) -> LoadedBindings {
        LoadedBindings {
            source,
            path: std::path::PathBuf::from("/tmp/keybindings.toml"),
            file: BindingFile {
                leader: leader.to_owned(),
                keymap: keymap.to_owned(),
                bindings,
            },
            problems: Vec::new(),
        }
    }

    #[test]
    fn no_file_at_all_is_the_documented_default() {
        let none = Bindings::default();
        assert_eq!(none.leader(), "ctrl-b");
        assert_eq!(none.keymap(), "default");
        assert_eq!(none.resolver(&[]).0.bindings().len(), 0);
    }

    #[test]
    fn the_higher_layer_names_the_leader() {
        let bindings = Bindings::from_files(vec![
            loaded(Source::User, "ctrl-a", "vim", Vec::new()),
            loaded(Source::Project, "ctrl-x", "emacs", Vec::new()),
        ]);
        assert_eq!(bindings.leader(), "ctrl-x");
        assert_eq!(bindings.keymap(), "emacs");
    }

    #[test]
    fn a_user_binding_reaches_the_resolver_with_its_source() {
        let bindings = Bindings::from_files(vec![loaded(
            Source::User,
            "ctrl-b",
            "default",
            vec![Binding::simple("<leader> c", "session.create")],
        )]);
        let (resolver, problems) = bindings.resolver(&["session.create".to_owned()]);
        assert_eq!(problems, Vec::new());
        assert_eq!(resolver.bindings().len(), 1);
        assert_eq!(resolver.bindings()[0].source, Source::User);
    }

    #[test]
    fn modes_are_allowed_exactly_when_the_flavour_is_modal() {
        let modal = Binding {
            modes: vec!["normal".to_owned()],
            ..Binding::simple("d d", "explorer.delete")
        };
        let under_default = Bindings::from_files(vec![loaded(
            Source::User,
            "ctrl-b",
            "default",
            vec![modal.clone()],
        )]);
        assert_eq!(under_default.resolver(&[]).1.len(), 1);

        let under_vim =
            Bindings::from_files(vec![loaded(Source::User, "ctrl-b", "vim", vec![modal])]);
        assert_eq!(under_vim.resolver(&[]).1, Vec::new());
    }
}
