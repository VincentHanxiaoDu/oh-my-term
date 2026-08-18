//! The overlay lists the leader keys open: palette, agents, recall,
//! commands, conversation, sessions.
//!
//! Split out of `run.rs` when it crossed the hard line limit. The seam is a
//! real one: each arm is "rows in, list out", and the frame loop owns both
//! sides of it.

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
