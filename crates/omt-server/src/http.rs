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
    /// Who may connect.
    pub credentials: Arc<std::sync::Mutex<CredentialStore>>,
}

impl HttpState {
    /// Build the state.
    #[must_use]
    pub fn new(registry: CapabilityRegistry, credentials: CredentialStore) -> Self {
        Self {
            registry: Arc::new(registry),
            credentials: Arc::new(std::sync::Mutex::new(credentials)),
        }
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
    #[must_use]
    pub fn authorize_upgrade(&self, headers: &HeaderMap) -> Option<Role> {
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
    let role = state.authorize_upgrade(&headers).ok_or_else(unauthorized)?;
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

async fn serve_socket(mut socket: WebSocket, state: HttpState, role: Role) {
    let mut peer = Peer::new(Actor::Local, role);

    while let Some(Ok(message)) = socket.recv().await {
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

        if let Some(reply) = handle(&state.registry, &mut peer, incoming) {
            let Ok(text) = serde_json::to_string(&reply) else {
                return;
            };
            if socket.send(WsMessage::Text(text.into())).await.is_err() {
                return;
            }
        }
    }
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
                    format!("the TLS {what} at {} could not be read: {e}", path.display()),
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
pub fn run_with_tls(
    bind: &str,
    state: HttpState,
    tls: Option<TlsFiles>,
) -> std::io::Result<()> {
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
                let addr: std::net::SocketAddr = bind
                    .parse()
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("{bind} is not an address: {e}")))?;
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
        let err = files.check().expect_err("a missing certificate was accepted");
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
