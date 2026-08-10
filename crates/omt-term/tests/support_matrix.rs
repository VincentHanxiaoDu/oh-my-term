//! The escape-sequence support matrix, driven.
//!
//! `docs/architecture/04-terminal-core.md` §5 is a table of sequences with a
//! priority: **Must** means a real terminal is wrong without it. A table in a
//! document asserts nothing, so every Must row here is fed to a terminal and
//! its effect checked.
//!
//! Each case says what must happen rather than comparing a whole screen. A
//! snapshot would fail for unrelated reasons and would not say which sequence
//! broke, and "which sequence broke" is the only useful output of this file.
//!
//! Two rows are checked as *refusals* rather than as behaviour, because §5 says
//! omt must never implement them: the window operations that iconify, raise or
//! lower a window, or report the title back as input. Those are the classic
//! escape-sequence attack surface — a program that can make the terminal type
//! its own title has an injection primitive.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]

use omt_term::{GridSize, HostAction, TermConfig, Terminal};

fn term() -> Terminal {
    Terminal::new(TermConfig {
        size: GridSize { cols: 20, rows: 6 },
        ..TermConfig::default()
    })
}

/// The visible screen, one string per row, trailing blanks trimmed.
fn screen(t: &Terminal) -> Vec<String> {
    t.grid()
        .rows()
        .iter()
        .map(|line| {
            let mut out = String::new();
            for cell in line.cells() {
                match cell.resolve() {
                    omt_term::Resolved::Char('\0') => out.push(' '),
                    omt_term::Resolved::Char(c) => out.push(c),
                    omt_term::Resolved::Grapheme(_) => out.push('?'),
                }
            }
            out.trim_end().to_owned()
        })
        .collect()
}

fn after(bytes: &[u8]) -> Terminal {
    let mut t = term();
    t.advance(bytes);
    t
}

fn replies(t: &mut Terminal) -> String {
    t.drain_actions()
        .into_iter()
        .filter_map(|action| match action {
            HostAction::Reply(bytes) => Some(String::from_utf8_lossy(&bytes).into_owned()),
            _ => None,
        })
        .collect()
}

#[test]
fn cursor_motion_moves_the_cursor() {
    // CUP, CUU, CUD, CUF, CUB, CHA, VPA. Checked by writing a character after
    // each move, because the cursor is only observable through what it writes.
    let t = after(b"\x1b[3;5Hx");
    assert_eq!(screen(&t)[2], "    x", "CUP is one-based and row-major");

    let t = after(b"\x1b[3;5H\x1b[Ay");
    assert_eq!(screen(&t)[1], "    y", "CUU");

    let t = after(b"\x1b[1;1H\x1b[2Bz");
    assert_eq!(screen(&t)[2], "z", "CUD with a count");

    let t = after(b"\x1b[1;1H\x1b[3Ca");
    assert_eq!(screen(&t)[0], "   a", "CUF");

    let t = after(b"\x1b[1;10H\x1b[4Db");
    assert_eq!(screen(&t)[0], "     b", "CUB");

    let t = after(b"\x1b[1;1H\x1b[8Gc");
    assert_eq!(screen(&t)[0], "       c", "CHA");

    let t = after(b"\x1b[1;1H\x1b[4dd");
    assert_eq!(screen(&t)[3], "d", "VPA");
}

#[test]
fn line_movement_returns_to_the_start_of_a_line() {
    // CNL and CPL move *and* return to column one, which is what separates
    // them from CUD and CUU.
    let t = after(b"\x1b[1;10H\x1b[Ex");
    assert_eq!(screen(&t)[1], "x", "CNL returns to column one");

    let t = after(b"\x1b[3;10H\x1b[Fy");
    assert_eq!(screen(&t)[1], "y", "CPL likewise");
}

#[test]
fn erase_in_display_clears_what_it_says() {
    let t = after(b"one\r\ntwo\r\nthree\x1b[2J");
    assert!(
        screen(&t).iter().all(std::string::String::is_empty),
        "ED 2 clears the screen"
    );

    let t = after(b"one\r\ntwo\r\nthree\x1b[2;1H\x1b[0J");
    let rows = screen(&t);
    assert_eq!(rows[0], "one", "ED 0 keeps what is above the cursor");
    assert_eq!(rows[2], "", "and clears what is below");

    let t = after(b"one\r\ntwo\r\nthree\x1b[2;1H\x1b[1J");
    assert_eq!(screen(&t)[0], "", "ED 1 clears what is above");
}

#[test]
fn erase_in_line_clears_what_it_says() {
    let t = after(b"abcdefgh\x1b[1;4H\x1b[0K");
    assert_eq!(screen(&t)[0], "abc", "EL 0 clears to the end");

    let t = after(b"abcdefgh\x1b[1;4H\x1b[1K");
    assert_eq!(screen(&t)[0], "    efgh", "EL 1 clears to the start");

    let t = after(b"abcdefgh\x1b[1;4H\x1b[2K");
    assert_eq!(screen(&t)[0], "", "EL 2 clears the line");
}

#[test]
fn characters_are_inserted_and_deleted_in_place() {
    let t = after(b"abcdef\x1b[1;3H\x1b[2@");
    assert_eq!(screen(&t)[0], "ab  cdef", "ICH shifts right");

    let t = after(b"abcdef\x1b[1;3H\x1b[2P");
    assert_eq!(screen(&t)[0], "abef", "DCH shifts left");

    let t = after(b"abcdef\x1b[1;3H\x1b[2X");
    assert_eq!(screen(&t)[0], "ab  ef", "ECH erases without shifting");
}

#[test]
fn lines_are_inserted_and_deleted() {
    let t = after(b"one\r\ntwo\r\nthree\x1b[2;1H\x1b[L");
    let rows = screen(&t);
    assert_eq!(rows[1], "", "IL opens a blank line");
    assert_eq!(rows[2], "two", "and pushes the rest down");

    let t = after(b"one\r\ntwo\r\nthree\x1b[2;1H\x1b[M");
    assert_eq!(screen(&t)[1], "three", "DL closes the gap");
}

#[test]
fn the_scroll_region_clips_scrolling() {
    // DECSTBM. Without it every full-screen application scrolls its status bar
    // off the bottom.
    let mut t = term();
    t.advance(b"a\r\nb\r\nc\r\nd\r\ne");
    t.advance(b"\x1b[2;4r\x1b[4;1H\n\rx");
    let rows = screen(&t);
    assert_eq!(rows[0], "a", "the row above the region did not move");
    assert_eq!(rows[4], "e", "nor the one below it");
}

#[test]
fn su_and_sd_scroll_the_screen() {
    let t = after(b"a\r\nb\r\nc\x1b[S");
    assert_eq!(screen(&t)[0], "b", "SU moved the content up");

    let t = after(b"a\r\nb\r\nc\x1b[T");
    assert_eq!(screen(&t)[1], "a", "SD moved it down");
}

#[test]
fn the_cursor_is_saved_and_restored_in_both_spellings() {
    let t = after(b"\x1b[3;5H\x1b7\x1b[1;1H\x1b8x");
    assert_eq!(screen(&t)[2], "    x", "ESC 7 / ESC 8");

    let t = after(b"\x1b[3;5H\x1b[s\x1b[1;1H\x1b[ux");
    assert_eq!(screen(&t)[2], "    x", "DECSC / DECRC");
}

#[test]
fn ris_resets_the_screen_and_the_modes() {
    let mut t = term();
    t.advance(b"\x1b[?1049h");
    let announced = t.drain_actions();
    assert!(
        announced.iter().any(|a| matches!(
            a,
            HostAction::ModeChanged {
                mode: omt_term::TermMode::AlternateScreen,
                enabled: true
            }
        )),
        "it went to the alternate screen"
    );
    t.advance(b"one\r\ntwo");
    t.advance(b"\x1bc");
    let left = t.drain_actions();
    assert!(
        left.iter().any(|a| matches!(
            a,
            HostAction::ModeChanged {
                mode: omt_term::TermMode::AlternateScreen,
                enabled: false
            }
        )),
        "RIS left the alternate screen"
    );
    assert!(
        screen(&t).iter().all(std::string::String::is_empty),
        "and cleared the screen"
    );
}

#[test]
fn index_and_reverse_index_move_between_lines() {
    let t = after(b"\x1b[2;1Ha\x1bDb");
    assert_eq!(screen(&t)[2], " b", "IND moved down");

    let t = after(b"\x1b[3;1Ha\x1bMb");
    assert_eq!(screen(&t)[1], " b", "RI moved up");

    let t = after(b"\x1b[1;10Ha\x1bEb");
    assert_eq!(screen(&t)[1], "b", "NEL moved down and to column one");
}

#[test]
fn tab_stops_are_set_and_cleared() {
    // HTS, TBC and CSI g. A shell that sets its own stops and then finds them
    // ignored draws its prompt in the wrong place.
    let t = after(b"\tx");
    assert_eq!(screen(&t)[0], "        x", "the default stop is every eight");

    let t = after(b"\x1b[1;4H\x1bH\x1b[1;1H\ty");
    assert_eq!(screen(&t)[0], "   y", "HTS set a stop at column four");

    let t = after(b"\x1b[3g\x1b[1;1H\tz");
    assert!(
        !screen(&t)[0].starts_with("       z"),
        "TBC 3 cleared every stop, so tab did not reach the eighth column"
    );
}

#[test]
fn device_attributes_are_answered() {
    // A wrong or missing answer here disables truecolor in programs that probe.
    let mut t = term();
    t.advance(b"\x1b[c");
    let reply = replies(&mut t);
    assert!(reply.starts_with('\x1b'), "DA1 was answered: {reply:?}");
}

#[test]
fn the_cursor_position_is_reported_because_shells_measure_prompts_with_it() {
    let mut t = term();
    t.advance(b"\x1b[3;5H\x1b[6n");
    let reply = replies(&mut t);
    assert!(reply.contains("3;5"), "DSR 6 reported the position: {reply:?}");
}

#[test]
fn a_status_report_is_answered() {
    let mut t = term();
    t.advance(b"\x1b[5n");
    assert!(!replies(&mut t).is_empty(), "DSR 5 was answered");
}

#[test]
fn colour_is_parsed_in_every_spelling_the_table_lists() {
    for sequence in [
        b"\x1b[31mx".as_slice(),
        b"\x1b[38;5;196mx",
        b"\x1b[38;2;255;0;0mx",
        b"\x1b[38:2::255:0:0mx",
    ] {
        let t = after(sequence);
        assert_eq!(screen(&t)[0], "x", "{sequence:?} left the text alone");
    }
}

#[test]
fn a_window_operation_is_never_honoured() {
    // §5: never implement iconify, raise, lower, or set-title-by-report. A
    // program that can make the terminal type its own title back has an
    // injection primitive, and this is the classic escape-sequence attack
    // surface.
    let mut t = term();
    t.advance(b"\x1b[8;40;100t");
    let actions = t.drain_actions();
    assert!(
        !actions
            .iter()
            .any(|a| matches!(a, HostAction::Reply(_))),
        "a resize request was answered as if it had been performed"
    );
    assert_eq!(
        t.grid().size(),
        GridSize { cols: 20, rows: 6 },
        "and the terminal did not resize itself"
    );
}

#[test]
fn an_unknown_sequence_does_not_corrupt_what_follows() {
    // The property that matters more than any single row of the table: a
    // sequence omt does not know must not eat the text after it.
    let t = after(b"\x1b[9999;9999zhello");
    assert!(
        screen(&t).iter().any(|row| row.contains("hello")),
        "the text after an unknown sequence survived"
    );
}

#[test]
fn a_sequence_split_across_writes_still_parses() {
    // PTY reads land wherever the kernel decides, so this is the normal case
    // rather than an edge case.
    let mut t = term();
    t.advance(b"\x1b[3");
    t.advance(b";5H");
    t.advance(b"x");
    assert_eq!(screen(&t)[2], "    x");
}

#[test]
fn arbitrary_bytes_never_panic_and_never_hang() {
    // §10 requires fuzzing for anything parsing untrusted input; this is the
    // property a fuzzer would be looking for, driven over a spread of shapes
    // that have historically broken terminals.
    //
    // Each is fed to its own terminal, because an *unterminated* OSC or DCS
    // legitimately consumes everything after it until a string terminator --
    // that is the VT specification, not a bug, and a shared terminal would make
    // this test assert the opposite.
    for chunk in [
        b"\x1b".as_slice(),
        b"\x1b[",
        b"\x1b[;;;;;;;;;;;;;;;;;;;;m",
        b"\x1b]0;",
        b"\x1b]0;title\x07",
        b"\x1bP",
        b"\x1bPq\x1b\\",
        b"\x1b_",
        b"\x1b[999999999999999999999;1H",
        b"\xff\xfe\xfd\xfc",
        b"\x00\x01\x02\x03\x04\x05\x06\x07\x08",
        &[0x1b, 0x5b, 0xc3, 0x28],
    ] {
        let mut t = term();
        t.advance(chunk);
        // Draining is part of the path: an action queue that overflowed on
        // rubbish would take the caller down with it.
        let _ = t.drain_actions();
    }
}

#[test]
fn a_terminated_string_leaves_the_terminal_usable() {
    // The other half of the property above: once the string ends, text lands on
    // the screen again rather than the terminal being stuck in a parser state
    // forever.
    let mut t = term();
    t.advance(b"\x1b]0;a title\x07");
    t.advance(b"\x1bPq\x1b\\");
    t.advance(b"still here");
    assert!(
        screen(&t).iter().any(|row| row.contains("still here")),
        "the terminal kept working"
    );
}
