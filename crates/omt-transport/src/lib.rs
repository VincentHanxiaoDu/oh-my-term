//! Moving bytes. Framing only — no auth, no routing, no policy.
//!
//! Authorization lives in dispatch, deliberately: a transport that decided who
//! may do what would be a second place to get it wrong, and the one that
//! forgot would be a silent hole rather than a loud one.

mod framing;
pub mod ssh;
#[cfg(unix)]
mod unix;

pub use framing::{FramingError, PREFIX_LEN, read_frame, write_frame};
pub use ssh::{Remote, push_blob_command, remote_blob_path, shell_quote};
#[cfg(unix)]
pub use unix::{
    PeerCredentials, SOCKET_MODE, SocketListener, connect, current_uid, peer_credentials,
};
