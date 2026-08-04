//! What the core asks the host to do.
//!
//! Every variant here is a **request to the host**, never something the core
//! did. By the time these are drained the grid is already updated; these are
//! precisely the effects the core is forbidden to perform itself, because
//! performing them would mean the terminal core did I/O, owned a clipboard
//! policy, or decided its own size.

use crate::cell::Color;

/// Which clipboard an OSC 52 refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardSelection {
    /// The system clipboard.
    Clipboard,
    /// The X-style primary selection.
    Primary,
}

/// A window operation a program asked for (CSI t).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowOp {
    /// Report the text-area size in characters.
    ReportTextAreaSize,
    /// Report the window position.
    ReportPosition,
    /// Asked to be resized in characters.
    ///
    /// Reported, never honoured: the size is the session layer's to negotiate
    /// with the PTY, and a program that could resize its own window would fight
    /// the user's layout.
    RequestResize {
        /// Requested rows.
        rows: u16,
        /// Requested columns.
        cols: u16,
    },
    /// Something in the CSI t space this build does not act on.
    Other(u16),
}

/// An OSC 133 shell-integration transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockEvent {
    /// A prompt began (OSC 133 A).
    PromptStart,
    /// The user's command begins here (B).
    CommandStart,
    /// The command was submitted and output follows (C).
    OutputStart,
    /// The command finished (D), with its exit status where the shell gave one.
    CommandEnd {
        /// The exit code, if reported.
        exit_code: Option<i32>,
    },
}

/// An OSC 8 hyperlink transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HyperlinkEvent {
    /// A link opened; cells written from here carry it.
    Opened {
        /// Its id, where the emitter supplied one.
        id: Option<String>,
        /// The target.
        uri: String,
    },
    /// The link closed.
    Closed,
}

/// A mode the host must know about because it changes how input is encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TermMode {
    /// Bracketed paste (2004).
    BracketedPaste,
    /// Focus reporting (1004).
    FocusReporting,
    /// Any mouse tracking mode (1000/1002/1003).
    MouseTracking,
    /// SGR mouse encoding (1006).
    SgrMouse,
    /// The kitty keyboard protocol.
    KittyKeyboard,
    /// Application cursor keys (DECCKM, 1).
    ApplicationCursor,
    /// Application keypad.
    ApplicationKeypad,
    /// The alternate screen (1049).
    ///
    /// The one the agent-detection downgrade keys on: a full-screen program is
    /// drawing its own frame, and block semantics do not apply to it.
    AlternateScreen,
    /// Synchronized output (2026).
    SynchronizedOutput,
}

/// A recoverable anomaly worth surfacing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Warning {
    /// A sequence this build does not implement.
    Unsupported {
        /// A short description, for the log.
        what: String,
    },
    /// The host-action queue overflowed and coalescible actions were dropped.
    ActionsDropped {
        /// How many.
        count: usize,
    },
    /// A payload exceeded its budget.
    PayloadTooLarge {
        /// How many bytes arrived.
        bytes: usize,
    },
}

/// Something the host must do, or must be told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostAction {
    /// Bytes to write back to the PTY, pre-encoded.
    ///
    /// The host writes them verbatim and never interprets them; a reply the
    /// host rewrote would be a reply the program did not ask for.
    Reply(Vec<u8>),
    /// OSC 0 / 2.
    SetTitle(String),
    /// OSC 1.
    SetIconName(String),
    /// OSC 7 — where the shell says it is.
    SetCwd {
        /// A file URL.
        url: String,
    },
    /// OSC 1337 SetUserVar.
    SetUserVar {
        /// The variable's name.
        key: String,
        /// Its value.
        value: String,
    },
    /// BEL.
    Bell,
    /// A CSI t window operation.
    WindowOp(WindowOp),
    /// An OSC 52 write, honoured per clipboard policy and never automatically.
    ClipboardWrite {
        /// Which clipboard.
        selection: ClipboardSelection,
        /// The decoded bytes.
        data: Vec<u8>,
    },
    /// An OSC 52 read, answered only with explicit consent.
    ClipboardRead {
        /// Which clipboard.
        selection: ClipboardSelection,
    },
    /// A shell-integration transition.
    Block(BlockEvent),
    /// A hyperlink transition.
    Hyperlink(HyperlinkEvent),
    /// A mode the host's input encoder depends on.
    ModeChanged {
        /// Which mode.
        mode: TermMode,
        /// Whether it is now on.
        enabled: bool,
    },
    /// The program resized the grid itself; the host must push the new size to
    /// the PTY. Distinct from a host-initiated resize.
    NotifyResize {
        /// New columns.
        cols: u16,
        /// New rows.
        rows: u16,
    },
    /// The program set a palette or dynamic colour.
    SetColor {
        /// Which slot: a palette index, or one of the dynamic ones.
        slot: ColorSlot,
        /// The colour, or `None` to reset it.
        color: Option<Color>,
    },
    /// Something worth telling the user or the log.
    Warn(Warning),
}

/// Which colour a program asked to change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorSlot {
    /// A palette entry.
    Indexed(u8),
    /// The default foreground.
    Foreground,
    /// The default background.
    Background,
    /// The cursor colour.
    Cursor,
}

impl HostAction {
    /// Whether the newest of these supersedes the older ones.
    ///
    /// Overflow drops the oldest *coalescible* action rather than the oldest
    /// action: a title set twice only needs its latest value, but a protocol
    /// reply that was dropped is a program that hangs.
    #[must_use]
    pub const fn is_coalescible(&self) -> bool {
        matches!(
            self,
            Self::SetTitle(_)
                | Self::SetIconName(_)
                | Self::SetCwd { .. }
                | Self::NotifyResize { .. }
        )
    }

    /// Whether dropping this would break the program talking to us.
    #[must_use]
    pub const fn is_never_dropped(&self) -> bool {
        matches!(self, Self::Reply(_) | Self::ClipboardWrite { .. })
    }
}

/// A bounded, order-preserving queue of host actions.
///
/// Bounded because an unbounded one is a program that can make the terminal
/// allocate without limit by asking the same question in a loop. Order is
/// preserved exactly as the bytes arrived: a `Reply` must not overtake a
/// `ModeChanged` the program is about to depend on.
#[derive(Debug, Default)]
pub struct ActionQueue {
    actions: Vec<HostAction>,
    limit: usize,
    dropped: usize,
}

impl ActionQueue {
    /// A queue holding at most `limit` actions.
    #[must_use]
    pub fn new(limit: usize) -> Self {
        Self {
            actions: Vec::new(),
            limit: limit.max(1),
            dropped: 0,
        }
    }

    /// Queue an action, making room by dropping a coalescible one if needed.
    ///
    /// Returns whether the caller should stop consuming input: that happens
    /// only when an undroppable action cannot be queued, which applies
    /// backpressure rather than losing a protocol reply.
    pub fn push(&mut self, action: HostAction) -> Backpressure {
        if self.actions.len() < self.limit {
            self.actions.push(action);
            return Backpressure::Continue;
        }
        if let Some(i) = self.actions.iter().position(HostAction::is_coalescible) {
            self.actions.remove(i);
            self.dropped += 1;
            self.actions.push(action);
            return Backpressure::Continue;
        }
        if action.is_never_dropped() {
            // Nothing here may be dropped and this may not be lost, so the
            // only correct answer is to stop reading and let the caller drain.
            return Backpressure::Stop(action);
        }
        self.dropped += 1;
        Backpressure::Continue
    }

    /// Take everything queued, oldest first, and report any drops.
    pub fn drain(&mut self) -> Vec<HostAction> {
        let mut out = std::mem::take(&mut self.actions);
        if self.dropped > 0 {
            out.push(HostAction::Warn(Warning::ActionsDropped {
                count: self.dropped,
            }));
            self.dropped = 0;
        }
        out
    }

    /// How many are queued.
    #[must_use]
    pub fn len(&self) -> usize {
        self.actions.len()
    }

    /// Whether anything is queued.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }
}

/// Whether the caller must stop feeding bytes.
#[derive(Debug, PartialEq, Eq)]
pub enum Backpressure {
    /// Keep going.
    Continue,
    /// The queue is full of things that may not be dropped; drain first. The
    /// action that did not fit is handed back so it is not lost either.
    Stop(HostAction),
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn title(s: &str) -> HostAction {
        HostAction::SetTitle(s.to_owned())
    }

    #[test]
    fn order_is_the_order_the_bytes_arrived() {
        // A Reply that overtook a ModeChanged would answer a question the
        // program had not finished asking.
        let mut q = ActionQueue::new(8);
        q.push(HostAction::ModeChanged {
            mode: TermMode::BracketedPaste,
            enabled: true,
        });
        q.push(HostAction::Reply(b"x".to_vec()));
        let out = q.drain();
        assert!(matches!(out[0], HostAction::ModeChanged { .. }));
        assert!(matches!(out[1], HostAction::Reply(_)));
    }

    #[test]
    fn overflow_drops_the_title_and_keeps_the_reply() {
        let mut q = ActionQueue::new(2);
        q.push(title("first"));
        q.push(HostAction::Reply(b"important".to_vec()));
        assert_eq!(q.push(title("second")), Backpressure::Continue);
        let out = q.drain();
        assert!(
            out.iter()
                .any(|a| matches!(a, HostAction::Reply(b) if b == b"important")),
            "the reply survived"
        );
        assert!(
            out.iter()
                .any(|a| matches!(a, HostAction::SetTitle(t) if t == "second")),
            "and the newest title did"
        );
    }

    #[test]
    fn a_reply_that_cannot_be_queued_stops_the_caller() {
        // Applying backpressure rather than dropping it: a lost reply is a
        // program that hangs waiting for an answer that will never come.
        let mut q = ActionQueue::new(2);
        q.push(HostAction::Reply(b"a".to_vec()));
        q.push(HostAction::Reply(b"b".to_vec()));
        let r = q.push(HostAction::Reply(b"c".to_vec()));
        assert_eq!(r, Backpressure::Stop(HostAction::Reply(b"c".to_vec())));
        assert_eq!(q.len(), 2, "and nothing already queued was lost");
    }

    #[test]
    fn drops_are_reported_rather_than_silent() {
        let mut q = ActionQueue::new(1);
        q.push(title("a"));
        q.push(title("b"));
        q.push(title("c"));
        let out = q.drain();
        assert!(
            out.iter().any(
                |a| matches!(a, HostAction::Warn(Warning::ActionsDropped { count }) if *count == 2)
            ),
            "{out:?}"
        );
    }

    #[test]
    fn draining_clears_the_queue() {
        let mut q = ActionQueue::new(4);
        q.push(HostAction::Bell);
        assert_eq!(q.drain().len(), 1);
        assert!(q.is_empty());
        assert!(q.drain().is_empty(), "and does not report a phantom drop");
    }

    #[test]
    fn a_clipboard_write_is_never_dropped() {
        // Silently losing a copy is worse than not supporting one: the user
        // pastes what was there before and does not notice.
        assert!(
            HostAction::ClipboardWrite {
                selection: ClipboardSelection::Clipboard,
                data: vec![],
            }
            .is_never_dropped()
        );
        assert!(!title("x").is_never_dropped());
    }
}
