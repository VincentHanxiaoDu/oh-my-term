//! Adding omt's command-mark snippet to a shell's configuration.
//!
//! `shell.auto_install` has been `true` in `10-configuration.md` §7.4 since it
//! was written, and nothing installed anything: a default installation was
//! promised command marks and got the heuristic fallback. That is a slightly
//! worse experience rather than an error, which is why it survived — the
//! blocks look nearly right, so nobody reports it.
//!
//! Two rules run through this file, and both are about somebody else's file:
//!
//! - **omt owns a marked block and nothing else.** A file with the marker is
//!   left exactly as it is; a file without one gains the block at the end.
//!   Nothing else is rewritten, reordered or removed, ever.
//! - **It says what it did.** A line added to a shell's configuration in
//!   silence is indistinguishable from a shell that behaved differently today.

use std::path::{Path, PathBuf};

/// What marks the block as omt's.
///
/// A comment rather than a checksum: somebody reading their own rc file has to
/// be able to tell at a glance what put this here and how to remove it.
pub(crate) const MARKER: &str = "# omt: command marks (OSC 133)";

/// The end of it, so a later version can replace the block rather than append
/// a second copy beside it.
pub(crate) const END_MARKER: &str = "# omt: end";

/// What happened, in words a caller can print.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Installed {
    /// The block was added to this file.
    Added(PathBuf),
    /// The file already carried omt's marker.
    AlreadyThere(PathBuf),
    /// Nothing was done, and this is why.
    ///
    /// A reason rather than a bare `None`: "omt did not install anything" and
    /// "omt could not" are different answers, and only one of them is worth
    /// telling somebody about.
    Skipped(&'static str),
}

/// Whether the configuration asks for the snippet to be installed.
///
/// Absent is on, because §7.4's default is `true`. A user who has never
/// thought about it gets working blocks, which is the point of a default.
pub(crate) fn wanted(state: &crate::state::State) -> bool {
    !crate::terminal_bits::setting(state, "shell.auto_install").contains("false")
        && crate::terminal_bits::emits_marks(state)
}

/// Whether this shell is one the configuration named.
///
/// An empty `shell.shells` means every shell omt has a snippet for — the
/// setting's own default is "auto-detected", and a list nobody wrote is not a
/// list of none.
pub(crate) fn named(state: &crate::state::State, shell: &str) -> bool {
    let configured = crate::terminal_bits::setting(state, "shell.shells");
    let listed: Vec<String> = configured
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();
    listed.is_empty() || listed.iter().any(|s| s == shell)
}

/// Install the snippet for a shell, if it is wanted and possible.
pub(crate) fn install(state: &crate::state::State, shell_path: &str, home: &Path) -> Installed {
    if !wanted(state) {
        return Installed::Skipped("shell.auto_install is off");
    }
    let shell = shell_name(shell_path);
    if !named(state, &shell) {
        return Installed::Skipped("this shell is not in shell.shells");
    }
    let Some(snippet) = crate::capabilities::snippet_for(&shell) else {
        // Deliberately not another shell's line: bash's snippet in a fish
        // configuration is an error on every login, printed by something the
        // user never asked to run.
        return Installed::Skipped("omt has no snippet for this shell");
    };
    let Some(file) = resolve(&snippet.file, home) else {
        return Installed::Skipped("the shell's configuration file is not under $HOME");
    };
    install_into(&file, &snippet.line)
}

/// The same, into a named file.
///
/// Separated from the lookup so the behaviour that touches somebody's file can
/// be tested exactly, without a configuration or a shell anywhere near it.
pub(crate) fn install_into(file: &Path, line: &str) -> Installed {
    let existing = std::fs::read_to_string(file).unwrap_or_default();
    if existing.contains(MARKER) {
        return Installed::AlreadyThere(file.to_path_buf());
    }
    if let Some(parent) = file.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return Installed::Skipped("the shell's configuration directory cannot be created");
    }
    // Appended, with a blank line in front when the file does not end in one.
    // A block welded onto the end of somebody's last line is a syntax error in
    // their shell, caused by omt, on their next login.
    let mut out = existing.clone();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(MARKER);
    out.push('\n');
    out.push_str(line);
    out.push('\n');
    out.push_str(END_MARKER);
    out.push('\n');
    if std::fs::write(file, out).is_err() {
        return Installed::Skipped("the shell's configuration file cannot be written");
    }
    Installed::Added(file.to_path_buf())
}

/// The shell's own name, from whatever path it was started as.
pub(crate) fn shell_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// A `~`-relative path against a named home.
///
/// Only under that home: a snippet path that escaped it would have omt writing
/// wherever the string pointed, which is not a thing a terminal should be able
/// to do because a setting said so.
fn resolve(spec: &str, home: &Path) -> Option<PathBuf> {
    let rest = spec.strip_prefix("~/")?;
    if rest.contains("..") {
        return None;
    }
    Some(home.join(rest))
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::{END_MARKER, Installed, MARKER, install_into, resolve, shell_name};

    #[test]
    fn the_snippet_is_added_once_and_only_once() {
        // Twice must be once. An installer that appends every time turns a
        // shell's configuration into a list of its own history.
        let dir = tempfile::tempdir().expect("a temp dir");
        let rc = dir.path().join(".zshrc");
        std::fs::write(&rc, "export EDITOR=vi\n").expect("the fixture writes");

        assert_eq!(
            install_into(&rc, "precmd() { : }"),
            Installed::Added(rc.clone())
        );
        let after = std::fs::read_to_string(&rc).expect("it reads");
        assert!(after.starts_with("export EDITOR=vi\n"), "{after:?}");
        assert!(
            after.contains(MARKER) && after.contains(END_MARKER),
            "{after:?}"
        );

        assert_eq!(
            install_into(&rc, "precmd() { : }"),
            Installed::AlreadyThere(rc.clone())
        );
        assert_eq!(
            std::fs::read_to_string(&rc).expect("it reads"),
            after,
            "the second run changed the file"
        );
    }

    #[test]
    fn what_was_already_in_the_file_is_untouched() {
        // omt owns its marked block and nothing else in somebody's file.
        let dir = tempfile::tempdir().expect("a temp dir");
        let rc = dir.path().join(".bashrc");
        let theirs = "alias ll='ls -l'\nPS1='> '\n";
        std::fs::write(&rc, theirs).expect("writes");
        install_into(&rc, "PROMPT_COMMAND=:");
        let after = std::fs::read_to_string(&rc).expect("reads");
        assert!(after.starts_with(theirs), "{after:?}");
    }

    #[test]
    fn a_file_that_does_not_end_in_a_newline_still_gets_a_block_of_its_own() {
        // Welded onto the end of their last line, this is a syntax error in
        // their shell, caused by omt, on their next login.
        let dir = tempfile::tempdir().expect("a temp dir");
        let rc = dir.path().join(".bashrc");
        std::fs::write(&rc, "PS1='> '").expect("writes");
        install_into(&rc, "PROMPT_COMMAND=:");
        let after = std::fs::read_to_string(&rc).expect("reads");
        assert!(after.contains("PS1='> '\n\n#"), "{after:?}");
    }

    #[test]
    fn a_missing_file_is_created_with_only_the_block_in_it() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let rc = dir.path().join("fish").join("config.fish");
        assert!(matches!(
            install_into(&rc, "function omt_prompt; end"),
            Installed::Added(_)
        ));
        let after = std::fs::read_to_string(&rc).expect("reads");
        assert!(after.starts_with(MARKER), "{after:?}");
    }

    #[test]
    fn a_path_is_only_ever_under_the_named_home() {
        let home = std::path::Path::new("/tmp/home");
        assert_eq!(resolve("~/.zshrc", home), Some(home.join(".zshrc")));
        // Not an escape, and not an absolute path from somewhere else: a
        // terminal that writes wherever a string points because a setting said
        // so is a terminal nobody should run.
        assert_eq!(resolve("~/../../etc/profile", home), None);
        assert_eq!(resolve("/etc/profile", home), None);
    }

    #[test]
    fn a_shell_is_named_by_its_file_name() {
        assert_eq!(shell_name("/usr/local/bin/zsh"), "zsh");
        assert_eq!(shell_name("/bin/sh"), "sh");
        assert_eq!(shell_name(""), "");
    }
}
