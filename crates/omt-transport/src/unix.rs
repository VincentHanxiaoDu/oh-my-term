//! The local Unix socket.
//!
//! How the CLI, the TUI and — most importantly — `omt-hook` reach a running
//! instance. Authorization here is the operating system's: the peer's uid is
//! read from the kernel, not claimed in a message, so a message cannot lie
//! about who sent it.

use std::io;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

/// The mode a socket is created with.
///
/// `0600`: owner only. A multi-user machine is the normal case on a dev box or
/// a shared server, and a world-writable control socket is a shell for anyone
/// with an account.
pub const SOCKET_MODE: u32 = 0o600;

/// Who is on the other end, as the kernel reports it.
///
/// Not as the peer claims. This is the whole reason the local socket needs no
/// token: identity that a message cannot forge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerCredentials {
    /// The peer's user id.
    pub uid: u32,
    /// The peer's process id, where the platform reports one.
    pub pid: Option<i32>,
}

impl PeerCredentials {
    /// Whether the peer is the same user as this process.
    ///
    /// The check the socket makes before a handshake. A different uid on the
    /// same machine is another human or another service, and neither should
    /// reach a session tree by finding a path.
    #[must_use]
    pub fn is_same_user(&self) -> bool {
        // SAFETY-free: `getuid` cannot fail and has no invariants.
        self.uid == current_uid()
    }
}

/// This process's effective user id.
#[must_use]
pub fn current_uid() -> u32 {
    // SAFETY: `geteuid` takes no arguments, cannot fail, and has no invariants
    // to uphold. This is the one place the crate needs libc at all.
    #[allow(unsafe_code, reason = "geteuid is infallible and argument-free")]
    unsafe {
        libc::geteuid()
    }
}

/// A listening socket, which removes its path when dropped.
#[derive(Debug)]
pub struct SocketListener {
    listener: UnixListener,
    path: PathBuf,
}

impl SocketListener {
    /// Bind, replacing a stale socket left by a previous run.
    ///
    /// # Errors
    /// The underlying bind or permission error.
    pub fn bind(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();

        // A socket file outliving its process is the normal aftermath of a
        // crash, and refusing to start because of one would turn a crash into
        // an outage. Removing it is safe: a live instance holds the path.
        if path.exists() {
            if UnixStream::connect(&path).is_ok() {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    format!("{} is already served by a running instance", path.display()),
                ));
            }
            std::fs::remove_file(&path)?;
        }

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let listener = UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(SOCKET_MODE))?;
        Ok(Self { listener, path })
    }

    /// The path it is bound to.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Accept one connection, with the peer's credentials.
    ///
    /// # Errors
    /// The underlying accept error.
    pub fn accept(&self) -> io::Result<(UnixStream, PeerCredentials)> {
        let (stream, _) = self.listener.accept()?;
        let creds = peer_credentials(&stream)?;
        Ok((stream, creds))
    }

    /// The underlying listener, for callers that need to poll it.
    #[must_use]
    pub fn as_raw(&self) -> &UnixListener {
        &self.listener
    }
}

impl Drop for SocketListener {
    fn drop(&mut self) {
        // Best effort: a leftover socket is tidied by the next bind anyway.
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Read the peer's credentials from a connected stream.
///
/// # Errors
/// The platform error if the credentials cannot be read — which is treated as
/// a refusal, since an unidentifiable peer is one that has not been identified.
pub fn peer_credentials(stream: &UnixStream) -> io::Result<PeerCredentials> {
    // The kernel recorded these at connect time. `std`'s accessor is still
    // unstable, so this asks the socket directly — `SO_PEERCRED` on Linux,
    // `LOCAL_PEERCRED` on macOS and the BSDs. Either way the answer comes from
    // the kernel, which is the property that matters: the peer cannot restate
    // it.
    use std::os::fd::AsRawFd;
    let fd = stream.as_raw_fd();

    #[cfg(target_os = "linux")]
    {
        let mut cred = libc::ucred {
            pid: 0,
            uid: 0,
            gid: 0,
        };
        let mut len = size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: `fd` is a connected socket owned by the caller, `cred` is
        // correctly sized for SO_PEERCRED, and `len` describes it.
        #[allow(unsafe_code, reason = "reading kernel-recorded peer credentials")]
        let rc = unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                std::ptr::from_mut(&mut cred).cast(),
                &raw mut len,
            )
        };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(PeerCredentials {
            uid: cred.uid,
            pid: Some(cred.pid),
        })
    }

    #[cfg(not(target_os = "linux"))]
    {
        let mut cred = libc::xucred {
            cr_version: 0,
            cr_uid: 0,
            cr_ngroups: 0,
            cr_groups: [0; 16],
        };
        let mut len = size_of::<libc::xucred>() as libc::socklen_t;
        // SAFETY: as above, with the BSD/macOS option and struct.
        #[allow(unsafe_code, reason = "reading kernel-recorded peer credentials")]
        let rc = unsafe {
            libc::getsockopt(
                fd,
                0, // SOL_LOCAL
                libc::LOCAL_PEERCRED,
                std::ptr::from_mut(&mut cred).cast(),
                &raw mut len,
            )
        };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        // LOCAL_PEERCRED does not report a pid; asking separately would be a
        // second syscall for something nothing uses.
        Ok(PeerCredentials {
            uid: cred.cr_uid,
            pid: None,
        })
    }
}

/// Connect to an instance's socket.
///
/// # Errors
/// The underlying connect error.
pub fn connect(path: impl AsRef<Path>) -> io::Result<UnixStream> {
    UnixStream::connect(path)
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;
    use crate::framing::{read_frame, write_frame};
    use omt_proto::FrameKind;

    fn socket_path(dir: &tempfile::TempDir, name: &str) -> PathBuf {
        dir.path().join(name)
    }

    #[test]
    fn a_socket_is_owner_only() {
        // A world-writable control socket is a shell for anyone with an account
        // on the machine.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = socket_path(&dir, "a.sock");
        let listener = SocketListener::bind(&path).expect("bind");
        let mode = std::fs::metadata(listener.path())
            .expect("stat")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, SOCKET_MODE, "socket must be 0600");
    }

    #[test]
    fn the_socket_is_removed_on_drop() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = socket_path(&dir, "b.sock");
        {
            let _listener = SocketListener::bind(&path).expect("bind");
            assert!(path.exists());
        }
        assert!(!path.exists(), "a leftover socket blocks the next start");
    }

    #[test]
    fn a_stale_socket_does_not_prevent_starting() {
        // A socket file outliving its process is the normal aftermath of a
        // crash; refusing to start would turn a crash into an outage.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = socket_path(&dir, "c.sock");
        std::fs::write(&path, b"").expect("leave a stale file");
        assert!(path.exists());
        let listener = SocketListener::bind(&path).expect("must replace a stale socket");
        assert_eq!(listener.path(), path);
    }

    #[test]
    fn a_live_socket_is_not_stolen() {
        // The other half: a *running* instance must not have its socket taken
        // out from under it by a second start.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = socket_path(&dir, "d.sock");
        let _first = SocketListener::bind(&path).expect("first bind");
        let err = SocketListener::bind(&path).expect_err("second bind must refuse");
        assert_eq!(err.kind(), io::ErrorKind::AddrInUse);
    }

    #[test]
    fn a_peer_is_identified_by_the_kernel() {
        // Identity a message cannot forge, which is why the local socket needs
        // no token.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = socket_path(&dir, "e.sock");
        let listener = SocketListener::bind(&path).expect("bind");

        let client = std::thread::spawn(move || {
            let mut stream = connect(&path).expect("connect");
            write_frame(&mut stream, FrameKind::Text, b"hi").expect("write");
        });

        let (mut stream, creds) = listener.accept().expect("accept");
        let (_, payload) = read_frame(&mut stream).expect("read");
        assert_eq!(payload, b"hi");
        assert!(creds.is_same_user(), "the peer is this user");
        assert_eq!(creds.uid, current_uid());

        client.join().expect("client thread");
    }

    #[test]
    fn frames_survive_a_real_socket() {
        // The framing tests use in-memory buffers; this asserts the same
        // property where a write is genuinely not a read.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = socket_path(&dir, "f.sock");
        let listener = SocketListener::bind(&path).expect("bind");

        let sent: Vec<Vec<u8>> = (0..20).map(|n| vec![n as u8; n * 37]).collect();
        let to_send = sent.clone();

        let client = std::thread::spawn(move || {
            let mut stream = connect(&path).expect("connect");
            for payload in &to_send {
                write_frame(&mut stream, FrameKind::Binary, payload).expect("write");
            }
        });

        let (mut stream, _) = listener.accept().expect("accept");
        for expected in &sent {
            let (kind, payload) = read_frame(&mut stream).expect("read");
            assert_eq!(kind, FrameKind::Binary);
            assert_eq!(&payload, expected);
        }
        client.join().expect("client thread");
    }
}
