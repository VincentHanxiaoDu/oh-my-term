//! Reflow policy through the public API.

use omt_term::{GridSize, Reflow, TermConfig, Terminal};

#[test]
fn reflow_none_truncates_instead_of_rewriting() {
    // `terminal.reflow.on_resize = "none"` reached nothing: a resize always
    // re-wrapped the history, so the setting was decoration. The difference
    // that is observable: reflow scrolls to the newest lines (the screen shows
    // the bottom after a shrink), None keeps the top.
    let mut cut = Terminal::new(TermConfig {
        reflow: Reflow::None,
        ..TermConfig::default()
    });
    cut.advance(b"one\r\ntwo\r\nthree\r\n");
    cut.resize(GridSize::new(80, 2));
    let report = cut.resize(GridSize::new(80, 8));
    assert!(!report.reflowed, "reflow none re-wrapped");
    assert!(
        cut.grid().rows().iter().any(|l| l.text().contains("one")),
        "None dropped the top instead of keeping it"
    );
    assert!(
        !cut.grid().rows().iter().any(|l| l.text().contains("three")),
        "None scrolled in lines that were off the cut screen"
    );

    let mut reflowing = Terminal::new(TermConfig::default());
    reflowing.advance(b"one\r\ntwo\r\nthree\r\n");
    reflowing.resize(GridSize::new(80, 2));
    assert!(
        reflowing
            .grid()
            .rows()
            .iter()
            .any(|l| l.text().contains("three")),
        "the default scrolled to the newest output on a shrink"
    );
}

#[test]
fn the_growth_after_none_does_not_restore_history() {
    // And growing back does not bring the cut lines with it: shrinking then
    // growing is the operation reflow makes lossless, and None declines it.
    let mut cut = Terminal::new(TermConfig {
        reflow: Reflow::None,
        ..TermConfig::default()
    });
    cut.advance(b"a\r\nb\r\nc\r\n");
    cut.resize(GridSize::new(80, 2));
    cut.resize(GridSize::new(80, 4));
    let text: Vec<String> = cut.grid().rows().iter().map(|l| l.text()).collect();
    assert!(text.iter().any(|l| l.contains('a')), "{text:?}");
    assert!(!text.iter().any(|l| l.contains('c')), "{text:?}");
}
