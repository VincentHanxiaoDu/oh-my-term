//! The browser's way in: HTTP and a WebSocket, over the same dispatch.
//!
//! The WebSocket carries exactly the messages the Unix socket carries, so a
//! browser is not a third kind of client with its own semantics — it is the
//! same client over a different pipe. That is the whole reason the protocol is
//! transport-independent, and it is why nothing below the socket knows which
//! one it is talking to.
//!
//! **Every route requires a credential, including the one that lists the
//! catalog.** A capability list tells an attacker what an instance can do and
//! which version it is, and "it is only metadata" is how that ends up
//! unauthenticated.

use std::sync::Arc;

use axum::extract::State as AxumState;
use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use omt_auth::CredentialStore;
use omt_catalog::CapabilityRegistry;
use omt_proto::ProtoMessage;
use omt_types::{Actor, Role, Timestamp};

use crate::dispatch::{Peer, catalog_hash, handle};

/// What every request needs to reach.
#[derive(Clone)]
pub struct HttpState {
    /// The capabilities this instance offers.
    pub registry: Arc<CapabilityRegistry>,
    /// Origins a browser request may come from.
    ///
    /// 13 §6. The daemon serves the web client and must assume the same
    /// browser is running hostile pages: `evil.example` can open a socket to
    /// loopback, and the browser tells omt where the page came from without
    /// letting it lie. Empty means nothing browser-originated is accepted,
    /// which is the safe direction for a misconfiguration.
    pub allowed_origins: Vec<String>,
    /// Who may connect.
    pub credentials: Arc<std::sync::Mutex<CredentialStore>>,
    /// What this instance calls itself, for a device list somebody reads.
    pub name: String,
    /// Who is connected, what they have done, and how the process is bearing
    /// up. Written here and read by the capabilities that report it.
    pub watch: crate::watch::Shared,
    /// Who owns the devices that have joined.
    ///
    /// A credential answers "may this token do that". An identity answers
    /// "whose phone is that" — and revoking somebody's access means revoking
    /// *their* devices, which needs the second question to have an answer.
    pub identities: Arc<std::sync::Mutex<omt_identity::Directory>>,
    /// Where events come from, for connections that subscribe.
    ///
    /// A trait rather than the instance itself, because the instance lives a
    /// layer above this crate and `cargo xtask layering` is not a style rule.
    /// Absent means an instance that serves capabilities and pushes nothing,
    /// which is what this was before: `subscribe` was accepted, acknowledged
    /// by silence, and no event was ever sent — so every client polled.
    pub events: Option<Arc<dyn EventSource>>,
}

/// Somewhere events can be read from, by position.
///
/// The cursor is the client's own position in each stream. Passing it back in
/// is what makes a reconnecting client resume rather than replay everything or
/// silently skip what it missed.
pub trait EventSource: Send + Sync + 'static {
    /// Events after `cursor`, advancing it past what is returned.
    fn since(&self, cursor: &mut EventCursor) -> EventsSince;
}

/// A client's position in every stream it follows.
#[derive(Debug, Default, Clone)]
pub struct EventCursor {
    /// The instance-wide stream.
    pub instance: u64,
    /// Per session, by its wire id.
    pub sessions: std::collections::BTreeMap<String, u64>,
}

/// What a poll of the event source produced.
#[derive(Debug, Default)]
pub struct EventsSince {
    /// The events, oldest first.
    pub events: Vec<omt_events::Event>,
    /// Streams whose history no longer reaches back to where the client was.
    ///
    /// Announced rather than papered over: a client must never be left
    /// believing it is current when it is not.
    pub resync: Vec<omt_proto::Resync>,
}

impl HttpState {
    /// Build the state.
    #[must_use]
    pub fn new(registry: CapabilityRegistry, credentials: CredentialStore) -> Self {
        Self {
            registry: Arc::new(registry),
            credentials: Arc::new(std::sync::Mutex::new(credentials)),
            allowed_origins: Vec::new(),
            name: hostname(),
            watch: Arc::default(),
            identities: Arc::default(),
            events: None,
        }
    }

    /// Accept browser requests from these origins.
    ///
    /// Builder-style rather than a constructor argument, because most callers
    /// serve no browser at all and should not have to say so.
    #[must_use]
    pub fn with_origins(mut self, origins: Vec<String>) -> Self {
        self.allowed_origins = origins;
        self
    }

    /// Build the state around a credential store somebody else already holds.
    ///
    /// The device capabilities read and revoke through the same store this
    /// server authenticates against — two copies would let a revoked device
    /// keep working until whichever one the server consulted was updated.
    #[must_use]
    pub fn sharing(
        registry: CapabilityRegistry,
        credentials: Arc<std::sync::Mutex<CredentialStore>>,
    ) -> Self {
        Self {
            registry: Arc::new(registry),
            credentials,
            allowed_origins: Vec::new(),
            name: hostname(),
            watch: Arc::default(),
            identities: Arc::default(),
            events: None,
        }
    }

    /// Share the identity directory somebody else already holds.
    #[must_use]
    pub fn with_identities(
        mut self,
        identities: Arc<std::sync::Mutex<omt_identity::Directory>>,
    ) -> Self {
        self.identities = identities;
        self
    }

    /// Share a watch somebody else already holds.
    ///
    /// The capabilities that report presence and the socket that records it
    /// must be looking at one registry: two would answer "who is connected"
    /// differently depending on which was asked.
    #[must_use]
    pub fn watching(mut self, watch: crate::watch::Shared) -> Self {
        self.watch = watch;
        self
    }

    /// Give this server somewhere to read events from.
    #[must_use]
    pub fn with_events(mut self, source: Arc<dyn EventSource>) -> Self {
        self.events = Some(source);
        self
    }

    /// Check a request's credential.
    ///
    /// Returns the role it maps to, or nothing. Deliberately coarse: the
    /// caller learns whether it was accepted and never why it was not, because
    /// distinguishing "no such token" from "expired" confirms a guess.
    #[must_use]
    pub fn authorize(&self, headers: &HeaderMap) -> Option<Role> {
        let token = bearer(headers)?;
        let store = self.credentials.lock().ok()?;
        store
            .verify(&token, Timestamp::now(), false)
            .ok()
            .map(|c| c.role)
    }

    /// Check the credential on a WebSocket upgrade.
    ///
    /// Separate from `authorize` because it accepts one thing that method must
    /// not: a token in the subprotocol list. A browser cannot set a header on a
    /// `WebSocket`, so that is the only channel it has — and keeping it to this
    /// method means an ordinary API request still cannot authenticate that way.
    /// Whether this request's `Origin` is one omt serves.
    ///
    /// Checked before the credential, because a hostile page holding a stolen
    /// token is exactly the case this defends against — and answering "bad
    /// token" to it would confirm the token is the only thing missing.
    #[must_use]
    pub fn origin_allowed(&self, headers: &HeaderMap, state_changing: bool) -> bool {
        let origin = headers
            .get(axum::http::header::ORIGIN)
            .and_then(|v| v.to_str().ok());
        // A request with no `Origin` at all is not from a browser: `curl`, the
        // TUI and the thin client all omit it. Browsers always send it on an
        // upgrade, so its absence cannot be a page pretending — and requiring
        // it would break every non-browser client for no gain.
        if origin.is_none() {
            return true;
        }
        crate::origin::check(origin, &self.allowed_origins, state_changing).allowed()
    }

    /// Check the credential on a WebSocket upgrade.
    ///
    /// Separate from `authorize` because it accepts one thing that method must
    /// not: a token in the subprotocol list. A browser cannot set a header on a
    /// `WebSocket`, so that is the only channel it has — and keeping it to this
    /// method means an ordinary API request still cannot authenticate that way.
    #[must_use]
    pub fn authorize_upgrade(&self, headers: &HeaderMap) -> Option<Role> {
        if !self.origin_allowed(headers, true) {
            return None;
        }
        if let Some(role) = self.authorize(headers) {
            return Some(role);
        }
        let token = subprotocol_token(headers)?;
        let store = self.credentials.lock().ok()?;
        store
            .verify(&token, Timestamp::now(), false)
            .ok()
            .map(|c| c.role)
    }
}

/// Pull the bearer token out of the headers.
///
/// Only the `Authorization` header. A token in a query string lands in access
/// logs, browser history and any `Referer` the page sends, which is why the
/// invite link puts it in the fragment and the API does not accept it there at
/// all.
#[must_use]
pub fn bearer(headers: &HeaderMap) -> Option<String> {
    let value = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
        .map(str::to_owned)
}

/// The token a browser sent, which cannot be a header.
///
/// `WebSocket` in a browser has no way to set one — the only field it controls
/// is the subprotocol list. So the client offers `omt.token.<token>` alongside
/// `omt.v1`, and it is read here and nowhere else: this is the one place a
/// credential arrives outside `Authorization`, and confining it to the upgrade
/// keeps it out of the ordinary request path.
#[must_use]
pub fn subprotocol_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::SEC_WEBSOCKET_PROTOCOL)?
        .to_str()
        .ok()?
        .split(',')
        .map(str::trim)
        .find_map(|p| p.strip_prefix("omt.token."))
        .map(str::to_owned)
}

/// Every route the browser reaches.
pub fn router(state: HttpState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/catalog", get(catalog))
        .route("/api/ws", get(websocket))
        .fallback(get(asset))
        .with_state(state)
}

include!(concat!(env!("OUT_DIR"), "/assets.rs"));

/// The web client itself.
///
/// Unauthenticated, deliberately: these bytes are the same for every install
/// and contain no instance state. The token gates `/api/*`, which is where
/// anything about *this* machine lives. Gating the shell as well would mean the
/// page that asks for a token could not load without one.
///
/// Anything not found falls back to the shell rather than to 404, because a
/// client-routed path is a real URL to the user even though no file matches it.
async fn asset(uri: axum::http::Uri) -> impl IntoResponse {
    let path = uri.path();
    let wanted = if path == "/" { "/index.html" } else { path };
    let found = ASSETS
        .iter()
        .find(|(route, _, _)| *route == wanted)
        // A path under /api that reached here is a genuine 404 and must not be
        // answered with HTML, or a client bug reads as a parse error.
        .or_else(|| {
            if path.starts_with("/api/") || path.contains('.') {
                None
            } else {
                ASSETS.iter().find(|(route, _, _)| *route == "/index.html")
            }
        });
    match found {
        Some((_, mime, bytes)) => (
            StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, *mime)],
            *bytes,
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

/// What to call this machine.
///
/// The hostname, because that is what somebody recognises in a list of devices
/// and instances. Unknown rather than invented when there is none.
fn hostname() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|h| !h.trim().is_empty())
        .unwrap_or_else(|| "this instance".to_owned())
}

/// Whether the instance is up.
///
/// The one unauthenticated route, and it says nothing: a load balancer needs
/// to know the process is alive, and it does not need the version, the
/// capability list, or how many sessions are running.
async fn health() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

async fn catalog(
    AxumState(state): AxumState<HttpState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, Response> {
    let role = state.authorize(&headers).ok_or_else(unauthorized)?;
    let names: Vec<&str> = state
        .registry
        .decls()
        // Only what this credential could actually call. Listing capabilities a
        // viewer cannot invoke would have a UI draw buttons that always fail.
        .filter(|d| role >= d.role)
        .map(|d| d.name)
        .collect();
    Ok(Json(serde_json::json!({
        "proto": omt_proto::PROTO_VERSION,
        "catalog_hash": catalog_hash(&state.registry),
        "capabilities": names,
    })))
}

async fn websocket(
    AxumState(state): AxumState<HttpState>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Result<Response, Response> {
    // Checked *before* the upgrade. Accepting the socket and then closing it
    // would let an unauthenticated caller hold a connection, and browsers
    // report the failure as an opaque network error rather than a 401.
    //
    // A connection with no credential is allowed through with none, and can do
    // exactly one thing: exchange an invite for a credential of its own. It has
    // to be allowed, because an invite is the right to *obtain* a credential —
    // requiring one to use it is a door that only opens from inside.
    let role = state.authorize_upgrade(&headers);
    // The negotiated subprotocol has to be echoed or the browser rejects the
    // upgrade it just completed — and reports it as a bare connection failure
    // with nothing about why.
    Ok(upgrade
        .protocols(["omt.v1"])
        .on_upgrade(move |socket| serve_socket(socket, state, role)))
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": "the credential was not accepted" })),
    )
        .into_response()
}

/// How often a subscribed connection is checked for new events.
///
/// Polled rather than pushed from the instance, deliberately: the instance is a
/// plain `Mutex` shared with the pty pump and the capability handlers, and
/// threading a broadcast channel out of it would put a notification path
/// through every lock that already exists. At this interval a change is on a
/// phone before a thumb has left the glass, and an idle instance costs one
/// uncontended lock per tick per connection.
const EVENT_POLL: std::time::Duration = std::time::Duration::from_millis(40);

async fn serve_socket(mut socket: WebSocket, state: HttpState, role: Option<Role>) {
    // Without a credential the peer is given the lowest role there is, and the
    // dispatcher refuses everything it is not allowed to do. The join exchange
    // is handled here, before dispatch, and is the only thing such a connection
    // can accomplish.
    let joined = role.is_some();
    // What this connection may authenticate with, advertised in the welcome.
    // Derived here because only the accept path knows where the connection
    // came from, and tailnet identity offered on a loopback socket would be
    // authentication bypassed by asserting it.
    let backends = omt_auth::methods::Backends {
        bearer: true,
        invite: true,
        ..omt_auth::methods::Backends::default()
    };
    let offered: Vec<String> = omt_auth::methods::offered_for(backends, omt_auth::methods::Peer::Loopback)
        .into_iter()
        .map(|m| m.id().to_owned())
        .collect();
    let mut peer = Peer::new(Actor::Local, role.unwrap_or(Role::Viewer)).offering(offered);
    // Its own identity for as long as it is connected, so presence can name a
    // client and the audit log can say which one did something.
    let connection = format!("conn_{}", omt_types::ClientId::new().to_wire());
    if !joined {
        peer.unauthenticated();
    }
    let mut cursor = EventCursor::default();
    let mut subscribed = false;
    let mut ticker = tokio::time::interval(EVENT_POLL);

    loop {
        let incoming = tokio::select! {
            received = socket.recv() => match received {
                Some(Ok(message)) => message,
                // Closed, or a transport error. Either way this connection is
                // over; the session it was watching is not.
                _ => {
                    if let Ok(mut watch) = state.watch.lock() {
                        watch.left(&connection);
                    }
                    return;
                }
            },
            _ = ticker.tick(), if subscribed => {
                let Some(source) = state.events.as_ref() else { continue };
                let batch = source.since(&mut cursor);
                for resync in batch.resync {
                    if send(&mut socket, &ProtoMessage::Resync(resync)).await.is_err() {
                        return;
                    }
                }
                for event in batch.events {
                    if send(&mut socket, &ProtoMessage::Event(Box::new(event)))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                continue;
            }
        };

        // The join exchange, handled here rather than in the dispatcher: it
        // needs the credential store, which is this crate's, and it is the one
        // message a connection with no credential is allowed to send.
        if let Ok(ProtoMessage::JoinExchange(wanted)) =
            serde_json::from_slice::<ProtoMessage>(&payload_of(&incoming))
        {
            let reply = exchange_invite(&state, &wanted);
            match reply {
                Some(credential) => {
                    // Promoted in place. The socket that joined is the socket
                    // that carries on, so a phone does not have to reconnect
                    // the moment it is let in.
                    peer.role = credential.role;
                    peer.anonymous = false;
                    if send(
                        &mut socket,
                        &ProtoMessage::JoinCredential(Box::new(credential)),
                    )
                    .await
                    .is_err()
                    {
                        return;
                    }
                }
                None => {
                    let _ = send(
                        &mut socket,
                        &ProtoMessage::Goodbye(omt_proto::Goodbye {
                            reason: omt_proto::GoodbyeReason::Unauthorized,
                            detail: "that invite cannot be exchanged".to_owned(),
                        }),
                    )
                    .await;
                    return;
                }
            }
            continue;
        }

        if matches!(incoming, WsMessage::Text(_) | WsMessage::Binary(_)) {
            // Noted before dispatch, because `Subscribe` is answered with
            // silence and there is nothing downstream to learn it from.
            if let Ok(parsed) = serde_json::from_slice::<ProtoMessage>(&payload_of(&incoming))
                && let ProtoMessage::Subscribe(ref wanted) = parsed
            {
                subscribed = true;
                seed_cursor(&mut cursor, wanted);
            }
        }

        let message = incoming;
        let payload = match message {
            WsMessage::Text(text) => text.as_bytes().to_vec(),
            WsMessage::Binary(bytes) => bytes.to_vec(),
            // Ping and pong are the transport's business; a close ends it.
            WsMessage::Close(_) => return,
            WsMessage::Ping(_) | WsMessage::Pong(_) => continue,
        };

        let Ok(incoming) = serde_json::from_slice::<ProtoMessage>(&payload) else {
            // Unparseable input ends this connection and nothing else. Trying
            // to resynchronize a stream whose framing may be wrong is how one
            // bad client corrupts another's replies.
            return;
        };

        // Noted before dispatch, recorded after: the log needs the outcome, and
        // a log of only what succeeded answers the wrong question — "who tried
        // to do what" is most of what an audit is for.
        let audited = match incoming {
            ProtoMessage::Call(ref call) => {
                Some((call.capability.clone(), call.intent.map(|i| i.to_wire())))
            }
            _ => None,
        };
        if let ProtoMessage::Hello(ref hello) = incoming
            && let Ok(mut watch) = state.watch.lock()
        {
            watch.arrived(
                &connection,
                &hello.client,
                peer.role,
                omt_types::Timestamp::now(),
            );
        }

        let reply = handle(&state.registry, &mut peer, incoming);

        if let Some((capability, intent)) = audited
            && let Ok(mut watch) = state.watch.lock()
        {
            let ok = matches!(
                reply,
                Some(ProtoMessage::Result(ref r))
                    if matches!(r.outcome, omt_proto::CallOutcome::Ok { .. })
            );
            watch.record(crate::watch::Audited {
                at: omt_types::Timestamp::now().to_string(),
                capability,
                client: connection.clone(),
                intent,
                ok,
            });
        }

        if let Some(reply) = reply
            && send(&mut socket, &reply).await.is_err()
        {
            // Gone. Taken out of presence here as well as at the end, because
            // this is the path a dropped connection actually takes.
            if let Ok(mut watch) = state.watch.lock() {
                watch.left(&connection);
            }
            return;
        }
    }
}

/// Swap an invite for a credential, or refuse.
///
/// One coarse refusal whatever went wrong — unknown, expired, already used.
/// Distinguishing them tells whoever is guessing which guesses were close.
fn exchange_invite(
    state: &HttpState,
    wanted: &omt_proto::JoinExchange,
) -> Option<omt_proto::JoinCredential> {
    let mut store = state.credentials.lock().ok()?;
    let minted = store
        .exchange(&wanted.invite, &wanted.device, omt_types::Timestamp::now())
        .ok()?;

    // Registered against a person, not just issued. One instance has one
    // owner — whoever started it — and every device that joins is theirs until
    // there is a reason to model more, which is what `23` describes and what
    // makes "revoke everything that person has" expressible at all.
    if let Ok(mut directory) = state.identities.lock() {
        let existing = directory.identities().first().map(|i| i.id);
        let owner = match existing {
            Some(id) => id,
            None => directory.add_identity("owner"),
        };
        // A browser holds no key it can keep secret from itself — a key in
        // local storage is exfiltrated with the token it would protect. So an
        // unbound device is recorded as unbound, by an identifier that says so,
        // rather than by a made-up key that would read as binding.
        let key = wanted
            .device
            .pubkey
            .clone()
            .unwrap_or_else(|| format!("unbound:{}", minted.credential.id));
        let _ = directory.register_device(owner, &wanted.device.name, &key);
    }
    Some(omt_proto::JoinCredential {
        request: wanted.request,
        instance: omt_proto::JoinedInstance {
            name: state.name.clone(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            proto: omt_proto::PROTO_VERSION,
        },
        token: minted.token,
        role: minted.credential.role,
    })
}

/// A message's bytes, whatever frame carried it.
fn payload_of(message: &WsMessage) -> Vec<u8> {
    match message {
        WsMessage::Text(text) => text.as_bytes().to_vec(),
        WsMessage::Binary(bytes) => bytes.to_vec(),
        _ => Vec::new(),
    }
}

/// Start a subscription where the client asked to.
///
/// A client that reconnects sends the position it had, so it resumes rather
/// than replaying from the beginning — which on a session with an hour of
/// output is a client that hangs on reconnect.
fn seed_cursor(cursor: &mut EventCursor, wanted: &omt_proto::Subscribe) {
    for (scope, seq) in &wanted.since {
        if scope == "instance" {
            cursor.instance = seq.get();
        } else {
            cursor.sessions.insert(scope.clone(), seq.get());
        }
    }
}

async fn send(socket: &mut WebSocket, message: &ProtoMessage) -> Result<(), ()> {
    let Ok(text) = serde_json::to_string(message) else {
        return Err(());
    };
    socket
        .send(WsMessage::Text(text.into()))
        .await
        .map_err(|_| ())
}

/// Run the server until the process ends.
///
/// # Errors
/// Fails if the address cannot be bound or the runtime cannot be built.
pub fn run(bind: &str, state: HttpState) -> std::io::Result<()> {
    run_with_tls(bind, state, None)
}

/// A certificate and its key.
#[derive(Debug, Clone)]
pub struct TlsFiles {
    /// PEM certificate chain.
    pub cert: std::path::PathBuf,
    /// PEM private key.
    pub key: std::path::PathBuf,
}

impl TlsFiles {
    /// Check both files before a listener is built.
    ///
    /// Checked here rather than left to the TLS layer because the failure this
    /// prevents is the confusing one: a server that starts, binds the port, and
    /// then refuses every connection with a handshake error the browser reports
    /// as an unrelated network failure. Refusing to start says which file.
    ///
    /// # Errors
    /// Fails if either file cannot be read.
    pub fn check(&self) -> std::io::Result<()> {
        for (what, path) in [("certificate", &self.cert), ("key", &self.key)] {
            std::fs::File::open(path).map_err(|e| {
                std::io::Error::new(
                    e.kind(),
                    format!(
                        "the TLS {what} at {} could not be read: {e}",
                        path.display()
                    ),
                )
            })?;
        }
        Ok(())
    }
}

/// Serve, with TLS when a certificate is given.
///
/// # Errors
/// Fails if the address cannot be bound or the certificate cannot be loaded.
pub fn run_with_tls(bind: &str, state: HttpState, tls: Option<TlsFiles>) -> std::io::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        match tls {
            Some(files) => {
                files.check()?;
                let config =
                    axum_server::tls_rustls::RustlsConfig::from_pem_file(&files.cert, &files.key)
                        .await?;
                let addr: std::net::SocketAddr = bind.parse().map_err(|e| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("{bind} is not an address: {e}"),
                    )
                })?;
                axum_server::bind_rustls(addr, config)
                    .serve(router(state).into_make_service())
                    .await
            }
            None => {
                let listener = tokio::net::TcpListener::bind(bind).await?;
                axum::serve(listener, router(state)).await
            }
        }
    })
}

/// The default bind address.
///
/// Loopback, always. A terminal multiplexer that bound every interface by
/// default would put the user's shell on their coffee-shop network, and the
/// person it happens to would not be the person who chose the default.
pub const DEFAULT_BIND: &str = "127.0.0.1:7717";

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn headers(value: Option<&str>) -> HeaderMap {
        let mut h = HeaderMap::new();
        if let Some(v) = value {
            h.insert(
                axum::http::header::AUTHORIZATION,
                v.parse().expect("header"),
            );
        }
        h
    }

    fn state_with_token() -> (HttpState, String) {
        let mut store = CredentialStore::new();
        let minted = store.mint(Role::Operator, "phone", None, None);
        (
            HttpState::new(CapabilityRegistry::new(), store),
            minted.token,
        )
    }

    #[test]
    fn a_valid_bearer_token_is_accepted() {
        let (state, token) = state_with_token();
        assert_eq!(
            state.authorize(&headers(Some(&format!("Bearer {token}")))),
            Some(Role::Operator)
        );
    }

    #[test]
    fn a_request_with_no_credential_is_refused() {
        let (state, _) = state_with_token();
        assert_eq!(state.authorize(&headers(None)), None);
    }

    #[test]
    fn a_wrong_token_is_refused() {
        let (state, _) = state_with_token();
        assert_eq!(
            state.authorize(&headers(Some("Bearer omt_c_notarealtokenatall"))),
            None
        );
    }

    #[test]
    fn the_scheme_is_case_insensitive_but_the_header_is_required() {
        // Browsers and CLIs disagree about capitalization; a config that works
        // in curl should work in the app.
        let (state, token) = state_with_token();
        assert!(
            state
                .authorize(&headers(Some(&format!("bearer {token}"))))
                .is_some()
        );
        assert_eq!(bearer(&headers(Some(&token))), None, "no scheme, no token");
    }

    #[test]
    fn a_token_in_a_query_string_is_not_accepted_anywhere() {
        // It would land in access logs, browser history and any Referer the
        // page sends. The invite link puts it in the fragment for the same
        // reason, and the API must not offer a second door.
        let mut h = HeaderMap::new();
        h.insert("x-token", "omt_c_something".parse().expect("header"));
        assert_eq!(bearer(&h), None);
    }

    #[test]
    fn a_missing_certificate_is_refused_before_the_port_is_bound() {
        // The failure this prevents is the confusing one: a server that starts,
        // binds, and then refuses every connection with a handshake error the
        // browser reports as an unrelated network failure.
        let files = TlsFiles {
            cert: std::path::PathBuf::from("/definitely/not/here.pem"),
            key: std::path::PathBuf::from("/definitely/not/here.key"),
        };
        let err = files
            .check()
            .expect_err("a missing certificate was accepted");
        assert!(
            err.to_string().contains("certificate"),
            "the error does not say which file: {err}"
        );
    }

    #[test]
    fn a_missing_key_names_the_key_and_not_the_certificate() {
        // Naming the wrong file sends somebody to check something that is fine.
        let dir = std::env::temp_dir().join("omt-tls-test");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let cert = dir.join("cert.pem");
        std::fs::write(&cert, b"not a real certificate").expect("write");
        let files = TlsFiles {
            cert,
            key: dir.join("absent.key"),
        };
        let err = files.check().expect_err("a missing key was accepted");
        assert!(err.to_string().contains("key"), "{err}");
    }

    #[test]
    fn the_web_client_is_embedded_in_the_binary() {
        // Not a build artifact somebody has to copy next to the binary: `cargo
        // install omt` has to produce a working web client, and a missing
        // asset directory is a blank page with nothing in the log.
        assert!(
            ASSETS.iter().any(|(r, _, _)| *r == "/index.html"),
            "the shell is missing; run `npm run build` in web/"
        );
        assert!(
            ASSETS.iter().any(|(r, _, _)| *r == "/app/main.js"),
            "the entry point is missing"
        );
    }

    #[test]
    fn every_script_is_served_as_a_module_not_as_text() {
        // A browser refuses to execute a module served as text/plain, and the
        // page then loads with no error anywhere that says so.
        for (route, mime, _) in ASSETS {
            if route.ends_with(".js") {
                assert!(mime.starts_with("text/javascript"), "{route} is {mime}");
            }
        }
    }

    #[test]
    fn a_revoked_token_stops_working() {
        let mut store = CredentialStore::new();
        let minted = store.mint(Role::Operator, "stolen", None, None);
        store.revoke(&minted.credential.id);
        let state = HttpState::new(CapabilityRegistry::new(), store);
        assert_eq!(
            state.authorize(&headers(Some(&format!("Bearer {}", minted.token)))),
            None
        );
    }

    #[test]
    fn an_expired_token_stops_working() {
        let mut store = CredentialStore::new();
        let minted = store.mint(
            Role::Viewer,
            "invite",
            Some(Timestamp::from_unix_seconds(1)),
            None,
        );
        let state = HttpState::new(CapabilityRegistry::new(), store);
        assert_eq!(
            state.authorize(&headers(Some(&format!("Bearer {}", minted.token)))),
            None
        );
    }

    #[test]
    fn the_default_bind_is_loopback() {
        // Binding every interface by default would put somebody's shell on
        // their coffee-shop network, and the person it happens to is not the
        // person who chose the default.
        assert!(DEFAULT_BIND.starts_with("127.0.0.1"), "{DEFAULT_BIND}");
    }

    #[test]
    fn a_router_can_be_built() {
        let (state, _) = state_with_token();
        let _ = router(state);
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod origin_tests {
    use super::*;

    fn state() -> HttpState {
        HttpState::new(
            CapabilityRegistry::new(),
            CredentialStore::new(),
        )
        .with_origins(vec!["http://127.0.0.1:7788".to_owned()])
    }

    fn headers(origin: Option<&str>) -> HeaderMap {
        let mut h = HeaderMap::new();
        if let Some(origin) = origin {
            h.insert(
                axum::http::header::ORIGIN,
                origin.parse().expect("a header value"),
            );
        }
        h
    }

    #[test]
    fn an_upgrade_from_a_hostile_page_is_refused_before_the_credential() {
        // A hostile page holding a stolen token is exactly this case, and
        // answering "bad token" to it would confirm the token is the only
        // thing missing.
        assert!(!state().origin_allowed(&headers(Some("https://evil.example")), true));
        assert!(state().authorize_upgrade(&headers(Some("https://evil.example"))).is_none());
    }

    #[test]
    fn an_upgrade_from_the_page_omt_serves_passes_the_origin_check() {
        assert!(state().origin_allowed(&headers(Some("http://127.0.0.1:7788")), true));
    }

    #[test]
    fn a_request_with_no_origin_is_not_from_a_browser() {
        // curl, the TUI and the thin client all omit it; browsers always send
        // it on an upgrade. Requiring it would break every non-browser client
        // for no gain, because its absence cannot be a page pretending.
        assert!(state().origin_allowed(&headers(None), true));
    }

    #[test]
    fn an_instance_serving_no_browser_still_refuses_a_page() {
        // The default allow-list is empty, so a daemon nobody configured for
        // the web does not quietly accept browser traffic.
        let bare = HttpState::new(
            CapabilityRegistry::new(),
            CredentialStore::new(),
        );
        assert!(!bare.origin_allowed(&headers(Some("http://127.0.0.1:7788")), true));
        assert!(bare.origin_allowed(&headers(None), true), "and the TUI still connects");
    }
}
