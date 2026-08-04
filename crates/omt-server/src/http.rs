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

/// Every route the browser reaches.
pub fn router(state: HttpState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/catalog", get(catalog))
        .route("/api/ws", get(websocket))
        .with_state(state)
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
    let role = state.authorize(&headers).ok_or_else(unauthorized)?;
    Ok(upgrade.on_upgrade(move |socket| serve_socket(socket, state, role)))
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
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(bind).await?;
        axum::serve(listener, router(state)).await
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
