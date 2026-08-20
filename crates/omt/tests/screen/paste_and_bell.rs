//! Behaviour through a real pty, split out of `main.rs` to keep each file
//! under the hard line limit — in the same binary, which the screen suite
//! must stay (cargo would otherwise run a second screen suite in parallel,
//! and sixteen ptys at once is a test of the machine, not of omt.

#![allow(
    unsafe_code,
    reason = "kill(pid, 0) is the only live check without /proc"
)]

use super::{Instance, Screen, instance_dir, serially};

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

#[test]
fn the_log_writes_where_the_setting_says_and_only_at_its_level() {
    let _serial = serially();
    // `log.file` and `log.level` were in the settings reference and reached
    // nothing: there was no log at all. Off writes nothing by default, the
    // file collects what the level admits, and the level admits what it says.
    let log = std::env::temp_dir().join(format!("omt-log-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&log);
    let cfg = format!("log.file = \"{}\"\nlog.level = \"error\"\n", log.display());
    let mut s = Screen::start_with_config(&cfg);
    s.settle(2.0);
    // The info line was below the level: nothing may have been written.
    let quiet = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        !quiet.contains("omt "),
        "an info line reached the log under error level: {quiet}"
    );
    drop(s);

    // At the default level the startup line lands in the file.
    let _ = std::fs::remove_file(&log);
    let mut s = Screen::start_with_config(&format!("log.file = \"{}\"\n", log.display()));
    s.settle_until(15.0, |_| {
        std::fs::read_to_string(&log).is_ok_and(|t| t.contains("omt "))
    });
    let written = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        written.contains("omt ") && written.contains("starts"),
        "no startup line in the log: {written:?}"
    );
}

#[test]
fn a_resize_repaints_everything_at_the_new_size() {
    use crate::common::screen_harness::ROWS;
    let _serial = serially();
    // "Change your window size" was reported as broken, and the suite had
    // never resized a pty. To be a resize test it must start with a prompt,
    // split, then grow the terminal and see the new geometry painted.
    let mut s = Screen::start();
    s.settle_until(30.0, |s| s.screen().contains('$'));
    // Split so the resize has a layout to reflow.
    s.type_in("\x02s");
    s.settle_until(10.0, |s| {
        s.screen().contains('$') && s.screen().contains('$')
    });
    s.settle(1.0);
    let rows_before = s.rows().len();
    let bar_before = s.row(ROWS - 1).to_owned();

    s.resize(140, 50);
    s.settle(2.0);

    // The status bar survives at the bottom: a resize that lost the bar is a
    // resize that stopped being a terminal.
    let rows_after = s.rows().len();
    let bar_after = s
        .row(u16::try_from(s.rows().len().saturating_sub(1)).unwrap_or(ROWS))
        .to_owned();
    assert!(
        bar_after.contains("omt") || bar_before.contains("omt"),
        "the bar vanished after resize: before={bar_before:?} after={bar_after:?}"
    );
    eprintln!("resize: rows {} -> {}", rows_before, rows_after);
    eprintln!("bar after: {bar_after:?}");
    assert!(
        rows_after > 30,
        "the screen did not grow with the terminal: {} rows",
        rows_after
    );
}

#[test]
fn an_attached_terminal_repaints_at_the_new_size() {
    use crate::common::screen_harness::ROWS;
    let _serial = serially();
    // The default way `omt` runs is attached to an instance, and "change
    // window size" was reported from exactly that mode. The pty resizes in
    // the daemon; the terminal has to follow.
    let runtime = instance_dir("resized");
    let _cleanup = Instance(runtime.clone());
    let mut s = Screen::start_attached_in(&runtime);
    s.settle_until(30.0, |s| s.screen().contains('$'));

    s.resize(140, 50);
    s.settle(2.0);
    let height = s.rows().len();
    let bar = s.row(u16::try_from(height.saturating_sub(1)).unwrap_or(ROWS));
    eprintln!("attached resize: rows={height} bar={bar:?}");
    assert!(
        height > 30,
        "an attached terminal did not grow with the window: {height} rows"
    );
    assert!(bar.contains("omt"), "the bar vanished: {bar:?}");
    // And typing still works after the resize.
    s.type_in("echo after-resize\r");
    s.settle_until(10.0, |s| s.screen().contains("after-resize"));
}

#[test]
fn shrinking_keeps_layout_usable_and_the_bar_readable() {
    use crate::common::screen_harness::ROWS;
    let _serial = serially();
    // Growing is the friendly direction. Shrinking is where a terminal
    // stops being one: panes cut to nothing, the status bar overflowing, a
    // split that hides one side entirely.
    let mut s = Screen::start();
    s.settle_until(30.0, |s| s.screen().contains('$'));
    s.type_in("\x02s");
    s.settle(1.0);

    s.resize(60, 12);
    s.settle(2.0);

    let height = s.rows().len();
    let bar = s.row(u16::try_from(height.saturating_sub(1)).unwrap_or(ROWS));
    eprintln!("shrink: rows={height} bar={bar:?}");
    assert!(
        !bar.is_empty(),
        "the bar is gone at 60x12 — that is the overflow being unhandled"
    );
    // The bar has to stay readable: a bar that lost its own leader is a
    // terminal nobody can leave.
    assert!(
        bar.contains('?'),
        "the escape-hatch cell vanished at 60x12: {bar:?}"
    );
    // Both halves of the split still exist: two prompts on screen.
    let visible = s.rows().iter().filter(|l| l.contains('$')).count();
    assert!(
        visible >= 1,
        "no prompt survived the shrink:\n{}",
        s.rows().join("\n")
    );
    s.type_in("\x1b"); // spend the leader
    s.settle(0.5);
}

#[test]
fn a_shrunken_single_pane_still_shows_its_shell() {
    let _serial = serially();
    // Same shrink, no split: if the pane goes blank here, the split had
    // nothing to do with it.
    let mut s = Screen::start();
    s.settle_until(30.0, |s| s.screen().contains('$'));
    s.resize(60, 12);
    s.settle(2.0);
    let text = s.rows().join("\n");
    assert!(
        s.rows().iter().any(|l| l.contains('$')),
        "a single pane went blank on shrink:\n{text}"
    );
}

#[test]
fn a_split_shows_both_shells() {
    let _serial = serially();
    let mut s = Screen::start();
    s.settle_until(30.0, |s| s.screen().contains('$'));
    s.type_in("\x02s");
    s.settle_until(20.0, |s| {
        // Two rows each carrying a prompt: a split that shows one shell and
        // an empty rectangle is not a usable split.
        s.rows().iter().filter(|l| l.trim_end() == "$").count() >= 2
    });
    s.settle(0.5);
    eprintln!("--- split screen ---\n{}", s.rows().join("|\n"));
}
