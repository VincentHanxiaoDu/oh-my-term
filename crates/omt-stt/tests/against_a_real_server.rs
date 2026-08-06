//! A provider driven against a real HTTP server.
//!
//! Not a mock of `ureq`: an actual socket, an actual request, an actual
//! response. What this catches is everything a unit test on the parser cannot —
//! the header spelling, the body encoding, whether the request is even
//! well-formed enough for a server to answer it.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;

use omt_stt::{Deepgram, OpenAi, SttProvider};

/// A one-request HTTP server that records what it was sent.
fn serve_once(response: &'static str) -> (String, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut reader = BufReader::new(stream.try_clone().expect("clone"));

        let mut request = String::new();
        let mut length = 0usize;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).expect("read") == 0 {
                break;
            }
            if let Some(v) = line.to_lowercase().strip_prefix("content-length:") {
                length = v.trim().parse().unwrap_or(0);
            }
            request.push_str(&line);
            if line == "\r\n" {
                break;
            }
        }
        let mut body = vec![0u8; length];
        reader.read_exact(&mut body).expect("body");
        request.push_str(&String::from_utf8_lossy(&body));

        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
            response.len()
        )
        .expect("write");
        stream.flush().ok();
        request
    });
    (format!("http://127.0.0.1:{port}/"), handle)
}

#[test]
fn deepgram_sends_the_key_the_way_deepgram_wants_it() {
    // `Token <key>`, not `Bearer`. Getting this wrong produces a 401 that reads
    // exactly like a wrong key, and somebody regenerates a perfectly good one.
    let (url, server) = serve_once(
        r#"{"results":{"channels":[{"alternatives":[{"transcript":"run the tests","confidence":0.9}]}]}}"#,
    );
    let provider = Deepgram::new("secret-key".to_owned()).at(url);
    let transcript = provider.transcribe(b"RIFF....fake wav").expect("transcribe");
    assert_eq!(transcript.text, "run the tests");

    // Compared lowercase, because header names are case-insensitive and a
    // client is free to normalise them. What matters is the scheme and the key.
    let request = server.join().expect("server").to_lowercase();
    assert!(
        request.contains("authorization: token secret-key"),
        "the key was not sent the way Deepgram expects:\n{request}"
    );
    assert!(
        request.contains("riff....fake wav"),
        "the audio never made it into the body"
    );
}

#[test]
fn openai_sends_a_multipart_body_a_server_can_actually_read() {
    // The boundary in the header has to match the one in the body. When it does
    // not, the server answers with a message about a missing file and the user
    // spends an afternoon on their key.
    let (url, server) = serve_once(r#"{"text":"hello there"}"#);
    let provider = OpenAi::new("sk-test".to_owned()).at(url);
    let transcript = provider.transcribe(b"RIFF....fake wav").expect("transcribe");
    assert_eq!(transcript.text, "hello there");

    let request = server.join().expect("server").to_lowercase();
    assert!(request.contains("authorization: bearer sk-test"), "{request}");
    let boundary = request
        .lines()
        .find(|l| l.to_lowercase().starts_with("content-type: multipart"))
        .and_then(|l| l.split("boundary=").nth(1))
        .map(str::trim)
        .expect("no multipart boundary in the header");
    assert!(
        request.contains(&format!("--{boundary}")),
        "the header's boundary is not the body's:\n{request}"
    );
    assert!(request.contains("name=\"model\""));
    assert!(request.contains("filename=\"audio.wav\""));
}

#[test]
fn a_provider_reports_what_the_server_said_rather_than_a_generic_failure() {
    // An error body carries the reason. Replacing it with "no transcript" sends
    // somebody to check their microphone.
    let (url, server) = serve_once(r#"{"error":{"message":"Incorrect API key provided"}}"#);
    let err = OpenAi::new("bad".to_owned())
        .at(url)
        .transcribe(b"x")
        .expect_err("an error body was treated as success");
    assert!(err.to_string().contains("Incorrect API key"), "{err}");
    server.join().ok();
}
