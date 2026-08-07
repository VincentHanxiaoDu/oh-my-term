//! Command blocks: one command, its output, and how it ended.
//!
//! This is what makes "jump to the previous command", "copy just the output"
//! and "re-run this" possible at all. Without it a terminal is one long stream
//! and every one of those becomes a heuristic over text.
//!
//! Built from OSC 133, which the parser already emits. The tracker never looks
//! at bytes — it takes transitions and rows, which is why it can be tested
//! without a terminal and why a shell that emits no marks simply produces no
//! blocks rather than wrong ones.

use omt_types::BlockId;

use crate::action::BlockEvent;

/// How a command ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Still running.
    Running,
    /// Finished with this code.
    Exited {
        /// The code the shell reported.
        code: i32,
    },
    /// The shell reported no code at all.
    ///
    /// Distinct from exiting with zero: a shell that emits `133;D` without a
    /// status has told omt the command ended and nothing more, and painting
    /// that green would be inventing the good news.
    Unknown,
}

impl Outcome {
    /// Whether this should be shown as a failure.
    ///
    /// 130 and 141 are not failures. 130 is Ctrl-C — the user meant that — and
    /// 141 is SIGPIPE, which is what `… | head` does to everything upstream of
    /// it every single time. A terminal that paints those red teaches people
    /// that red means nothing.
    #[must_use]
    pub const fn is_failure(self) -> bool {
        match self {
            Self::Exited { code } => code != 0 && code != 130 && code != 141,
            Self::Running | Self::Unknown => false,
        }
    }

    /// Whether the command is finished.
    #[must_use]
    pub const fn is_finished(self) -> bool {
        !matches!(self, Self::Running)
    }
}

/// One command and everything that came of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// Its identity, stable for as long as it is held.
    pub id: BlockId,
    /// The command as the user typed it, where the shell marked it.
    pub command: String,
    /// The absolute row its prompt began on.
    pub prompt_row: u64,
    /// The absolute row its output began on, once it started.
    pub output_row: Option<u64>,
    /// The absolute row after its last line, once it ended.
    pub end_row: Option<u64>,
    /// How it ended.
    pub outcome: Outcome,
}

impl Block {
    /// The rows holding this block's output, if it has any yet.
    ///
    /// Half-open, and `None` while the command has produced nothing — an empty
    /// range and "no output yet" are different things to a UI offering to copy
    /// it.
    #[must_use]
    pub fn output_rows(&self) -> Option<std::ops::Range<u64>> {
        let start = self.output_row?;
        let end = self.end_row.unwrap_or(start);
        (end > start).then_some(start..end)
    }
}

/// Turns shell-integration transitions into blocks.
#[derive(Debug, Default)]
pub struct BlockTracker {
    blocks: Vec<Block>,
    /// Where the command text is being collected, between B and C.
    collecting: Option<String>,
    limit: usize,
}

/// How many blocks are kept.
///
/// Bounded because this grows with every command in a long-lived session, and
/// a terminal that holds every block of a week-long tmux session is a terminal
/// that grows without limit. Large enough that scrolling back through a day's
/// work still finds them.
pub const DEFAULT_LIMIT: usize = 2_000;

impl BlockTracker {
    /// A tracker holding the default number of blocks.
    #[must_use]
    pub fn new() -> Self {
        Self {
            blocks: Vec::new(),
            collecting: None,
            limit: DEFAULT_LIMIT,
        }
    }

    /// A tracker holding at most `limit` blocks.
    #[must_use]
    pub fn with_limit(limit: usize) -> Self {
        Self {
            limit: limit.max(1),
            ..Self::new()
        }
    }

    /// Every block, oldest first.
    #[must_use]
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// The block still running, if one is.
    #[must_use]
    pub fn active(&self) -> Option<&Block> {
        self.blocks.last().filter(|b| !b.outcome.is_finished())
    }

    /// The most recent block that failed.
    ///
    /// What "jump to the last error" is built on, and the reason `is_failure`
    /// excludes 130 and 141: a list that treats every Ctrl-C as an error is a
    /// list nobody uses twice.
    #[must_use]
    pub fn last_failure(&self) -> Option<&Block> {
        self.blocks.iter().rev().find(|b| b.outcome.is_failure())
    }

    /// Apply a transition at an absolute row.
    ///
    /// Returns the block it opened or closed, so a caller can emit an event
    /// without searching for what changed.
    pub fn apply(&mut self, event: BlockEvent, row: u64) -> Option<BlockId> {
        match event {
            BlockEvent::PromptStart => {
                // A prompt while a command is still open means the shell never
                // sent D — it crashed, or was killed. The block is closed as
                // unknown rather than left running forever, because a spinner
                // that never stops is worse than an honest "it ended".
                self.close_dangling(row);
                let block = Block {
                    id: BlockId::new(),
                    command: String::new(),
                    prompt_row: row,
                    output_row: None,
                    end_row: None,
                    outcome: Outcome::Running,
                };
                let id = block.id;
                self.blocks.push(block);
                self.trim();
                Some(id)
            }
            BlockEvent::CommandStart => {
                self.collecting = Some(String::new());
                None
            }
            BlockEvent::OutputStart => {
                let command = self.collecting.take().unwrap_or_default();
                let block = self.blocks.last_mut()?;
                if !command.is_empty() {
                    block.command = command;
                }
                block.output_row = Some(row);
                Some(block.id)
            }
            BlockEvent::CommandEnd { exit_code } => {
                let block = self.blocks.last_mut()?;
                block.end_row = Some(row);
                block.outcome = match exit_code {
                    Some(code) => Outcome::Exited { code },
                    None => Outcome::Unknown,
                };
                Some(block.id)
            }
        }
    }

    /// Record command text seen between `CommandStart` and `OutputStart`.
    ///
    /// Fed by the host rather than scraped here: what the user typed is on the
    /// screen between two marks, and only the caller knows which cells those
    /// are.
    pub fn command_text(&mut self, text: &str) {
        if let Some(buffer) = self.collecting.as_mut() {
            buffer.push_str(text);
        }
    }

    /// Close a block the shell never closed.
    fn close_dangling(&mut self, row: u64) {
        if let Some(last) = self.blocks.last_mut()
            && !last.outcome.is_finished()
        {
            last.end_row = Some(row);
            last.outcome = Outcome::Unknown;
        }
    }

    fn trim(&mut self) {
        if self.blocks.len() > self.limit {
            let excess = self.blocks.len() - self.limit;
            self.blocks.drain(..excess);
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "in a test, expect() is the assertion")]
mod tests {
    use super::*;

    /// Drive one whole command through the tracker.
    fn run(tracker: &mut BlockTracker, command: &str, exit: Option<i32>, at: u64) {
        tracker.apply(BlockEvent::PromptStart, at);
        tracker.apply(BlockEvent::CommandStart, at);
        tracker.command_text(command);
        tracker.apply(BlockEvent::OutputStart, at + 1);
        tracker.apply(BlockEvent::CommandEnd { exit_code: exit }, at + 3);
    }

    #[test]
    fn a_command_becomes_a_block_with_its_text_and_its_outcome() {
        let mut t = BlockTracker::new();
        run(&mut t, "cargo test", Some(0), 10);
        let block = &t.blocks()[0];
        assert_eq!(block.command, "cargo test");
        assert_eq!(block.outcome, Outcome::Exited { code: 0 });
        assert_eq!(block.output_rows(), Some(11..13));
    }

    #[test]
    fn a_ctrl_c_is_not_a_failure() {
        // 130 is the user meaning it. A terminal that paints it red teaches
        // people that red means nothing.
        let mut t = BlockTracker::new();
        run(&mut t, "sleep 100", Some(130), 0);
        assert!(!t.blocks()[0].outcome.is_failure());
        assert!(t.last_failure().is_none());
    }

    #[test]
    fn a_broken_pipe_is_not_a_failure_either() {
        // 141 is what `… | head` does to everything upstream, every time.
        let mut t = BlockTracker::new();
        run(&mut t, "find / | head", Some(141), 0);
        assert!(!t.blocks()[0].outcome.is_failure());
    }

    #[test]
    fn a_real_failure_is_one() {
        let mut t = BlockTracker::new();
        run(&mut t, "cargo build", Some(101), 0);
        assert!(t.blocks()[0].outcome.is_failure());
        assert_eq!(t.last_failure().expect("a failure").command, "cargo build");
    }

    #[test]
    fn a_shell_that_reports_no_code_is_not_reported_as_success() {
        // Painting it green would be inventing the good news.
        let mut t = BlockTracker::new();
        run(&mut t, "something", None, 0);
        assert_eq!(t.blocks()[0].outcome, Outcome::Unknown);
        assert!(!t.blocks()[0].outcome.is_failure());
    }

    #[test]
    fn a_command_that_never_ended_is_closed_when_the_next_prompt_appears() {
        // The shell crashed or was killed. A spinner that never stops is worse
        // than an honest "it ended".
        let mut t = BlockTracker::new();
        t.apply(BlockEvent::PromptStart, 0);
        t.apply(BlockEvent::OutputStart, 1);
        t.apply(BlockEvent::PromptStart, 9);
        assert!(t.blocks()[0].outcome.is_finished());
        assert_eq!(t.blocks()[0].end_row, Some(9));
    }

    #[test]
    fn the_running_block_is_the_one_that_has_not_ended() {
        let mut t = BlockTracker::new();
        run(&mut t, "done", Some(0), 0);
        assert!(t.active().is_none());
        t.apply(BlockEvent::PromptStart, 20);
        t.apply(BlockEvent::OutputStart, 21);
        assert!(t.active().is_some());
    }

    #[test]
    fn a_command_with_no_output_yet_reports_none_rather_than_an_empty_range() {
        // "Nothing yet" and "nothing at all" are different to a UI offering to
        // copy the output.
        let mut t = BlockTracker::new();
        t.apply(BlockEvent::PromptStart, 0);
        t.apply(BlockEvent::OutputStart, 1);
        assert_eq!(t.blocks()[0].output_rows(), None);
    }

    #[test]
    fn a_shell_with_no_marks_produces_no_blocks_rather_than_wrong_ones() {
        // The property that makes this safe to run against every shell: no
        // OSC 133, no guessing.
        let t = BlockTracker::new();
        assert!(t.blocks().is_empty());
    }

    #[test]
    fn the_oldest_blocks_are_dropped_rather_than_growing_forever() {
        // A week-long session would otherwise hold every command it ever ran.
        let mut t = BlockTracker::with_limit(3);
        for i in 0..10u64 {
            run(&mut t, "x", Some(0), i * 10);
        }
        assert_eq!(t.blocks().len(), 3);
    }

    #[test]
    fn each_block_has_its_own_identity() {
        // Ids are what a client refers to when it asks to re-run one, and two
        // blocks sharing one would re-run the wrong command.
        let mut t = BlockTracker::new();
        run(&mut t, "a", Some(0), 0);
        run(&mut t, "b", Some(0), 10);
        assert_ne!(t.blocks()[0].id, t.blocks()[1].id);
    }
}
