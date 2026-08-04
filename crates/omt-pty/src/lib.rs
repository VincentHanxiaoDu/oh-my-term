//! PTY lifecycle on Unix — macOS and Linux, which is the whole of v1's target.
//!
//! The abstraction is [`Pty`], so a native-Windows ConPTY backend stays
//! implementable behind it later. Nothing here is written against ConPTY's
//! lifecycle model: no `SIGWINCH`, no process group, no `TIOCSWINSZ` leaks into
//! the public surface.

#![cfg(unix)]

mod process;
mod spawn;

pub use process::{ProcessInfo, foreground_process, process_info, read_environ};
pub use spawn::{ExitStatus, Pty, PtyConfig, PtyError, PtySize, Signal};
