//! Whether the terminal omt is drawn on can draw an image, and where.
//!
//! Two rules this repository already holds, and this is the one place they
//! meet:
//!
//! * **Confirmed, never assumed.** [`omt_media::tier`] refuses "probably kitty,
//!   so assume its protocol", because probably means a feature that silently
//!   does nothing and a user who cannot tell whether they mis-typed or omt
//!   mis-guessed. Support here is a positive answer to a question.
//! * **Never ask mid-session.** A query's reply arrives in the same stream as
//!   typing, and an answer mistaken for keystrokes puts escape text into
//!   somebody's shell — which is why `host_colours` reads the environment
//!   rather than asking. There is exactly one moment where that is not true:
//!   after raw mode is entered and before the first keystroke is read.
//!
//! And the reason this cannot go through the normal input path: `vte` swallows
//! APC in a pane and crossterm swallows it here, which is the same defect that
//! made an image invisible in the first place. The answer has to be read off
//! the descriptor.

use std::io::{Read, Write};
use std::os::fd::AsRawFd;

/// What the terminal on the other end can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Host {
    /// It said nothing, so nothing is ever sent to it.
    ///
    /// The default and the ordinary case: most terminals do not answer, and
    /// that is not a failure.
    #[default]
    Nothing,
    /// It answered kitty's own query.
    Kitty,
}

/// How long to wait for an answer.
///
/// Short, because this is on the path between somebody pressing a key to start
/// omt and omt drawing anything, and every terminal that will not answer pays
/// it in full.
pub const DEADLINE: std::time::Duration = std::time::Duration::from_millis(120);

/// The id used to ask. Distinct from anything a program in a pane would pick,
/// so an answer cannot be confused with a pane's own traffic.
const PROBE_ID: u32 = 7_654_321;

/// Ask the terminal whether it speaks kitty's graphics protocol.
///
/// A one-pixel transmission with `a=q`, which is a query rather than a
/// drawing: it is the only reliable positive test, and it puts nothing on the
/// screen of a terminal that does understand it.
///
/// Must be called **before any input is read**, for the reason in the module
/// note.
///
/// Returns whatever it read that was **not** the answer, because a probe that
/// swallowed it would eat the first thing somebody typed. Everything on that
/// descriptor belongs to the person at the keyboard unless it is the reply
/// that was asked for, and the caller hands the rest on.
pub fn probe(mut out: impl Write, mut input: impl Read + AsRawFd) -> (Host, Vec<u8>) {
    // One transparent pixel, which is the smallest legal transmission.
    let query = format!("\x1b_Gi={PROBE_ID},s=1,v=1,a=q,t=d,f=32;AAAAAA==\x1b\\");
    if out.write_all(query.as_bytes()).is_err() || out.flush().is_err() {
        return (Host::Nothing, Vec::new());
    }
    let mut seen = Vec::new();
    let deadline = std::time::Instant::now() + DEADLINE;
    let mut buf = [0u8; 256];
    while std::time::Instant::now() < deadline {
        if !readable(input.as_raw_fd(), deadline) {
            break;
        }
        match input.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                seen.extend_from_slice(&buf[..n]);
                if let Some(host) = answer(&seen) {
                    return (host, typed_around(&seen));
                }
            }
            Err(_) => break,
        }
    }
    // Nothing that was asked for, so all of it belongs to whoever typed it.
    (Host::Nothing, seen)
}

/// What arrived that was not the reply.
///
/// A reply and a keystroke can land in the same read, and the keystroke is
/// still the person's. Only the reply itself is taken out.
#[must_use]
pub fn typed_around(seen: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(seen);
    let marker = format!("\x1b_Gi={PROBE_ID}");
    let Some(at) = text.find(&marker) else {
        return seen.to_vec();
    };
    let rest = &text[at + marker.len()..];
    // Past the terminator, which is where the reply stops and anything else
    // begins.
    let end = rest
        .find("\x1b\\")
        .map(|i| at + marker.len() + i + 2)
        .or_else(|| rest.find('\x07').map(|i| at + marker.len() + i + 1))
        .unwrap_or(text.len());
    let mut out = text[..at].as_bytes().to_vec();
    out.extend_from_slice(text.get(end..).unwrap_or_default().as_bytes());
    out
}

/// Whether the descriptor has something to say before the deadline.
fn readable(fd: std::os::fd::RawFd, deadline: std::time::Instant) -> bool {
    let left = deadline.saturating_duration_since(std::time::Instant::now());
    if left.is_zero() {
        return false;
    }
    // A poll rather than a blocking read: a terminal that never answers is the
    // ordinary case, and a blocking read there would hang omt at startup on
    // most of the terminals people use.
    let mut fds = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let ms = i32::try_from(left.as_millis()).unwrap_or(i32::MAX);
    #[allow(
        unsafe_code,
        reason = "poll has no safe wrapper here; one initialized pollfd, a \
                  count matching it, and a timeout — and a blocking read \
                  instead would hang omt at startup on every terminal that \
                  does not answer, which is most of them"
    )]
    unsafe {
        libc::poll(&raw mut fds, 1, ms) > 0
    }
}

/// Read an answer out of whatever arrived.
///
/// Only kitty's own `OK`, addressed to the id omt asked with. Anything else on
/// the descriptor is somebody typing, or another terminal's idea of a reply,
/// and neither is support.
#[must_use]
pub fn answer(seen: &[u8]) -> Option<Host> {
    let text = String::from_utf8_lossy(seen);
    let marker = format!("_Gi={PROBE_ID}");
    let at = text.find(&marker)?;
    let rest = &text[at..];
    // The reply ends at the string terminator; a partial one is not an answer
    // yet, and saying so is what lets the caller keep reading.
    let end = rest.find('\x1b').or_else(|| rest.find('\x07'))?;
    rest[..end].contains("OK").then_some(Host::Kitty)
}

/// One image, placed on the terminal omt is drawn on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placement {
    /// Which image, as omt numbers them on the host.
    pub id: u32,
    /// Where its top-left corner goes, in the host's own cells.
    pub col: u16,
    /// Its row on the host.
    pub row: u16,
    /// How many of the image's rows are drawn.
    pub rows: u16,
    /// How many of its columns.
    pub cols: u16,
    /// How much of the image's top is cut off, in its own rows.
    pub skip_rows: u16,
}

/// How a cell measures, in pixels.
///
/// From the terminal, never assumed: a guessed cell size crops the wrong part
/// of an image, and a wrong crop looks like a rendering bug rather than a
/// missing measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
}

/// Work out where an image goes on the host, or that it does not.
///
/// `top` is the absolute row the pane's viewport begins at, and `started` the
/// absolute row the image begins at — the same coordinates everything else
/// here counts in.
#[must_use]
pub fn place(
    id: u32,
    started: u64,
    image_rows: u16,
    image_cols: u16,
    top: u64,
    pane: omt_tui::Rect,
) -> Option<Placement> {
    let ends = started.saturating_add(u64::from(image_rows));
    let bottom = top.saturating_add(u64::from(pane.rows));
    // Entirely above or below what the pane is showing.
    if ends <= top || started >= bottom {
        return None;
    }
    // Cut off the top when the image began before the viewport did.
    let skip_rows = u16::try_from(top.saturating_sub(started)).unwrap_or(0);
    let first = started.max(top);
    let offset = u16::try_from(first.saturating_sub(top)).unwrap_or(0);
    let visible = image_rows
        .saturating_sub(skip_rows)
        .min(pane.rows.saturating_sub(offset));
    if visible == 0 {
        return None;
    }
    // A pane is one region of somebody's screen. An image drawn past its edge
    // lands on another session's output, which is worse than not drawing it.
    let cols = image_cols.min(pane.cols);
    if cols == 0 {
        return None;
    }
    Some(Placement {
        id,
        col: pane.col,
        row: pane.row.saturating_add(offset),
        rows: visible,
        cols,
        skip_rows,
    })
}

/// The bytes that put one image on the host's screen.
///
/// Chunked, because a transmission is measured in megabytes and one escape
/// sequence of that size is a write nothing on the path expects.
#[must_use]
pub fn draw(placement: &Placement, data: &str, cell: Cell) -> Vec<u8> {
    let mut out = Vec::new();
    // Save and restore, because placing an image means moving the cursor and
    // the renderer that drew this frame believes it knows where the cursor is.
    out.extend_from_slice(b"\x1b7");
    // One-based on the wire.
    let (row, col) = (
        placement.row.saturating_add(1),
        placement.col.saturating_add(1),
    );
    out.extend_from_slice(format!("\x1b[{row};{col}H").as_bytes());

    // The source rectangle, in pixels, which is what kitty crops with.
    let y = u32::from(placement.skip_rows) * cell.height;
    let h = u32::from(placement.rows) * cell.height;
    let w = u32::from(placement.cols) * cell.width;
    let control = format!(
        "a=T,i={id},f=100,y={y},h={h},w={w},c={cols},r={rows},C=1",
        id = placement.id,
        cols = placement.cols,
        rows = placement.rows,
    );

    let bytes = data.as_bytes();
    let mut chunks = bytes.chunks(CHUNK);
    let first = chunks.next().unwrap_or(b"");
    let more = u8::from(bytes.len() > CHUNK);
    out.extend_from_slice(format!("\x1b_G{control},m={more};").as_bytes());
    out.extend_from_slice(first);
    out.extend_from_slice(b"\x1b\\");
    let mut rest: Vec<&[u8]> = chunks.collect();
    while let Some(chunk) = rest.first().copied() {
        rest.remove(0);
        let more = u8::from(!rest.is_empty());
        out.extend_from_slice(format!("\x1b_Gm={more};").as_bytes());
        out.extend_from_slice(chunk);
        out.extend_from_slice(b"\x1b\\");
    }
    out.extend_from_slice(b"\x1b8");
    out
}

/// How much payload goes in one escape sequence.
const CHUNK: usize = 3072;

/// The bytes that take one image back off the host's screen.
///
/// Sent when a placement goes away, so an image whose rows omt has stopped
/// drawing does not stay on the terminal underneath them.
#[must_use]
pub fn erase(id: u32) -> Vec<u8> {
    format!("\x1b_Ga=d,d=I,i={id}\x1b\\").into_bytes()
}

/// How a cell measures on the terminal omt is drawn on.
///
/// From the terminal, and `None` when it does not say. A guessed cell size
/// crops the wrong part of an image, and a wrong crop looks like a rendering
/// bug rather than a missing measurement — so nothing is drawn instead.
#[must_use]
pub fn cell_size() -> Option<Cell> {
    let size = crossterm::terminal::window_size().ok()?;
    if size.width == 0 || size.height == 0 || size.columns == 0 || size.rows == 0 {
        return None;
    }
    Some(Cell {
        width: u32::from(size.width) / u32::from(size.columns),
        height: u32::from(size.height) / u32::from(size.rows),
    })
    .filter(|cell| cell.width > 0 && cell.height > 0)
}

/// An id of omt's own for one image in one pane.
///
/// Never the id the program used: two panes can show pictures whose programs
/// both called theirs `1`, and one would take the other's placement off the
/// host's screen.
#[must_use]
pub fn host_id(pane: usize, image: usize) -> u32 {
    // Room for far more images per pane than a scrollback holds, and a base
    // that keeps omt's ids clear of the small numbers programs choose.
    const BASE: u32 = 900_000;
    BASE + u32::try_from(pane)
        .unwrap_or(0)
        .saturating_mul(1000)
        .saturating_add(u32::try_from(image).unwrap_or(0))
}

/// What omt has put on the host's screen, so it can tell when nothing changed.
///
/// A frame is drawn many times a second and an image is measured in megabytes;
/// re-sending one per frame is a link that never goes idle.
#[derive(Debug, Default)]
pub struct Drawn {
    /// The placements currently on the host, by the id omt gave them.
    on_screen: std::collections::BTreeMap<u32, Placement>,
}

impl Drawn {
    /// The bytes to bring the host's screen in line with these placements.
    ///
    /// Everything that moved is re-sent; everything that went away is erased;
    /// everything unchanged produces nothing at all.
    pub fn sync(&mut self, wanted: &[(Placement, String)], cell: Cell) -> Vec<u8> {
        let mut out = Vec::new();
        let mut still_there = std::collections::BTreeSet::new();
        for (placement, data) in wanted {
            still_there.insert(placement.id);
            if self.on_screen.get(&placement.id) == Some(placement) {
                continue;
            }
            out.extend_from_slice(&draw(placement, data, cell));
            self.on_screen.insert(placement.id, placement.clone());
        }
        let gone: Vec<u32> = self
            .on_screen
            .keys()
            .copied()
            .filter(|id| !still_there.contains(id))
            .collect();
        for id in gone {
            out.extend_from_slice(&erase(id));
            self.on_screen.remove(&id);
        }
        out
    }

    /// Take everything off, and forget it.
    ///
    /// For leaving: an image left behind outlives the program that drew it and
    /// sits on the shell somebody gets back.
    pub fn clear(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        for id in std::mem::take(&mut self.on_screen).into_keys() {
            out.extend_from_slice(&erase(id));
        }
        out
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

    fn reply(body: &str) -> Vec<u8> {
        format!("\x1b_G{body}\x1b\\").into_bytes()
    }

    #[test]
    fn the_terminals_own_ok_is_the_answer() {
        assert_eq!(
            answer(&reply(&format!("i={PROBE_ID};OK"))),
            Some(Host::Kitty)
        );
    }

    #[test]
    fn a_refusal_is_not_support() {
        // kitty answers a query it cannot satisfy too, and the answer is not
        // `OK`. Treating any reply as support is the assumption this exists
        // to avoid.
        assert_eq!(answer(&reply(&format!("i={PROBE_ID};ENOTSUPPORTED"))), None);
    }

    #[test]
    fn an_answer_to_a_question_omt_did_not_ask_is_ignored() {
        // A pane's own program can write these bytes. An id omt did not use is
        // somebody else's conversation.
        assert_eq!(answer(&reply("i=1;OK")), None);
    }

    #[test]
    fn somebody_typing_is_not_an_answer() {
        assert_eq!(answer(b"hello\x1b[A"), None);
    }

    #[test]
    fn half_an_answer_is_not_an_answer_yet() {
        // The read boundary falls anywhere. Saying "not yet" is what lets the
        // caller keep reading rather than concluding no support.
        let partial = format!("\x1b_Gi={PROBE_ID};O").into_bytes();
        assert_eq!(answer(&partial), None);
    }

    #[test]
    fn nothing_at_all_is_the_ordinary_case() {
        // Most terminals will not answer. That is not a failure, and it must
        // not read like one.
        assert_eq!(answer(b""), None);
        assert_eq!(Host::default(), Host::Nothing);
    }

    fn pane() -> omt_tui::Rect {
        omt_tui::Rect {
            col: 10,
            row: 5,
            cols: 40,
            rows: 20,
        }
    }

    #[test]
    fn an_image_in_view_lands_in_the_panes_own_coordinates() {
        // A pane is a region of somebody's screen; the image goes where the
        // pane is, not where the screen begins.
        let p = place(1, 108, 4, 12, 100, pane()).expect("in view");
        assert_eq!(p.col, 10, "the pane's left edge");
        assert_eq!(p.row, 5 + 8, "eight rows down the pane");
        assert_eq!(p.rows, 4);
        assert_eq!(p.skip_rows, 0);
    }

    #[test]
    fn an_image_hanging_off_the_bottom_is_cut_to_the_pane() {
        // Drawn past the edge it would land on another session's output,
        // which is worse than not drawing it.
        let p = place(1, 118, 10, 12, 100, pane()).expect("partly in view");
        assert_eq!(p.rows, 2, "only what fits inside the pane");
    }

    #[test]
    fn an_image_scrolled_off_the_top_is_drawn_from_where_it_resumes() {
        let p = place(1, 96, 8, 12, 100, pane()).expect("partly in view");
        assert_eq!(p.skip_rows, 4, "the four rows above the viewport");
        assert_eq!(p.row, 5, "at the top of the pane");
        assert_eq!(p.rows, 4);
    }

    #[test]
    fn an_image_nobody_can_see_is_not_drawn() {
        // Above the viewport entirely.
        assert!(place(1, 80, 4, 12, 100, pane()).is_none());
        // And below it.
        assert!(place(1, 200, 4, 12, 100, pane()).is_none());
    }

    #[test]
    fn an_image_wider_than_the_pane_is_narrowed_to_it() {
        let p = place(1, 100, 2, 100, 100, pane()).expect("in view");
        assert_eq!(p.cols, 40);
    }

    #[test]
    fn what_is_drawn_saves_the_cursor_and_puts_it_back() {
        // The renderer that drew this frame believes it knows where the cursor
        // is, and placing an image means moving it.
        let p = place(9, 100, 2, 4, 100, pane()).expect("in view");
        let bytes = draw(
            &p,
            "iVBORw0KGgo",
            Cell {
                width: 8,
                height: 16,
            },
        );
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.starts_with("\x1b7"), "{text:?}");
        assert!(text.ends_with("\x1b8"), "{text:?}");
        assert!(
            text.contains("\x1b[6;11H"),
            "positioned in the pane: {text:?}"
        );
        assert!(text.contains("i=9"), "{text:?}");
        assert!(text.contains("iVBORw0KGgo"), "{text:?}");
    }

    #[test]
    fn the_crop_is_in_pixels_because_that_is_what_the_terminal_crops_with() {
        let p = place(3, 96, 8, 4, 100, pane()).expect("partly in view");
        let bytes = draw(
            &p,
            "iVBORw0KGgo",
            Cell {
                width: 8,
                height: 16,
            },
        );
        let text = String::from_utf8_lossy(&bytes);
        // Four rows skipped, sixteen pixels each.
        assert!(text.contains("y=64"), "{text:?}");
        assert!(text.contains("h=64"), "four visible rows: {text:?}");
    }

    #[test]
    fn a_large_payload_is_sent_in_pieces() {
        // One escape sequence of several megabytes is a write nothing on the
        // path expects.
        let p = place(1, 100, 2, 4, 100, pane()).expect("in view");
        let data = "i".repeat(CHUNK * 2 + 10);
        let bytes = draw(
            &p,
            &data,
            Cell {
                width: 8,
                height: 16,
            },
        );
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            text.contains("m=1;"),
            "a continuation was announced: {}",
            &text[..80]
        );
        assert!(text.contains("m=0;"), "and a last chunk");
    }

    #[test]
    fn an_image_that_went_away_is_taken_off_the_screen() {
        let text = String::from_utf8(erase(4)).expect("utf8");
        assert!(text.contains("a=d"), "{text:?}");
        assert!(text.contains("i=4"), "{text:?}");
    }

    fn placed(id: u32, row: u16) -> (Placement, String) {
        (
            Placement {
                id,
                col: 0,
                row,
                rows: 2,
                cols: 4,
                skip_rows: 0,
            },
            "iVBORw0KGgo".to_owned(),
        )
    }

    #[test]
    fn a_frame_in_which_nothing_moved_sends_nothing() {
        // The whole reason this exists: an image is megabytes and a frame is
        // many times a second.
        let cell = Cell {
            width: 8,
            height: 16,
        };
        let mut drawn = Drawn::default();
        let wanted = vec![placed(1, 3)];
        assert!(
            !drawn.sync(&wanted, cell).is_empty(),
            "the first time draws"
        );
        assert!(drawn.sync(&wanted, cell).is_empty(), "the second does not");
    }

    #[test]
    fn an_image_that_moved_is_sent_again() {
        let cell = Cell {
            width: 8,
            height: 16,
        };
        let mut drawn = Drawn::default();
        drawn.sync(&[placed(1, 3)], cell);
        let after = drawn.sync(&[placed(1, 4)], cell);
        assert!(
            !after.is_empty(),
            "a scrolled image is drawn where it now is"
        );
    }

    #[test]
    fn an_image_that_went_away_is_erased_once() {
        let cell = Cell {
            width: 8,
            height: 16,
        };
        let mut drawn = Drawn::default();
        drawn.sync(&[placed(1, 3)], cell);
        let after = String::from_utf8(drawn.sync(&[], cell)).expect("utf8");
        assert!(after.contains("a=d"), "{after:?}");
        assert!(drawn.sync(&[], cell).is_empty(), "and not again");
    }

    #[test]
    fn leaving_takes_everything_off() {
        // An image left behind outlives the program that drew it and sits on
        // the shell somebody gets back.
        let cell = Cell {
            width: 8,
            height: 16,
        };
        let mut drawn = Drawn::default();
        drawn.sync(&[placed(1, 3), placed(2, 9)], cell);
        let bye = String::from_utf8(drawn.clear()).expect("utf8");
        assert_eq!(bye.matches("a=d").count(), 2);
        assert!(drawn.clear().is_empty());
    }

    #[test]
    fn a_keystroke_that_arrived_during_the_handshake_is_not_swallowed() {
        // The regression this caught for real: everything on that descriptor
        // belongs to the person at the keyboard unless it is the reply that
        // was asked for. Eating the first keystroke is a worse failure than
        // never drawing an image.
        let mixed = format!("ls\n\x1b_Gi={PROBE_ID};OK\x1b\\rest").into_bytes();
        assert_eq!(answer(&mixed), Some(Host::Kitty));
        assert_eq!(typed_around(&mixed), b"ls\nrest");
    }

    #[test]
    fn with_no_reply_in_it_everything_read_is_somebodys_typing() {
        assert_eq!(typed_around(b"ls -la\n"), b"ls -la\n");
    }
}
