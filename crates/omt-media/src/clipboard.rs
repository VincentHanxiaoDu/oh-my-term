//! Getting text onto somebody's local clipboard from the far end of an ssh.
//!
//! `docs/architecture/09-ssh-and-media.md` §3. OSC 52 is the mechanism: it is
//! emitted into the session's own output stream, so it rides the ssh connection
//! that is already there and is interpreted by whatever terminal the human is
//! actually sitting in front of.
//!
//! Two things here are easy to get wrong and expensive to debug.
//!
//! The wrapping is keyed off `TERM`, not `$TMUX`. `TERM` survives ssh and
//! `$TMUX` does not, so a multiplexer on the far side of a connection is
//! invisible to the variable people reach for first. Getting this backwards
//! produces a copy that works locally and silently does nothing over ssh, which
//! is the hardest possible version of this bug to find.
//!
//! And omt never claims the copy succeeded. **OSC 52 has no acknowledgement.**
//! Terminal.app does not implement it at all, kitty refuses silently over half
//! a megabyte, and older screen truncates at about a kilobyte. Inferring
//! success from terminal behaviour is the kind of heuristic that turns into a
//! confident false claim, so the surface says what was attempted and keeps the
//! fallback one tap away.

/// The default cap, in bytes of decoded text.
///
/// 64 KiB, chosen to stay under tmux's historical per-escape limit *after*
/// base64 expands it by a third. Larger payloads are not attempted, because a
/// truncated clipboard is worse than a refused one: the paste looks like it
/// worked and the tail is gone.
pub const DEFAULT_MAX_BYTES: usize = 64 * 1024;

/// Why a clipboard write was not attempted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ClipboardError {
    /// Over the cap, so the fallback is the honest route.
    #[error("{size} bytes is over the {max}-byte OSC 52 limit; use the fallback instead")]
    TooLarge {
        /// How large it was.
        size: usize,
        /// The cap.
        max: usize,
    },
    /// Nothing to copy.
    #[error("there is nothing to copy")]
    Empty,
}

/// Base64, as OSC 52 requires it.
fn encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = |i: usize| chunk.get(i).copied().unwrap_or(0) as u32;
        let n = (b(0) << 16) | (b(1) << 8) | b(2);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[((n >> (18 - 6 * i)) & 0x3f) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Wrap an escape sequence so an outer multiplexer passes it through.
///
/// Keyed off `TERM` rather than `$TMUX`, which is the whole point: `TERM`
/// survives ssh and `$TMUX` does not. tmux accepts only `ESC \` as the string
/// terminator, and every `ESC` inside the payload must be doubled or tmux ends
/// the passthrough early and prints the rest.
#[must_use]
pub fn wrap_osc(payload: &str, term: &str) -> String {
    if term.starts_with("screen") || term.starts_with("tmux") {
        // `ESC P tmux ; <payload, every ESC doubled> ESC \`.
        //
        // The doc's snippet has a stray `ESC` between `tmux;` and the payload.
        // With the payload's own leading ESC doubled that emits three in a row,
        // and tmux reads the third as the start of a new sequence — the copy
        // does nothing and a fragment of the escape is printed into the pane.
        // The code is right and 09 §3.1 needs the correction.
        format!("\x1bPtmux;{}\x1b\\", payload.replace('\x1b', "\x1b\x1b"))
    } else {
        payload.to_owned()
    }
}

/// The bytes to write into the session's output stream to set the clipboard.
///
/// # Errors
/// Refuses empty text, and text over the cap — above which OSC 52 is not
/// attempted at all, because a silently truncated clipboard is worse than a
/// refusal somebody can act on.
pub fn osc52(text: &str, term: &str, max_bytes: usize) -> Result<String, ClipboardError> {
    if text.is_empty() {
        return Err(ClipboardError::Empty);
    }
    if text.len() > max_bytes {
        return Err(ClipboardError::TooLarge {
            size: text.len(),
            max: max_bytes,
        });
    }
    // `c` is the clipboard selection. BEL rather than ST as the terminator: it
    // is what the widest set of terminals accept, and tmux's wrapper supplies
    // its own terminator around the whole thing regardless.
    let payload = format!("\x1b]52;c;{}\x07", encode(text.as_bytes()));
    Ok(wrap_osc(&payload, term))
}

/// What omt is allowed to say about a clipboard write.
///
/// Deliberately not "copied". There is no acknowledgement to base that on, and
/// a claim omt cannot verify is one the user will eventually catch it in.
#[must_use]
pub const fn what_to_say() -> &'static str {
    "Sent to your local clipboard via OSC 52. If nothing arrived, use the fallback."
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
    fn a_plain_terminal_gets_a_bare_osc_52() {
        let out = osc52("hello", "xterm-256color", DEFAULT_MAX_BYTES).expect("osc");
        assert!(out.starts_with("\x1b]52;c;"));
        assert!(out.ends_with('\x07'));
        assert!(out.contains(&encode(b"hello")));
    }

    #[test]
    fn tmux_gets_a_passthrough_wrapper() {
        let out = osc52("hello", "screen-256color", DEFAULT_MAX_BYTES).expect("osc");
        assert!(out.starts_with("\x1bPtmux;"), "{out:?}");
        assert!(out.ends_with("\x1b\\"), "tmux only accepts ESC backslash as ST");
    }

    #[test]
    fn every_escape_inside_a_wrapped_payload_is_doubled() {
        // Otherwise tmux ends the passthrough at the first inner ESC and prints
        // the remainder of the sequence as text.
        let wrapped = wrap_osc("\x1b]52;c;AAA\x07", "tmux-256color");
        let inner = wrapped
            .trim_start_matches("\x1bPtmux;")
            .trim_end_matches("\x1b\\");
        assert!(inner.starts_with("\x1b\x1b]52"), "{inner:?}");
    }

    #[test]
    fn the_wrapper_is_chosen_by_term_not_by_the_tmux_variable() {
        // The bug this ordering exists to prevent: `$TMUX` does not survive
        // ssh and `TERM` does, so keying off the variable produces a copy that
        // works locally and silently does nothing remotely.
        assert!(wrap_osc("x", "screen").starts_with("\x1bPtmux;"));
        assert_eq!(wrap_osc("x", "xterm-256color"), "x");
        assert_eq!(wrap_osc("x", "alacritty"), "x");
    }

    #[test]
    fn something_over_the_cap_is_refused_rather_than_truncated() {
        // A truncated clipboard is worse than a refused one: the paste looks
        // like it worked and the tail is gone.
        let big = "x".repeat(DEFAULT_MAX_BYTES + 1);
        let err = osc52(&big, "xterm", DEFAULT_MAX_BYTES).expect_err("must refuse");
        assert!(matches!(err, ClipboardError::TooLarge { .. }));
    }

    #[test]
    fn the_cap_leaves_room_for_base64_expansion() {
        // 64 KiB decoded becomes about 87 KiB encoded, which is what actually
        // has to fit through tmux.
        let at_limit = "x".repeat(DEFAULT_MAX_BYTES);
        let out = osc52(&at_limit, "xterm", DEFAULT_MAX_BYTES).expect("at the limit");
        assert!(out.len() > DEFAULT_MAX_BYTES, "base64 expands by a third");
    }

    #[test]
    fn copying_nothing_is_refused() {
        assert_eq!(osc52("", "xterm", DEFAULT_MAX_BYTES), Err(ClipboardError::Empty));
    }

    #[test]
    fn omt_never_claims_the_copy_succeeded() {
        // OSC 52 has no acknowledgement. Terminal.app does not implement it at
        // all, and kitty refuses silently above its own limit.
        let said = what_to_say().to_lowercase();
        assert!(said.contains("sent"));
        assert!(!said.contains("copied"), "a claim omt cannot verify");
        assert!(said.contains("fallback"));
    }

    #[test]
    fn utf8_survives_the_round_trip_of_encoding() {
        let out = osc52("日本語 — ✓", "xterm", DEFAULT_MAX_BYTES).expect("osc");
        let body = out
            .trim_start_matches("\x1b]52;c;")
            .trim_end_matches('\x07');
        assert_eq!(body, encode("日本語 — ✓".as_bytes()));
    }
}
