//! Writing down what was run, so it can be suggested later.
//!
//! `recall.suggest` has existed since the catalog did, the web client has had a
//! handler for it, and it has always answered with nothing — because nothing
//! ever recorded anything. `recall.record` was reachable and reached only by a
//! test. A suggestion list with no history in it is not an empty feature, it is
//! a feature that cannot work, and the difference is invisible from the outside.
//!
//! What fills it is the block model: a shell with shell integration says where
//! a command begins and where it ends, and that is the one place omt knows a
//! command *was a command* rather than a line that happened to be on the
//! screen.

use omt_types::{SessionId, WorkspaceId};

/// Which blocks have already been written down, per session.
///
/// A block is recorded once, when it closes. Kept per session because block
/// ids are per terminal, and two sessions' blocks are not comparable.
#[derive(Debug, Default)]
pub struct Remembered {
    seen: std::collections::BTreeMap<SessionId, std::collections::BTreeSet<omt_types::BlockId>>,
}

impl Remembered {
    /// Record whatever this session has finished running since the last call.
    ///
    /// Only **attributed** blocks: one opened from output alone has no command
    /// in it, and a history full of invented lines is worse than a short one.
    /// That flag exists precisely so nothing here has to guess what a prompt
    /// looked like.
    pub fn absorb(
        &mut self,
        history: &mut omt_recall::History,
        session: SessionId,
        workspace: WorkspaceId,
        terminal: &omt_term::Terminal,
    ) {
        let seen = self.seen.entry(session).or_default();
        for block in terminal.blocks() {
            let omt_term::Outcome::Exited { code } = block.outcome else {
                // Still running, or ended in a way the shell did not report.
                // Recorded when it closes, which is when the command is known
                // to have *been* one.
                continue;
            };
            if !block.attributed || block.command.trim().is_empty() {
                continue;
            }
            if !seen.insert(block.id) {
                continue;
            }
            history.record(omt_recall::HistoryEntry {
                command: block.command.clone(),
                workspace,
                session,
                // The block model records where its prompt was, not where the
                // shell was; the cwd comes from OSC 7 on the session and is
                // filled by the caller where it knows.
                cwd: None,
                at: omt_types::Timestamp::now(),
                exit_code: Some(code),
                duration_ms: None,
                // Unknown rather than assumed. A command in a pane may have
                // been typed by a person or run by the agent in it, and omt
                // learns which from the agent's own reports — never from the
                // fact that it appeared in a terminal.
                attribution: omt_recall::Attribution::Unknown,
            });
        }
    }

    /// Forget a session's blocks.
    ///
    /// Called when a session ends: the ids belonged to its terminal, and
    /// keeping them would grow with every session this process ever showed.
    pub fn forget(&mut self, session: SessionId) {
        self.seen.remove(&session);
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

    fn workspace() -> WorkspaceId {
        WorkspaceId::from_canonical_path("/w")
    }

    fn terminal_running(script: &str) -> omt_term::Terminal {
        let mut terminal = omt_term::Terminal::new(omt_term::TermConfig {
            size: omt_term::GridSize::new(40, 8),
            ..omt_term::TermConfig::default()
        });
        terminal.advance(script.as_bytes());
        terminal
    }

    /// One complete shell-integration cycle: prompt, command, output, end.
    ///
    /// No newline between the command and the output mark: the shell writes
    /// `C` when it submits, and a line break in between is the *echo*, not a
    /// transition — putting one there produced a block with no command in it,
    /// which is what an unattributed block looks like.
    fn one_command(command: &str, code: i32) -> String {
        format!(
            "\x1b]133;A\x07$ \x1b]133;B\x07{command}\x1b]133;C\x07output\r\n\x1b]133;D;{code}\x07"
        )
    }

    #[test]
    fn a_command_that_finished_is_written_down() {
        // The whole point: `recall.suggest` answered with nothing for as long
        // as omt existed, because nothing had ever recorded anything.
        let terminal = terminal_running(&one_command("cargo test", 0));
        let mut history = omt_recall::History::default();
        let mut remembered = Remembered::default();
        remembered.absorb(&mut history, SessionId::new(), workspace(), &terminal);
        assert_eq!(
            history.entries().len(),
            1,
            "blocks: {:?}",
            terminal
                .blocks()
                .iter()
                .map(|b| (b.command.clone(), b.attributed, b.outcome))
                .collect::<Vec<_>>()
        );
        assert_eq!(history.entries()[0].command, "cargo test");
        assert_eq!(history.entries()[0].exit_code, Some(0));
    }

    #[test]
    fn the_same_command_is_not_written_down_twice() {
        // The pump runs many times a second, and a history that grew by one
        // row per frame would be a disk full of the same line.
        let terminal = terminal_running(&one_command("ls", 0));
        let mut history = omt_recall::History::default();
        let mut remembered = Remembered::default();
        let (session, workspace) = (SessionId::new(), workspace());
        for _ in 0..5 {
            remembered.absorb(&mut history, session, workspace, &terminal);
        }
        assert_eq!(history.entries().len(), 1);
    }

    #[test]
    fn a_command_still_running_is_not_recorded_yet() {
        // It is recorded when it closes, which is when omt knows how it went —
        // and a suggestion list that offered commands nobody had finished
        // would be suggesting guesses.
        let terminal = terminal_running("\x1b]133;A\x07$ \x1b]133;B\x07sleep 60\x1b]133;C\x07");
        let mut history = omt_recall::History::default();
        Remembered::default().absorb(&mut history, SessionId::new(), workspace(), &terminal);
        assert!(history.entries().is_empty());
    }

    #[test]
    fn output_with_no_command_behind_it_is_never_invented() {
        // A block opened from output alone — a shell with no integration, a
        // bare ssh, output that arrived before the mark. Scraping the line
        // above for something that "looks like" a prompt is exactly the
        // heuristic the tier ladder exists to keep out.
        let terminal = terminal_running("just some output\r\nand more\r\n");
        let mut history = omt_recall::History::default();
        Remembered::default().absorb(&mut history, SessionId::new(), workspace(), &terminal);
        assert!(history.entries().is_empty());
    }

    #[test]
    fn a_failed_command_is_kept_with_how_it_failed() {
        // What failed is worth suggesting *and* worth marking: somebody
        // reaching for a command they ran an hour ago usually wants the one
        // that worked.
        let terminal = terminal_running(&one_command("cargo buidl", 101));
        let mut history = omt_recall::History::default();
        Remembered::default().absorb(&mut history, SessionId::new(), workspace(), &terminal);
        assert_eq!(history.entries()[0].exit_code, Some(101));
    }

    #[test]
    fn a_session_that_ended_takes_its_block_ids_with_it() {
        // Block ids belong to a terminal. Kept, they would grow with every
        // session this process ever showed.
        let mut remembered = Remembered::default();
        let session = SessionId::new();
        let terminal = terminal_running(&one_command("ls", 0));
        let mut history = omt_recall::History::default();
        remembered.absorb(&mut history, session, workspace(), &terminal);
        assert!(remembered.seen.contains_key(&session));
        remembered.forget(session);
        assert!(!remembered.seen.contains_key(&session));
    }
}
