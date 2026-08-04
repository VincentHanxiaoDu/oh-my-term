//! A real process, through the real terminal, out as real events.
//!
//! Every layer is exercised for what it is: a shell on a pty, bytes through the
//! parser, positions assigned once, a client resuming from where it was. Until
//! this passed, the crates were a parts bin.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use omt_daemon::{Instance, SessionRuntime};
use omt_events::{EventPayload, ResumeOutcome, SessionTreeEvent, TerminalEvent};
use omt_pty::{PtyConfig, PtySize};
use omt_session::{SessionKind, SessionMode};
use omt_types::Seq;

fn spawn_into(
    instance: &mut Instance,
    ws: omt_types::WorkspaceId,
    script: &str,
    size: PtySize,
) -> omt_types::SessionId {
    let id = instance
        .create_session(ws, SessionKind::Shell, SessionMode::Pty)
        .expect("session");
    let runtime = SessionRuntime::spawn(
        id,
        &PtyConfig {
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".into(), script.into()],
            size,
            ..PtyConfig::default()
        },
        omt_term::ScrollbackLimits::default(),
    )
    .expect("spawn");
    instance.attach(runtime).expect("attach");
    id
}

fn instance_running(script: &str, size: PtySize) -> (Instance, omt_types::SessionId) {
    let mut instance = Instance::new();
    let ws = instance.open_workspace("/tmp").expect("workspace");
    let id = spawn_into(&mut instance, ws, script, size);
    (instance, id)
}

fn drive(
    instance: &mut Instance,
    id: omt_types::SessionId,
    done: impl Fn(&Instance) -> bool,
) -> Vec<omt_events::Event> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut events = Vec::new();
    while Instant::now() < deadline {
        events.extend(instance.pump_session(id).expect("pump"));
        if done(instance) {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    events
}

fn finished(i: &Instance, id: omt_types::SessionId) -> bool {
    i.session(id).is_some_and(omt_session::Session::is_finished)
}

#[test]
fn a_shell_runs_and_its_output_becomes_positioned_events() {
    let (mut instance, id) = instance_running("echo hello; exit 0", PtySize::new(80, 24));
    let events = drive(&mut instance, id, |i| finished(i, id));

    assert!(!events.is_empty(), "nothing came out");

    // Consecutive within the session, because that is what a resuming client
    // counts on: a gap is indistinguishable from a lost event.
    let seqs: Vec<u64> = events.iter().map(|e| e.seq.get()).collect();
    for pair in seqs.windows(2) {
        assert_eq!(pair[1], pair[0] + 1, "gap in {seqs:?}");
    }

    assert!(
        events.iter().any(|e| matches!(
            e.payload,
            EventPayload::Terminal(TerminalEvent::Output { .. })
        )),
        "no output event"
    );
    assert!(
        events.iter().any(|e| matches!(
            e.payload,
            EventPayload::SessionTree(SessionTreeEvent::SessionClosed { code: Some(0), .. })
        )),
        "no close event carrying the exit code"
    );
}

#[test]
fn what_the_program_printed_is_on_the_grid() {
    let (mut instance, id) = instance_running("echo on-the-grid; sleep 3", PtySize::new(80, 24));
    drive(&mut instance, id, |i| {
        i.runtime(id).is_some_and(|r| {
            r.terminal()
                .screen_text()
                .iter()
                .any(|l| l.contains("on-the-grid"))
        })
    });
    let text = instance
        .runtime(id)
        .expect("runtime")
        .terminal()
        .screen_text()
        .join("\n");
    assert!(text.contains("on-the-grid"), "{text:?}");
}

#[test]
fn a_client_resumes_exactly_where_it_left_off() {
    let (mut instance, id) =
        instance_running("echo a; echo b; echo c; exit 0", PtySize::new(80, 24));
    let all = drive(&mut instance, id, |i| finished(i, id));
    assert!(all.len() >= 2, "{all:?}");

    let outcome = instance.resume(id, all[0].seq).expect("session exists");
    let ResumeOutcome::Replayed { events } = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(
        events.iter().map(|e| e.seq.get()).collect::<Vec<_>>(),
        all[1..].iter().map(|e| e.seq.get()).collect::<Vec<_>>(),
        "a resuming client got a different set than actually happened"
    );
}

#[test]
fn a_title_the_program_set_reaches_the_event_stream() {
    let (mut instance, id) = instance_running(
        "printf '\\033]0;set-by-the-program\\007'; echo marker; sleep 3",
        PtySize::new(80, 24),
    );
    let events = drive(&mut instance, id, |i| {
        i.runtime(id)
            .is_some_and(|r| r.terminal().title() == Some("set-by-the-program"))
    });
    assert!(
        events.iter().any(|e| matches!(
            &e.payload,
            EventPayload::SessionTree(SessionTreeEvent::Renamed { title, .. })
                if title == "set-by-the-program"
        )),
        "the title never became an event"
    );
}

#[test]
fn the_session_record_learns_the_process_is_gone() {
    let (mut instance, id) = instance_running("exit 7", PtySize::new(80, 24));
    drive(&mut instance, id, |i| finished(i, id));
    let session = instance.session(id).expect("session");
    assert_eq!(
        session.state,
        omt_session::SessionState::Exited { code: Some(7) }
    );
    assert!(session.exited_at.is_some());
}

#[test]
fn closing_a_session_stops_its_process() {
    // Dropping the runtime hangs the child up; nothing else has to remember to.
    let (mut instance, id) = instance_running("sleep 30", PtySize::new(80, 24));
    let pid = instance.runtime(id).expect("runtime").pty().pid();
    instance.close_session(id).expect("close");
    assert!(instance.runtime(id).is_none());

    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        // SAFETY: signal 0 only probes for the process's existence.
        #[allow(unsafe_code, reason = "signal 0 probes for a process's existence")]
        let alive = unsafe { libc::kill(pid, 0) } == 0;
        if !alive {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("the process outlived its session");
}

#[test]
fn two_sessions_number_independently_under_real_traffic() {
    // A shared counter would let a loud session advance a quiet one's position,
    // and a client resuming the quiet one would skip.
    let mut instance = Instance::new();
    let ws = instance.open_workspace("/tmp").expect("workspace");
    let ids: Vec<_> = [
        "echo loud; echo loud; echo loud; exit 0",
        "echo quiet; exit 0",
    ]
    .iter()
    .map(|s| spawn_into(&mut instance, ws, s, PtySize::new(80, 24)))
    .collect();

    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        instance.pump_all().expect("pump");
        if ids.iter().all(|id| finished(&instance, *id)) {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }

    for id in &ids {
        let outcome = instance.resume(*id, Seq::new(0)).expect("exists");
        let ResumeOutcome::Replayed { events } = outcome else {
            panic!("{outcome:?}");
        };
        assert_eq!(
            events.first().map(|e| e.seq.get()),
            Some(1),
            "a session's stream did not start at its own beginning"
        );
    }
}
