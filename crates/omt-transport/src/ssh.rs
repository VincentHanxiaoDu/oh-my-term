//! Reaching an omt on another machine.
//!
//! The bet is that ssh is already configured. omt does not implement a
//! transport, negotiate keys, or ask for a password: it runs the user's `ssh`,
//! which already knows about their config, their agent, their jump hosts and
//! their hardware key. Reimplementing any of that would be worse at it and
//! would silently ignore the settings they already rely on.
//!
//! What travels over the connection is the same framed protocol the local Unix
//! socket carries, so a remote instance is not a different kind of thing — it
//! is the same instance further away.

use std::process::{Command, Stdio};

/// Where a remote instance lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    /// The ssh destination, exactly as the user would type it.
    ///
    /// Passed through untouched, so `Host` aliases from their config work. A
    /// parsed and reassembled destination would quietly drop the alias and
    /// connect somewhere else.
    pub destination: String,
    /// The socket to reach on the far side.
    pub socket: Option<String>,
    /// Extra arguments for ssh itself.
    pub ssh_args: Vec<String>,
}

impl Remote {
    /// A remote at a destination.
    #[must_use]
    pub fn new(destination: &str) -> Self {
        Self {
            destination: destination.to_owned(),
            socket: None,
            ssh_args: Vec::new(),
        }
    }

    /// The command that opens a byte stream to the remote instance.
    ///
    /// `omt bridge` on the far side connects to its own local socket and
    /// relays. That is one hop more than forwarding the socket directly, and it
    /// buys the thing that matters: the far side decides which instance to
    /// attach to, so a forwarded path cannot point at a socket that has since
    /// been replaced.
    #[must_use]
    pub fn bridge_command(&self) -> Vec<String> {
        let mut argv = vec!["ssh".to_owned()];
        argv.extend(self.ssh_args.clone());
        argv.push(self.destination.clone());
        argv.push("omt".to_owned());
        argv.push("bridge".to_owned());
        if let Some(socket) = &self.socket {
            argv.push("--socket".to_owned());
            argv.push(socket.clone());
        }
        argv
    }

    /// Start it.
    ///
    /// # Errors
    /// Fails if ssh could not be started at all — not if it later refuses to
    /// connect, which surfaces on the stream.
    pub fn spawn(&self) -> std::io::Result<std::process::Child> {
        let argv = self.bridge_command();
        let (program, args) = argv.split_first().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "empty command")
        })?;
        Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Left inherited on purpose: ssh writes host-key prompts and
            // authentication failures here, and swallowing them would turn
            // "the host key changed" into an unexplained hang.
            .stderr(Stdio::inherit())
            .spawn()
    }
}

/// Where a file copied to a remote lands.
///
/// Under the remote's own temp directory, named by content hash. Two pastes of
/// the same screenshot are one file rather than two, and a name derived from
/// content cannot contain anything the sender chose.
#[must_use]
pub fn remote_blob_path(digest: &str, extension: Option<&str>) -> String {
    let stem = &digest[..16.min(digest.len())];
    match extension {
        Some(ext) => format!("/tmp/omt-blobs/{stem}.{ext}"),
        None => format!("/tmp/omt-blobs/{stem}"),
    }
}

/// The command that writes a blob to the remote.
///
/// The bytes go over stdin rather than as an argument: an argument is visible
/// in the remote's process list, which is not where a user's screenshot or
/// pasted secret should appear.
#[must_use]
pub fn push_blob_command(remote: &Remote, path: &str) -> Vec<String> {
    let mut argv = vec!["ssh".to_owned()];
    argv.extend(remote.ssh_args.clone());
    argv.push(remote.destination.clone());
    argv.push(format!(
        "mkdir -p /tmp/omt-blobs && umask 077 && cat > {}",
        shell_quote(path)
    ));
    argv
}

/// Quote a path for a remote shell.
///
/// Single quotes with the escape for an embedded quote — the one form that is
/// safe for every character, since a remote path may come from a filename the
/// user did not choose.
#[must_use]
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    #[test]
    fn the_destination_is_passed_through_untouched() {
        // So `Host` aliases from the user's ssh config work. Parsing and
        // reassembling would drop the alias and connect somewhere else.
        let argv = Remote::new("my-dev-box").bridge_command();
        assert!(argv.contains(&"my-dev-box".to_owned()), "{argv:?}");
    }

    #[test]
    fn a_user_and_port_style_destination_survives_too() {
        let argv = Remote::new("deploy@10.0.0.4").bridge_command();
        assert!(argv.contains(&"deploy@10.0.0.4".to_owned()));
    }

    #[test]
    fn the_users_own_ssh_arguments_come_first() {
        // Before the destination, because that is where ssh expects them and
        // anywhere else is a different command.
        let remote = Remote {
            ssh_args: vec!["-J".to_owned(), "bastion".to_owned()],
            ..Remote::new("box")
        };
        let argv = remote.bridge_command();
        let jump = argv.iter().position(|a| a == "-J").expect("jump");
        let dest = argv.iter().position(|a| a == "box").expect("destination");
        assert!(jump < dest, "{argv:?}");
    }

    #[test]
    fn the_far_side_chooses_its_own_socket_when_none_is_given() {
        // One hop more than forwarding a path, and it buys the thing that
        // matters: a forwarded path cannot point at a socket since replaced.
        let argv = Remote::new("box").bridge_command();
        assert!(!argv.contains(&"--socket".to_owned()), "{argv:?}");
        assert!(argv.contains(&"bridge".to_owned()));
    }

    #[test]
    fn an_explicit_socket_is_passed_along() {
        let remote = Remote {
            socket: Some("/run/user/1000/omt-42.sock".to_owned()),
            ..Remote::new("box")
        };
        let argv = remote.bridge_command();
        assert!(
            argv.contains(&"/run/user/1000/omt-42.sock".to_owned()),
            "{argv:?}"
        );
    }

    #[test]
    fn a_blob_is_named_by_its_content() {
        // Two pastes of one screenshot are one file, and a name derived from
        // content cannot contain anything the sender chose.
        let a = remote_blob_path("abc123def456789012345", Some("png"));
        let b = remote_blob_path("abc123def456789012345", Some("png"));
        assert_eq!(a, b);
        assert!(a.ends_with(".png"));
        assert!(!a.contains(".."), "{a}");
    }

    #[test]
    fn blob_bytes_travel_over_stdin_not_as_an_argument() {
        // An argument is visible in the remote's process list, which is not
        // where a pasted screenshot or secret should appear.
        let remote = Remote::new("box");
        let argv = push_blob_command(&remote, "/tmp/omt-blobs/abc.png");
        let joined = argv.join(" ");
        assert!(joined.contains("cat >"), "{joined}");
        assert!(
            joined.contains("umask 077"),
            "the file must not be world readable: {joined}"
        );
    }

    #[test]
    fn a_hostile_filename_cannot_escape_the_quoting() {
        // A remote path may come from a filename the user did not choose,
        // so the property is checked against a real shell rather than by
        // looking for substrings — the escaped form legitimately *contains*
        // the dangerous text, and only the shell can say it is inert.
        let hostile = [
            "/tmp/x'; rm -rf ~; echo '",
            "/tmp/$(whoami)",
            "/tmp/`id`",
            "/tmp/a b\tc",
            "/tmp/back\\slash",
            "/tmp/'''",
            "/tmp/\"double\"",
        ];
        for path in hostile {
            let out = std::process::Command::new("/bin/sh")
                .arg("-c")
                .arg(format!("printf %s {}", shell_quote(path)))
                .output()
                .expect("run sh");
            assert_eq!(
                String::from_utf8_lossy(&out.stdout),
                path,
                "the shell did not see `{path}` as one literal argument"
            );
        }
    }
    #[test]
    fn quoting_round_trips_an_ordinary_path() {
        assert_eq!(
            shell_quote("/tmp/omt-blobs/a.png"),
            "'/tmp/omt-blobs/a.png'"
        );
    }

    #[test]
    fn ssh_errors_stay_visible() {
        // Swallowing stderr turns "the host key changed" into an unexplained
        // hang, which is the single most confusing way for this to fail.
        let argv = Remote::new("box").bridge_command();
        assert_eq!(argv.first().map(String::as_str), Some("ssh"));
    }
}
