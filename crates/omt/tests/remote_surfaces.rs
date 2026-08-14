//! What each capability answers, reached over the same socket.
//!
//! The second half of `remote_call.rs`, split for P1's file-length limit.
//! Same frames and the same path — what differs is that these are about the
//! answers rather than about the call mechanism itself.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]

mod common;
use common::*;
use omt_proto::{Call, CallOutcome, Hello, ProtoMessage};
use omt_types::Role;

#[test]
fn a_job_can_be_scheduled_and_fires_once_it_is_due() {
    // `job.list` reported schedules that nothing ever fired. This drives the
    // decision the scheduler makes, without waiting on wall-clock time — the
    // schedule is a pure function over a timestamp, which is exactly why it is
    // separated from the thread that spawns processes.
    let dir = tempfile::tempdir().expect("tempdir");
    let state = omt::state::State::default();
    let (mut client, server) = client_server_pair();
    let served = state.clone();
    let worker = std::thread::spawn(move || serve(server, served));

    send(
        &mut client,
        &ProtoMessage::Hello(Hello {
            proto: omt_proto::PROTO_VERSION,
            client: "test".to_owned(),
            token: None,
        }),
    );
    let ProtoMessage::Welcome(_) = recv(&mut client) else {
        panic!("expected a welcome");
    };

    // Opened first: a job names a workspace by id, like every other capability
    // that names one. It took a bare directory path, so the generated client —
    // whose `workspace` argument carries an id everywhere else — got `is not a
    // directory` for the one obvious call.
    let opened = call_ok(
        &mut client,
        1,
        "workspace.open",
        serde_json::json!({ "root": dir.path().to_string_lossy() }),
        true,
    );
    let workspace = opened["id"].as_str().expect("a workspace id").to_owned();

    let created = call_ok(
        &mut client,
        2,
        "job.create",
        serde_json::json!({
            "name": "tests",
            "workspace": workspace,
            "run": "true",
            "every_seconds": 60,
        }),
        true,
    );
    assert_eq!(created["replaced"], false);

    let listed = call_ok(&mut client, 2, "job.list", serde_json::json!({}), false);
    assert_eq!(listed["jobs"][0]["name"], "tests");
    assert_eq!(listed["jobs"][0]["trigger"], "every 60s");

    // Due immediately, because a job that appears to do nothing for an hour
    // after you enable it is one people conclude is broken.
    let due = omt::scheduler::due_now(&state, 1_000);
    assert_eq!(due.len(), 1, "a new job was not due");
    assert!(
        omt::scheduler::due_now(&state, 1_001).is_empty(),
        "it fired twice"
    );

    omt::scheduler::finished(&state, "tests", false);
    let after = call_ok(&mut client, 3, "job.list", serde_json::json!({}), false);
    assert_eq!(
        after["jobs"][0]["consecutive_failures"], 1,
        "a failure was not visible to a client, so nobody would know it is broken"
    );

    // Same name replaces rather than adding a second copy that fires alongside
    // the first — which is what a retry after a dropped acknowledgement does.
    let again = call_ok(
        &mut client,
        4,
        "job.create",
        serde_json::json!({
            "name": "tests",
            "workspace": workspace,
            "run": "true",
            "every_seconds": 30,
        }),
        true,
    );
    assert_eq!(again["replaced"], true);
    let final_list = call_ok(&mut client, 5, "job.list", serde_json::json!({}), false);
    assert_eq!(final_list["jobs"].as_array().expect("jobs").len(), 1);

    drop(client);
    worker.join().expect("worker");
}

#[test]
fn a_job_pointing_at_a_directory_that_is_not_there_is_refused() {
    // Refused at creation, because the alternative is a job that fails every
    // minute forever and reports it only through a counter.
    let state = omt::state::State::default();
    let (mut client, server) = client_server_pair();
    let served = state.clone();
    let worker = std::thread::spawn(move || serve(server, served));

    send(
        &mut client,
        &ProtoMessage::Hello(Hello {
            proto: omt_proto::PROTO_VERSION,
            client: "test".to_owned(),
            token: None,
        }),
    );
    let ProtoMessage::Welcome(_) = recv(&mut client) else {
        panic!("expected a welcome");
    };

    send(
        &mut client,
        &ProtoMessage::Call(Call {
            request: request(1),
            capability: "job.create".to_owned(),
            input: serde_json::json!({
                "name": "bad",
                "workspace": "/definitely/not/here",
                "run": "true",
                "every_seconds": 60,
            }),
            intent: Some(omt_types::IntentId::new()),
        }),
    );
    let ProtoMessage::Result(result) = recv(&mut client) else {
        panic!("expected a result");
    };
    assert!(matches!(result.outcome, CallOutcome::Err { .. }));

    drop(client);
    worker.join().expect("worker");
}

#[test]
fn a_pane_is_a_view_of_a_session_and_closing_one_leaves_it_running() {
    // The property the whole pane model is built around, and the one a user
    // finds out about the hard way if it is wrong: closing a view of something
    // is not ending it.
    //
    // `date` rather than a shell: what is being tested is that the session
    // outlives its pane, and a program that exits proves that just as well as
    // one that does not — while leaving no process behind for the next run.
    let dir = tempfile::tempdir().expect("tempdir");
    let state = omt::state::State::default();
    let (mut client, server) = client_server_pair();
    let served = state.clone();
    let worker = std::thread::spawn(move || serve(server, served));

    send(
        &mut client,
        &ProtoMessage::Hello(Hello {
            proto: omt_proto::PROTO_VERSION,
            client: "test".to_owned(),
            token: None,
        }),
    );
    let ProtoMessage::Welcome(_) = recv(&mut client) else {
        panic!("expected a welcome");
    };

    let workspace = call_ok(
        &mut client,
        1,
        "workspace.open",
        serde_json::json!({ "root": dir.path().to_string_lossy() }),
        true,
    );
    let ws = workspace["id"].clone();
    let session = call_ok(
        &mut client,
        2,
        "session.create",
        serde_json::json!({ "workspace": ws, "program": "/bin/date", "cols": 40, "rows": 10 }),
        true,
    );
    let sid = session["session"].clone();

    let pane = call_ok(
        &mut client,
        3,
        "pane.open",
        serde_json::json!({ "workspace": ws, "session": sid }),
        true,
    );
    let pid = pane["pane"].as_str().expect("pane id").to_owned();

    let listed = call_ok(
        &mut client,
        4,
        "pane.list",
        serde_json::json!({ "workspace": ws }),
        false,
    );
    assert_eq!(listed["panes"].as_array().expect("panes").len(), 1);
    assert_eq!(
        listed["panes"][0]["focused"], true,
        "the first pane took focus"
    );

    call_ok(
        &mut client,
        5,
        "pane.close",
        serde_json::json!({ "workspace": ws, "pane": pid }),
        true,
    );

    // The session is still there. If closing a pane killed it, somebody
    // tidying their screen would lose a running agent.
    let sessions = call_ok(&mut client, 6, "session.list", serde_json::json!({}), false);
    assert_eq!(
        sessions["sessions"].as_array().expect("sessions").len(),
        1,
        "closing a pane killed the session it was showing"
    );

    drop(client);
    worker.join().expect("worker");
}

#[test]
fn focusing_a_pane_that_is_not_there_is_refused() {
    // Silently keeping focus where it was means the next keystroke goes
    // somewhere the user is not looking.
    let dir = tempfile::tempdir().expect("tempdir");
    let state = omt::state::State::default();
    let (mut client, server) = client_server_pair();
    let served = state.clone();
    let worker = std::thread::spawn(move || serve(server, served));

    send(
        &mut client,
        &ProtoMessage::Hello(Hello {
            proto: omt_proto::PROTO_VERSION,
            client: "test".to_owned(),
            token: None,
        }),
    );
    let ProtoMessage::Welcome(_) = recv(&mut client) else {
        panic!("expected a welcome");
    };
    let workspace = call_ok(
        &mut client,
        1,
        "workspace.open",
        serde_json::json!({ "root": dir.path().to_string_lossy() }),
        true,
    );

    send(
        &mut client,
        &ProtoMessage::Call(Call {
            request: request(2),
            capability: "pane.focus".to_owned(),
            input: serde_json::json!({
                "workspace": workspace["id"],
                "pane": omt_types::PaneId::new().to_wire(),
            }),
            intent: Some(omt_types::IntentId::new()),
        }),
    );
    let ProtoMessage::Result(result) = recv(&mut client) else {
        panic!("expected a result");
    };
    assert!(matches!(result.outcome, CallOutcome::Err { .. }));

    drop(client);
    worker.join().expect("worker");
}

#[test]
fn a_plugin_is_installed_with_an_explicit_grant_and_then_runs() {
    // "Nothing executes a plugin" was on the checklist. A plugin now runs as a
    // process holding a token, and reaches omt through the same capability
    // surface as any other client — there is no second API for plugins, which
    // is the whole reason the capability catalog exists.
    let state = omt::state::State::default();
    let (mut client, server) = client_server_pair();
    let served = state.clone();
    // Admin, because installing a plugin is an Admin capability: 13 §4 is
    // explicit that Operator cannot install plugins, and this test was
    // connecting as Operator only because the shared helper does.
    let worker = std::thread::spawn(move || serve_as(Role::Admin, server, served));

    send(
        &mut client,
        &ProtoMessage::Hello(Hello {
            proto: omt_proto::PROTO_VERSION,
            client: "test".to_owned(),
            token: None,
        }),
    );
    let ProtoMessage::Welcome(_) = recv(&mut client) else {
        panic!("expected a welcome");
    };

    let installed = call_ok(
        &mut client,
        1,
        "plugin.install",
        serde_json::json!({
            "id": "demo",
            "name": "Demo",
            "version": "1.0.0",
            "permissions": ["read_sessions", "write_input"],
            "grant": ["read_sessions"],
            "entry": ["/bin/echo", "started"],
        }),
        true,
    );
    // What it asked for and what it received are different things, and the
    // difference is reported rather than left to the user to work out.
    // In the spelling they were sent in. Written out with Rust's variant names,
    // a client that read this list back and re-sent it — which is what an
    // "edit these grants" screen does — was told its own values were not
    // permissions omt knows.
    assert_eq!(installed["granted"], serde_json::json!(["read_sessions"]));
    assert_eq!(installed["refused"], serde_json::json!(["write_input"]));

    let started = call_ok(
        &mut client,
        2,
        "plugin.start",
        serde_json::json!({ "id": "demo", "socket": "/tmp/omt-test.sock" }),
        true,
    );
    assert!(started["pid"].as_u64().is_some_and(|p| p > 0));
    // The role comes from the grant. A plugin refused WriteInput must not hold
    // a token that could type into somebody's shell.
    assert_eq!(started["role"], "viewer");

    drop(client);
    worker.join().expect("worker");
}

#[test]
fn a_permission_that_was_never_declared_cannot_be_granted() {
    // A plugin holding authority its manifest does not mention is a plugin
    // whose manifest no longer describes it — and that manifest is the document
    // somebody reads before trusting it.
    let state = omt::state::State::default();
    let (mut client, server) = client_server_pair();
    let served = state.clone();
    let worker = std::thread::spawn(move || serve(server, served));

    send(
        &mut client,
        &ProtoMessage::Hello(Hello {
            proto: omt_proto::PROTO_VERSION,
            client: "test".to_owned(),
            token: None,
        }),
    );
    let ProtoMessage::Welcome(_) = recv(&mut client) else {
        panic!("expected a welcome");
    };

    send(
        &mut client,
        &ProtoMessage::Call(Call {
            request: request(1),
            capability: "plugin.install".to_owned(),
            input: serde_json::json!({
                "id": "sneaky",
                "name": "Sneaky",
                "version": "1.0.0",
                "permissions": ["read_sessions"],
                "grant": ["read_sessions", "write_input"],
            }),
            intent: Some(omt_types::IntentId::new()),
        }),
    );
    let ProtoMessage::Result(result) = recv(&mut client) else {
        panic!("expected a result");
    };
    assert!(matches!(result.outcome, CallOutcome::Err { .. }));

    drop(client);
    worker.join().expect("worker");
}

#[test]
fn a_permission_omt_does_not_know_is_refused_rather_than_ignored() {
    // Dropping it silently installs a plugin with less authority than its
    // manifest claims, and the mismatch surfaces later as a capability that
    // mysteriously fails.
    let state = omt::state::State::default();
    let (mut client, server) = client_server_pair();
    let served = state.clone();
    let worker = std::thread::spawn(move || serve(server, served));

    send(
        &mut client,
        &ProtoMessage::Hello(Hello {
            proto: omt_proto::PROTO_VERSION,
            client: "test".to_owned(),
            token: None,
        }),
    );
    let ProtoMessage::Welcome(_) = recv(&mut client) else {
        panic!("expected a welcome");
    };

    send(
        &mut client,
        &ProtoMessage::Call(Call {
            request: request(1),
            capability: "plugin.install".to_owned(),
            input: serde_json::json!({
                "id": "odd",
                "name": "Odd",
                "version": "1.0.0",
                "permissions": ["fly_the_plane"],
                "grant": [],
            }),
            intent: Some(omt_types::IntentId::new()),
        }),
    );
    let ProtoMessage::Result(result) = recv(&mut client) else {
        panic!("expected a result");
    };
    assert!(matches!(result.outcome, CallOutcome::Err { .. }));

    drop(client);
    worker.join().expect("worker");
}

#[test]
fn a_session_survives_the_daemon_restarting_as_a_readable_orphan() {
    // What the architecture actually specifies, which is not "the pty
    // survives": a pty dies with the process that opened it. What survives is
    // everything else — the screen, the directory, the command — so the session
    // comes back readable, refuses writes, and offers a restart.
    //
    // The snapshot is written directly rather than produced by running a shell
    // and waiting for its output. Racing a real process to make a fixture is
    // how a test passes alone and hangs in a parallel run, which is what the
    // first version of this did.
    let dir = tempfile::tempdir().expect("tempdir");
    let work = dir.path().join("project");
    std::fs::create_dir_all(&work).expect("mkdir");
    let snapshot = dir.path().join("state.json");
    std::fs::write(
        &snapshot,
        serde_json::to_vec(&serde_json::json!({
            "version": 1,
            "workspaces": [{ "root": work.to_string_lossy() }],
            "sessions": [{
                "workspace_root": work.to_string_lossy(),
                "title": "shell",
                "program": "/bin/date",
                "cwd": work.to_string_lossy(),
                "screen": ["before-the-restart", "$ "],
            }],
        }))
        .expect("encode"),
    )
    .expect("write");

    let state = omt::state::State::default();
    let (mut client, server) = client_server_pair();
    let served = state.clone();
    let worker = std::thread::spawn(move || serve(server, served));
    send(
        &mut client,
        &ProtoMessage::Hello(Hello {
            proto: omt_proto::PROTO_VERSION,
            client: "test".to_owned(),
            token: None,
        }),
    );
    let ProtoMessage::Welcome(_) = recv(&mut client) else {
        panic!("expected a welcome");
    };

    let restored = call_ok(
        &mut client,
        1,
        "state.restore",
        serde_json::json!({ "path": snapshot.to_string_lossy() }),
        true,
    );
    assert_eq!(restored["sessions"], 1, "the session did not come back");

    let listed = call_ok(&mut client, 2, "session.list", serde_json::json!({}), false);
    let session = listed["sessions"][0]["id"]
        .as_str()
        .expect("session")
        .to_owned();
    // Named, not folded into "exited": a client has to tell "the program
    // finished" from "this can be brought back".
    assert_eq!(listed["sessions"][0]["state"], "orphaned");

    // Readable, which is the whole point of restoring one.
    let read = call_ok(
        &mut client,
        3,
        "session.read",
        serde_json::json!({ "session": session, "history": 0 }),
        false,
    );
    assert!(
        read["screen"].to_string().contains("before-the-restart"),
        "the restored session lost what was on it: {}",
        read["screen"]
    );

    // And refuses writes by name, because bytes accepted into a session with
    // nothing behind it look exactly like typing that worked.
    send(
        &mut client,
        &ProtoMessage::Call(Call {
            request: request(4),
            capability: "session.write".to_owned(),
            input: serde_json::json!({ "session": session, "text": "ls\n", "epoch": 1 }),
            intent: Some(omt_types::IntentId::new()),
        }),
    );
    let ProtoMessage::Result(result) = recv(&mut client) else {
        panic!("expected a result");
    };
    let CallOutcome::Err { error } = result.outcome else {
        panic!("a write to an orphan was accepted");
    };
    assert!(
        error.message.contains("restart"),
        "the refusal does not say what to do: {}",
        error.message
    );

    // Restarted: same id, old output kept above a separator.
    let restarted = call_ok(
        &mut client,
        5,
        "session.restart",
        serde_json::json!({ "session": session, "program": "/bin/sh" }),
        true,
    );
    assert_eq!(
        restarted["session"], session,
        "the id changed, so every pane and card referring to it is orphaned"
    );
    assert!(restarted["kept_lines"].as_u64().is_some_and(|n| n > 0));

    let after = call_ok(&mut client, 6, "session.list", serde_json::json!({}), false);
    assert_ne!(after["sessions"][0]["state"], "orphaned");

    // A second restart is refused rather than putting two processes on one pty.
    send(
        &mut client,
        &ProtoMessage::Call(Call {
            request: request(7),
            capability: "session.restart".to_owned(),
            input: serde_json::json!({ "session": session }),
            intent: Some(omt_types::IntentId::new()),
        }),
    );
    let ProtoMessage::Result(second) = recv(&mut client) else {
        panic!("expected a result");
    };
    assert!(matches!(second.outcome, CallOutcome::Err { .. }));

    drop(client);
    worker.join().expect("worker");
}

#[test]
fn a_configured_speech_provider_becomes_usable_and_its_key_never_leaves() {
    // BYOK, end to end: with no key there is no provider and dictation says so;
    // with one the provider appears — and what comes back over the wire is that
    // a key is present, never the key.
    //
    // The key is supplied through a lookup rather than the environment: setting
    // a process-wide variable is unsafe in this edition, and a test that has to
    // run alone is a test that will eventually run alongside something.
    let state = omt::state::State::with_providers(|name| {
        (name == "OMT_DEEPGRAM_KEY").then(|| "super-secret-value".to_owned())
    });

    let (mut client, server) = client_server_pair();
    let served = state.clone();
    let worker = std::thread::spawn(move || serve(server, served));
    send(
        &mut client,
        &ProtoMessage::Hello(Hello {
            proto: omt_proto::PROTO_VERSION,
            client: "test".to_owned(),
            token: None,
        }),
    );
    let ProtoMessage::Welcome(_) = recv(&mut client) else {
        panic!("expected a welcome");
    };

    let out = call_ok(
        &mut client,
        1,
        "voice.providers",
        serde_json::json!({}),
        false,
    );
    assert_eq!(
        out["usable"], true,
        "a configured key did not enable anything"
    );
    assert_eq!(out["providers"][0]["id"], "deepgram");
    assert_eq!(out["providers"][0]["key_present"], true);

    // The key itself must not be anywhere in the answer. A capability that
    // returned one would put it in the first log somebody captured.
    let text = out.to_string();
    assert!(
        !text.contains("super-secret-value"),
        "the key came back over the wire: {text}"
    );

    drop(client);
    worker.join().expect("worker");
}

#[test]
fn an_instance_with_no_key_offers_no_speech_provider() {
    // The shipped state, and it says so rather than pretending: no key, no
    // provider, and no audio leaving the machine.
    // No key at all, whatever this machine's environment happens to hold.
    let state = omt::state::State::with_providers(|_| None);
    let (mut client, server) = client_server_pair();
    let served = state.clone();
    let worker = std::thread::spawn(move || serve(server, served));
    send(
        &mut client,
        &ProtoMessage::Hello(Hello {
            proto: omt_proto::PROTO_VERSION,
            client: "test".to_owned(),
            token: None,
        }),
    );
    let ProtoMessage::Welcome(_) = recv(&mut client) else {
        panic!("expected a welcome");
    };
    let out = call_ok(
        &mut client,
        1,
        "voice.providers",
        serde_json::json!({}),
        false,
    );
    assert!(out["providers"].as_array().expect("providers").is_empty());
    assert_eq!(out["usable"], false);

    drop(client);
    worker.join().expect("worker");
}

#[test]
fn a_protocol_agent_is_driven_and_its_events_reach_the_session() {
    // The source, connected: a process speaking Codex's app-server protocol,
    // its notifications decoded by the adapter and folded into the session's
    // event stream. The agent here is a script rather than the vendor's binary
    // — what that leaves unverified is whether Codex spells its notifications
    // the way this adapter expects, which needs Codex and not more code.
    let dir = tempfile::tempdir().expect("tempdir");
    let state = omt::state::State::default();
    let (mut client, server) = client_server_pair();
    let served = state.clone();
    let worker = std::thread::spawn(move || serve(server, served));

    send(
        &mut client,
        &ProtoMessage::Hello(Hello {
            proto: omt_proto::PROTO_VERSION,
            client: "test".to_owned(),
            token: None,
        }),
    );
    let ProtoMessage::Welcome(_) = recv(&mut client) else {
        panic!("expected a welcome");
    };

    let ws = call_ok(
        &mut client,
        1,
        "workspace.open",
        serde_json::json!({ "root": dir.path().to_string_lossy() }),
        true,
    )["id"]
        .clone();
    let session = call_ok(
        &mut client,
        2,
        "session.create",
        serde_json::json!({ "workspace": ws, "program": "/bin/date", "cols": 40, "rows": 10 }),
        true,
    )["session"]
        .as_str()
        .expect("session")
        .to_owned();

    // An agent that speaks the protocol: a turn, a command it wants to run,
    // the result, and one notification omt has never heard of.
    let script = r#"printf '%s\n' \
'{"jsonrpc":"2.0","method":"turn/started","params":{"turn":{"id":"t1"}}}' \
'{"jsonrpc":"2.0","method":"item/started","params":{"item":{"type":"commandExecution","id":"c1","command":"cargo test"}}}' \
'{"jsonrpc":"2.0","method":"item/completed","params":{"item":{"type":"commandExecution","id":"c1","aggregatedOutput":"ok"}}}' \
'{"jsonrpc":"2.0","id":7,"result":{"this":"is a reply, not an event"}}' \
'{"jsonrpc":"2.0","method":"something/omt/has-never-heard-of","params":{}}' \
'{"jsonrpc":"2.0","method":"turn/completed","params":{"turn":{"status":"completed"}}}'"#;

    let out = call_ok(
        &mut client,
        3,
        "agent.connect",
        serde_json::json!({
            "session": session,
            "agent": "codex",
            "command": ["/bin/sh", "-c", script],
        }),
        true,
    );

    // Four notifications carried payloads; the reply carried none.
    assert!(
        out["events"].as_u64().is_some_and(|n| n >= 4),
        "too few events decoded: {out}"
    );
    // And the one nobody has a mapping for is named rather than dropped.
    assert_eq!(
        out["unmapped"],
        serde_json::json!(["something/omt/has-never-heard-of"]),
        "an unmapped method was swallowed: {out}"
    );

    drop(client);
    worker.join().expect("worker");
}

#[test]
fn an_agent_omt_cannot_drive_over_a_protocol_is_refused_by_name() {
    // Connecting the wrong adapter would decode one agent's notifications with
    // another's vocabulary — a transcript that is subtly wrong rather than
    // obviously broken, which is far harder to notice.
    let state = omt::state::State::default();
    let (mut client, server) = client_server_pair();
    let served = state.clone();
    let worker = std::thread::spawn(move || serve(server, served));
    send(
        &mut client,
        &ProtoMessage::Hello(Hello {
            proto: omt_proto::PROTO_VERSION,
            client: "test".to_owned(),
            token: None,
        }),
    );
    let ProtoMessage::Welcome(_) = recv(&mut client) else {
        panic!("expected a welcome");
    };

    send(
        &mut client,
        &ProtoMessage::Call(Call {
            request: request(1),
            capability: "agent.connect".to_owned(),
            input: serde_json::json!({
                "session": omt_types::SessionId::new().to_wire(),
                "agent": "aider",
                "command": ["/bin/true"],
            }),
            intent: Some(omt_types::IntentId::new()),
        }),
    );
    let ProtoMessage::Result(result) = recv(&mut client) else {
        panic!("expected a result");
    };
    assert!(matches!(result.outcome, CallOutcome::Err { .. }));

    drop(client);
    worker.join().expect("worker");
}

#[test]
fn a_layout_opens_every_pane_and_names_the_one_that_failed() {
    // A layout that half-opened and reported "error" leaves somebody to work
    // out which of six panes is missing. Every pane comes back, in order.
    //
    // The panes run `date` rather than a shell. What is being tested is which
    // panes opened and which failed; a shell that never exits leaves a live
    // process behind every run, and a test suite that accumulates those is one
    // that hangs when enough of them are running at once.
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(dir.path().join("sub")).expect("mkdir");
    let state = omt::state::State::default();
    let (mut client, server) = client_server_pair();
    let served = state.clone();
    let worker = std::thread::spawn(move || serve(server, served));

    send(
        &mut client,
        &ProtoMessage::Hello(Hello {
            proto: omt_proto::PROTO_VERSION,
            client: "test".to_owned(),
            token: None,
        }),
    );
    let ProtoMessage::Welcome(_) = recv(&mut client) else {
        panic!("expected a welcome");
    };
    let ws = call_ok(
        &mut client,
        1,
        "workspace.open",
        serde_json::json!({ "root": dir.path().to_string_lossy() }),
        true,
    )["id"]
        .clone();

    let out = call_ok(
        &mut client,
        2,
        "layout.open",
        serde_json::json!({
            "workspace": ws,
            "panes": [
                { "program": "/bin/date", "title": "clock" },
                { "program": "/bin/date", "cwd": "sub" },
                { "program": "/bin/date", "cwd": "not-there" }
            ]
        }),
        true,
    );

    let panes = out["panes"].as_array().expect("panes");
    assert_eq!(panes.len(), 3, "not every pane was reported: {out}");
    assert_eq!(out["opened"], 2);
    assert!(panes[0]["session"].is_string());
    assert!(panes[1]["session"].is_string());
    // The failure names its index, so a client can point at the right row.
    assert_eq!(panes[2]["index"], 2);
    assert!(
        panes[2]["error"].is_string(),
        "the missing directory was not reported: {out}"
    );

    // And the ones that opened are really panes, not just sessions.
    let listed = call_ok(
        &mut client,
        3,
        "pane.list",
        serde_json::json!({ "workspace": ws }),
        false,
    );
    assert_eq!(listed["panes"].as_array().expect("panes").len(), 2);

    drop(client);
    worker.join().expect("worker");
}

#[test]
fn a_layout_pane_cannot_point_outside_its_workspace() {
    // A layout file is something people share, and a shared file that opens a
    // shell in somebody's home directory is a different kind of thing.
    let dir = tempfile::tempdir().expect("tempdir");
    let state = omt::state::State::default();
    let (mut client, server) = client_server_pair();
    let served = state.clone();
    let worker = std::thread::spawn(move || serve(server, served));

    send(
        &mut client,
        &ProtoMessage::Hello(Hello {
            proto: omt_proto::PROTO_VERSION,
            client: "test".to_owned(),
            token: None,
        }),
    );
    let ProtoMessage::Welcome(_) = recv(&mut client) else {
        panic!("expected a welcome");
    };
    let ws = call_ok(
        &mut client,
        1,
        "workspace.open",
        serde_json::json!({ "root": dir.path().to_string_lossy() }),
        true,
    )["id"]
        .clone();

    let out = call_ok(
        &mut client,
        2,
        "layout.open",
        serde_json::json!({
            "workspace": ws,
            "panes": [{ "program": "/bin/date", "cwd": "../.." }]
        }),
        true,
    );
    assert_eq!(out["opened"], 0, "a pane escaped its workspace: {out}");
    assert!(out["panes"][0]["error"].is_string());

    drop(client);
    worker.join().expect("worker");
}

#[test]
fn a_session_with_no_shell_integration_still_reports_blocks() {
    // What `ssh` to a host with no marks looks like. It used to report nothing,
    // so the session appeared empty — and `shell_integration` says plainly that
    // these blocks cannot be re-run.
    let dir = tempfile::tempdir().expect("tempdir");
    let state = omt::state::State::default();
    let (mut client, server) = client_server_pair();
    let served = state.clone();
    let worker = std::thread::spawn(move || serve(server, served));

    send(
        &mut client,
        &ProtoMessage::Hello(Hello {
            proto: omt_proto::PROTO_VERSION,
            client: "test".to_owned(),
            token: None,
        }),
    );
    let ProtoMessage::Welcome(_) = recv(&mut client) else {
        panic!("expected a welcome");
    };
    let ws = call_ok(
        &mut client,
        1,
        "workspace.open",
        serde_json::json!({ "root": dir.path().to_string_lossy() }),
        true,
    )["id"]
        .clone();
    let session = call_ok(
        &mut client,
        2,
        "session.create",
        // `date` rather than `echo`: bare `echo` prints only a newline, and a
        // newline is not output — a status line that repaints and ends with one
        // must not open a block.
        serde_json::json!({ "workspace": ws, "program": "/bin/date", "cols": 40, "rows": 10 }),
        true,
    )["session"]
        .as_str()
        .expect("session")
        .to_owned();

    // Pumped until something has been drawn — `/bin/echo` writes one line and
    // exits, and it emits no OSC 133 at all.
    let mut out = serde_json::Value::Null;
    for _ in 0..100 {
        {
            let mut instance = state.lock().expect("lock");
            let ids: Vec<_> = instance.sessions().iter().map(|s| s.id).collect();
            for each in ids {
                let _ = instance.pump_session(each);
            }
        }
        out = call_ok(
            &mut client,
            3,
            "session.blocks",
            serde_json::json!({ "session": session }),
            false,
        );
        if !out["blocks"].as_array().is_none_or(std::vec::Vec::is_empty) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    let blocks = out["blocks"].as_array().expect("blocks");
    assert!(
        !blocks.is_empty(),
        "a session with output reported no blocks"
    );
    assert_eq!(
        blocks[0]["attributed"], false,
        "a command was invented for a shell that marked none"
    );
    assert_eq!(
        out["shell_integration"], false,
        "a shell with no marks was reported as having integration"
    );

    drop(client);
    worker.join().expect("worker");
}

#[test]
fn a_shell_with_no_marks_is_offered_the_line_that_adds_them() {
    // P11: a feature works before it is configured, and is also discoverable.
    // Blocks work without shell integration; the *good* version needs a line in
    // a shell rc, and a line nobody is ever shown is a feature shipped off.
    let state = omt::state::State::default();
    let (mut client, server) = client_server_pair();
    let served = state.clone();
    let worker = std::thread::spawn(move || serve(server, served));

    send(
        &mut client,
        &ProtoMessage::Hello(Hello {
            proto: omt_proto::PROTO_VERSION,
            client: "test".to_owned(),
            token: None,
        }),
    );
    let ProtoMessage::Welcome(_) = recv(&mut client) else {
        panic!("expected a welcome");
    };

    let out = call_ok(
        &mut client,
        1,
        "shell.integration",
        serde_json::json!({}),
        false,
    );
    assert_eq!(
        out["active"], false,
        "nothing has run, so nothing can have marked anything"
    );
    // A snippet, or an honest absence for a shell omt does not know. Returning
    // one shell's line for another puts an error in a terminal on every login.
    if let Some(snippet) = out["snippet"].as_object() {
        assert!(
            snippet.contains_key("file"),
            "a snippet with no file: {out}"
        );
        assert!(
            snippet["line"].as_str().is_some_and(|l| l.contains("133")),
            "the snippet does not emit the marks omt reads: {out}"
        );
    }

    drop(client);
    worker.join().expect("worker");
}

#[test]
fn every_setting_is_listed_whether_or_not_anyone_set_it() {
    // The other half of P11. `config.get` reports what is set; a settings
    // screen built from that alone shows only what somebody already changed,
    // which makes every other key discoverable only by being told its name.
    let state = omt::state::State::default();
    let (mut client, server) = client_server_pair();
    let served = state.clone();
    let worker = std::thread::spawn(move || serve(server, served));

    send(
        &mut client,
        &ProtoMessage::Hello(Hello {
            proto: omt_proto::PROTO_VERSION,
            client: "test".to_owned(),
            token: None,
        }),
    );
    let ProtoMessage::Welcome(_) = recv(&mut client) else {
        panic!("expected a welcome");
    };

    let out = call_ok(
        &mut client,
        1,
        "config.schema",
        serde_json::json!({}),
        false,
    );
    let settings = out["settings"].as_array().expect("settings");
    assert!(!settings.is_empty(), "no settings were reported");

    for setting in settings {
        assert!(
            setting["default"] != serde_json::Value::Null,
            "{} has no default, so it must be set before it does anything",
            setting["key"]
        );
        assert!(
            setting["doc"].as_str().is_some_and(|d| d.len() > 10),
            "{} has no description, so a settings screen lists a name to guess at",
            setting["key"]
        );
    }

    // An enumerated setting carries its choices, or a client cannot offer them.
    let enums: Vec<_> = settings.iter().filter(|s| s["kind"] == "enum").collect();
    assert!(!enums.is_empty(), "no enumerated setting to check");
    for setting in enums {
        assert!(
            !setting["choices"]
                .as_array()
                .is_none_or(std::vec::Vec::is_empty),
            "{} is an enum with no choices",
            setting["key"]
        );
    }

    drop(client);
    worker.join().expect("worker");
}

#[test]
fn usage_accumulates_and_never_invents_a_cost() {
    // The events already arrived and were dropped a line later — every token
    // counted into a value nothing read. The ledger now belongs to the
    // instance, and `usage.report` is how a surface sees it.
    let state = omt::state::State::default();
    let (mut client, server) = client_server_pair();
    let served = state.clone();
    let worker = std::thread::spawn(move || serve(server, served));

    send(
        &mut client,
        &ProtoMessage::Hello(Hello {
            proto: omt_proto::PROTO_VERSION,
            client: "test".to_owned(),
            token: None,
        }),
    );
    let ProtoMessage::Welcome(_) = recv(&mut client) else {
        panic!("expected a welcome");
    };

    // Agents report *cumulatively*, so the ledger takes the larger of the two
    // rather than adding. Adding a cumulative report to a running total
    // double-counts everything before it and the number grows quadratically
    // over a session — which is why these go up rather than repeating.
    let session = omt_types::SessionId::new();
    {
        let mut instance = state.lock().expect("lock");
        for n in 1..=3u64 {
            instance.observe_usage(
                session,
                &omt_events::AgentPayload::Usage {
                    input: 100 * n,
                    output: 40 * n,
                    cache_read: 10,
                    cache_write: 5,
                    cost_usd: None,
                },
            );
        }
    }

    let out = call_ok(
        &mut client,
        1,
        "usage.report",
        serde_json::json!({ "session": session.to_wire() }),
        false,
    );
    assert_eq!(
        out["session"]["input"], 300,
        "the latest cumulative report did not win"
    );
    assert_eq!(out["session"]["output"], 120);
    assert_eq!(out["total"]["input"], 300);
    assert!(
        out["session"]["cost_usd"].is_null(),
        "a cost was invented from token counts: {out}"
    );

    drop(client);
    worker.join().expect("worker");
}

#[test]
fn a_repeated_cumulative_report_does_not_double_the_total() {
    // The failure the ledger's max-rather-than-add rule prevents: an agent
    // that re-sends the same cumulative figure would otherwise make the total
    // grow every time it spoke.
    let state = omt::state::State::default();
    let session = omt_types::SessionId::new();
    {
        let mut instance = state.lock().expect("lock");
        for _ in 0..5 {
            instance.observe_usage(
                session,
                &omt_events::AgentPayload::Usage {
                    input: 500,
                    output: 200,
                    cache_read: 0,
                    cache_write: 0,
                    cost_usd: None,
                },
            );
        }
        assert_eq!(
            instance.usage.session(session).input,
            500,
            "the same cumulative report was counted more than once"
        );
    }
}

#[test]
fn a_session_that_reported_nothing_reports_zero_rather_than_failing() {
    let state = omt::state::State::default();
    let (mut client, server) = client_server_pair();
    let served = state.clone();
    let worker = std::thread::spawn(move || serve(server, served));

    send(
        &mut client,
        &ProtoMessage::Hello(Hello {
            proto: omt_proto::PROTO_VERSION,
            client: "test".to_owned(),
            token: None,
        }),
    );
    let ProtoMessage::Welcome(_) = recv(&mut client) else {
        panic!("expected a welcome");
    };

    let out = call_ok(
        &mut client,
        1,
        "usage.report",
        serde_json::json!({ "session": omt_types::SessionId::new().to_wire() }),
        false,
    );
    assert_eq!(out["session"]["input"], 0);
    // And silence about limits stays silence. A surface that drew this as an
    // empty bar would invent the one figure a user acts on.
    assert_eq!(out["headroom"], "unknown");
    assert!(out["limit_status"].is_null());

    drop(client);
    worker.join().expect("worker");
}

#[test]
fn a_reported_rate_limit_reaches_a_client_in_the_agents_own_words() {
    let state = omt::state::State::default();
    let (mut client, server) = client_server_pair();
    let served = state.clone();
    let worker = std::thread::spawn(move || serve(server, served));

    send(
        &mut client,
        &ProtoMessage::Hello(Hello {
            proto: omt_proto::PROTO_VERSION,
            client: "test".to_owned(),
            token: None,
        }),
    );
    let ProtoMessage::Welcome(_) = recv(&mut client) else {
        panic!("expected a welcome");
    };

    let session = omt_types::SessionId::new();
    {
        let mut instance = state.lock().expect("lock");
        instance.observe_usage(
            session,
            &omt_events::AgentPayload::RateLimit {
                status: "80% of weekly used".to_owned(),
                resets_at: None,
            },
        );
    }

    let out = call_ok(
        &mut client,
        1,
        "usage.report",
        serde_json::json!({ "session": session.to_wire() }),
        false,
    );
    assert_eq!(out["headroom"], "reported");
    assert_eq!(
        out["limit_status"], "80% of weekly used",
        "the agent's own words did not survive: {out}"
    );

    drop(client);
    worker.join().expect("worker");
}

#[test]
fn a_worktree_is_created_and_is_immediately_a_workspace() {
    // The payoff of deriving `WorkspaceId` from a canonical path: a worktree is
    // a workspace the moment it exists, and nothing in the session tree, the
    // pane model or the capability surface needs a case for it.
    let dir = git_repo();
    let state = omt::state::State::default();
    let (mut client, server) = client_server_pair();
    let served = state.clone();
    let worker = std::thread::spawn(move || serve(server, served));

    send(
        &mut client,
        &ProtoMessage::Hello(Hello {
            proto: omt_proto::PROTO_VERSION,
            client: "test".to_owned(),
            token: None,
        }),
    );
    let ProtoMessage::Welcome(_) = recv(&mut client) else {
        panic!("expected a welcome");
    };
    let ws = call_ok(
        &mut client,
        1,
        "workspace.open",
        serde_json::json!({ "root": dir.path().to_string_lossy() }),
        true,
    )["id"]
        .clone();

    let added = call_ok(
        &mut client,
        2,
        "worktree.add",
        serde_json::json!({ "workspace": ws, "path": "wt-feature", "branch": "feature" }),
        true,
    );
    assert_eq!(added["worktree"]["branch"], "feature");
    let wt_path = added["worktree"]["path"].as_str().expect("path").to_owned();
    let wt_workspace = added["worktree"]["workspace"].clone();
    assert!(
        std::path::Path::new(&wt_path).join("a.txt").is_file(),
        "the worktree has no files in it"
    );

    // The id it reported is the id opening that path actually produces.
    let opened = call_ok(
        &mut client,
        3,
        "workspace.open",
        serde_json::json!({ "root": wt_path }),
        true,
    );
    assert_eq!(
        opened["id"], wt_workspace,
        "the reported workspace id is not the one the path opens as"
    );

    let listed = call_ok(
        &mut client,
        4,
        "worktree.list",
        serde_json::json!({ "workspace": ws }),
        false,
    );
    let worktrees = listed["worktrees"].as_array().expect("worktrees");
    assert_eq!(worktrees.len(), 2);
    assert_eq!(
        worktrees[0]["is_main"], true,
        "the main checkout is not first"
    );

    drop(client);
    worker.join().expect("worker");
}

#[test]
fn removing_a_worktree_with_changes_is_refused_rather_than_losing_them() {
    // An agent's output is usually why the worktree was wanted. Deleting it on
    // a tap is unrecoverable, so it refuses and says why.
    let dir = git_repo();
    let state = omt::state::State::default();
    let (mut client, server) = client_server_pair();
    let served = state.clone();
    let worker = std::thread::spawn(move || serve(server, served));

    send(
        &mut client,
        &ProtoMessage::Hello(Hello {
            proto: omt_proto::PROTO_VERSION,
            client: "test".to_owned(),
            token: None,
        }),
    );
    let ProtoMessage::Welcome(_) = recv(&mut client) else {
        panic!("expected a welcome");
    };
    let ws = call_ok(
        &mut client,
        1,
        "workspace.open",
        serde_json::json!({ "root": dir.path().to_string_lossy() }),
        true,
    )["id"]
        .clone();
    let path = call_ok(
        &mut client,
        2,
        "worktree.add",
        serde_json::json!({ "workspace": ws, "path": "wt-dirty", "branch": "dirty" }),
        true,
    )["worktree"]["path"]
        .as_str()
        .expect("path")
        .to_owned();

    std::fs::write(std::path::Path::new(&path).join("a.txt"), "changed\n").expect("write");

    send(
        &mut client,
        &ProtoMessage::Call(Call {
            request: request(3),
            capability: "worktree.remove".to_owned(),
            input: serde_json::json!({ "workspace": ws, "path": path, "force": false }),
            intent: Some(omt_types::IntentId::new()),
        }),
    );
    let ProtoMessage::Result(result) = recv(&mut client) else {
        panic!("expected a result");
    };
    let CallOutcome::Err { error } = result.outcome else {
        panic!("uncommitted changes were discarded");
    };
    assert!(
        error.message.contains("uncommitted"),
        "the refusal does not say what would be lost: {}",
        error.message
    );
    assert!(
        std::path::Path::new(&path).is_dir(),
        "it was removed anyway"
    );

    // With force it goes.
    let removed = call_ok(
        &mut client,
        4,
        "worktree.remove",
        serde_json::json!({ "workspace": ws, "path": path, "force": true }),
        true,
    );
    assert_eq!(removed["remaining"], 1);

    drop(client);
    worker.join().expect("worker");
}

#[test]
fn the_lines_of_a_changed_file_are_available_not_only_its_name() {
    // `git.diff` reports which files changed. Reviewing what an agent did needs
    // what changed in them, and that used to happen entirely outside omt.
    let dir = git_repo();
    std::fs::write(dir.path().join("a.txt"), "one\nCHANGED\nthree\n").expect("write");

    let state = omt::state::State::default();
    let (mut client, server) = client_server_pair();
    let served = state.clone();
    let worker = std::thread::spawn(move || serve(server, served));

    send(
        &mut client,
        &ProtoMessage::Hello(Hello {
            proto: omt_proto::PROTO_VERSION,
            client: "test".to_owned(),
            token: None,
        }),
    );
    let ProtoMessage::Welcome(_) = recv(&mut client) else {
        panic!("expected a welcome");
    };
    let ws = call_ok(
        &mut client,
        1,
        "workspace.open",
        serde_json::json!({ "root": dir.path().to_string_lossy() }),
        true,
    )["id"]
        .clone();

    let out = call_ok(
        &mut client,
        2,
        "git.hunks",
        serde_json::json!({ "workspace": ws, "path": "a.txt" }),
        false,
    );
    let hunks = out["hunks"].as_array().expect("hunks");
    assert!(!hunks.is_empty(), "a changed file reported no hunks: {out}");

    let lines: Vec<&str> = hunks[0]["lines"]
        .as_array()
        .expect("lines")
        .iter()
        .filter_map(|l| l.as_str())
        .collect();
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with('+') && l.contains("CHANGED")),
        "the added line is missing: {lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with('-') && l.contains("two")),
        "the removed line is missing: {lines:?}"
    );

    // Staged is a separate answer, and nothing has been staged.
    let staged = call_ok(
        &mut client,
        3,
        "git.hunks",
        serde_json::json!({ "workspace": ws, "path": "a.txt", "staged": true }),
        false,
    );
    assert!(
        staged["hunks"].as_array().expect("hunks").is_empty(),
        "unstaged changes leaked into the staged answer: {staged}"
    );

    drop(client);
    worker.join().expect("worker");
}

#[test]
fn a_fanout_gives_every_agent_its_own_worktree() {
    // What people already do with three terminal windows and a lot of
    // stashing. A worktree per agent is the correct shape, and omt was
    // unusually close to it: a workspace id derived from a canonical path
    // means N worktrees are already N workspaces.
    let dir = git_repo();
    let state = omt::state::State::default();
    let (mut client, server) = client_server_pair();
    let served = state.clone();
    let worker = std::thread::spawn(move || serve(server, served));

    send(
        &mut client,
        &ProtoMessage::Hello(Hello {
            proto: omt_proto::PROTO_VERSION,
            client: "test".to_owned(),
            token: None,
        }),
    );
    let ProtoMessage::Welcome(_) = recv(&mut client) else {
        panic!("expected a welcome");
    };
    let ws = call_ok(
        &mut client,
        1,
        "workspace.open",
        serde_json::json!({ "root": dir.path().to_string_lossy() }),
        true,
    )["id"]
        .clone();

    let out = call_ok(
        &mut client,
        2,
        "fanout.start",
        serde_json::json!({
            "workspace": ws,
            "name": "try",
            "prompt": "fix the build",
            "agents": ["claude", "codex", "not-a-real-agent"]
        }),
        true,
    );

    assert_eq!(out["prepared"], 2, "two agents should have been prepared");
    let arms = out["arms"].as_array().expect("arms");
    // Every arm, including the one that could not be prepared: a fan-out that
    // half-started and said "error" leaves somebody guessing which is missing.
    assert_eq!(arms.len(), 3, "not every arm was reported: {out}");
    let failed: Vec<_> = arms.iter().filter(|a| a["state"] == "failed").collect();
    assert_eq!(failed.len(), 1);
    assert!(
        failed[0]["error"]
            .as_str()
            .is_some_and(|e| e.contains("not an agent"))
    );

    // Two real worktrees exist, each its own workspace.
    let listed = call_ok(
        &mut client,
        3,
        "worktree.list",
        serde_json::json!({ "workspace": ws }),
        false,
    );
    assert_eq!(
        listed["worktrees"].as_array().expect("worktrees").len(),
        3,
        "the main checkout plus one worktree per prepared arm"
    );

    // And every prepared arm reports the workspace its worktree opens as.
    for arm in arms.iter().filter(|a| a["state"] != "failed") {
        assert!(
            arm["workspace"]
                .as_str()
                .is_some_and(|w| w.starts_with("wksp_")),
            "an arm has no workspace id: {arm}"
        );
        assert!(
            arm["branch"]
                .as_str()
                .is_some_and(|b| b.starts_with("omt/try/"))
        );
    }

    drop(client);
    worker.join().expect("worker");
}

#[test]
fn choosing_an_arm_records_it_and_merges_nothing() {
    // A tool that merges when you tap "choose" is one you stop trusting the
    // first time it picks wrong. And the losing worktree is kept, because
    // comparing the two is what you would do to check the choice was right.
    let dir = git_repo();
    let state = omt::state::State::default();
    let (mut client, server) = client_server_pair();
    let served = state.clone();
    let worker = std::thread::spawn(move || serve(server, served));

    send(
        &mut client,
        &ProtoMessage::Hello(Hello {
            proto: omt_proto::PROTO_VERSION,
            client: "test".to_owned(),
            token: None,
        }),
    );
    let ProtoMessage::Welcome(_) = recv(&mut client) else {
        panic!("expected a welcome");
    };
    let ws = call_ok(
        &mut client,
        1,
        "workspace.open",
        serde_json::json!({ "root": dir.path().to_string_lossy() }),
        true,
    )["id"]
        .clone();
    call_ok(
        &mut client,
        2,
        "fanout.start",
        serde_json::json!({
            "workspace": ws,
            "name": "pick",
            "prompt": "go",
            "agents": ["claude", "codex"]
        }),
        true,
    );

    // Choosing before anything has finished is refused, and rightly: picking a
    // winner among attempts that have not run is picking at random.
    send(
        &mut client,
        &ProtoMessage::Call(Call {
            request: request(3),
            capability: "fanout.choose".to_owned(),
            input: serde_json::json!({ "name": "pick", "agent": "codex" }),
            intent: Some(omt_types::IntentId::new()),
        }),
    );
    let ProtoMessage::Result(early) = recv(&mut client) else {
        panic!("expected a result");
    };
    assert!(
        matches!(early.outcome, CallOutcome::Err { .. }),
        "an arm was chosen before any had finished"
    );

    // Both finish, as they would once the agents were done.
    {
        let mut fanouts = state.fanouts().expect("fanouts");
        let fanout = fanouts.get_mut("pick").expect("the fan-out");
        for agent in [
            omt_types::AgentKind::ClaudeCode,
            omt_types::AgentKind::Codex,
        ] {
            fanout
                .set_state(agent, omt_session::ArmState::Finished { files_changed: 2 })
                .expect("finish");
        }
    }

    let chosen = call_ok(
        &mut client,
        4,
        "fanout.choose",
        serde_json::json!({ "name": "pick", "agent": "codex" }),
        true,
    );
    assert_eq!(chosen["branch"], "omt/pick/codex");
    assert_eq!(chosen["merged"], false, "something was merged");

    // The main branch is untouched — nothing was merged into it.
    let status = call_ok(
        &mut client,
        5,
        "git.status",
        serde_json::json!({ "workspace": ws }),
        false,
    );
    assert_eq!(status["branch"], "main", "the checkout was moved");

    // Both worktrees are still there. Deleting the loser at the moment of
    // choosing destroys what you would look at to check.
    let listed = call_ok(
        &mut client,
        6,
        "worktree.list",
        serde_json::json!({ "workspace": ws }),
        false,
    );
    assert_eq!(listed["worktrees"].as_array().expect("worktrees").len(), 3);

    let reported = call_ok(
        &mut client,
        7,
        "fanout.status",
        serde_json::json!({ "name": "pick" }),
        false,
    );
    assert_eq!(reported["fanouts"][0]["chosen"], "codex");

    drop(client);
    worker.join().expect("worker");
}
