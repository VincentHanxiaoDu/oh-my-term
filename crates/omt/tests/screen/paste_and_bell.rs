//! Behaviour through a real pty, split out of `main.rs` to keep each file
//! under the hard line limit — in the same binary, which the screen suite
//! must stay (cargo would otherwise run a second screen suite in parallel,
//! and sixteen ptys at once is a test of the machine, not of omt).

use super::{Screen, serially};

#[test]
fn a_multiline_paste_is_asked_about_before_it_reaches_the_shell() {
    let _serial = serially();
    // `paste_confirm_lines` and `paste_confirm_on_newline` were in the
    // settings reference and reached nothing: a paste of two hundred lines
    // went straight into the shell, where its first line ran. Here a
    // two-line paste is typed with bracketed markers, which is exactly how a
    // terminal delivers one.
    let mut s = Screen::start_with_config("terminal.paste_confirm_lines = 1\n");
    s.settle(2.0);
    s.type_in("\x1b[200~line-one\ndanger\n\x1b[201~");
    s.settle_until(10.0, |s| s.screen().contains("lines? y to send"));
    assert!(
        s.screen().contains("paste 3 lines? y to send"),
        "no confirmation was asked:\n{}",
        s.screen()
    );
    assert!(
        !s.screen().contains("danger"),
        "the paste reached the shell before the question was answered:\n{}",
        s.screen()
    );
    // y hands it over; the shell echoes it.
    s.type_in("y");
    s.settle_until(10.0, |s| s.screen().contains("danger"));
    assert!(
        s.screen().contains("danger"),
        "the approved paste never reached the shell:\n{}",
        s.screen()
    );

    // And a second multiline paste can be dropped with any other key.
    s.type_in("\x1b[200~line-three\nfour\n\x1b[201~");
    s.settle_until(10.0, |s| s.screen().contains("lines? y to send"));
    s.type_in("n");
    s.settle(1.0);
    assert!(
        !s.screen().contains("line-three"),
        "a dropped paste reached the shell:\n{}",
        s.screen()
    );
}

#[test]
fn an_ordinary_paste_is_never_asked_about() {
    let _serial = serially();
    // The positive control: with a threshold of one line, a paste of exactly
    // one line goes straight through — the confirmation code path is not the
    // only way a paste can arrive. (A "single-line" paste that ends in a
    // newline is two lines, which is why the control uses no newline at all:
    // the first version of this test passed vacuously, because the
    // confirmation hint was not being rendered yet.)
    let mut s = Screen::start_with_config("terminal.paste_confirm_lines = 1\n");
    s.settle(2.0);
    s.type_in("\x1b[200~single-line\x1b[201~");
    s.settle_until(10.0, |s| s.screen().contains("single-line"));
    assert!(
        !s.screen().contains("paste "),
        "a single-line paste was asked about:\n{}",
        s.screen()
    );
}

#[test]
fn a_bell_in_a_pane_is_audible_when_that_is_what_the_setting_says() {
    let _serial = serially();
    // `terminal.bell` reached nothing: the daemon emitted a Bell event and no
    // one consumed it. Audible means the outer terminal's own bell, which is
    // the byte `\x07` on stdout — the one thing a pty test can assert.
    let mut s = Screen::start_with_config("terminal.bell = \"audible\"\n");
    s.settle(2.0);
    let before = s.raw.len();
    s.type_in("printf '\\a'\n");
    s.settle_until(10.0, |s| s.raw.len() > before + 4);
    assert!(s.raw.len() > before, "the shell echoed nothing");
    let raw = s.raw.clone();
    assert!(
        raw[before.min(raw.len())..].contains(&7),
        "no bell byte reached the outer terminal"
    );
}

#[test]
fn a_bell_in_a_pane_is_silent_when_turned_off() {
    let _serial = serially();
    let mut s = Screen::start_with_config("terminal.bell = \"off\"\n");
    s.settle(2.0);
    s.type_in("printf '\\a'\n");
    s.settle(1.0);
    // The shell echoes the quoted command, which contains no raw control byte.
    let raw = s.raw.clone();
    let window = &raw[..raw.len().saturating_sub(2)];
    assert!(
        !window.contains(&7),
        "the bell byte reached the outer terminal with bells off:\n{:?}",
        s.raw
    );
}
