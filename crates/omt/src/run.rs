//! Running a session in the local terminal.
//!
//! The loop is here rather than in `omt-tui` because this is the layer allowed
//! to assemble surfaces: the TUI provides rendering and key translation, the
//! daemon provides the session, and joining them is a binary's job.

use std::io::{self, Write};
use std::time::Duration;

use anyhow::{Context, Result};
use omt_daemon::{Instance, SessionRuntime};
use omt_input::EncodeMode;
use omt_pty::{PtyConfig, PtySize};
use omt_session::{SessionKind, SessionMode};
use omt_session::{WriterPolicy, WriterState};
use omt_term::TermMode;
use omt_tui::{Input, RawGuard, Screen};

use crate::rows::{agent_rows, agent_short_name, session_rows};

/// How long to wait for input before checking the session again.
///
/// Short enough that output feels immediate, long enough that an idle session
/// is not a busy loop. The real fix is polling both the pty and stdin at once,
/// which needs an event source this does not have yet.
const TICK: Duration = Duration::from_millis(8);

/// Rows of chrome above the panes.
///
/// None. 19 §2.1 gives omt **one** row and puts it at the bottom: "One row.
/// Always present. Never more than one row, in any configuration." What
/// shipped was two — a tab bar across the top and a hint along the bottom —
/// which on a 24-row terminal is 8% of the screen given to chrome that the
/// document does not describe.
const CHROME_ROWS: u16 = 0;

/// Rows of chrome below the panes: the status bar, and only that.
const STATUS_ROWS: u16 = 1;

/// Where a pane's cursor lands on the real screen, one-based.
///
/// A function rather than an expression inlined at the one place it is used,
/// because it was wrong there: the pane is drawn at `rect.row + CHROME_ROWS`
/// and the cursor was placed at `rect.row`, so it sat one row above the
/// character it belonged after. A multi-line shell prompt shows that
/// immediately — the cursor appears on the line above the one being typed on.
const fn cursor_on_screen(rect: &omt_tui::Rect, cursor_row: u16, cursor_col: u16) -> (u16, u16) {
    (
        rect.row + CHROME_ROWS + cursor_row + 1,
        rect.col + cursor_col + 1,
    )
}

/// Run one shell in this terminal until it exits or the user detaches.
///
/// # Errors
/// Fails if the terminal cannot be put into raw mode, if no pty is available,
/// or if the program cannot be started.
pub fn run_shell(program: Option<String>) -> Result<()> {
    // Clamped, not just defaulted. A terminal really does report zero — under
    // `script`, in a pipeline, mid-drag — and a pty sized zero by zero is one
    // no program can draw on, so the fallback has to cover a bad answer and not
    // only a missing one.
    let (cols, rows) = match crossterm::terminal::size() {
        Ok((c, r)) if c > 0 && r > 1 => (c, r),
        _ => (80, 24),
    };
    let shell = program
        .or_else(|| std::env::var("SHELL").ok())
        .unwrap_or_else(|| "/bin/sh".to_owned());

    // Printed before anything else, so it is above the session rather than
    // interleaved with it. Prints and continues — no question, no wait. Every
    // question is a step, and a step is where people stop.
    first_run_summary();

    let mut instance = Instance::new();
    let cwd = crate::state::canonical_root(
        &std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| ".".to_owned()),
    );
    let workspace = instance
        .open_workspace(&cwd)
        .context("opening a workspace")?;

    // Two rows are omt's: a tab bar at the top and the hint at the bottom. Taken
    // rather than drawn over, because a program that believes it owns a row will
    // draw there — and the screen was omt's *only* contribution to the display
    // before this, which made a multiplexer indistinguishable from the shell it
    // wraps.
    let usable_rows = rows.saturating_sub(STATUS_ROWS).max(1);
    // The arrangement, as a tree. One pane to begin with; every split adds a
    // node rather than another equal share of the width, which is what makes
    // an editor beside two stacked shells expressible at all.
    let mut next_tile = 0u32;
    let mut tile_id = || {
        next_tile += 1;
        omt_tui::PaneId(next_tile)
    };
    let first_tile = tile_id();
    let mut layout = omt_tui::Node::leaf(first_tile);
    let mut panes = vec![LocalPane::open(
        &mut instance,
        workspace,
        &shell,
        &cwd,
        PtySize::new(cols, usable_rows),
        first_tile,
    )?];
    let mut focus = 0usize;

    // Entered last, so anything that fails above reports on a working terminal
    // rather than into raw mode.
    let _guard = RawGuard::enter().context("entering raw mode")?;
    let mut out = io::stdout();
    let mut armed = false;
    // When the leader was pressed, so the which-key strip can wait 250 ms
    // before appearing (16 §3.3). Somebody executing `⟨leader⟩ s` from muscle
    // memory never sees a flash; somebody who forgot gets the menu.
    let mut armed_at: Option<std::time::Instant> = None;
    let started = std::time::Instant::now();
    let mut has_used_prefix = false;
    let mut shown_hint: Option<Option<&'static str>> = None;
    let mut shown_bar: Option<String> = None;
    let mut shown_bottom: Option<String> = None;
    // The list on screen, if any: the palette, the agents, or the sessions.
    let mut overlay: Option<omt_tui::Overlay> = None;
    let mut chosen: Option<String> = None;
    let mut term_rows = rows;
    let mut term_cols = cols;
    let mut layout_dirty = true;
    // Where the cursor was last put on the real screen, so a frame that did
    // not move it writes nothing at all.
    let mut placed: Option<(u16, u16)> = None;
    // Whether the terminal is currently showing one. `RawGuard` hides it on
    // the way in, so this starts false and the first frame that wants one asks
    // for it.
    let mut cursor_shown = false;
    // Built once. Rebuilding it twice a second per pane would be the most
    // expensive thing in this loop by an order of magnitude.
    let adapter_set = omt_agent_adapters::builtin();
    let mut branch = BranchCache::default();

    // Labelled, because the input drain below is itself a loop: an unlabelled
    // `break` on detach would leave the drain and carry straight on drawing
    // frames, and omt would refuse to exit.
    'session: loop {
        for pane in &mut panes {
            // Whether anything came off the pty is the floor's only signal, so
            // it is recorded here rather than asked for later: by the time the
            // probe runs, the pump that saw the bytes has returned.
            let produced = instance
                .pump_session(pane.session)
                .context("pumping the session")?;
            if !produced.is_empty() {
                pane.last_output_ms = monotonic_ms();
            }
        }

        // What is running in each pane. The agent layer existed and nothing
        // fed it from here, so `⟨leader⟩ a` was structurally always empty:
        // starting Claude Code in a pane and asking omt about it showed
        // "nothing matches", because nothing had ever looked.
        let now = monotonic_ms();
        for pane in &mut panes {
            if now.saturating_sub(pane.probed_at) < crate::probe::PROBE_EVERY_MS {
                continue;
            }
            pane.probed_at = now;
            if instance.runtime(pane.session).is_none() {
                continue;
            }
            let quiet_ms = now.saturating_sub(pane.last_output_ms);
            let refs: Vec<&dyn omt_agent_adapters::AgentAdapter> = adapter_set.all();

            // One observation, in the daemon, so a session opened from a phone
            // is looked at by the same code that looks at this one. This used
            // to live here, which meant the web client's sessions were never
            // probed at all — two surfaces, one of which could see agents and
            // one of which structurally could not.
            let detection = instance.observe_session(pane.session, pane.binding, &refs, now);
            pane.agent = detection.map(|detection| crate::probe::Probed {
                activity: crate::probe::activity(quiet_ms < 1_000, false, quiet_ms / 1_000),
                detection,
            });
            if pane.agent.is_some() {
                // The floor, which is this surface's own contribution: it is
                // the one that knows whether bytes arrived on *this* pty.
                if let Some(probed) = pane.agent.clone() {
                    for reading in crate::probe::readings(&probed, now) {
                        instance.observe_agent(pane.binding, reading);
                    }
                }
                // An agent that starts working is one whose result nobody has
                // seen yet. Cleared here rather than when it finishes, because
                // "finished" is the moment there is something to look at and
                // the flag has to already be false by then.
                if matches!(
                    instance.agent_state(pane.binding, now),
                    Some(omt_types::AgentState::Working { .. })
                        | Some(omt_types::AgentState::Blocked { .. })
                ) {
                    pane.seen = false;
                }
            }
        }

        // Panes whose process ended stop being drawn. The session stays in the
        // tree — it is finished, not forgotten — but a dead pane holding screen
        // space is space the live ones need.
        let before = panes.len();
        panes.retain(|p| {
            !instance
                .session(p.session)
                .is_some_and(omt_session::Session::is_finished)
        });
        if panes.len() != before {
            layout_dirty = true;
            focus = focus.min(panes.len().saturating_sub(1));
        }
        if panes.is_empty() {
            break 'session;
        }

        // Where each pane goes, from the tree rather than from a count. Panes
        // whose leaf is gone are not drawn, which cannot happen — the tree and
        // the list are changed together — but is cheaper to handle than to
        // prove.
        let arrangement = layout.place(omt_tui::Rect {
            col: 0,
            row: 0,
            cols: term_cols,
            rows: term_rows.saturating_sub(STATUS_ROWS).max(1),
        });
        let rects: Vec<omt_tui::Rect> = panes
            .iter()
            .map(|pane| {
                arrangement.iter().find(|p| p.pane == pane.tile).map_or(
                    omt_tui::Rect {
                        col: 0,
                        row: 0,
                        cols: 0,
                        rows: 0,
                    },
                    |p| p.rect,
                )
            })
            .collect();
        // One frame, presented whole. A terminal is free to show the screen
        // between any two writes, and a frame here is dozens of them — the
        // panes, the separators, the status bar, an overlay, the cursor. What
        // that looks like without this is a status bar from this frame above a
        // pane from the last one, for one refresh, every frame.
        let mut frame = omt_tui::Frame::begin(&mut out);
        let mut out = frame.writer();
        if layout_dirty {
            // Everything moved. Diffing against where a pane used to be paints
            // its old contents into its neighbour.
            write!(out, "\x1b[2J").context("clearing")?;
            placed = None;
            for pane in &mut panes {
                pane.screen.invalidate();
            }
            for (pane, rect) in panes.iter().zip(&rects) {
                if let Some(rt) = instance.runtime_mut(pane.session) {
                    rt.resize(rect.cols, rect.rows).context("resizing")?;
                }
            }
            draw_separators(&mut out, &rects).context("drawing separators")?;
            layout_dirty = false;
        }

        let mut mode = EncodeMode::default();
        for (i, (pane, rect)) in panes.iter_mut().zip(&rects).enumerate() {
            let Some(runtime) = instance.runtime(pane.session) else {
                continue;
            };
            if i == focus {
                mode = EncodeMode {
                    application_cursor: runtime
                        .terminal()
                        .modes()
                        .contains(TermMode::ApplicationCursor),
                };
            }
            pane.screen
                // Offset by the tab bar, which owns the first row.
                .draw_at(
                    &mut out,
                    runtime.terminal().grid(),
                    rect.col,
                    rect.row + CHROME_ROWS,
                )
                .context("drawing")?;
        }

        // Whatever is on screen and focused is being looked at, so its result
        // is not something somebody has yet to find.
        if let Some(pane) = panes.get_mut(focus) {
            pane.seen = true;
        }
        let focused_session = panes[focus].session;
        let situation = omt_tui::Situation {
            prefix_armed: armed
                && armed_at.is_some_and(|at| {
                    at.elapsed().as_millis() as u64 >= omt_tui::PENDING_STRIP_DELAY_MS
                }),
            blocked: instance
                .threads(focused_session)
                .map_or(0, |roster| roster.summary().blocked),
            agent_running: instance.threads(focused_session).is_some(),
            age_secs: started.elapsed().as_secs(),
            remote_clients: 0,
            has_used_prefix,
        };
        // The tab bar. Everything omt knows that the shell does not — which
        // workspace this is, what else is open, what is waiting on somebody —
        // and until now it appeared nowhere at all.
        // Read before the roster is borrowed: `agent_state` needs the instance
        // mutably and `threads` needs it shared, and one pass cannot hold both.
        let pane_states: Vec<(&LocalPane, Option<omt_types::AgentState>)> = {
            let mut out = Vec::with_capacity(panes.len());
            for pane in &panes {
                let state = pane
                    .agent
                    .as_ref()
                    .and(instance.agent_state(pane.binding, now));
                out.push((pane, state));
            }
            out
        };
        let chrome = omt_tui::Chrome {
            workspace: std::path::Path::new(&cwd)
                .file_name()
                .map_or_else(|| cwd.clone(), |n| n.to_string_lossy().into_owned()),
            // Cached for a second. This ran twice per frame — `git status`
            // spawns two processes — at up to 125 frames a second, which made
            // pasting a paragraph arrive character by character and kept a
            // laptop's fan on. A branch is stale for at most a second after a
            // checkout, which nobody can perceive; two process spawns per
            // keystroke is what everybody perceives.
            branch: branch.get(&cwd, monotonic_ms()),
            tabs: pane_states
                .iter()
                .enumerate()
                .map(|(i, (pane, state))| {
                    let roster = instance.threads(pane.session);
                    // The probe first, the roster second. A roster is filled
                    // by hooks and protocols, so a pane running an agent on a
                    // plain pty had an empty one — and the bar went on saying
                    // "shell" with a live agent in it, which is the one thing
                    // this cell exists to make impossible.
                    let detected = pane.agent.as_ref();
                    let state = state.as_ref();
                    omt_tui::Tab {
                        title: detected.map_or_else(
                            || {
                                instance
                                    .session(pane.session)
                                    .map_or_else(|| "shell".to_owned(), |s| s.title.clone())
                            },
                            |probed| agent_short_name(probed.detection.agent),
                        ),
                        focused: i == focus,
                        blocked: roster.map_or(0, |r| r.awaiting_human().len())
                            + usize::from(matches!(
                                state,
                                Some(omt_types::AgentState::Blocked { .. })
                            )),
                        working: roster.is_some_and(|r| !r.is_empty())
                            || matches!(state, Some(omt_types::AgentState::Working { .. })),
                    }
                })
                .collect(),
        };
        if let Some(open) = &mut overlay {
            // Rebuilt each frame, not only when it was opened. These lists
            // are opened *because* something is expected to change: an agent
            // that starts needing a human while the list is on screen is the
            // event it exists to show, and a snapshot cannot show it. The
            // filter and the selected row survive, so this is invisible to
            // somebody typing.
            match open.title.as_str() {
                "Agents" => open.set_choices(agent_rows(&mut instance, &panes, monotonic_ms())),
                "Sessions" => open.set_choices(session_rows(&instance, &panes)),
                // The palette is the catalog, which does not change while a
                // process is running.
                _ => {}
            }

            // Unconditionally, not only when its text changed. The panes
            // underneath repaint every frame from their own damage, so an
            // overlay drawn once is painted over within milliseconds and what
            // is left is half a list interleaved with a shell — which is
            // exactly what it looked like.
            let lines =
                omt_tui::render_overlay(open, term_cols, term_rows.saturating_sub(STATUS_ROWS));
            for (i, line) in lines.iter().enumerate() {
                write!(out, "\x1b[{};1H\x1b[7m{line}\x1b[0m", i as u16 + 2)
                    .context("drawing the overlay")?;
            }
        }

        // One row, at the bottom, per 19 §2.1. The tab bar that used to sit
        // across the top is gone: the document gives omt one row of chrome and
        // says so twice, and cell B of this bar is the session list that bar
        // was trying to be.
        let status = omt_tui::Status {
            instance: "omt".to_owned(),
            remote_host: None,
            sessions: chrome.tabs.clone(),
            native: instance
                .session(focused_session)
                .is_some_and(|s| s.mode == SessionMode::Native),
            cwd: instance
                .session(focused_session)
                .and_then(|s| s.cwd.clone()),
            branch: chrome.branch.clone(),
            dirty: false,
            leader: omt_tui::leader(),
        };
        let bar = omt_tui::status_bar(&status, term_cols);
        if shown_bar.as_deref() != Some(bar.as_str()) {
            write!(out, "\x1b[{};1H\x1b[7m{bar}\x1b[0m", term_rows)
                .context("drawing the status bar")?;
            shown_bar = Some(bar);
        }

        // The which-key strip, and only when there is one. 16 §3.3 draws it
        // *over the pane's last row* and restores the row afterwards, rather
        // than reserving one: a row kept for a hint that is absent almost
        // always is a row spent on nothing.
        // Within the width, because the leader strip is a menu: built too wide
        // it is truncated, and what is lost is the end of it.
        let hint = omt_tui::hint_for_within(&situation, term_cols);
        let over = hint.map(|text| omt_tui::render_hint(Some(&text), term_cols));
        if shown_bottom != over {
            match &over {
                Some(line) => {
                    write!(
                        out,
                        "\x1b[{};1H{line}",
                        term_rows.saturating_sub(STATUS_ROWS)
                    )
                    .context("drawing the leader strip")?;
                }
                None => {
                    // Gone: the pane owns that row again, so it is repainted
                    // rather than blanked. Blanking it would leave a hole in
                    // whatever the program had drawn there.
                    for pane in &mut panes {
                        pane.screen.invalidate();
                    }
                }
            }
            shown_bottom = over;
        }
        let _ = &mut shown_hint;

        // **Last.** Every write above moves the cursor, so placing it earlier
        // leaves it wherever the final piece of chrome finished — which is the
        // bottom row, and which is what "the cursor is dragged to the bottom
        // and does not line up" was. The save/restore on the status row was an
        // attempt at the same thing and could not work: it saved a position
        // the tab bar had already moved.
        // Whether the pane's cursor belongs on the screen at all. Derived
        // rather than assumed: a list is open *over* the pane, and a cursor
        // blinking in the program behind it says the program is where the
        // typing goes — which it is not, because the list takes every key.
        // Somebody typing to filter watches a cursor sit still somewhere else.
        //
        // The program's own choice is honoured too: a pager that hid the
        // cursor meant it, and drawing one over its output is omt overriding a
        // decision it has no part in.
        let show_cursor = overlay.is_none()
            && panes
                .get(focus)
                .and_then(|pane| instance.runtime(pane.session))
                .is_some_and(|runtime| runtime.terminal().grid().cursor.visible);
        if !show_cursor && cursor_shown {
            write!(out, "\x1b[?25l").context("hiding the cursor")?;
            cursor_shown = false;
            placed = None;
        }
        if show_cursor
            && let (Some(pane), Some(rect)) = (panes.get(focus), rects.get(focus))
            && let Some(runtime) = instance.runtime(pane.session)
        {
            if !cursor_shown {
                write!(out, "\x1b[?25h").context("showing the cursor")?;
                cursor_shown = true;
            }
            let cursor = runtime.terminal().grid().cursor;
            let (row, col) = cursor_on_screen(rect, cursor.row, cursor.col);
            // Only when it moved. This frame runs at `TICK`, so writing the
            // position unconditionally sent one escape sequence every 8 ms
            // whether or not anything had changed — 619 bytes a second into a
            // terminal where nobody was typing and nothing was running,
            // measured on this machine.
            //
            // That is not free anywhere and it is expensive in the two places
            // this product lives: over ssh it is a steady trickle on a link
            // whose whole value is latency, and interleaved with an agent's
            // own repaints it is the thing that makes a screen look like it is
            // struggling to keep up.
            //
            // Cleared on a layout change, because the screen was wiped and the
            // terminal's idea of where the cursor is went with it.
            if placed != Some((row, col)) {
                write!(out, "\x1b[{row};{col}H").context("placing the cursor")?;
                placed = Some((row, col));
            }
        }
        frame.present().context("presenting the frame")?;

        // Every event that is already waiting, not one per frame. A paste is
        // delivered as a burst — one event per character on a terminal with no
        // bracketed-paste support — and reading a single one per frame meant a
        // 400-character paste took 400 frames to appear. `TICK` is only waited
        // for when nothing is pending at all.
        let mut first = true;
        while if first {
            first = false;
            crossterm::event::poll(TICK).context("polling for input")?
        } else {
            crossterm::event::poll(std::time::Duration::ZERO).context("polling for input")?
        } {
            let event = crossterm::event::read().context("reading input")?;
            // An open overlay takes every key: it is a list somebody is
            // choosing from, and letting a keystroke reach the shell underneath
            // would type into a program nobody is looking at.
            if let Some(open) = overlay.as_mut() {
                if let crossterm::event::Event::Key(key) = &event {
                    match key.code {
                        crossterm::event::KeyCode::Esc => overlay = None,
                        crossterm::event::KeyCode::Up => open.move_by(-1),
                        crossterm::event::KeyCode::Down => open.move_by(1),
                        crossterm::event::KeyCode::Backspace => open.backspace(),
                        crossterm::event::KeyCode::Enter => {
                            // Picking is closing. What a choice *does* depends
                            // on which list it came from, and that is decided
                            // where the list was built.
                            chosen = open.selected().map(|c| c.label.clone());
                            overlay = None;
                        }
                        crossterm::event::KeyCode::Char(ch) => open.push(ch),
                        _ => {}
                    }
                }
                // `layout_dirty` is what brings the panes back: it clears the
                // screen and invalidates every pane's renderer, which is
                // exactly what an overlay written over cells those renderers
                // believe they own requires. Setting it here is not
                // bookkeeping — it is the repaint.
                layout_dirty = true;
                if let Some(label) = chosen.take() {
                    // A workspace row is a session to focus; anything else is a
                    // capability name, which the palette shows and the CLI runs.
                    if let Some(index) = panes.iter().position(|p| {
                        instance
                            .session(p.session)
                            .is_some_and(|s| s.title == label)
                    }) {
                        focus = index;
                    }
                }
                continue;
            }

            let (input, next) = omt_tui::translate(&event, armed, mode);
            // Recorded the moment it is first armed: somebody who has pressed
            // it knows it exists, and the introduction stops being useful.
            has_used_prefix = has_used_prefix || next;
            if next {
                // Only the first arming starts the clock. Re-arming on a key
                // that was already pending would restart the delay and the
                // strip would never appear.
                armed_at = armed_at.or_else(|| Some(std::time::Instant::now()));
            } else {
                armed_at = None;
            }
            armed = next;
            match input {
                Input::Bytes(bytes) => {
                    let pane = &mut panes[focus];
                    let session = pane.session;
                    if let Some(rt) = instance.runtime_mut(session) {
                        rt.write_input(&mut pane.writer, pane.epoch, monotonic_ms(), &bytes)
                            .context("writing input")?;
                    }
                }
                Input::Palette => {
                    // Built from the catalog, so the list is what this build
                    // actually registers rather than a menu maintained beside
                    // it — which is how a palette comes to offer something that
                    // is not there.
                    overlay = Some(omt_tui::Overlay::new(
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
                    ));
                    layout_dirty = true;
                }
                Input::Agents => {
                    overlay = Some(omt_tui::Overlay::new(
                        "Agents",
                        agent_rows(&mut instance, &panes, monotonic_ms()),
                    ));
                    layout_dirty = true;
                }
                Input::Sessions => {
                    // Every session this instance holds, not only the ones with
                    // a pane on screen. A session that is running somewhere
                    // else is exactly the one somebody opens this list to find,
                    // and a list that showed only what was already visible
                    // answered a question nobody had.
                    overlay = Some(omt_tui::Overlay::new(
                        "Sessions",
                        session_rows(&instance, &panes),
                    ));
                    layout_dirty = true;
                }
                Input::Resize { cols, rows } => {
                    term_rows = rows;
                    term_cols = cols;
                    layout_dirty = true;
                    shown_hint = None;
                    shown_bar = None;
                    shown_bottom = None;
                }
                Input::Split | Input::SplitDown => {
                    // Which way the space is cut is the whole difference
                    // between the two keys; everything after it is the same.
                    let split = if matches!(input, Input::SplitDown) {
                        omt_tui::Split::Horizontal
                    } else {
                        omt_tui::Split::Vertical
                    };
                    // Asked of the tree, not of a count: whether there is room
                    // depends on how the space is already divided, and only
                    // the tree knows that. Refused rather than attempted when
                    // there is none — two unusable panes are worse than one
                    // usable one, and nobody can see why nothing is legible.
                    let here = panes[focus].tile;
                    let new_tile = tile_id();
                    let mut proposed = layout.clone();
                    if proposed.split(here, new_tile, split) {
                        let area = omt_tui::Rect {
                            col: 0,
                            row: 0,
                            cols: term_cols,
                            rows: term_rows.saturating_sub(STATUS_ROWS).max(1),
                        };
                        // It splits only if the arrangement it produces
                        // actually contains the new pane. `place` declines to
                        // divide space it does not have, so this is the whole
                        // check.
                        if proposed.place(area).iter().any(|p| p.pane == new_tile) {
                            let size = PtySize::new(term_cols, term_rows.saturating_sub(1).max(1));
                            if let Ok(pane) = LocalPane::open(
                                &mut instance,
                                workspace,
                                &shell,
                                &cwd,
                                size,
                                new_tile,
                            ) {
                                layout = proposed;
                                panes.push(pane);
                                focus = panes.len() - 1;
                                layout_dirty = true;
                            }
                        }
                    }
                }
                Input::Focus(towards) => {
                    // Where a finger would land, not the next one in a list:
                    // with more than two panes, cycling is a guessing game.
                    let area = omt_tui::Rect {
                        col: 0,
                        row: 0,
                        cols: term_cols,
                        rows: term_rows.saturating_sub(STATUS_ROWS).max(1),
                    };
                    let arrangement = layout.place(area);
                    if let Some(landed) =
                        omt_tui::neighbour(&arrangement, panes[focus].tile, towards)
                        && let Some(index) = panes.iter().position(|p| p.tile == landed)
                    {
                        focus = index;
                        shown_hint = None;
                        shown_bar = None;
                        shown_bottom = None;
                    }
                }
                Input::NextPane => {
                    focus = (focus + 1) % panes.len();
                    shown_hint = None;
                    shown_bar = None;
                    shown_bottom = None;
                }
                Input::ClosePane => {
                    // The pane goes; the session does not. Closing a view of
                    // something is not ending it.
                    let closed = panes.remove(focus);
                    if let Some(pane) = closed.pane {
                        instance.remove_pane(workspace, pane);
                    }
                    // The tree collapses the split that held it, so the
                    // sibling takes the space. Without this the arrangement
                    // keeps a leaf for a pane that is gone and everything
                    // below it shifts.
                    match layout.clone().close(closed.tile) {
                        Some(rest) => layout = rest,
                        // The last pane. An empty tree is not representable,
                        // and closing the last pane is closing the window.
                        None => break 'session,
                    }
                    if panes.is_empty() {
                        break 'session;
                    }
                    focus = focus.min(panes.len() - 1);
                    layout_dirty = true;
                }
                Input::Help => {
                    // The cheat sheet is a capability, and duplicating it here
                    // would be a second list to keep in step with the keymap.
                    shown_hint = None;
                    shown_bar = None;
                    shown_bottom = None;
                }
                Input::Detach => break 'session,
                Input::Ignore => {}
            }
        }
    }

    out.flush().ok();
    Ok(())
}

/// One pane the local TUI is drawing.
///
/// Each carries its own `Screen`, because the damage tracking is per region:
/// one screen for the whole terminal would diff a pane's output against its
/// neighbour's and repaint both.
pub(crate) struct LocalPane {
    pub(crate) session: omt_types::SessionId,
    /// Its identity in the workspace's view, so closing it here closes the same
    /// pane a remote client sees.
    pub(crate) pane: Option<omt_types::PaneId>,
    pub(crate) screen: Screen,
    pub(crate) writer: WriterState,
    pub(crate) epoch: omt_session::Epoch,
    /// This pane's agent binding. Minted per pane rather than per detection:
    /// a binding is "the agent occupying this pane", and re-minting it when
    /// the detection changed would throw away the history the merge machine
    /// decides from.
    pub(crate) binding: omt_types::BindingId,
    /// What was last seen running here, and when it was last looked for.
    pub(crate) agent: Option<crate::probe::Probed>,
    pub(crate) probed_at: u64,
    /// When this pane last produced anything, which is the entirety of what
    /// the heuristic floor has to go on.
    pub(crate) last_output_ms: u64,
    /// Which leaf of the layout tree this pane is.
    ///
    /// The tree holds the arrangement and knows nothing about sessions; this
    /// is the join. Kept on the pane rather than in a map because a pane and
    /// its place are created and destroyed together, and two structures that
    /// have to be kept in step are two structures that will not be.
    pub(crate) tile: omt_tui::PaneId,
    /// Whether anybody has looked at this pane since its agent stopped
    /// working.
    ///
    /// Not on the wire and not in `AgentState`, because it is not something
    /// the agent knows: an agent is idle either way, and whether *you* have
    /// seen it is a fact about you. It is what separates "it finished while
    /// you were elsewhere" from "it is sitting there and you have been
    /// watching it", which is the distinction this whole product is for.
    pub(crate) seen: bool,
}

impl LocalPane {
    /// Start a session and take its writer token.
    fn open(
        instance: &mut Instance,
        workspace: omt_types::WorkspaceId,
        program: &str,
        cwd: &str,
        size: PtySize,
        tile: omt_tui::PaneId,
    ) -> Result<Self> {
        let session = instance
            .create_session(workspace, SessionKind::Shell, SessionMode::Pty)
            .context("creating a session")?;
        let runtime = SessionRuntime::spawn(
            session,
            &PtyConfig {
                program: program.into(),
                args: Vec::new(),
                cwd: Some(cwd.into()),
                size,
                env: vec![("OMT_SESSION".to_owned(), session.to_wire())],
                ..PtyConfig::default()
            },
            omt_term::ScrollbackLimits::default(),
        )
        .context("starting the shell")?;
        instance.attach(runtime).context("attaching")?;
        let pane = instance.add_pane(workspace, session);

        // The local terminal holds the token like any other actor rather than
        // writing around it. Auto-acquire makes that invisible to somebody
        // alone at their keyboard, and it means the epoch check on every
        // keystroke is the real path rather than a branch only remotes take.
        let mut writer = WriterState::new(WriterPolicy::default());
        let epoch = writer
            .acquire(omt_types::Actor::Local, monotonic_ms(), false, true)
            .context("acquiring the writer token")?;
        Ok(Self {
            session,
            pane,
            screen: Screen::new(),
            writer,
            epoch,
            binding: omt_types::BindingId::new(),
            agent: None,
            probed_at: 0,
            last_output_ms: 0,
            tile,
            // A pane somebody just opened is one they are looking at.
            seen: true,
        })
    }
}

/// Draw the separator between panes.
///
/// A vertical rule rather than a blank column: two shells side by side with
/// nothing between them read as one shell with very strange output.
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
struct BranchCache {
    /// Which directory the answer is for, so a `cd` is not answered with the
    /// previous workspace's branch.
    dir: String,
    branch: Option<String>,
    at: Option<u64>,
}

impl BranchCache {
    /// How long an answer is reused.
    const FRESH_MS: u64 = 1_000;

    fn get(&mut self, dir: &str, now: u64) -> Option<String> {
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

fn draw_separators(out: &mut impl Write, rects: &[omt_tui::Rect]) -> io::Result<()> {
    for pair in rects.windows(2) {
        let col = pair[0].col + pair[0].cols + 1;
        for row in 0..pair[0].rows {
            write!(out, "\x1b[{};{col}H\x1b[2m│\x1b[0m", row + 1)?;
        }
    }
    Ok(())
}

/// Say what omt found, once, on a machine that has never run it.
///
/// The trigger is the absence of a configuration directory, because that is
/// what "first run" means and it needs no state of its own. The directory is
/// created here, so a second run says nothing — including a second run that
/// changes nothing else.
fn first_run_summary() {
    let Some(home) = config_home() else {
        return;
    };
    if home.exists() {
        return;
    }
    // Created before printing. If creating it fails the summary is skipped
    // rather than printed on every start, which would be worse than never.
    if std::fs::create_dir_all(&home).is_err() {
        return;
    }

    let shell = std::env::var("SHELL")
        .ok()
        .and_then(|p| {
            std::path::Path::new(&p)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "your shell".to_owned());

    let agents = detected_agents();

    println!("omt {}", env!("CARGO_PKG_VERSION"));
    println!("  shell: {shell}");
    if agents.is_empty() {
        println!("  agents: none on PATH — omt runs anything, and detects these when present");
    } else {
        println!("  agents: {}", agents.join(", "));
    }
    let leader = omt_tui::leader();
    println!("  {leader} ? for every key · {leader} d to detach · `omt web` for a phone");
    println!();
}

/// Which agent CLIs are on PATH.
///
/// Named rather than counted: "3 agents detected" tells somebody nothing they
/// can act on, and the list is what makes it obvious omt already knows about
/// the one they use.
fn detected_agents() -> Vec<String> {
    [
        "claude",
        "codex",
        "copilot",
        "gemini",
        "cursor-agent",
        "opencode",
    ]
    .iter()
    .filter(|name| which(name))
    .map(|name| (*name).to_owned())
    .collect()
}

/// Whether a program is on PATH.
fn which(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
}

/// Where omt keeps its configuration.
fn config_home() -> Option<std::path::PathBuf> {
    std::env::var("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|_| std::env::var("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
        .ok()
        .map(|base| base.join("omt"))
}

/// Milliseconds since the process started.
///
/// Monotonic rather than wall-clock: the writer token's idle timeout must not
/// move because somebody's clock was corrected.
pub(crate) fn monotonic_ms() -> u64 {
    use std::sync::OnceLock;
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    let start = START.get_or_init(std::time::Instant::now);
    start.elapsed().as_millis() as u64
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "in a test, expect() is the assertion")]
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

    #[test]
    fn the_cursor_lands_on_the_row_the_pane_was_drawn_at() {
        // The bug this pins: the pane is drawn `CHROME_ROWS` below its rect and
        // the cursor was not, so on a two-line shell prompt the cursor appeared
        // on the line above the one being typed on.
        let rect = omt_tui::Rect {
            row: 0,
            col: 0,
            rows: 10,
            cols: 80,
        };
        // Grid row 1 — the second line of the prompt — with no chrome above it
        // is screen row 2, one-based. It was row 3 when a tab bar owned the
        // top row; 19 §2.1 gives omt one row and puts it at the bottom, so the
        // program starts at the top and this is the arithmetic that follows.
        assert_eq!(cursor_on_screen(&rect, 1, 2), (2, 3));
    }

    #[test]
    fn a_second_panes_cursor_carries_that_panes_offset() {
        let rect = omt_tui::Rect {
            row: 5,
            col: 40,
            rows: 5,
            cols: 40,
        };
        assert_eq!(cursor_on_screen(&rect, 0, 0), (6, 41));
    }
}
