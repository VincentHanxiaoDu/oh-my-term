//! Running a session in the local terminal.
//!
//! The loop is here rather than in `omt-tui` because this is the layer allowed
//! to assemble surfaces: the TUI provides rendering and key translation, the
//! daemon provides the session, and joining them is a binary's job.

use std::io::{self, Write};
use std::time::Duration;

use anyhow::{Context, Result};
use omt_daemon::Instance;
use omt_input::EncodeMode;
use omt_pty::PtySize;
use omt_session::SessionMode;

use crate::panes::{LocalPane, Opening};
use omt_term::TermMode;
use omt_tui::{Input, RawGuard};

use crate::backing::{Attached, backing};
use crate::rows::{agent_rows, agent_short_name, conversation_of, session_rows};
use crate::terminal_bits::{announce, setting, sound_file};

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
pub(crate) const CHROME_ROWS: u16 = 0;

/// The title of the one overlay whose answer is Enter itself.
///
/// Compared rather than remembered in a flag: an overlay takes every key while
/// it is open, so the state that decides what Enter means has to be the
/// overlay.
const QUIT_TITLE: &str = "Quit — this ends what is running";

/// The title of the agent's own command list.
///
/// Compared rather than remembered in a flag, for the same reason as the quit
/// overlay: what a chosen row *does* depends on which list it came from, and
/// the list is the thing that knows.
const COMMANDS_TITLE: &str = "This agent's commands";

/// The title of the list of what has been run here before.
const RECALL_TITLE: &str = "Run here before";

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
    // The state has to exist before the shell is chosen, because the shell is
    // a setting — and `terminal.default_shell` was documented, importable and
    // read by nothing.
    let state = crate::state::State::new(Instance::new());
    let shell = program
        .or_else(|| crate::terminal_bits::default_shell(&state))
        .or_else(|| std::env::var("SHELL").ok())
        .unwrap_or_else(|| "/bin/sh".to_owned());

    // How much scrollback each session keeps, from the setting that reached
    // nothing until now. Read once: it is a cap applied when a session starts,
    // and re-reading it per pane would open the file again for an answer that
    // cannot have changed.
    let limits = crate::terminal_bits::session_limits(&state);

    // The snippet omt has documented as installed by default since before
    // anything installed it. Once, before the first pane starts, so the shell
    // it starts is the one that reads the file — and said out loud, because a
    // line added to somebody's shell configuration in silence is
    // indistinguishable from a shell that behaved differently today.
    if let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) {
        match crate::shell_install::install(&state, &shell, &home) {
            crate::shell_install::Installed::Added(file) => {
                println!("omt: added its command-mark line to {}", file.display());
            }
            // Already there and deliberately off are both ordinary, and saying
            // so on every start is how a message stops being read.
            crate::shell_install::Installed::AlreadyThere(_)
            | crate::shell_install::Installed::Skipped(_) => {}
        }
    }

    // Printed before anything else, so it is above the session rather than
    // interleaved with it. Prints and continues — no question, no wait. Every
    // question is a step, and a step is where people stop.
    crate::firstrun::first_run_summary();

    // The `State` above is held rather than owned outright because something
    // else needs to reach it: the agents in these panes fire hooks, and a hook
    // talks to a socket. A terminal with no socket is a terminal whose agents
    // can report nothing, which is what every one of them was doing.
    // Attached is the default: a session that dies with the terminal showing
    // it is the one thing a multiplexer exists to prevent. `OMT_LOCAL` is the
    // way back, for a machine where an instance cannot be started and for the
    // tests that drive the path that owns its own ptys.
    //
    // Set *and non-empty*. An empty variable is how a harness says "not this
    // one" while still handing the child a fixed set of names, and reading one
    // as "yes" once made every local test try to attach.
    let asked_local = std::env::var("OMT_LOCAL").is_ok_and(|v| !v.trim().is_empty());
    let attaching = !asked_local;
    let hook_socket = crate::serve::default_socket_path();
    if !attaching {
        let served = state.clone();
        let path = hook_socket.clone();
        // Serving, but not owning: this process pumps its own sessions and
        // persists nothing, so the listener does neither.
        //
        // Only when this terminal owns the sessions. Attached, the instance is
        // the one with ptys for hooks to report about, and a second listener
        // here would leave a socket in the discovery directory that answers
        // for an instance holding nothing — which is how the next terminal
        // attaches to an empty one.
        std::thread::spawn(move || {
            let _ = crate::serve::serve_beside(&path, served);
        });
    }
    // Whether this terminal owns its sessions or watches an instance that
    // does. Behind a switch until the attached path is at parity: a
    // half-migrated terminal is worse than either end of the migration, and
    // the way to find out which end this is is to be able to run both.
    let mut attached: Option<Attached> = match attaching.then(crate::backing::join) {
        Some(Ok(joined)) => Some(joined),
        // Never refuse to start. A terminal that will not open because a
        // daemon would not is worse than one whose sessions do not outlive it
        // — but somebody has to be *told* which of the two they have, because
        // the difference only shows up when they close it.
        Some(Err(why)) => {
            eprintln!(
                "omt: running without an instance, so these sessions end when this terminal does\n     ({why})"
            );
            None
        }
        None => None,
    };
    let socket_text = hook_socket.display().to_string();
    let mut instance = state.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
    // Where a new pane starts, which is a setting and was nothing but a
    // constant: `terminal.default_cwd` is in the settings reference and in the
    // importer, and whatever it said, panes started in the directory omt had
    // been started in. `home` is `$HOME`; `last` is where the earliest
    // session is sitting, because "the last place" is what it means when a
    // multiplexer is asked; `workspace` (the default) is the current
    // directory, which is what omt always did.
    let cwd = crate::state::canonical_root(&match crate::terminal_bits::setting(
        &state,
        "terminal.default_cwd",
    )
    .as_str()
    {
        // `$HOME` is the one directory that cannot be stale: it is a
        // shell convention as old as shells.
        "home" => std::env::var("HOME").unwrap_or_else(|_| ".".to_owned()),
        // The earliest session's cwd, when one exists; the current
        // directory otherwise, which is the honest "there was no last".
        "last" => instance
            .sessions()
            .first()
            .and_then(|s| s.cwd.clone())
            .unwrap_or_else(|| {
                std::env::current_dir()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| ".".to_owned())
            }),
        _ => std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| ".".to_owned()),
    });
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
    // The sessions this terminal is about to show. Attached, they may already
    // exist — which is the entire point: what was left running is what comes
    // back — and one is created only when there is nothing to come back to.
    let mut panes = if let Some(joined) = attached.as_mut() {
        let session = crate::backing::first_session(joined, &cwd, cols, usable_rows)?;
        vec![LocalPane::watching(session, first_tile)]
    } else {
        vec![LocalPane::open(
            &mut instance,
            &Opening {
                workspace,
                program: &shell,
                cwd: &cwd,
                size: PtySize::new(cols, usable_rows),
                tile: first_tile,
                socket: &socket_text,
                limits,
            },
        )?]
    };
    let mut focus = 0usize;

    // Entered last, so anything that fails above reports on a working terminal
    // rather than into raw mode.
    let _guard = RawGuard::enter_with(crate::terminal_bits::terminal_modes(&state))
        .context("entering raw mode")?;
    // Why omt stopped, said out loud on the way out.
    //
    // A terminal that closes without a word is indistinguishable from one that
    // crashed — that is exactly what it looked like the first time somebody's
    // shell exited under it, and the answer was sitting in the session's
    // state the whole time.
    let mut why: Option<String> = None;
    let mut out = io::stdout();
    // Asked once, here and nowhere else: a query's reply arrives in the same
    // stream as typing, and this is the one moment between raw mode and the
    // first keystroke where that cannot be mistaken for somebody's input.
    // No answer means no image is ever sent, which is most terminals and is
    // not a failure.
    // Anything it read that was not the reply is still the person's: a probe
    // that swallowed the first keystroke would lose input, which is a worse
    // failure than never drawing an image.
    // Whether to ask at all is the user's. `media.images.enabled` and
    // `media.images.protocol` were in the settings reference and the settings
    // screen, and the image path was unconditional: the switch could not turn
    // anything off, and `protocol = "none"` drew images anyway.
    let (host_graphics, typed_during_probe) = if crate::media::images_wanted(&state) {
        crate::host_graphics::probe(&mut out, io::stdin())
    } else {
        (crate::host_graphics::Host::Nothing, Vec::new())
    };
    if !typed_during_probe.is_empty()
        && let Some(pane) = panes.get_mut(focus)
    {
        let session = pane.session;
        if attached.is_some() {
            backing(&mut instance, &mut attached).write(session, &typed_during_probe)?;
        } else if let Some(rt) = instance.runtime_mut(session) {
            rt.write_input(
                &mut pane.writer,
                pane.epoch,
                monotonic_ms(),
                &typed_during_probe,
            )
            .context("passing on what was typed during the handshake")?;
        }
    }
    let mut drawn_images = crate::host_graphics::Drawn::default();
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
    // Which list the chosen row came from.
    let mut picked_from: Option<String> = None;
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
    // Whether the agent panel is on. Off by default, per 19 §2.1b: somebody
    // running omt in an eighty-column terminal must not have their panes
    // narrowed by a change they did not ask for.
    let mut panel_on = crate::rows::panel_enabled();
    // What the panel last showed, so an unchanged list costs nothing.
    let mut shown_panel: Option<Vec<String>> = None;
    // Which pane fills the screen, where one does. A view over the tree rather
    // than a rearrangement of it, so zooming out restores the ratios exactly.
    let mut zoomed: Option<omt_tui::PaneId> = None;
    // Where focus was before this pane, for going back without thinking about
    // which direction that is.
    let mut previous_focus: Option<omt_tui::PaneId> = None;
    // Built once. Rebuilding it twice a second per pane would be the most
    // expensive thing in this loop by an order of magnitude.
    let adapter_set = omt_agent_adapters::builtin();
    // Where somebody is reading, when they are reading rather than typing.
    // `None` is the ordinary case: keys go to the pane.
    let mut copying: Option<omt_tui::CopyMode> = None;
    // What this terminal has already written into history. Local only: an
    // attached terminal's sessions are pumped by the instance, which records
    // them there — two recorders on one session would write every command
    // twice.
    let mut remembered = crate::remember::Remembered::default();
    // When the roster was last asked for. Attached only; the local path reads
    // its own instance and has nothing to be stale about.
    let mut rostered_at = 0u64;
    let mut branch = BranchCache::default();

    // Labelled, because the input drain below is itself a loop: an unlabelled
    // `break` on detach would leave the drain and carry straight on drawing
    // frames, and omt would refuse to exit.
    // Read once, before the loop. A setting re-read every frame is a file
    // opened 125 times a second for an answer that changes at most when
    // somebody edits it.
    let when = omt_notify::When::parse(&setting(&state, "notifications.when"));
    // From the binding file, which is the file the documents put it in — and
    // which nothing opened until now. Read once: a keymap does not change
    // under somebody's fingers mid-session, and re-reading it per keystroke
    // would put a file lookup in the input path.
    let bindings = state.bindings().ok();
    let keymap = crate::terminal_bits::Flavour::named(
        &bindings
            .as_ref()
            .map_or_else(|| "default".to_owned(), |b| b.keymap()),
    );
    // The leader, likewise. A file that names one relocates the whole
    // namespace with one edit, which is what the documents have promised since
    // before anything read them.
    // What ends a word in copy mode. Read once, beside the keymap flavour and
    // for the same reason: this is on the path of every keystroke.
    // Read once, beside the other startup settings.
    let hints_enabled = !setting(&state, "hints.enabled").contains("false");
    // What a bell in a pane does. `terminal.bell` reached nothing: the events
    // were emitted and no one consumed them, which is the "vocabulary with no
    // consumer" shape again. `off` | `visual` | `audible` | `both`.
    let bell_mode = setting(&state, "terminal.bell");
    let bell_audible = bell_mode.contains("audible") || bell_mode.contains("both");
    let bell_visual = bell_mode.contains("visual") || bell_mode.contains("both");
    let mut bell_until = 0u64;
    // Whether a paste is waiting for a yes or a no, and the thresholds that
    // decide it is worth asking. `paste_confirm_lines` of zero or fewer asks
    // nothing: a number a machine wrote is a machine declining the question.
    let paste_confirm_lines =
        crate::terminal_bits::number_setting(&state, "terminal.paste_confirm_lines")
            .unwrap_or(0)
            .clamp(0, 10_000) as usize;
    let paste_confirm_on_newline =
        setting(&state, "terminal.paste_confirm_on_newline").contains("true");
    let mut pending_paste: Option<Vec<u8>> = None;
    // Survives an iteration boundary the way `layout_dirty` does. The hint
    // line is drawn inside the frame, and a flag set while processing input
    // at the end of one iteration is lost when the next one starts — which is
    // how the paste confirmation first managed to never appear.
    let mut hint_dirty = false;
    let separators = {
        let configured = setting(&state, "terminal.word_separators");
        configured.trim().trim_matches('"').to_owned()
    };
    let leader = bindings
        .as_ref()
        .map_or_else(omt_tui::app::default_leader, |b| b.leader());
    let leader_key = omt_tui::app::leader_chord(&leader);
    // omt's own furniture, from the theme that has existed the whole time and
    // that none of the chrome ever read. Worked out once: a terminal's colour
    // depth does not change under somebody's fingers, and asking per frame
    // would put an environment lookup in the draw path.
    let depth = omt_tui::paint::Depth::of_environment();
    let palette = omt_tui::paint::Chrome::builtin();
    let notify_env = omt_notify::Environment::from_process();
    let sounds = omt_notify::sound::Sounds {
        needs_you: sound_file(&state, "notifications.sound.needs_you"),
        finished: sound_file(&state, "notifications.sound.finished"),
        muted: setting(&state, "notifications.sound").contains("false"),
    };

    // Released before the frame loop and taken again per iteration, so the
    // hook listener gets the instance between frames rather than never.
    drop(instance);

    'session: loop {
        let mut instance = state.lock().map_err(|e| anyhow::anyhow!("{e}"))?;

        // Whether this iteration has anything to draw.
        //
        // The loop runs at `TICK` and drew every time, which is 125 walks of
        // every pane's grid per second on a screen where nothing is happening
        // — measured at 1% of a core per pane, idle, which is a laptop on
        // battery being asked to do nothing at a steady rate.
        //
        // The pump still runs every tick: it is what *reads* the pty, and a
        // read skipped is output delayed. What is gated is the drawing, and
        // the signals below are deliberately generous — a missed repaint is a
        // stale screen, which is far worse than a wasted one.
        // The leader being armed counts, and it is the interesting case: the
        // which-key strip appears 250 ms *after* the key, on a timer rather
        // than on an event. A gate that only knows about events skips the
        // frame that would draw it, and the strip never appears — which is
        // exactly what happened, and is the shape of every bug this kind of
        // optimisation causes. Anything that changes with time and not with
        // input has to say so here.
        let mut anything_happened = layout_dirty || hint_dirty || overlay.is_some() || armed;

        for pane in &mut panes {
            // Whether anything came off the pty is the floor's only signal, so
            // it is recorded here rather than asked for later: by the time the
            // probe runs, the pump that saw the bytes has returned.
            let produced = backing(&mut instance, &mut attached).pump(pane.session)?;
            if !produced.is_empty() {
                pane.last_output_ms = monotonic_ms();
                anything_happened = true;
            }
            // A program in the pane asking for somebody's attention — the
            // long build that finished while its window was behind a browser.
            // Passed on rather than drawn: omt is the terminal it is talking
            // to, and a terminal that swallowed these is why people run their
            // builds outside the multiplexer.
            for event in &produced {
                if let omt_events::EventPayload::Terminal(omt_events::TerminalEvent::Bell) =
                    &event.payload
                {
                    if bell_audible {
                        // The outer terminal's own bell, which is what a
                        // terminal does with a bell it was handed.
                        let _ = io::stdout().write_all(b"\x07");
                    }
                    if bell_visual {
                        bell_until = monotonic_ms() + 350;
                    }
                }
                if let omt_events::EventPayload::Terminal(omt_events::TerminalEvent::Notified {
                    title,
                    body,
                }) = &event.payload
                {
                    let heading = title.clone().unwrap_or_else(|| body.clone());
                    let _ = omt_notify::raise(
                        &mut io::stdout(),
                        &notify_env,
                        &heading,
                        title.as_ref().map(|_| body.as_str()),
                    );
                }
            }
        }

        // What is running in each pane. The agent layer existed and nothing
        // fed it from here, so `⟨leader⟩ a` was structurally always empty:
        // starting Claude Code in a pane and asking omt about it showed
        // "nothing matches", because nothing had ever looked.
        let now = monotonic_ms();
        for (index, pane) in panes.iter_mut().enumerate() {
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
            // What the agent wrote down itself, on the same timer as the
            // probe. Only where an agent reported a path; a local terminal
            // whose instance already reads this file will find every record
            // already seen and take nothing, which is what the suppression is
            // there for.
            crate::absorb::from_session_file(&state, &mut instance, pane.session);

            let detection = instance.observe_session(pane.session, pane.binding, &refs, now);
            let before = pane.agent.as_ref().map(|p| p.detection.agent);
            pane.agent = detection.map(|detection| crate::probe::Probed {
                activity: crate::probe::activity(quiet_ms < 1_000, false, quiet_ms / 1_000),
                detection,
            });
            // Observation runs on its own timer, so what it finds arrives
            // without any event to hang a repaint on. The status bar names the
            // agent in a pane; without this the pane keeps saying "shell"
            // until something else happens to redraw it, which for a shell
            // sitting at a prompt is until somebody types.
            if before != pane.agent.as_ref().map(|p| p.detection.agent) {
                anything_happened = true;
            }
            // The panel shows what every agent is *doing*, which changes on
            // this timer with no event behind it. While it is on, an
            // observation pass is a reason to draw — and the draw itself is
            // gated on the panel's own content changing, so a pass that found
            // nothing new still costs nothing.
            if panel_on {
                anything_happened = true;
            }
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
            // The whole premise of this product is that agents work while you
            // are doing something else — and everything omt draws answers "who
            // needs me" only to somebody already looking at the screen, which
            // is the one case where the answer did not need finding. This is
            // the part that reaches somebody in a browser, in an editor, in
            // another window.
            //
            // Outside the block above deliberately: that one asks whether omt
            // has *identified* an agent in this pane, and a hook is an agent
            // saying what it is doing whether or not the probe has named it
            // yet. Gating on identification meant the strongest signal omt has
            // could not raise a notification.
            if let Some(now_state) = instance.agent_state(pane.binding, now) {
                let watching = focus == index && overlay.is_none();
                if let Some(moment) =
                    omt_notify::worth_saying(when, pane.said.as_ref(), &now_state, watching)
                {
                    announce(moment, pane, &notify_env, &sounds);
                }
                pane.said = Some(now_state);
            }
        }

        // Panes whose process ended stop being drawn. The session stays in the
        // tree — it is finished, not forgotten — but a dead pane holding screen
        // space is space the live ones need.
        let before = panes.len();
        // Kept, not just counted: when the last one goes it is the only thing
        // that can say why omt is about to close.
        let mut gone = Vec::new();
        panes.retain(|p| {
            let finished = instance
                .session(p.session)
                .is_some_and(omt_session::Session::is_finished);
            if finished {
                gone.push(p.session);
            }
            !finished
        });
        if panes.len() != before {
            layout_dirty = true;
            focus = focus.min(panes.len().saturating_sub(1));
        }
        if panes.is_empty() {
            why = gone
                .last()
                .and_then(|id| instance.session(*id))
                .map(ended_because);
            break 'session;
        }
        if panes.len() != before {
            // A pane went. Everything moves.
            anything_happened = true;
        }

        // What finished running in a pane this terminal owns, into the history
        // the suggestions come from. After the pump, because a block closes in
        // the bytes the pump just read.
        if attached.is_none()
            && let Ok(mut history) = state.recall()
        {
            for pane in &panes {
                let Some(workspace) = instance.session(pane.session).map(|s| s.workspace) else {
                    continue;
                };
                if let Some(runtime) = instance.runtime(pane.session) {
                    remembered.absorb(&mut history, pane.session, workspace, runtime.terminal());
                }
            }
        }

        // One view of the sessions per frame, which every list, the panel and
        // the quit confirmation then read. Built here rather than per surface
        // because it is the same question asked four times — and because it is
        // the seam: a terminal watching an instance it does not own produces
        // this from a roster instead of a probe, and nothing above this line
        // changes.
        let watched = if let Some(joined) = attached.as_mut() {
            // On the probe's timer rather than per frame: it is a socket round
            // trip, and what it carries — titles, agent state, thread counts —
            // changes on human timescales, not on frame ones.
            if now.saturating_sub(rostered_at) >= crate::probe::PROBE_EVERY_MS {
                rostered_at = now;
                // A refresh that fails is an instance that has gone, which the
                // frame below reports. Losing a roster is not worth ending the
                // terminal over.
                let _ = crate::backing::refresh_roster(joined);
            }
            crate::rows::watched_remote(&joined.roster.clone(), &panes, &cwd)
        } else {
            crate::rows::watched_local(&mut instance, &panes, now)
        };

        // Nothing changed and nothing is waiting to be typed. Wait out the
        // tick here and go round again without drawing.
        //
        // `poll` does not consume, so the input loop at the bottom of the
        // frame still reads whatever arrives — this only decides whether the
        // walk over every pane's grid is worth doing, and the pump above has
        // already run, so output is never delayed by it.
        if !anything_happened && !crossterm::event::poll(TICK).context("polling for input")? {
            continue 'session;
        }

        // Where each pane goes, from the tree rather than from a count. Panes
        // whose leaf is gone are not drawn, which cannot happen — the tree and
        // the list are changed together — but is cheaper to handle than to
        // prove.
        // The panel takes a fixed column when it fits and nothing when it does
        // not, against the same minimum a split is refused for — a panel may
        // never do what splitting is not allowed to do (19 §2.1b).
        let panel_cols = if panel_on && omt_tui::fits(term_cols, panes.len()) {
            omt_tui::PANEL_COLS
        } else {
            0
        };
        let arrangement = omt_tui::place_with_zoom(
            &layout,
            omt_tui::Rect {
                col: panel_cols,
                row: 0,
                cols: term_cols.saturating_sub(panel_cols),
                rows: term_rows.saturating_sub(STATUS_ROWS).max(1),
            },
            zoomed,
        );
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
                backing(&mut instance, &mut attached).resize(pane.session, rect.cols, rect.rows)?;
            }
            draw_separators(&mut out, &rects, palette, depth).context("drawing separators")?;
            layout_dirty = false;
        }

        let mut mode = EncodeMode::default();
        for (i, (pane, rect)) in panes.iter_mut().zip(&rects).enumerate() {
            let seam = backing(&mut instance, &mut attached);
            let Some(terminal) = seam.terminal(pane.session) else {
                continue;
            };
            if i == focus {
                mode = EncodeMode {
                    application_cursor: terminal.modes().contains(TermMode::ApplicationCursor),
                };
            }
            pane.screen
                // Offset by the tab bar, which owns the first row.
                .draw_at(&mut out, terminal.grid(), rect.col, rect.row + CHROME_ROWS)
                .context("drawing")?;
        }

        // The images, over the cards that were just drawn for them. An
        // overlay rather than a change to the renderer: every path that ends
        // in no image — a terminal that said nothing, no cell size, bytes
        // evicted, a clip that leaves nothing visible — lands on the card,
        // because the card is already there.
        if host_graphics != crate::host_graphics::Host::Nothing
            && let Some(cell) = crate::host_graphics::cell_size()
        {
            let mut wanted = Vec::new();
            for (i, (pane, rect)) in panes.iter().zip(&rects).enumerate() {
                let seam = backing(&mut instance, &mut attached);
                let Some(terminal) = seam.terminal(pane.session) else {
                    continue;
                };
                let grid_rows = terminal.grid().size().rows;
                // The absolute row the live screen begins at, which is the
                // coordinate images are recorded against.
                let top = terminal.total_rows().saturating_sub(u64::from(grid_rows));
                let area = omt_tui::Rect {
                    col: rect.col,
                    row: rect.row + CHROME_ROWS,
                    cols: rect.cols,
                    rows: rect.rows,
                };
                for (index, (started, image)) in terminal.images().iter().enumerate() {
                    if image.data.is_empty() {
                        // Evicted, or a format omt cannot hand back. Its card
                        // is the answer, and the card is already drawn.
                        continue;
                    }
                    // An id of omt's own, per pane and per image, so two panes
                    // showing the same picture do not overwrite each other's
                    // placement on the host.
                    let id = crate::host_graphics::host_id(i, index);
                    if let Some(placement) =
                        crate::host_graphics::place(id, *started, image.rows, image.cols, top, area)
                    {
                        wanted.push((placement, image.data.clone()));
                    }
                }
            }
            let bytes = drawn_images.sync(&wanted, cell);
            if !bytes.is_empty() {
                out.write_all(&bytes).context("drawing images")?;
            }
        }

        // Whatever is on screen and focused is being looked at, so its result
        // is not something somebody has yet to find.
        if let Some(pane) = panes.get_mut(focus) {
            pane.seen = true;
        }
        // The panel, in the column the panes were moved over for. Drawn before
        // the status bar and after the panes, so nothing paints over it.
        if panel_cols > 0 {
            // The branch from the cache the status bar already fills, so the
            // panel costs no extra process spawns.
            let on_branch = branch.get(&cwd, monotonic_ms());
            let sessions = crate::rows::panel_sessions(&watched, on_branch.as_deref());
            let rows_for_panel = term_rows.saturating_sub(STATUS_ROWS).max(1);
            let lines = omt_tui::panel::render(&omt_tui::entries(&sessions), rows_for_panel);
            // Only when it changed. Forcing a draw every tick because the
            // panel is on wrote 168 KB in two idle seconds — the whole of the
            // saving this loop was just given, spent on redrawing a list of
            // unchanged names sixty times a second.
            if shown_panel.as_ref() != Some(&lines) {
                for (i, line) in lines.iter().enumerate() {
                    write!(out, "\x1b[{};1H\x1b[2m{line}\x1b[0m", i + 1)
                        .context("drawing the agent panel")?;
                }
                shown_panel = Some(lines);
            }
        } else {
            shown_panel = None;
        }

        let focused_session = panes[focus].session;
        let situation = omt_tui::Situation {
            leader: leader_key.display(),
            paste_pending_lines: pending_paste
                .as_ref()
                .map(|p| p.iter().filter(|b| **b == b'\n').count() + 1),
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
                "Agents" => open.set_choices(agent_rows(&watched)),
                "Sessions" => open.set_choices(session_rows(&watched)),
                // Rebuilt like the others, and for a stronger reason: a
                // conversation open on screen is one somebody is watching an
                // agent write into.
                "Conversation" => {
                    open.set_choices(conversation_of(
                        &instance,
                        &mut attached,
                        &state,
                        panes[focus].session,
                        term_cols,
                    ));
                }
                // The palette is the catalog, which does not change while a
                // process is running.
                _ => {}
            }

            // Unconditionally, not only when its text changed. The panes
            // underneath repaint every frame from their own damage, so an
            // overlay drawn once is painted over within milliseconds and what
            // is left is half a list interleaved with a shell — which is
            // exactly what it looked like.
            let lines = omt_tui::overlay::render_rows(
                open,
                term_cols,
                term_rows.saturating_sub(STATUS_ROWS),
            );
            for (i, (row, line)) in lines.iter().enumerate() {
                let painted = omt_tui::paint::overlay_line(*row, line, palette, depth);
                write!(out, "\x1b[{};1H{painted}", i as u16 + 2).context("drawing the overlay")?;
            }
        }

        // History, over the pane it belongs to. Drawn after the panes and
        // before the bar, so the pane's own renderer — which believes it owns
        // those cells and is still drawing live output into them — cannot
        // paint over what somebody is reading.
        if let (Some(reading), Some(rect)) = (copying.as_ref(), rects.get(focus)) {
            // The rows first, while the borrow is short: the terminal belongs
            // to whichever side of the seam owns the session, and the frame is
            // about to write.
            let session = panes.get(focus).map(|pane| pane.session);
            let lines = if let (Some(joined), Some(id)) = (attached.as_mut(), session) {
                // From the instance, whose row numbers count from the
                // session's first line. A mirror starts counting at the moment
                // this terminal attached and holds nothing before it, so
                // reading history from it shows a screen of blanks — which is
                // exactly what it did, with no error anywhere to say so.
                match crate::backing::history(joined, id, reading.top, u32::from(rect.rows)) {
                    Ok((rows, _)) => rows
                        .into_iter()
                        .enumerate()
                        .map(|(offset, text)| (reading.top + offset as u64, text))
                        .collect::<Vec<_>>(),
                    // Said rather than drawn as emptiness: a blank history and
                    // a history omt could not fetch are different answers, and
                    // only one of them is a bug.
                    Err(e) => vec![(reading.top, format!("history unavailable: {e}"))],
                }
            } else {
                // The rows while the borrow is short: the terminal belongs to
                // whichever side of the seam owns the session, and the frame
                // is about to write.
                let seam = backing(&mut instance, &mut attached);
                let terminal = session.and_then(|id| seam.terminal(id));
                crate::terminal_bits::history_rows(reading, *rect, terminal)
            };
            crate::terminal_bits::draw_history(
                &mut out, reading, *rect, &lines, keymap, palette, depth,
            )
            .context("drawing history")?;
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
            leader: leader_key.display(),
            bell: now < bell_until,
        };
        let bar = omt_tui::status_bar(&status, term_cols);
        if shown_bar.as_deref() != Some(bar.as_str()) {
            write!(
                out,
                "\x1b[{};1H{}",
                term_rows,
                omt_tui::paint::status_line(&bar, palette, depth)
            )
            .context("drawing the status bar")?;
            shown_bar = Some(bar);
        }

        // The which-key strip, and only when there is one. 16 §3.3 draws it
        // *over the pane's last row* and restores the row afterwards, rather
        // than reserving one: a row kept for a hint that is absent almost
        // always is a row spent on nothing.
        // Within the width, because the leader strip is a menu: built too wide
        // it is truncated, and what is lost is the end of it.
        // `hints.enabled` is in the settings reference and reached nothing, so
        // the one surface that exists to stop nagging could not be turned off.
        // The leader strip is not a hint and stays: it is a menu somebody
        // opened by pressing the leader, and hiding what they just asked for
        // is a different thing entirely.
        let hint = if hints_enabled || situation.prefix_armed {
            omt_tui::hint_for_within(&situation, term_cols)
        } else {
            None
        };
        hint_dirty = false;
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
                        crossterm::event::KeyCode::Enter if open.title == QUIT_TITLE => {
                            // The one list whose answer is the list itself.
                            // Handled here because an open overlay takes every
                            // key — a confirmation that let its answer through
                            // to the keymap could never be answered, which is
                            // what happened.
                            break 'session;
                        }
                        crossterm::event::KeyCode::Enter => {
                            // Picking is closing. What a choice *does* depends
                            // on which list it came from, and that is decided
                            // where the list was built.
                            picked_from = Some(open.title.clone());
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
                    // The agent's own command, typed into the agent's own
                    // interface — which is where it belongs, since omt has no
                    // composer of its own and the agent's menu is the one that
                    // knows what the command's arguments mean. Typed, not
                    // sent: the newline is somebody's to press once they have
                    // finished the line.
                    if picked_from.as_deref() == Some(RECALL_TITLE) {
                        // Typed, not run. A command from an hour ago may want
                        // a different argument today, and a list that pressed
                        // Enter for somebody would be running things they only
                        // meant to look at.
                        picked_from = None;
                        if label.starts_with("nothing has been recorded") {
                            continue;
                        }
                        let session = panes[focus].session;
                        let bytes = label.into_bytes();
                        if attached.is_some() {
                            backing(&mut instance, &mut attached).write(session, &bytes)?;
                        } else {
                            let pane = &mut panes[focus];
                            if let Some(rt) = instance.runtime_mut(session) {
                                rt.write_input(
                                    &mut pane.writer,
                                    pane.epoch,
                                    monotonic_ms(),
                                    &bytes,
                                )
                                .context("writing input")?;
                            }
                        }
                        continue;
                    }
                    if picked_from.as_deref() == Some(COMMANDS_TITLE) {
                        if !label.starts_with('/') {
                            // The row that says the agent has not reported
                            // any. Typing that sentence into a shell would be
                            // absurd, and it is a row rather than an absence
                            // for a reason.
                            picked_from = None;
                            continue;
                        }
                        let session = panes[focus].session;
                        let bytes = label.into_bytes();
                        if attached.is_some() {
                            backing(&mut instance, &mut attached).write(session, &bytes)?;
                        } else {
                            let pane = &mut panes[focus];
                            if let Some(rt) = instance.runtime_mut(session) {
                                rt.write_input(
                                    &mut pane.writer,
                                    pane.epoch,
                                    monotonic_ms(),
                                    &bytes,
                                )
                                .context("writing input")?;
                            }
                        }
                        picked_from = None;
                        continue;
                    }
                    picked_from = None;
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

            // The pointer, before anything that reads keys. Where it goes is
            // decided per event and mostly says "not mine": the program in
            // the pane asked for the mouse, and a multiplexer that swallows
            // its clicks has broken every full-screen program somebody runs.
            if let crossterm::event::Event::Mouse(pointer) = &event {
                let rect = rects.get(focus).copied().unwrap_or(omt_tui::Rect {
                    col: 0,
                    row: 0,
                    cols: 0,
                    rows: 0,
                });
                if crate::mouse::handle(
                    pointer,
                    rect,
                    &mut instance,
                    &mut attached,
                    &mut panes,
                    focus,
                    &mut copying,
                )? {
                    layout_dirty = true;
                }
                continue;
            }

            // Copy mode takes every key, like an overlay does and for the same
            // reason: somebody is reading, and a keystroke that reached the
            // shell would type into a program nobody is looking at. Checked
            // before the leader so `[` twice does not re-enter it.
            if let Some(reading) = copying.as_mut()
                && !armed
            {
                if let crossterm::event::Event::Key(key) = &event {
                    let rect = rects.get(focus).copied().unwrap_or(omt_tui::Rect {
                        col: 0,
                        row: 0,
                        cols: 0,
                        rows: 0,
                    });
                    let session = panes.get(focus).map(|pane| pane.session);
                    // Where the shell said its prompts are. Owned rather than
                    // borrowed, because the attached branch below needs the
                    // instance back — and filtered here, where `attributed`
                    // means something: a block opened from output alone has no
                    // prompt, and jumping to a row omt guessed at would land
                    // somewhere arbitrary while looking deliberate.
                    let prompts: Vec<u64> = {
                        let seam = backing(&mut instance, &mut attached);
                        session
                            .and_then(|id| seam.terminal(id))
                            .map(|t| {
                                t.blocks()
                                    .iter()
                                    .filter(|block| block.attributed)
                                    .map(|block| block.prompt_row)
                                    .collect()
                            })
                            .unwrap_or_default()
                    };
                    // How far the history goes, and how to read a line of it.
                    // Attached, both come from the instance: a mirror starts
                    // counting rows at the moment this terminal attached, and
                    // a reader blending the two would be shown a line other
                    // than the one they are on.
                    let outcome = if let (Some(joined), Some(id)) = (attached.as_mut(), session) {
                        let total = crate::backing::history(joined, id, 0, 1)
                            .map(|(_, total)| total)
                            .unwrap_or_default();
                        crate::terminal_bits::reading_key(
                            key,
                            reading,
                            crate::terminal_bits::Viewport {
                                rect,
                                total,
                                flavour: keymap,
                                separators: &separators,
                                prompts: &prompts,
                            },
                            &mut |row| crate::backing::history_line(joined, id, row),
                        )
                    } else {
                        let seam = backing(&mut instance, &mut attached);
                        let terminal = session.and_then(|id| seam.terminal(id));
                        let total = terminal.map_or(0, omt_term::Terminal::total_rows);
                        crate::terminal_bits::reading_key(
                            key,
                            reading,
                            crate::terminal_bits::Viewport {
                                rect,
                                total,
                                flavour: keymap,
                                separators: &separators,
                                prompts: &prompts,
                            },
                            &mut |row| {
                                terminal
                                    .and_then(|t| t.line_at(row))
                                    .map(omt_term::Line::text)
                            },
                        )
                    };
                    match outcome {
                        crate::terminal_bits::Reading::Yank => {
                            if let (Some(joined), Some(id)) = (attached.as_mut(), session) {
                                let span = reading.span().map(|(from, to)| (from.0, to.0));
                                let rows = span
                                    .and_then(|(from, to)| {
                                        let count =
                                            u32::try_from(to.saturating_sub(from) + 1).ok()?;
                                        crate::backing::history(joined, id, from, count).ok()
                                    })
                                    .map(|(rows, _)| rows)
                                    .unwrap_or_default();
                                let base = span.map_or(0, |(from, _)| from);
                                crate::terminal_bits::yank(reading, &mut |row| {
                                    usize::try_from(row.checked_sub(base)?)
                                        .ok()
                                        .and_then(|i| rows.get(i).cloned())
                                });
                            } else {
                                let seam = backing(&mut instance, &mut attached);
                                let terminal = session.and_then(|id| seam.terminal(id));
                                crate::terminal_bits::yank(reading, &mut |row| {
                                    terminal
                                        .and_then(|t| t.line_at(row))
                                        .map(omt_term::Line::text)
                                });
                            }
                            copying = None;
                        }
                        crate::terminal_bits::Reading::Leave => copying = None,
                        crate::terminal_bits::Reading::Stay => {}
                    }
                }
                layout_dirty = true;
                continue;
            }

            let (input, next) = omt_tui::translate(&event, armed, mode, leader_key);
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
                // A paste that has to be asked about before it reaches the
                // program. `paste_confirm_lines` and
                // `paste_confirm_on_newline` were in the settings reference
                // and reached nothing: a paste of two hundred lines went
                // straight into the shell, where the first line ran. When the
                // confirmation is pending, the next key answers it, so the
                // paste cannot land while the question is up.
                Input::Paste(bytes) if pending_paste.is_none() => {
                    let pane = &mut panes[focus];
                    let session = pane.session;
                    let lines = bytes.iter().filter(|b| **b == b'\n').count() + 1;
                    let confirm = paste_confirm_lines > 0
                        && (lines > paste_confirm_lines || (paste_confirm_on_newline && lines > 1));
                    if confirm {
                        pending_paste = Some(bytes);
                        hint_dirty = true;
                    } else if attached.is_some() {
                        backing(&mut instance, &mut attached).write(session, &bytes)?;
                    } else if let Some(rt) = instance.runtime_mut(session) {
                        rt.write_input(&mut pane.writer, pane.epoch, monotonic_ms(), &bytes)
                            .context("writing input")?;
                    }
                }
                // A second paste while one is waiting for its answer: the
                // first is still on the screen, and stacking pastes invites
                // the wrong one through on a stray y.
                Input::Paste(_) => {}
                Input::Bytes(bytes) if pending_paste.is_some() => {
                    // The answer to the question on screen. y hands the paste
                    // over; anything else drops it, which is the safe side of
                    // a yes/no question with no wrong answer worth regret.
                    let paste = pending_paste.take();
                    hint_dirty = true;
                    if (bytes == b"y" || bytes == b"Y")
                        && let Some(paste) = paste
                    {
                        let pane = &mut panes[focus];
                        let session = pane.session;
                        if attached.is_some() {
                            backing(&mut instance, &mut attached).write(session, &paste)?;
                        } else if let Some(rt) = instance.runtime_mut(session) {
                            rt.write_input(&mut pane.writer, pane.epoch, monotonic_ms(), &paste)
                                .context("writing input")?;
                        }
                    }
                }
                Input::Bytes(bytes) => {
                    let pane = &mut panes[focus];
                    let session = pane.session;
                    if attached.is_some() {
                        // Over the socket, with the epoch the instance handed
                        // out. The token is arbitrated where the session lives,
                        // which is the only place that can see every actor.
                        backing(&mut instance, &mut attached).write(session, &bytes)?;
                    } else if let Some(rt) = instance.runtime_mut(session) {
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
                    overlay = Some(omt_tui::Overlay::new("Agents", agent_rows(&watched)));
                    layout_dirty = true;
                }
                Input::CopyMode => {
                    // How much history there is, from whichever side owns it.
                    // Asking the local instance while attached answered zero,
                    // and a reader entering at row zero of nothing sees a
                    // blank screen rather than the end of their session.
                    let session = panes.get(focus).map(|p| p.session);
                    let total = if let (Some(joined), Some(id)) = (attached.as_mut(), session) {
                        crate::backing::history(joined, id, 0, 1)
                            .map(|(_, total)| total)
                            .unwrap_or_default()
                    } else {
                        session
                            .and_then(|id| instance.runtime(id))
                            .map_or(0, |rt| rt.terminal().total_rows())
                    };
                    let rows = rects.get(focus).map_or(1, |r| r.rows);
                    copying = Some(omt_tui::CopyMode::entering(total, rows));
                    layout_dirty = true;
                }
                Input::Recall => {
                    let suggestions = crate::rows::recall_of(&mut attached, &state, workspace);
                    overlay = Some(omt_tui::Overlay::new(
                        RECALL_TITLE,
                        crate::rows::recall_rows(&suggestions),
                    ));
                    layout_dirty = true;
                }
                Input::Commands => {
                    let session = panes[focus].session;
                    let commands = crate::rows::offers_of(&mut attached, &state, session);
                    overlay = Some(omt_tui::Overlay::new(
                        COMMANDS_TITLE,
                        crate::rows::command_rows(&commands),
                    ));
                    layout_dirty = true;
                }
                Input::Conversation => {
                    overlay = Some(omt_tui::Overlay::new(
                        "Conversation",
                        conversation_of(
                            &instance,
                            &mut attached,
                            &state,
                            panes[focus].session,
                            term_cols,
                        ),
                    ));
                    layout_dirty = true;
                }
                Input::Sessions => {
                    // Every session this instance holds, not only the ones with
                    // a pane on screen. A session that is running somewhere
                    // else is exactly the one somebody opens this list to find,
                    // and a list that showed only what was already visible
                    // answered a question nobody had.
                    overlay = Some(omt_tui::Overlay::new("Sessions", session_rows(&watched)));
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
                            // Where the new session is made depends on who
                            // owns them. Attached, asking the instance is not
                            // a detail: a session this process spawned would
                            // die with it, in a terminal whose whole promise
                            // is that they do not.
                            let opened = if let Some(joined) = attached.as_mut() {
                                crate::backing::split_session(joined, &cwd, size.cols, size.rows)
                                    .map(|session| LocalPane::watching(session, new_tile))
                            } else {
                                LocalPane::open(
                                    &mut instance,
                                    &Opening {
                                        workspace,
                                        program: &shell,
                                        cwd: &cwd,
                                        size,
                                        tile: new_tile,
                                        socket: &socket_text,
                                        limits,
                                    },
                                )
                            };
                            if let Ok(pane) = opened {
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
                        previous_focus = Some(panes[focus].tile);
                        focus = index;
                        shown_hint = None;
                        shown_bar = None;
                        shown_bottom = None;
                    }
                }
                Input::Panel => {
                    // 19 §2.1b: a panel somebody has to re-open every session
                    // is one they stop using, so the choice is remembered.
                    panel_on = !panel_on;
                    crate::rows::remember_panel(panel_on);
                    layout_dirty = true;
                }
                Input::Zoom => {
                    // A view, not a rearrangement: the tree is untouched, so
                    // this comes back to exactly what it was.
                    zoomed = if zoomed.is_some() {
                        None
                    } else {
                        Some(panes[focus].tile)
                    };
                    layout_dirty = true;
                }
                Input::LastPane => {
                    if let Some(back) = previous_focus
                        && let Some(index) = panes.iter().position(|p| p.tile == back)
                    {
                        previous_focus = Some(panes[focus].tile);
                        focus = index;
                        shown_hint = None;
                        shown_bar = None;
                        shown_bottom = None;
                    }
                }
                Input::Swap(towards) => {
                    // The pane moves and the focus follows it. Focus staying
                    // put means the pane just moved is no longer the one being
                    // typed in, which is what every implementation that does
                    // it that way gets reports about.
                    let area = omt_tui::Rect {
                        col: panel_cols,
                        row: 0,
                        cols: term_cols.saturating_sub(panel_cols),
                        rows: term_rows.saturating_sub(STATUS_ROWS).max(1),
                    };
                    let here = panes[focus].tile;
                    if let Some(there) = omt_tui::neighbour(&layout.place(area), here, towards)
                        && layout.swap(here, there)
                    {
                        layout_dirty = true;
                    }
                }
                Input::NextWaiting => {
                    // The product's own subject, in one key. Needing a list and
                    // a choice to reach the agent that is waiting is two steps
                    // for the thing omt exists to make one.
                    let waiting: Vec<usize> = panes
                        .iter()
                        .enumerate()
                        .filter(|(_, pane)| {
                            matches!(
                                instance.agent_state(pane.binding, now),
                                Some(omt_types::AgentState::Blocked { .. })
                            )
                        })
                        .map(|(i, _)| i)
                        .collect();
                    match waiting
                        .iter()
                        .find(|i| **i > focus)
                        .or_else(|| waiting.first())
                    {
                        Some(&landed) => {
                            previous_focus = Some(panes[focus].tile);
                            focus = landed;
                            // Zoom follows: jumping to a waiting agent while
                            // zoomed on another shows the one you jumped to.
                            if zoomed.is_some() {
                                zoomed = Some(panes[focus].tile);
                            }
                            layout_dirty = true;
                        }
                        // Said rather than nothing happening. A key that
                        // silently does nothing is one people press again.
                        None => shown_hint = None,
                    }
                }
                Input::Goto(n) => {
                    // One-based, matching the numbers the status bar writes.
                    // A key that disagreed with what is on the screen would be
                    // worse than no key.
                    if let Some(index) = n.checked_sub(1).filter(|i| *i < panes.len()) {
                        previous_focus = Some(panes[focus].tile);
                        focus = index;
                        if zoomed.is_some() {
                            zoomed = Some(panes[focus].tile);
                        }
                        layout_dirty = true;
                    }
                }
                Input::NextPane => {
                    previous_focus = Some(panes[focus].tile);
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
                    // The zoom goes with it. `place_with_zoom` already falls
                    // back rather than blanking the screen, but leaving a
                    // stale id means the next zoom toggles nothing.
                    if zoomed == Some(closed.tile) {
                        zoomed = None;
                    }
                    if previous_focus == Some(closed.tile) {
                        previous_focus = None;
                    }
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
                Input::Detach => {
                    // What this actually ends, before it ends it. The key is
                    // bound where every multiplexer binds "detach", and in
                    // omt it kills every pty this process holds — so the one
                    // thing it must not do is take an agent mid-turn away
                    // without saying so.
                    // Attached, quitting ends nothing: the sessions are the
                    // instance's and they keep running. Asking would be a
                    // question about a consequence that no longer happens,
                    // and a confirmation nobody needs is one people learn to
                    // dismiss without reading.
                    let at_work = if attached.is_some() {
                        Vec::new()
                    } else {
                        crate::terminal_bits::still_working(&watched)
                    };
                    if at_work.is_empty() {
                        break 'session;
                    }
                    overlay = Some(omt_tui::Overlay::new(QUIT_TITLE, at_work));
                    layout_dirty = true;
                }
                Input::Ignore => {}
            }
        }
    }

    // Take the images off before leaving. One left behind outlives the program
    // that drew it and sits on the shell somebody gets back.
    let bye = drawn_images.clear();
    if !bye.is_empty() {
        out.write_all(&bye).ok();
    }
    out.flush().ok();
    // The terminal goes back to the person *before* anything is said, or the
    // sentence is written onto a screen that is about to be thrown away —
    // which is how "it just vanished" happens.
    drop(_guard);
    if let Some(reason) = why {
        println!("omt: {reason}");
    }
    Ok(())
}

/// Why omt is closing, from the session that ended it.
///
/// A terminal that closes without a word cannot be told apart from one that
/// crashed. The session knows — it has been carrying the exit status the whole
/// time — and nothing ever asked it.
fn ended_because(session: &omt_session::Session) -> String {
    match session.state {
        omt_session::SessionState::Exited { code: Some(0) }
        | omt_session::SessionState::Exited { code: None } => {
            "the last session ended, so there is nothing left to show".to_owned()
        }
        // Above 128 is a signal wearing an exit status. Saying which one is
        // the difference between "my shell died" and "I pressed something".
        omt_session::SessionState::Exited { code: Some(code) } if code > 128 => format!(
            "the last session's program was stopped by signal {} (exit {code}), \
             so there is nothing left to show",
            code - 128
        ),
        omt_session::SessionState::Exited { code: Some(code) } => format!(
            "the last session's program exited with {code}, so there is nothing left to show"
        ),
        other => format!("the last session ended ({other:?})"),
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

fn draw_separators(
    out: &mut impl Write,
    rects: &[omt_tui::Rect],
    palette: omt_tui::paint::Chrome,
    depth: omt_tui::paint::Depth,
) -> io::Result<()> {
    // Built once rather than per row: the rule is the same every time, and a
    // hundred-row window is a hundred allocations of the same short string.
    let rule = omt_tui::paint::separator(palette, depth);
    for pair in rects.windows(2) {
        let col = pair[0].col + pair[0].cols + 1;
        for row in 0..pair[0].rows {
            write!(out, "\x1b[{};{col}H{rule}", row + 1)?;
        }
    }
    Ok(())
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

#[cfg(test)]
#[allow(clippy::expect_used, reason = "in a test, expect() is the assertion")]
mod colour_tests {
    use crate::terminal_bits::ansi_end;

    #[test]
    fn only_the_two_ends_of_the_palette_are_named() {
        // An index in between names a colour from a palette omt has not been
        // told, and a guessed midtone is exactly the answer that renders
        // somebody's agent unreadable.
        assert_eq!(ansi_end("0").as_deref(), Some("rgb:0000/0000/0000"));
        assert_eq!(ansi_end("15").as_deref(), Some("rgb:ffff/ffff/ffff"));
        assert_eq!(ansi_end("7").as_deref(), Some("rgb:ffff/ffff/ffff"));
        assert_eq!(ansi_end("4"), None);
        assert_eq!(ansi_end("default"), None);
    }
}
