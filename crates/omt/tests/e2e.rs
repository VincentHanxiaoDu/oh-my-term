//! End to end, against real everything.
//!
//! A real shell on a real pty, a real socket, a real git repository, a real
//! file transfer. Nothing here is mocked, because every bug this project has
//! actually had lived in the space between two components that each worked.
//!
//! Written to run under Linux in a container (`tests/e2e/run.sh`) as well as on
//! a developer's machine, so a difference between the two shows up here rather
//! than in CI.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use omt_daemon::{Instance, SessionRuntime};
use omt_pty::{PtyConfig, PtySize};
use omt_session::{SessionKind, SessionMode};

/// Start a session running a script, attached to an instance.
fn session(instance: &mut Instance, script: &str, cwd: Option<&Path>) -> omt_types::SessionId {
    let ws = instance
        .open_workspace(cwd.map_or("/tmp", |p| p.to_str().expect("utf8")))
        .expect("workspace");
    let id = instance
        .create_session(ws, SessionKind::Shell, SessionMode::Pty)
        .expect("session");
    let runtime = SessionRuntime::spawn(
        id,
        &PtyConfig {
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".into(), script.into()],
            cwd: cwd.map(Path::to_path_buf),
            size: PtySize::new(80, 24),
            ..PtyConfig::default()
        },
        omt_term::ScrollbackLimits::default(),
    )
    .expect("spawn");
    instance.attach(runtime).expect("attach");
    id
}

/// Pump until the screen says something, or give up.
fn wait_for(instance: &mut Instance, id: omt_types::SessionId, needle: &str) -> bool {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        instance.pump_session(id).expect("pump");
        if instance.runtime(id).is_some_and(|r| {
            r.terminal()
                .screen_text()
                .iter()
                .any(|l| l.contains(needle))
                || r.terminal()
                    .scrollback()
                    .lines()
                    .any(|l| l.text().contains(needle))
        }) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

fn git_repo() -> tempfile::TempDir {
    let d = tempfile::tempdir().expect("tempdir");
    for args in [
        vec!["init", "-q", "-b", "main"],
        vec!["config", "user.email", "e2e@example.com"],
        vec!["config", "user.name", "e2e"],
    ] {
        Command::new("git")
            .args(&args)
            .current_dir(d.path())
            .output()
            .expect("git");
    }
    d
}

#[test]
fn a_shell_runs_and_omt_sees_what_it_printed() {
    let mut instance = Instance::new();
    let id = session(&mut instance, "echo e2e-marker; sleep 2", None);
    assert!(
        wait_for(&mut instance, id, "e2e-marker"),
        "the shell's output never reached the grid"
    );
}

#[test]
fn a_long_running_command_scrolls_into_history_and_stays_findable() {
    // The path a user takes constantly: run something noisy, then scroll back.
    let mut instance = Instance::new();
    let id = session(
        &mut instance,
        "for i in $(seq 1 200); do echo line$i; done; echo DONE",
        None,
    );
    assert!(wait_for(&mut instance, id, "DONE"), "never finished");

    let found = instance
        .runtime(id)
        .expect("runtime")
        .terminal()
        .scrollback()
        .lines()
        .any(|l| l.text().contains("line1"));
    assert!(found, "output that scrolled off was lost rather than filed");
}

#[test]
fn typed_input_reaches_the_shell_and_its_answer_comes_back() {
    let mut instance = Instance::new();
    let id = session(&mut instance, "read line; echo got:$line", None);

    let mut writer = omt_session::WriterState::default();
    let epoch = writer
        .acquire(omt_types::Actor::Local, 0, false, false)
        .expect("acquire");
    instance
        .runtime_mut(id)
        .expect("runtime")
        .write_input(&mut writer, epoch, 1, b"e2e-typed\n")
        .expect("write");

    assert!(
        wait_for(&mut instance, id, "got:e2e-typed"),
        "the keystroke never reached the shell"
    );
}

#[test]
fn a_resize_reaches_the_program_and_the_grid_together() {
    // A program told one size while the kernel believes another draws a frame
    // that does not fit, and nobody can see why.
    let mut instance = Instance::new();
    let id = session(
        &mut instance,
        "trap 'stty size' WINCH; echo ready; for i in 1 2 3 4 5 6 7 8 9 10; do sleep 0.3; done",
        None,
    );
    assert!(wait_for(&mut instance, id, "ready"), "never started");

    instance
        .runtime_mut(id)
        .expect("runtime")
        .resize(100, 40)
        .expect("resize");
    assert!(
        wait_for(&mut instance, id, "40 100"),
        "the program was never told about the resize"
    );
    assert_eq!(
        instance.runtime(id).expect("runtime").pty().size(),
        PtySize::new(100, 40)
    );
}

#[test]
fn git_status_and_diff_agree_with_the_repository() {
    let d = git_repo();
    std::fs::write(d.path().join("a.txt"), "one\ntwo\n").expect("write");
    for args in [vec!["add", "-A"], vec!["commit", "-qm", "first"]] {
        Command::new("git")
            .args(&args)
            .current_dir(d.path())
            .output()
            .expect("git");
    }

    let clean = omt_workspace_fs::status(d.path()).expect("status");
    assert!(!clean.is_dirty());
    assert_eq!(clean.branch.as_deref(), Some("main"));

    std::fs::write(d.path().join("a.txt"), "one\nCHANGED\nthree\n").expect("write");
    let dirty = omt_workspace_fs::status(d.path()).expect("status");
    assert!(dirty.is_dirty());

    let changes =
        omt_workspace_fs::changed_files(d.path(), omt_workspace_fs::DiffTarget::Unstaged, None)
            .expect("diff");
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].path, "a.txt");

    let hunks = omt_workspace_fs::hunks(
        d.path(),
        "a.txt",
        omt_workspace_fs::DiffTarget::Unstaged,
        None,
    )
    .expect("hunks");
    assert!(
        hunks[0].lines.iter().any(|l| l.starts_with("+CHANGED")),
        "{:?}",
        hunks[0].lines
    );
}

#[test]
fn the_file_tree_lists_a_real_directory_and_refuses_to_leave_it() {
    let d = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir(d.path().join("src")).expect("mkdir");
    std::fs::write(d.path().join("src/main.rs"), "fn main() {}").expect("write");

    let fs = omt_workspace_fs::WorkspaceFs::new(d.path()).expect("open");
    let entries = fs.list("src").expect("list");
    assert!(entries.iter().any(|e| e.name == "main.rs"));

    // A relative path from a listing must resolve again — a client sends these
    // straight back.
    let rel = &entries[0].rel;
    assert!(fs.resolve(rel).is_ok(), "`{rel}` did not round trip");

    for escape in ["..", "/etc/passwd", "src/../../etc"] {
        assert!(fs.resolve(escape).is_err(), "`{escape}` was allowed");
    }
}

#[test]
fn a_file_transfers_in_chunks_and_resumes_where_it_stopped() {
    // Dragging a large file onto a pane over a link that drops.
    let payload: Vec<u8> = (0..omt_media::CHUNK_BYTES * 3 + 99)
        .map(|i| (i % 251) as u8)
        .collect();
    let plan = omt_media::TransferPlan::of(&payload).expect("plan");
    let mut rx = omt_media::Receiver::new(plan.clone());

    // Two chunks arrive, then the link drops.
    for (i, chunk) in payload.chunks(omt_media::CHUNK_BYTES).enumerate().take(2) {
        rx.accept(i, chunk).expect("accept");
    }
    let missing = plan.missing(&rx.have());
    assert_eq!(missing, vec![2, 3], "a resume would re-send everything");

    // It comes back and sends only what is left.
    for i in missing {
        let start = i * omt_media::CHUNK_BYTES;
        let end = (start + omt_media::CHUNK_BYTES).min(payload.len());
        rx.accept(i, &payload[start..end]).expect("accept");
    }
    assert!(rx.progress().is_complete());
    assert_eq!(rx.finish().expect("finish"), payload);
}

#[test]
fn a_capability_call_over_a_socket_reaches_the_same_instance_the_tui_drives() {
    // The architecture's central claim, exercised rather than asserted.
    let dir = std::env::temp_dir().join(format!("omt-e2e-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("tempdir");
    let socket = dir.join("omt.sock");

    let state = omt::state::State::default();
    let served = state.clone();
    let path = socket.clone();
    std::thread::spawn(move || {
        let _ = omt::serve::serve(&path, served);
    });

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut stream = loop {
        if let Ok(s) = omt_transport::connect(&socket) {
            break s;
        }
        assert!(Instant::now() < deadline, "never started listening");
        std::thread::sleep(Duration::from_millis(20));
    };

    fn send(stream: &mut std::os::unix::net::UnixStream, m: &omt_proto::ProtoMessage) {
        let bytes = serde_json::to_vec(m).expect("encode");
        omt_transport::write_frame(&mut *stream, omt_proto::FrameKind::Text, &bytes)
            .expect("write");
        stream.flush().ok();
    }
    fn recv(stream: &mut std::os::unix::net::UnixStream) -> omt_proto::ProtoMessage {
        let (_, payload) = omt_transport::read_frame(stream).expect("read");
        serde_json::from_slice(&payload).expect("decode")
    }

    send(
        &mut stream,
        &omt_proto::ProtoMessage::Hello(omt_proto::Hello {
            proto: omt_proto::PROTO_VERSION,
            client: "e2e".to_owned(),
            token: None,
        }),
    );
    assert!(matches!(
        recv(&mut stream),
        omt_proto::ProtoMessage::Welcome(_)
    ));

    send(
        &mut stream,
        &omt_proto::ProtoMessage::Call(omt_proto::Call {
            request: omt_catalog::RequestId {
                device: omt_types::DeviceId::new(),
                n: 1,
            },
            capability: "workspace.open".to_owned(),
            input: serde_json::json!({ "root": "/tmp" }),
            intent: Some(omt_types::IntentId::new()),
        }),
    );
    let omt_proto::ProtoMessage::Result(result) = recv(&mut stream) else {
        panic!("expected a result");
    };
    assert!(matches!(result.outcome, omt_proto::CallOutcome::Ok { .. }));

    assert_eq!(
        state.lock().expect("lock").workspaces().len(),
        1,
        "the remote call did not reach the instance the local side holds"
    );

    drop(stream);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn importing_a_real_vscode_theme_produces_a_usable_one() {
    let text = r##"{
      "name": "E2E Dark",
      "type": "dark",
      "colors": {
        "terminal.background": "#1e1e1e",
        "terminal.foreground": "#d4d4d4",
        "terminal.ansiRed": "#cd3131",
        "editor.lineHighlightBackground": "#2a2a2a"
      }
    }"##;
    let imported = omt_theme::from_vscode("E2E Dark", text).expect("import");
    assert!(
        imported.theme.is_readable(),
        "{:?}",
        imported.theme.warnings()
    );
    assert!(imported.theme.appearance_matches_background());
    assert!(
        imported
            .unmapped
            .contains(&"editor.lineHighlightBackground".to_owned()),
        "what a terminal cannot show was not reported: {:?}",
        imported.unmapped
    );
}

#[test]
fn a_plugin_cannot_read_outside_the_workspace_it_was_granted() {
    use std::collections::BTreeSet;
    let permissions: BTreeSet<_> = [omt_plugin_host::Permission::ReadWorkspace]
        .into_iter()
        .collect();
    let plugin = omt_plugin_host::Installed::new(
        omt_plugin_host::Manifest {
            id: "e2e".to_owned(),
            name: "E2E".to_owned(),
            version: "1".to_owned(),
            permissions: permissions.clone(),
            description: String::new(),
            entry: Vec::new(),
        },
        permissions,
    );

    let inside = omt_plugin_host::PluginCall::FsList {
        workspace: "wksp_x".to_owned(),
        path: "src".to_owned(),
    };
    assert!(omt_plugin_host::authorize(&plugin, &inside).is_ok());

    for escape in ["../../etc", "/etc/passwd"] {
        let out = omt_plugin_host::PluginCall::FsRead {
            workspace: "wksp_x".to_owned(),
            path: escape.to_owned(),
        };
        assert!(
            omt_plugin_host::authorize(&plugin, &out).is_err(),
            "`{escape}` was allowed"
        );
    }

    // And it may not write, having only been granted read.
    let write = omt_plugin_host::PluginCall::FsWrite {
        workspace: "wksp_x".to_owned(),
        path: "a.txt".to_owned(),
        contents: "x".to_owned(),
    };
    assert!(omt_plugin_host::authorize(&plugin, &write).is_err());
}

#[test]
fn a_worktree_fanout_gives_each_agent_its_own_workspace() {
    let fanout = omt_session::Fanout::new(
        "add retries",
        "abc123",
        vec![
            (
                omt_types::AgentKind::ClaudeCode,
                "/tmp/wt/claude".to_owned(),
                "try/claude".to_owned(),
            ),
            (
                omt_types::AgentKind::Codex,
                "/tmp/wt/codex".to_owned(),
                "try/codex".to_owned(),
            ),
        ],
    )
    .expect("fanout");

    let a = fanout
        .workspace_of(omt_types::AgentKind::ClaudeCode)
        .expect("arm");
    let b = fanout
        .workspace_of(omt_types::AgentKind::Codex)
        .expect("arm");
    assert_ne!(a, b, "two arms shared a workspace");
}
