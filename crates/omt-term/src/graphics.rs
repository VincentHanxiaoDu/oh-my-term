//! Seeing an image a program drew.
//!
//! An APC string — which is how the kitty graphics protocol travels — is
//! consumed by `vte` in a parser state that makes **no callback at all**. So a
//! program drawing an image inside omt produced nothing: not a placeholder,
//! not a warning, not a gap. The bytes vanished and the next line landed
//! exactly where the image should have been, which makes a program that drew
//! something look like a program that did nothing.
//!
//! This is the part that has to see it. Rendering it is a separate machine —
//! asking the *outer* terminal to draw, clipped to the pane's rectangle — and
//! it can only be built on top of knowing an image is there and how much room
//! it takes.

/// The largest transmission held while it is being assembled.
///
/// The bound is on the *whole* accumulated image, and passing it drops all of
/// it. Drawing an image from a prefix of its own bytes is a wrong image, and a
/// wrong image is worse than an absent one.
pub const MAX_TRANSMISSION: usize = 16 << 20; // 16 MiB

/// An image that arrived, and where it went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// The id the program gave it, where it gave one.
    pub id: Option<u32>,
    /// Which protocol it came in on.
    pub via: Protocol,
    /// How wide, in cells, as far as the sequence says.
    pub cols: u16,
    /// How tall, in cells.
    pub rows: u16,
    /// How many bytes of payload arrived.
    pub bytes: usize,
    /// What it is called, where the sequence said.
    pub name: Option<String>,
}

/// Which protocol carried it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// kitty's, over APC.
    Kitty,
    /// iTerm2's, over OSC 1337.
    ITerm2,
}

/// Where the scanner is.
///
/// A pty read boundary falls anywhere, so every one of these states has to
/// survive being left in the middle of a sequence and resumed on the next
/// read.
#[derive(Debug, Default)]
enum Where {
    /// Not in a sequence.
    #[default]
    Idle,
    /// An escape has been seen and is being held until its next byte says
    /// what it introduces.
    Escape,
    /// Inside an APC string.
    Apc {
        /// What has arrived so far.
        payload: Vec<u8>,
        /// Whether the last byte was an escape, which is half a terminator.
        escaped: bool,
        /// Whether this transmission has already outgrown the bound. Kept
        /// rather than aborted so the *rest* of the sequence is still consumed
        /// and does not land on the screen as text.
        overflowed: bool,
    },
}

/// What the scanner decided about one byte.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    /// Feed these bytes onward to the parser.
    Pass(Vec<u8>),
    /// Taken; the parser must not see it.
    Took,
    /// Taken, and it completed a transmission.
    Complete(Box<Complete>),
}

/// A finished APC string, with whatever an image needs read out of it.
#[derive(Debug, PartialEq, Eq)]
pub struct Complete {
    /// The image, where the string described one.
    pub image: Option<Image>,
    /// Whether the program asked for the cursor to stay put.
    pub keep_cursor: bool,
}

/// Takes APC strings out of the stream before the parser can swallow them.
#[derive(Debug, Default)]
pub struct Scanner {
    at: Where,
    /// Chunks of a transmission being assembled across `m=1` continuations.
    pending: Option<Pending>,
}

/// A transmission still arriving in pieces.
#[derive(Debug)]
struct Pending {
    keys: Keys,
    bytes: usize,
    overflowed: bool,
}

impl Scanner {
    /// Offer one byte.
    pub fn step(&mut self, byte: u8) -> Step {
        match std::mem::take(&mut self.at) {
            Where::Idle => {
                if byte == 0x1b {
                    // Held, not passed. Whether this escape belongs to omt or
                    // to the parser is not knowable until the next byte, and
                    // that byte may be in the next read.
                    self.at = Where::Escape;
                    Step::Took
                } else {
                    Step::Pass(vec![byte])
                }
            }
            Where::Escape => {
                if byte == b'_' {
                    self.at = Where::Apc {
                        payload: Vec::new(),
                        escaped: false,
                        overflowed: false,
                    };
                    Step::Took
                } else {
                    // Not ours. The held escape goes on with it, in order.
                    Step::Pass(vec![0x1b, byte])
                }
            }
            Where::Apc {
                mut payload,
                escaped,
                mut overflowed,
            } => {
                // Either terminator: the string form and the bell some
                // programs use instead.
                if (escaped && byte == b'\\') || byte == 0x07 {
                    if escaped && byte == b'\\' {
                        payload.pop();
                    }
                    return Step::Complete(Box::new(self.finish(&payload, overflowed)));
                }
                if payload.len() >= MAX_TRANSMISSION {
                    // Marked and still consumed. Stopping here would put the
                    // remainder of the image on the screen as text.
                    overflowed = true;
                    payload.clear();
                }
                payload.push(byte);
                self.at = Where::Apc {
                    payload,
                    escaped: byte == 0x1b,
                    overflowed,
                };
                Step::Took
            }
        }
    }

    /// Whether a sequence is in flight, and so whether a caller holding bytes
    /// back is right to.
    #[must_use]
    pub const fn mid_sequence(&self) -> bool {
        !matches!(self.at, Where::Idle)
    }

    fn finish(&mut self, payload: &[u8], overflowed: bool) -> Complete {
        let nothing = Complete {
            image: None,
            keep_cursor: false,
        };
        // Everything that is not `G` is somebody else's APC — a shell
        // integration, a private protocol, a terminal omt does not model.
        // Consumed and not drawn from, which is what it was before, minus the
        // part where it left the payload on the screen.
        let Some(body) = payload.strip_prefix(b"G") else {
            self.pending = None;
            return nothing;
        };
        let (control, data) = match body.iter().position(|b| *b == b';') {
            Some(at) => (&body[..at], &body[at + 1..]),
            None => (body, &body[body.len()..]),
        };
        let keys = Keys::read(control);
        let keep_cursor = keys.keep_cursor;

        // A continuation carries its keys only in the first chunk, so the
        // first chunk's are the ones kept.
        let mut held = self.pending.take().unwrap_or(Pending {
            keys,
            bytes: 0,
            overflowed: false,
        });
        held.bytes += data.len();
        held.overflowed = held.overflowed || overflowed || held.bytes > MAX_TRANSMISSION;

        if keys.more {
            self.pending = Some(held);
            return Complete {
                image: None,
                keep_cursor,
            };
        }
        if held.overflowed {
            // The whole transmission, not the part that fitted.
            return Complete {
                image: None,
                keep_cursor,
            };
        }
        Complete {
            image: Some(Image {
                id: held.keys.id,
                via: Protocol::Kitty,
                cols: held.keys.cols,
                rows: held.keys.rows,
                bytes: held.bytes,
                name: None,
            }),
            keep_cursor,
        }
    }
}

/// The keys omt reads out of a graphics command.
///
/// Only what is needed to know an image arrived and how much room it takes.
/// A key omt does not model is not a key omt should refuse: the rest is left
/// alone rather than treated as an error.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Keys {
    id: Option<u32>,
    more: bool,
    cols: u16,
    rows: u16,
    keep_cursor: bool,
}

impl Keys {
    fn read(control: &[u8]) -> Self {
        let mut keys = Self::default();
        let text = String::from_utf8_lossy(control);
        let (mut width, mut height) = (0u32, 0u32);
        for pair in text.split(',') {
            let Some((key, value)) = pair.split_once('=') else {
                continue;
            };
            match key {
                "i" => keys.id = value.parse().ok(),
                "m" => keys.more = value == "1",
                "c" => keys.cols = value.parse().unwrap_or(0),
                "r" => keys.rows = value.parse().unwrap_or(0),
                "s" => width = value.parse().unwrap_or(0),
                "v" => height = value.parse().unwrap_or(0),
                // A program that asked for the cursor to stay is drawing
                // something it means to write over.
                "C" => keys.keep_cursor = value == "1",
                _ => {}
            }
        }
        if keys.cols == 0 || keys.rows == 0 {
            // No cell size given. Derived from the pixel size against a cell
            // omt has to assume, because the sequence does not carry one — and
            // an image whose height cannot be worked out still gets one row,
            // so there is somewhere to say it is there.
            let (cell_w, cell_h) = (ASSUMED_CELL.0, ASSUMED_CELL.1);
            if keys.cols == 0 {
                keys.cols = u16::try_from(width.div_ceil(cell_w))
                    .unwrap_or(u16::MAX)
                    .max(1);
            }
            if keys.rows == 0 {
                keys.rows = u16::try_from(height.div_ceil(cell_h))
                    .unwrap_or(u16::MAX)
                    .max(1);
            }
        }
        keys
    }
}

/// The cell size assumed when a sequence gives pixels and nothing gives cells.
///
/// Stated rather than hidden: the real size belongs to the terminal omt is
/// drawn on and arrives with a resize. Until then this is a guess, and the
/// only thing it decides is how many rows a card occupies.
const ASSUMED_CELL: (u32, u32) = (8, 16);

/// Read an iTerm2 inline-image OSC into the same shape.
///
/// The other product parses this key and logs it as unimplemented, which is a
/// silent drop. It is the same problem as the kitty one and gets the same
/// answer.
#[must_use]
pub fn iterm2(argument: &str) -> Option<Image> {
    // `File=` introduces the arguments; without stripping it the first pair
    // reads as `File=inline` and the image is never recognised as inline.
    let argument = argument
        .strip_prefix("File=")
        .or_else(|| argument.strip_prefix("MultipartFile="))
        .unwrap_or(argument);
    let (keys, payload) = argument.split_once(':')?;
    let mut image = Image {
        id: None,
        via: Protocol::ITerm2,
        cols: 0,
        rows: 0,
        bytes: payload.len(),
        name: None,
    };
    let mut inline = false;
    for pair in keys.split(';') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        match key {
            "inline" => inline = value == "1",
            "width" => image.cols = cells(value),
            "height" => image.rows = cells(value),
            "name" => image.name = decode_name(value),
            _ => {}
        }
    }
    if !inline {
        // `inline=0` is a download, not something to draw. Treating it as an
        // image would reserve rows for a file that was never meant to appear.
        return None;
    }
    image.cols = image.cols.max(1);
    image.rows = image.rows.max(1);
    Some(image)
}

/// A size in the iTerm2 spelling, which may be cells, pixels or a percentage.
fn cells(value: &str) -> u16 {
    if let Some(pixels) = value.strip_suffix("px") {
        return pixels
            .parse::<u32>()
            .map(|p| u16::try_from(p.div_ceil(ASSUMED_CELL.1)).unwrap_or(u16::MAX))
            .unwrap_or(0);
    }
    if value.ends_with('%') {
        // A proportion of a viewport this layer does not know. Left to the
        // caller's default rather than guessed at.
        return 0;
    }
    value.parse().unwrap_or(0)
}

/// The name, which travels base64-encoded.
fn decode_name(value: &str) -> Option<String> {
    let index = |c: u8| -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some(u32::from(c - b'A')),
            b'a'..=b'z' => Some(u32::from(c - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(c - b'0') + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    };
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    for byte in value.bytes() {
        let Some(value) = index(byte) else { continue };
        acc = (acc << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            #[allow(clippy::cast_possible_truncation, reason = "masked to a byte")]
            out.push(((acc >> bits) & 0xff) as u8);
        }
    }
    String::from_utf8(out).ok().filter(|name| !name.is_empty())
}

/// What to draw in the rows an image reserved.
///
/// Blankness is the one outcome that cannot be told apart from a failure, so
/// the rows say what arrived. This is `GraphicsProtocol::None` from the media
/// document, and it is written into the grid rather than painted by a
/// renderer: cells scroll, copy, seed and reach the web client on their own,
/// and an overlay would have to be reimplemented on each surface to match.
#[must_use]
pub fn card(image: &Image, cols: u16, rows: u16) -> Vec<String> {
    let width = usize::from(cols.max(4));
    let what = image.name.as_deref().unwrap_or("image");
    let via = match image.via {
        Protocol::Kitty => "kitty",
        Protocol::ITerm2 => "iterm2",
    };
    let headline = format!("▶ {what} · {}×{} · {via}", image.cols, image.rows);
    let mut lines = vec![clip(&headline, width)];
    if rows >= 2 {
        lines.push(clip(&format!("  {} · o to open", size(image.bytes)), width));
    }
    // The rest stays blank: the rows belong to the image, and filling them
    // with border would make a small image look like a large box.
    while lines.len() < usize::from(rows) {
        lines.push(String::new());
    }
    lines.truncate(usize::from(rows.max(1)));
    lines
}

/// Bytes, in what somebody reads rather than what a machine counted.
fn size(bytes: usize) -> String {
    // Base-1024, because that is what the tools somebody compares this against
    // print, and a number that disagrees with `ls -lh` reads as a bug.
    match bytes {
        0..=1023 => format!("{bytes} B"),
        1024..=1_048_575 => format!("{:.0} KiB", bytes as f64 / 1024.0),
        _ => format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0)),
    }
}

/// Cut to the pane's width, by characters rather than bytes.
fn clip(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    text.chars()
        .take(width.saturating_sub(1))
        .collect::<String>()
        + "…"
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    /// Feed a whole string and collect what came out.
    fn scan(scanner: &mut Scanner, text: &[u8]) -> (Vec<u8>, Vec<Complete>) {
        let (mut passed, mut done) = (Vec::new(), Vec::new());
        for byte in text {
            match scanner.step(*byte) {
                Step::Pass(bytes) => passed.extend(bytes),
                Step::Took => {}
                Step::Complete(complete) => done.push(*complete),
            }
        }
        (passed, done)
    }

    #[test]
    fn a_graphics_sequence_is_taken_out_of_the_stream() {
        // The defect underneath everything: the parser consumes an APC string
        // and calls nothing, so an image was invisible rather than unhandled.
        let mut scanner = Scanner::default();
        let (passed, done) = scan(&mut scanner, b"before\x1b_Gi=1,c=10,r=4;AAAA\x1b\\after");
        assert_eq!(passed, b"beforeafter", "the payload leaked onto the screen");
        assert_eq!(done.len(), 1);
        let image = done[0].image.as_ref().expect("an image");
        assert_eq!((image.cols, image.rows), (10, 4));
        assert_eq!(image.id, Some(1));
    }

    #[test]
    fn a_sequence_split_across_reads_is_still_one_sequence() {
        // A pty read boundary falls anywhere, including between the escape and
        // the byte that says what it introduces.
        let mut scanner = Scanner::default();
        let (passed, done) = scan(&mut scanner, b"\x1b");
        assert!(passed.is_empty() && done.is_empty());
        assert!(scanner.mid_sequence());
        let (passed, done) = scan(&mut scanner, b"_Gc=2,r=1;AA");
        assert!(passed.is_empty() && done.is_empty());
        let (passed, done) = scan(&mut scanner, b"\x1b\\");
        assert!(passed.is_empty());
        assert_eq!(done.len(), 1);
        assert!(done[0].image.is_some());
    }

    #[test]
    fn an_escape_that_introduces_something_else_is_passed_on_whole() {
        // The held escape has to come back out in order, or every other
        // sequence in the stream breaks.
        let mut scanner = Scanner::default();
        let (passed, done) = scan(&mut scanner, b"\x1b[31mred");
        assert_eq!(passed, b"\x1b[31mred");
        assert!(done.is_empty());
    }

    #[test]
    fn an_apc_that_is_not_graphics_is_consumed_and_draws_nothing() {
        // Somebody else's private protocol. Consumed, which is what it was
        // before — minus leaving its payload on the screen.
        let mut scanner = Scanner::default();
        let (passed, done) = scan(&mut scanner, b"\x1b_Xsomething\x1b\\rest");
        assert_eq!(passed, b"rest");
        assert_eq!(done.len(), 1);
        assert!(done[0].image.is_none());
    }

    #[test]
    fn a_bell_ends_it_too() {
        let mut scanner = Scanner::default();
        let (_, done) = scan(&mut scanner, b"\x1b_Gc=1,r=1;A\x07");
        assert_eq!(done.len(), 1);
        assert!(done[0].image.is_some());
    }

    #[test]
    fn an_image_sent_in_pieces_becomes_one_image() {
        let mut scanner = Scanner::default();
        let (_, done) = scan(&mut scanner, b"\x1b_Gi=7,c=3,r=2,m=1;AAAA\x1b\\");
        assert_eq!(done.len(), 1);
        assert!(done[0].image.is_none(), "not finished yet");
        let (_, done) = scan(&mut scanner, b"\x1b_Gm=0;BBBB\x1b\\");
        let image = done[0].image.as_ref().expect("the finished image");
        assert_eq!(image.id, Some(7), "the first chunk's keys are the image's");
        assert_eq!((image.cols, image.rows), (3, 2));
        assert_eq!(image.bytes, 8, "both chunks");
    }

    #[test]
    fn a_transmission_that_never_finishes_is_not_an_image() {
        let mut scanner = Scanner::default();
        let (_, done) = scan(&mut scanner, b"\x1b_Gc=3,r=2,m=1;AAAA\x1b\\");
        assert!(done[0].image.is_none());
        // And nothing later invents one from it.
        let (passed, done) = scan(&mut scanner, b"plain text");
        assert_eq!(passed, b"plain text");
        assert!(done.is_empty());
    }

    #[test]
    fn a_program_can_ask_for_the_cursor_to_stay() {
        let mut scanner = Scanner::default();
        let (_, done) = scan(&mut scanner, b"\x1b_Gc=1,r=1,C=1;A\x1b\\");
        assert!(done[0].keep_cursor);
    }

    #[test]
    fn a_size_in_pixels_becomes_a_size_in_rows() {
        let mut scanner = Scanner::default();
        let (_, done) = scan(&mut scanner, b"\x1b_Gs=80,v=64;A\x1b\\");
        let image = done[0].image.as_ref().expect("an image");
        assert_eq!((image.cols, image.rows), (10, 4));
    }

    #[test]
    fn an_image_with_no_size_at_all_still_gets_somewhere_to_say_it_is_there() {
        // Zero rows is an image the next line writes over, which is
        // indistinguishable from one that never arrived.
        let mut scanner = Scanner::default();
        let (_, done) = scan(&mut scanner, b"\x1b_Ga=T;AAAA\x1b\\");
        let image = done[0].image.as_ref().expect("an image");
        assert!(image.rows >= 1 && image.cols >= 1);
    }

    #[test]
    fn the_iterm_sequence_is_read_rather_than_dropped() {
        // The other product parses this key and logs it as unimplemented,
        // which on screen is indistinguishable from nothing having happened.
        let image =
            iterm2("File=inline=1;width=12;height=3;name=cGljLnBuZw==:AAAA").expect("an image");
        assert_eq!((image.cols, image.rows), (12, 3));
        assert_eq!(image.name.as_deref(), Some("pic.png"));
        assert_eq!(image.via, Protocol::ITerm2);
    }

    #[test]
    fn the_rows_say_what_arrived() {
        // Never nothing. Blankness is the one outcome indistinguishable from
        // a failure, and it is what somebody saw before this existed.
        let image = Image {
            id: Some(1),
            via: Protocol::Kitty,
            cols: 20,
            rows: 3,
            bytes: 4096,
            name: Some("plot.png".into()),
        };
        let card = card(&image, 40, 3);
        assert_eq!(card.len(), 3);
        assert!(card[0].contains("plot.png"), "{card:?}");
        assert!(card[0].contains("20×3"), "{card:?}");
        assert!(card[1].contains("4 KiB"), "{card:?}");
    }

    #[test]
    fn a_card_never_outgrows_the_rows_or_the_width_it_was_given() {
        // A card wider than the pane would wrap, and a wrapped card takes
        // rows the image did not reserve — which puts the next line back on
        // top of it.
        let image = Image {
            id: None,
            via: Protocol::ITerm2,
            cols: 1,
            rows: 1,
            bytes: 10,
            name: Some("a-very-long-file-name-indeed.png".into()),
        };
        let card = card(&image, 12, 1);
        assert_eq!(card.len(), 1);
        assert!(card[0].chars().count() <= 12, "{card:?}");
    }

    #[test]
    fn a_download_is_not_an_image() {
        // `inline=0` means send me a file. Reserving rows for it would put a
        // card on the screen for something never meant to appear.
        assert!(iterm2("File=inline=0;name=cGljLnBuZw==:AAAA").is_none());
    }

    /// A terminal of a known size, driven the way a pane's program drives one.
    fn term(cols: u16, rows: u16) -> crate::Terminal {
        crate::Terminal::new(crate::TermConfig {
            size: crate::GridSize::new(cols, rows),
            ..crate::TermConfig::default()
        })
    }

    fn feed(t: &mut crate::Terminal, text: &str) {
        t.advance(text.as_bytes());
    }

    #[test]
    fn an_image_takes_the_rows_it_needs_and_what_follows_lands_below_it() {
        // The whole defect: an image occupying nothing is one the next line
        // writes over, which looks exactly like the program having done
        // nothing at all.
        let mut t = term(40, 10);
        feed(&mut t, "before\r\n");
        feed(&mut t, "\x1b_Gi=1,c=6,r=3;AAAA\x1b\\");
        feed(&mut t, "after\r\n");
        let screen = t.screen_text();
        assert!(screen[0].starts_with("before"), "{screen:?}");
        // The rows are the image's, and they say so — blankness is the one
        // outcome nobody can tell apart from a failure.
        assert!(screen[1].contains("image"), "{screen:?}");
        assert!(screen[1].contains("6×3"), "{screen:?}");
        assert!(screen[4].starts_with("after"), "{screen:?}");

        let images = t.images();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].0, 1, "the row it started on");
        assert_eq!(images[0].1.rows, 3);
    }

    #[test]
    fn an_image_that_asked_to_keep_the_cursor_takes_no_rows() {
        // A program that set `C=1` is drawing something it means to write
        // over, and moving its cursor would break its own layout.
        let mut t = term(40, 6);
        feed(&mut t, "\x1b_Gc=4,r=2,C=1;AAAA\x1b\\");
        feed(&mut t, "here");
        assert!(
            t.screen_text()[0].starts_with("here"),
            "the cursor moved when the program asked it not to"
        );
        assert_eq!(t.images().len(), 1, "it is still recorded");
    }

    #[test]
    fn a_graphics_payload_never_lands_on_the_screen_as_text() {
        // What made this worth doing: before, the parser consumed the whole
        // string and called nothing, so the image was invisible — and a
        // scanner that stopped mid-string would be worse, printing base64
        // across somebody's terminal.
        let mut t = term(40, 6);
        feed(&mut t, "\x1b_Gi=9,c=2,r=1;SGVsbG8gd29ybGQ=\x1b\\x");
        let screen = t.screen_text().join("\n");
        assert!(
            !screen.contains("SGVsbG8"),
            "the payload was drawn as text: {screen:?}"
        );
    }

    #[test]
    fn the_iterm_inline_image_reaches_the_screen_as_rows_too() {
        // Same problem, other protocol, same answer. `imgcat` is the common
        // way an image gets into a terminal at all.
        let mut t = term(40, 10);
        feed(
            &mut t,
            "\x1b]1337;File=inline=1;width=8;height=2;name=cGljLnBuZw==:QUFB\x07",
        );
        feed(&mut t, "after");
        let screen = t.screen_text();
        assert!(screen[2].starts_with("after"), "{screen:?}");
        let images = t.images();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].1.name.as_deref(), Some("pic.png"));
    }
}
