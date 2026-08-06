//! The SSE decoder against the opencode that is installed.
//!
//! Everything else about `sse.rs` is checked against strings written by hand,
//! which proves the decoder is self-consistent and nothing more. This starts
//! the real `opencode serve`, reads its `/event` stream off a socket, and
//! asserts the decoder gets whole events out of it — which is the only thing
//! that catches a format assumption that was wrong.
//!
//! Ignored by default because it needs opencode installed and a free port. Run
//! it with `cargo test -p omt-agent-adapters -- --include-ignored`.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};

use omt_agent_adapters::sse::Decoder;

/// A port unlikely to collide with anything a developer is running.
const PORT: u16 = 7893;

fn opencode_is_installed() -> bool {
    Command::new("opencode")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Start the server and wait until it answers.
fn serve() -> Option<Child> {
    let child = Command::new("opencode")
        .args(["serve", "--port", &PORT.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    for _ in 0..100 {
        if TcpStream::connect(("127.0.0.1", PORT)).is_ok() {
            return Some(child);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    None
}

/// Read the raw event stream for a moment.
///
/// A hand-rolled request rather than an HTTP client, because what is being
/// tested is the decoder against bytes off a socket — putting a client in
/// between would be testing the client.
fn read_event_stream(bytes_wanted: usize) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", PORT)).expect("connect");
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(8)))
        .ok();
    write!(
        stream,
        "GET /event HTTP/1.1\r\nHost: 127.0.0.1\r\nAccept: text/event-stream\r\n\r\n"
    )
    .expect("request");
    stream.flush().ok();

    let mut reader = BufReader::new(stream);
    // Past the headers, to the body — which is where the events are.
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
            break;
        }
    }
    let mut body = Vec::new();
    let mut chunk = [0u8; 1024];
    while body.len() < bytes_wanted {
        match reader.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => body.extend_from_slice(&chunk[..n]),
        }
    }
    String::from_utf8_lossy(&body).into_owned()
}

#[test]
#[ignore = "needs opencode installed"]
fn the_real_event_stream_decodes_into_whole_events() {
    if !opencode_is_installed() {
        return;
    }
    let Some(mut server) = serve() else {
        return;
    };
    let raw = read_event_stream(400);
    server.kill().ok();
    server.wait().ok();

    assert!(!raw.is_empty(), "the event stream produced nothing");

    let mut decoder = Decoder::new();
    let events = decoder.feed(&raw);
    assert!(
        !events.is_empty(),
        "the decoder got no whole event out of a real stream:\n{raw}"
    );

    // Every event's data is complete JSON. Half an event decodes into a
    // fragment that parses as nothing, which is the failure this catches —
    // and a fragment that *does* parse is worse, because it becomes a card.
    for event in &events {
        serde_json::from_str::<serde_json::Value>(&event.data).unwrap_or_else(|e| {
            panic!("an event's data was not whole JSON: {e}\n{}", event.data)
        });
    }
}

#[test]
#[ignore = "needs opencode installed"]
fn opencode_names_its_events_inside_the_payload_not_in_an_event_field() {
    // Worth asserting because it is the opposite of what the format allows and
    // what a reasonable person would assume. opencode sends no `event:` line at
    // all — the type is a field inside `data`. An adapter that switched on the
    // SSE event name would match nothing, forever, silently.
    if !opencode_is_installed() {
        return;
    }
    let Some(mut server) = serve() else {
        return;
    };
    let raw = read_event_stream(400);
    server.kill().ok();
    server.wait().ok();

    let mut decoder = Decoder::new();
    let events = decoder.feed(&raw);
    let first = events.first().expect("no events");

    assert!(
        first.name.is_none(),
        "opencode started sending an `event:` line: {:?}",
        first.name
    );
    let value: serde_json::Value = serde_json::from_str(&first.data).expect("json");
    assert!(
        value.get("type").and_then(|t| t.as_str()).is_some(),
        "the type is not inside the payload either: {}",
        first.data
    );
}
