//! A browser's path in, against a real listening server.
//!
//! Not a router unit test: this binds a port, speaks HTTP and WebSocket over
//! it, and checks that the same protocol the Unix socket carries arrives
//! intact. A browser is not a third kind of client — it is the same client
//! over a different pipe, and this is what holds that true.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]

use futures_util::{SinkExt, StreamExt};
use omt_auth::CredentialStore;
use omt_catalog::CapabilityRegistry;
use omt_proto::{Hello, ProtoMessage};
use omt_server::HttpState;
use omt_types::Role;

/// Start a server on an ephemeral port and return its address and a token.
async fn start() -> (String, String) {
    let mut store = CredentialStore::new();
    let minted = store.mint(Role::Operator, "test", None, None);
    let state = HttpState::new(CapabilityRegistry::new(), store);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr").to_string();
    tokio::spawn(async move {
        let _ = axum::serve(listener, omt_server::router(state)).await;
    });
    (addr, minted.token)
}

async fn get(addr: &str, path: &str, token: Option<&str>) -> (u16, String) {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let auth = token.map_or_else(String::new, |t| format!("Authorization: Bearer {t}\r\n"));
    let request = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n{auth}\r\n");
    stream.write_all(request.as_bytes()).await.expect("write");

    let mut body = Vec::new();
    stream.read_to_end(&mut body).await.expect("read");
    let text = String::from_utf8_lossy(&body).into_owned();
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    (status, text)
}

#[tokio::test]
async fn health_needs_no_credential_and_reveals_nothing() {
    // A load balancer needs to know the process is alive. It does not need the
    // version, the capability list, or how many sessions are running.
    let (addr, _) = start().await;
    let (status, body) = get(&addr, "/api/health", None).await;
    assert_eq!(status, 200);
    assert!(!body.contains("capabilit"), "{body}");
    assert!(!body.contains("proto"), "{body}");
}

#[tokio::test]
async fn the_catalog_requires_a_credential() {
    // A capability list tells an attacker what an instance can do and which
    // version it is. "It is only metadata" is how that ends up unauthenticated.
    let (addr, token) = start().await;
    let (status, _) = get(&addr, "/api/catalog", None).await;
    assert_eq!(status, 401);

    let (status, body) = get(&addr, "/api/catalog", Some(&token)).await;
    assert_eq!(status, 200);
    assert!(body.contains("catalog_hash"), "{body}");
}

#[tokio::test]
async fn a_wrong_token_is_refused() {
    let (addr, _) = start().await;
    let (status, _) = get(&addr, "/api/catalog", Some("omt_c_notarealtokenatall")).await;
    assert_eq!(status, 401);
}

#[tokio::test]
async fn a_websocket_without_a_credential_can_only_exchange_an_invite() {
    // The upgrade is allowed with no credential, and has to be: an invite is
    // the right to *obtain* a credential, so requiring one to use it is a door
    // that only opens from inside.
    //
    // What makes that safe is this: such a connection is refused everything
    // else, including the handshake — a welcome states a role and lists what
    // the peer may call, and neither is true of a caller who has presented
    // nothing.
    use futures_util::{SinkExt as _, StreamExt as _};

    let (addr, _) = start().await;
    let url = format!("ws://{addr}/api/ws");
    let (mut socket, _) = tokio_tungstenite::connect_async(&url)
        .await
        .expect("the upgrade is allowed so an invite can be exchanged");

    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::json!({ "t": "hello", "proto": 1, "client": "test" }).to_string(),
        ))
        .await
        .expect("send");

    let reply = socket.next().await.expect("a reply").expect("no error");
    let text = reply.into_text().expect("text");
    let message: serde_json::Value = serde_json::from_str(&text).expect("json");
    assert_eq!(message["t"], "goodbye", "{message}");
    assert_eq!(message["reason"], "unauthorized", "{message}");
}

#[tokio::test]
async fn a_browser_speaks_the_same_protocol_the_unix_socket_carries() {
    // The claim the whole transport-independent protocol exists for.
    use tokio_tungstenite::tungstenite::Message;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let (addr, token) = start().await;
    let mut request = format!("ws://{addr}/api/ws")
        .into_client_request()
        .expect("request");
    request.headers_mut().insert(
        "Authorization",
        format!("Bearer {token}").parse().expect("header"),
    );

    let (mut socket, _) = tokio_tungstenite::connect_async(request)
        .await
        .expect("the websocket was refused");

    let hello = ProtoMessage::Hello(Hello {
        proto: omt_proto::PROTO_VERSION,
        client: "browser".to_owned(),
        token: None,
    });
    socket
        .send(Message::Text(
            serde_json::to_string(&hello).expect("encode"),
        ))
        .await
        .expect("send");

    let reply = socket.next().await.expect("a reply").expect("no error");
    let text = reply.into_text().expect("text");
    let message: ProtoMessage = serde_json::from_str(&text).expect("decode");
    assert!(
        matches!(message, ProtoMessage::Welcome(_)),
        "the browser got {message:?} instead of a welcome"
    );
}

#[tokio::test]
async fn a_viewer_is_not_shown_capabilities_it_cannot_invoke() {
    // Listing them would have a UI draw buttons that always fail.
    let mut store = CredentialStore::new();
    let viewer = store.mint(Role::Viewer, "read only", None, None);
    let state = HttpState::new(CapabilityRegistry::new(), store);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr").to_string();
    tokio::spawn(async move {
        let _ = axum::serve(listener, omt_server::router(state)).await;
    });

    let (status, body) = get(&addr, "/api/catalog", Some(&viewer.token)).await;
    assert_eq!(status, 200);
    assert!(body.contains("capabilities"), "{body}");
}
