//! Finding the keybinding file, in the layers configuration already has.
//!
//! `docs/architecture/10-configuration.md` §8.2 names the file and §2.1 names
//! the layers; a keybinding is a config value and obeys the config precedence,
//! with no second rulebook. This module is only the *finding*: what the file
//! means belongs to `omt-input`, beside the chords it is about.
//!
//! It exists because nothing opened the file. It was specified, documented to
//! users and validated by a capability, and a rebind did nothing and said
//! nothing.

use std::path::{Path, PathBuf};

use omt_input::bindings::{BindingFile, BindingProblem, Source};

use crate::layer::Layer;
use crate::load::{LoadError, Located, PROJECT_DIR, find_project_config};

/// The file, beside `config.toml`.
pub const BINDINGS_FILE: &str = "keybindings.toml";

/// One file that was read, with where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedBindings {
    /// Which layer it is.
    pub source: Source,
    /// Where it was.
    pub path: PathBuf,
    /// What it says.
    pub file: BindingFile,
    /// Everything in it that could not be read.
    ///
    /// Carried rather than returned separately: a problem without its file is
    /// a sentence nobody can act on.
    pub problems: Vec<BindingProblem>,
}

/// Every place omt will look for a binding file, in precedence order.
///
/// Returned whether or not they exist, for the same reason `config.toml`'s
/// search is: "omt looked here and found nothing" is what somebody debugging a
/// key that does nothing needs to see.
#[must_use]
pub fn search_paths(config_home: &Path, workspace: Option<&Path>) -> Vec<Located> {
    let user = config_home.join(BINDINGS_FILE);
    let mut out = vec![Located {
        layer: Layer::User,
        exists: user.is_file(),
        path: user,
    }];
    if let Some(dir) = workspace {
        let project =
            find_project_bindings(dir).unwrap_or_else(|| dir.join(PROJECT_DIR).join(BINDINGS_FILE));
        out.push(Located {
            layer: Layer::Project,
            exists: project.is_file(),
            path: project,
        });
    }
    out
}

/// The project's binding file, walking up exactly as the config search does.
///
/// Sharing the walk rather than repeating it: two searches that agree today
/// and are maintained separately are two searches that disagree later, and the
/// symptom would be a keymap that changes with the directory somebody started
/// omt in.
#[must_use]
pub fn find_project_bindings(start: &Path) -> Option<PathBuf> {
    let config = find_project_config(start)?;
    let candidate = config.with_file_name(BINDINGS_FILE);
    candidate.is_file().then_some(candidate)
}

/// Read every binding file that exists, lowest layer first.
///
/// # Errors
/// Refuses a file that exists and is not valid TOML, rather than starting with
/// a keyboard silently different from what the file says. A file with one bad
/// *line* is not this: that installs the rest and reports the line.
pub fn load(
    config_home: &Path,
    workspace: Option<&Path>,
) -> Result<Vec<LoadedBindings>, LoadError> {
    let mut out = Vec::new();
    for located in search_paths(config_home, workspace) {
        if !located.exists {
            continue;
        }
        let text = std::fs::read_to_string(&located.path).map_err(|e| LoadError::Unreadable {
            path: located.path.display().to_string(),
            detail: e.to_string(),
        })?;
        let (file, problems) =
            omt_input::bindings::parse_text(&text).map_err(|detail| LoadError::Malformed {
                path: located.path.display().to_string(),
                detail,
            })?;
        out.push(LoadedBindings {
            source: source_of(located.layer),
            path: located.path,
            file,
            problems,
        });
    }
    Ok(out)
}

/// The binding layer a config layer is.
///
/// One mapping in one place: `omt-input` orders bindings by source and
/// `omt-config` owns the layers, and the two must not each hold an opinion.
const fn source_of(layer: Layer) -> Source {
    match layer {
        Layer::Builtin => Source::Builtin,
        Layer::User => Source::User,
        Layer::Project => Source::Project,
        Layer::Instance => Source::Instance,
        Layer::Session => Source::Session,
        Layer::Runtime => Source::Runtime,
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::{BINDINGS_FILE, load, search_paths};
    use omt_input::bindings::Source;

    #[test]
    fn the_user_file_is_read_from_beside_the_config() {
        let home = tempfile::tempdir().expect("a temp dir");
        std::fs::write(
            home.path().join(BINDINGS_FILE),
            "leader = \"ctrl-a\"\n\"<leader> c\" = \"session.create\"\n",
        )
        .expect("the fixture writes");
        let loaded = load(home.path(), None).expect("it loads");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].source, Source::User);
        assert_eq!(loaded[0].file.leader, "ctrl-a");
        assert_eq!(loaded[0].file.bindings.len(), 1);
        assert_eq!(loaded[0].problems, Vec::new());
    }

    #[test]
    fn a_missing_file_contributes_nothing_and_is_not_an_error() {
        let home = tempfile::tempdir().expect("a temp dir");
        assert_eq!(load(home.path(), None).expect("it loads"), Vec::new());
        // And the search still reports where it looked.
        let paths = search_paths(home.path(), None);
        assert_eq!(paths.len(), 1);
        assert!(!paths[0].exists);
    }

    #[test]
    fn a_file_that_is_not_toml_refuses_rather_than_starting_different() {
        let home = tempfile::tempdir().expect("a temp dir");
        std::fs::write(home.path().join(BINDINGS_FILE), "leader = \n").expect("writes");
        let err = load(home.path(), None).expect_err("it refuses");
        assert!(format!("{err}").contains("not valid TOML"), "{err}");
    }

    #[test]
    fn a_project_file_is_found_beside_the_project_config() {
        let home = tempfile::tempdir().expect("a temp dir");
        let project = tempfile::tempdir().expect("a temp dir");
        std::fs::create_dir_all(project.path().join(".omt")).expect("the dir");
        std::fs::write(project.path().join(".omt").join("config.toml"), "").expect("writes");
        std::fs::write(
            project.path().join(".omt").join(BINDINGS_FILE),
            "\"ctrl-g\" = \"palette.open\"\n",
        )
        .expect("writes");
        let loaded = load(home.path(), Some(project.path())).expect("it loads");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].source, Source::Project);
    }

    #[test]
    fn one_bad_line_is_carried_with_its_file_rather_than_refusing_it() {
        let home = tempfile::tempdir().expect("a temp dir");
        std::fs::write(
            home.path().join(BINDINGS_FILE),
            "\"ctrl-g\" = \"palette.open\"\n\"nonsensekey\" = 7\n",
        )
        .expect("writes");
        let loaded = load(home.path(), None).expect("it loads");
        assert_eq!(loaded[0].file.bindings.len(), 1);
        assert_eq!(loaded[0].problems.len(), 1);
    }
}
