//! Looking at what is actually running.
//!
//! This is the evidence behind agent detection tier 1: which process holds the
//! foreground of a pty, what it was invoked as, and what environment it was
//! given. All three are observations about a live process — nothing here infers
//! anything, and nothing here is told by the agent itself.

use std::io;
use std::os::fd::RawFd;
use std::path::PathBuf;

/// What could be learned about a process.
///
/// Every field is optional because every field comes from a platform interface
/// that may be restricted, and a partial answer is worth more than none: a pid
/// with no command line still tells the detector that *something* is in the
/// foreground.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProcessInfo {
    /// Its pid.
    pub pid: i32,
    /// Its parent's pid.
    pub ppid: Option<i32>,
    /// Argv, as the process was invoked.
    pub argv: Vec<String>,
    /// Its executable, where that could be resolved.
    pub exe: Option<PathBuf>,
    /// Its working directory.
    pub cwd: Option<PathBuf>,
}

impl ProcessInfo {
    /// The executable's base name, which is what a fingerprint matches on.
    #[must_use]
    pub fn command_name(&self) -> Option<&str> {
        self.exe
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .or_else(|| {
                self.argv
                    .first()
                    .map(|a| a.rsplit('/').next().unwrap_or(a.as_str()))
            })
    }
}

/// Which process group holds the foreground of a pty.
///
/// The question agent detection actually wants answered: not "what did the
/// session start" but "what is the user looking at *now*". A shell that
/// launched an agent is still the session's process; the agent is what has the
/// terminal.
///
/// # Errors
/// Fails if the descriptor is not a terminal or the call is refused.
pub fn foreground_process(fd: RawFd) -> io::Result<i32> {
    // SAFETY: tcgetpgrp takes a file descriptor and returns a pgid or -1; it
    // has no memory effects.
    #[allow(unsafe_code, reason = "no safe wrapper for tcgetpgrp")]
    let pgid = unsafe { libc::tcgetpgrp(fd) };
    if pgid < 0 {
        return Err(io::Error::last_os_error());
    }
    if pgid == 0 {
        // Zero is not a process group. It is what the kernel reports in the
        // window before anything has claimed the terminal, and returning it as
        // an answer would have the detector inspect pid 0.
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "nothing holds the foreground of this terminal yet",
        ));
    }
    Ok(pgid)
}

/// What can be learned about a process.
///
/// # Errors
/// Fails if the process does not exist or cannot be inspected.
pub fn process_info(pid: i32) -> io::Result<ProcessInfo> {
    #[cfg(target_os = "linux")]
    {
        linux::info(pid)
    }
    #[cfg(target_os = "macos")]
    {
        darwin::info(pid)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = pid;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "process inspection is implemented for Linux and macOS",
        ))
    }
}

/// A process's environment, as key/value pairs.
///
/// How a session claims a process it did not spawn: if `OMT_SESSION` is in
/// there, the correlation is direct rather than inferred.
///
/// # Errors
/// Fails if the environment cannot be read — which is the normal outcome for
/// another user's process, and on macOS for anything but our own children.
pub fn read_environ(pid: i32) -> io::Result<Vec<(String, String)>> {
    #[cfg(target_os = "linux")]
    {
        let raw = std::fs::read(format!("/proc/{pid}/environ"))?;
        Ok(split_environ(&raw))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        // macOS exposes this only through sysctl KERN_PROCARGS2, which is
        // restricted and reports argv and environ interleaved in a format with
        // no length prefixes. Returning an honest "cannot" beats a parser that
        // is wrong in ways nobody can see.
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "reading another process's environment is not available on this platform",
        ))
    }
}

#[cfg(target_os = "linux")]
fn split_environ(raw: &[u8]) -> Vec<(String, String)> {
    raw.split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .filter_map(|entry| {
            let text = String::from_utf8_lossy(entry);
            let (k, v) = text.split_once('=')?;
            Some((k.to_owned(), v.to_owned()))
        })
        .collect()
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{ProcessInfo, io};

    pub fn info(pid: i32) -> io::Result<ProcessInfo> {
        let base = format!("/proc/{pid}");
        let argv: Vec<String> = std::fs::read(format!("{base}/cmdline"))
            .map(|raw| {
                raw.split(|b| *b == 0)
                    .filter(|s| !s.is_empty())
                    .map(|s| String::from_utf8_lossy(s).into_owned())
                    .collect()
            })
            .unwrap_or_default();
        let exe = std::fs::read_link(format!("{base}/exe")).ok();
        let cwd = std::fs::read_link(format!("{base}/cwd")).ok();
        let ppid = std::fs::read_to_string(format!("{base}/status"))
            .ok()
            .and_then(|s| {
                s.lines()
                    .find_map(|l| l.strip_prefix("PPid:"))
                    .and_then(|v| v.trim().parse().ok())
            });
        if argv.is_empty() && exe.is_none() && ppid.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("no such process: {pid}"),
            ));
        }
        Ok(ProcessInfo {
            pid,
            ppid,
            argv,
            exe,
            cwd,
        })
    }
}

#[cfg(target_os = "macos")]
mod darwin {
    use super::{PathBuf, ProcessInfo, io};

    pub fn info(pid: i32) -> io::Result<ProcessInfo> {
        // proc_pidpath resolves the executable for any process the caller may
        // see. It is the one piece of this that is reliably available without
        // elevated privileges, which is why detection leans on the command
        // name rather than on argv here.
        let mut buf = vec![0u8; 4096];
        // SAFETY: proc_pidpath writes at most `buf.len()` bytes into `buf`.
        #[allow(unsafe_code, reason = "no safe wrapper for proc_pidpath")]
        let n = unsafe { proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
        if n <= 0 {
            return Err(io::Error::last_os_error());
        }
        buf.truncate(n as usize);
        let exe = PathBuf::from(String::from_utf8_lossy(&buf).into_owned());

        let ppid = parent_of(pid);
        Ok(ProcessInfo {
            pid,
            ppid,
            argv: Vec::new(),
            exe: Some(exe),
            cwd: None,
        })
    }

    fn parent_of(pid: i32) -> Option<i32> {
        // SAFETY: proc_bsdinfo is plain old data, so an all-zero value is a
        // valid one; proc_pidinfo overwrites it before it is read.
        #[allow(unsafe_code, reason = "zeroing a plain-old-data struct")]
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = size_of::<libc::proc_bsdinfo>() as i32;
        // SAFETY: proc_pidinfo writes `size` bytes into `info`, which is
        // exactly that large.
        #[allow(unsafe_code, reason = "no safe wrapper for proc_pidinfo")]
        let rc = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                std::ptr::from_mut(&mut info).cast(),
                size,
            )
        };
        (rc == size).then_some(info.pbi_ppid as i32)
    }

    #[allow(unsafe_code, reason = "declaring a libproc symbol libc does not bind")]
    unsafe extern "C" {
        fn proc_pidpath(pid: i32, buffer: *mut libc::c_void, buffersize: u32) -> i32;
    }
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
    fn this_process_can_describe_itself() {
        let me = std::process::id() as i32;
        let info = process_info(me).expect("inspect self");
        assert_eq!(info.pid, me);
        assert!(
            info.command_name().is_some(),
            "a fingerprint has something to match on: {info:?}"
        );
    }

    #[test]
    fn a_parent_is_reported() {
        let me = std::process::id() as i32;
        let info = process_info(me).expect("inspect self");
        assert!(info.ppid.is_some_and(|p| p > 0), "{info:?}");
    }

    #[test]
    fn a_process_that_does_not_exist_is_an_error_not_an_empty_answer() {
        // Returning a blank ProcessInfo would make the detector confidently
        // report "no agent" about a pid it never looked at.
        let err = process_info(-12345);
        assert!(err.is_err(), "{err:?}");
    }

    #[test]
    fn the_command_name_falls_back_to_argv() {
        // On macOS argv is not readable for most processes, so detection leans
        // on the executable; on Linux either may be the one that resolves.
        let info = ProcessInfo {
            pid: 1,
            argv: vec!["/usr/local/bin/claude".into(), "--flag".into()],
            ..ProcessInfo::default()
        };
        assert_eq!(info.command_name(), Some("claude"));
    }

    #[test]
    fn the_executable_wins_over_argv() {
        // argv[0] is whatever the parent felt like passing; the executable is
        // what is actually running.
        let info = ProcessInfo {
            pid: 1,
            argv: vec!["-sh".into()],
            exe: Some(PathBuf::from("/bin/zsh")),
            ..ProcessInfo::default()
        };
        assert_eq!(info.command_name(), Some("zsh"));
    }

    #[test]
    fn a_process_with_nothing_known_has_no_command_name() {
        assert_eq!(ProcessInfo::default().command_name(), None);
    }

    #[test]
    fn the_foreground_of_a_pty_is_the_process_on_it() {
        // The question detection actually asks: not what the session started,
        // but what has the terminal now.
        use crate::spawn::{Pty, PtyConfig};
        use std::path::PathBuf as P;
        let pty = Pty::spawn(&PtyConfig {
            program: P::from("/bin/sh"),
            args: vec!["-c".into(), "echo up; sleep 3".into()],
            ..PtyConfig::default()
        })
        .expect("spawn");
        // Wait for the child to actually be running: before it has claimed the
        // terminal there is genuinely no foreground to report, and asserting
        // otherwise would be asserting on a race.
        let mut r = pty.reader();
        let mut buf = [0u8; 64];
        use std::io::Read as _;
        let _ = r.read(&mut buf);
        let pgid = foreground_process(pty.as_raw_fd()).expect("foreground");
        assert_eq!(pgid, pty.pid(), "the child's own group holds the terminal");
    }

    #[test]
    fn asking_a_non_terminal_for_its_foreground_fails() {
        let f = std::fs::File::open("/dev/null").expect("open");
        use std::os::fd::AsRawFd;
        assert!(foreground_process(f.as_raw_fd()).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn an_injected_variable_is_visible_in_the_child_environment() {
        // How a session claims a process it did not spawn: a direct
        // correlation rather than an inferred one.
        use crate::spawn::{Pty, PtyConfig};
        let pty = Pty::spawn(&PtyConfig {
            program: std::path::PathBuf::from("/bin/sh"),
            args: vec!["-c".into(), "echo up; sleep 3".into()],
            env: vec![("OMT_SESSION".into(), "s-abc".into())],
            ..PtyConfig::default()
        })
        .expect("spawn");
        // Wait until the child has actually exec'd. Between fork and execve it
        // still carries *our* environment, so reading too early reports the
        // test runner's variables and calls it the child's.
        use std::io::Read as _;
        let mut buf = [0u8; 64];
        let _ = pty.reader().read(&mut buf);
        let env = read_environ(pty.pid()).expect("environ");
        assert!(
            env.iter().any(|(k, v)| k == "OMT_SESSION" && v == "s-abc"),
            "{env:?}"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_environment_parser_ignores_malformed_entries() {
        let raw = b"A=1\0notavariable\0B=2\0\0";
        let parsed = split_environ(raw);
        assert_eq!(
            parsed,
            [
                ("A".to_owned(), "1".to_owned()),
                ("B".to_owned(), "2".to_owned())
            ]
        );
    }
}
