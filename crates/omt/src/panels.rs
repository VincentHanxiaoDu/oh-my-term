//! The overlay lists the leader keys open: palette, agents, recall,
//! commands, conversation, sessions.
//!
//! Split out of `run.rs` when it crossed the hard line limit. The seam is a
//! real one: each arm is "rows in, list out", and the frame loop owns both
//! sides of it.

use std::io::{self, Write};

use omt_tui::Input;

/// Everything building a list needs from the caller.
///
/// One value rather than seven words: the same parameters travel to every
/// arm, and the seventh unlabelled word is where somebody swaps two things.
pub(crate) struct OverlayCtx<'a> {
    /// The session the list is about.
    pub(crate) session: omt_types::SessionId,
    /// The workspace the session lives in.
    pub(crate) workspace: omt_types::WorkspaceId,
    /// The instance behind it.
    pub(crate) instance: &'a mut omt_daemon::Instance,
    /// The attached client, when there is one.
    pub(crate) attached: &'a mut Option<crate::backing::Attached>,
    /// The configuration.
    pub(crate) state: &'a crate::state::State,
    /// The width lists wrap to.
    pub(crate) term_cols: u16,
}

/// The overlay for an input, when it opens one.
#[must_use]
pub(crate) fn overlay_for(
    input: &Input,
    watched: &[crate::rows::Watched],
    ctx: OverlayCtx<'_>,
) -> Option<omt_tui::Overlay> {
    let OverlayCtx {
        session,
        workspace,
        instance,
        attached,
        state,
        term_cols,
    } = ctx;
    match input {
        Input::Palette => {
            // Built from the catalog, so the list is what this build actually
            // registers rather than a menu maintained beside it — which is how
            // a palette comes to offer something that is not there.
            Some(omt_tui::Overlay::new(
                "Commands",
                omt_tui::ACTIONS
                    .iter()
                    .map(|a| omt_tui::Choice {
                        label: a.capability.to_owned(),
                        detail: String::new(),
                        chord: a.binding.map(str::to_owned),
                        mark: None,
                    })
                    .collect(),
            ))
        }
        Input::Agents => Some(omt_tui::Overlay::new(
            "Agents",
            crate::rows::agent_rows(watched),
        )),
        Input::Recall => {
            let suggestions = crate::rows::recall_of(attached, state, workspace);
            Some(omt_tui::Overlay::new(
                crate::run::RECALL_TITLE,
                crate::rows::recall_rows(&suggestions),
            ))
        }
        Input::Commands => {
            let commands = crate::rows::offers_of(attached, state, session);
            Some(omt_tui::Overlay::new(
                crate::run::COMMANDS_TITLE,
                crate::rows::command_rows(&commands),
            ))
        }
        Input::Conversation => Some(omt_tui::Overlay::new(
            "Conversation",
            crate::rows::conversation_of(instance, attached, state, session, term_cols),
        )),
        Input::Sessions => {
            // Every session this instance holds, not only the ones with a pane
            // on screen. A session that is running somewhere else is exactly
            // the one somebody opens this list to find, and a list that showed
            // only what was already visible answered a question nobody had.
            Some(omt_tui::Overlay::new(
                "Sessions",
                crate::rows::session_rows(watched),
            ))
        }
        _ => None,
    }
}

/// The branch, remembered for a moment.
///
/// `git status` spawns two processes. Asking it per frame — which is what
/// "read once per frame, not cached" turned into at up to 125 frames a second
/// — is 250 process spawns a second to answer a question whose answer changes
/// when somebody types `git checkout`. It was the single largest cost in the
/// loop, and it is why a pasted paragraph arrived a character at a time.
///
/// A second of staleness after a checkout is imperceptible. A quarter of a
/// second of latency per keystroke is not.
#[derive(Default)]
pub(crate) struct BranchCache {
    /// Which directory the answer is for, so a `cd` is not answered with the
    /// previous workspace's branch.
    dir: String,
    branch: Option<String>,
    at: Option<u64>,
}

impl BranchCache {
    /// How long an answer is reused.
    const FRESH_MS: u64 = 1_000;

    pub(crate) fn get(&mut self, dir: &str, now: u64) -> Option<String> {
        self.get_with(dir, now, |dir| {
            omt_workspace_fs::git::status(std::path::Path::new(dir))
                .ok()
                .and_then(|s| s.branch)
        })
    }

    /// The caching itself, with the lookup passed in.
    ///
    /// Split out so what this type is *for* — asking rarely — is testable
    /// without a repository and without counting process spawns, which is a
    /// measurement that passes by luck whichever way the code goes.
    fn get_with(
        &mut self,
        dir: &str,
        now: u64,
        lookup: impl FnOnce(&str) -> Option<String>,
    ) -> Option<String> {
        let fresh = self.dir == dir
            && self
                .at
                .is_some_and(|at| now.saturating_sub(at) < Self::FRESH_MS);
        if !fresh {
            self.dir = dir.to_owned();
            self.at = Some(now);
            self.branch = lookup(dir);
        }
        self.branch.clone()
    }
}

pub(crate) fn draw_separators(
    out: &mut impl Write,
    rects: &[omt_tui::Rect],
    palette: omt_tui::paint::Chrome,
    depth: omt_tui::paint::Depth,
) -> io::Result<()> {
    // Built once rather than per row: the rule is the same every time, and a
    // hundred-row window is a hundred allocations of the same short string.
    let rule = omt_tui::paint::separator(palette, depth);
    for pair in rects.windows(2) {
        let col = pair[0].col + pair[0].cols + 1;
        for row in 0..pair[0].rows {
            write!(out, "\x1b[{};{col}H{rule}", row + 1)?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    #[test]
    fn the_branch_is_asked_for_at_most_once_a_second() {
        // `git status` spawns two processes and this ran per frame, at up to
        // 125 frames a second. It was the single largest cost in the loop and
        // the reason a pasted paragraph arrived a character at a time.
        let mut cache = BranchCache::default();
        let mut asked = 0;
        for now in 0..500 {
            let got = cache.get_with("/repo", now, |_| {
                asked += 1;
                Some("main".to_owned())
            });
            assert_eq!(got.as_deref(), Some("main"));
        }
        assert_eq!(asked, 1, "asked git {asked} times in half a second");
    }

    #[test]
    fn a_stale_answer_is_replaced_rather_than_kept() {
        // A second of staleness after a checkout is imperceptible; never
        // updating would show the wrong branch forever.
        let mut cache = BranchCache::default();
        cache.get_with("/repo", 0, |_| Some("main".to_owned()));
        let after = cache.get_with("/repo", BranchCache::FRESH_MS, |_| {
            Some("feature".to_owned())
        });
        assert_eq!(after.as_deref(), Some("feature"));
    }

    #[test]
    fn a_different_directory_is_never_answered_from_the_previous_one() {
        // The failure this prevents is a `cd` into another worktree showing
        // the branch of the one just left, which is worse than showing none.
        let mut cache = BranchCache::default();
        cache.get_with("/one", 0, |_| Some("main".to_owned()));
        let other = cache.get_with("/two", 1, |_| Some("release".to_owned()));
        assert_eq!(other.as_deref(), Some("release"));
    }
}
