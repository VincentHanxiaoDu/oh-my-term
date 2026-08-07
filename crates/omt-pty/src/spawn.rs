//! Opening a PTY and putting a process on the far end of it.

use std::ffi::{CString, OsStr};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

/// How big a terminal is, in both units the kernel tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtySize {
    /// Columns.
    pub cols: u16,
    /// Rows.
    pub rows: u16,
    /// Pixel width, for programs that draw images.
    pub pixel_width: u16,
    /// Pixel height.
    pub pixel_height: u16,
}

impl Default for PtySize {
    fn default() -> Self {
        Self {
            cols: 80,
            rows: 24,
            pixel_width: 0,
            pixel_height: 0,
        }
    }
}

impl PtySize {
    /// A size in characters, with the pixel dimensions left unset.
    #[must_use]
    pub const fn new(cols: u16, rows: u16) -> Self {
        Self {
            cols,
            rows,
            pixel_width: 0,
            pixel_height: 0,
        }
    }
}

/// What to spawn, and in what world.
#[derive(Debug, Clone, Default)]
pub struct PtyConfig {
    /// The program.
    pub program: PathBuf,
    /// Its arguments, not including the program name.
    pub args: Vec<String>,
    /// Working directory.
    pub cwd: Option<PathBuf>,
    /// Variables to add or override.
    ///
    /// Added rather than replacing the environment wholesale: a shell spawned
    /// without PATH or HOME is a shell that cannot do anything, and the caller
    /// almost never means that.
    pub env: Vec<(String, String)>,
    /// Variables to remove.
    pub env_remove: Vec<String>,
    /// The starting size.
    pub size: PtySize,
}

/// What went wrong.
#[derive(Debug, thiserror::Error)]
pub enum PtyError {
    /// The kernel refused to give us a pty.
    #[error("could not open a pty: {0}")]
    Open(io::Error),
    /// The child could not be created.
    #[error("could not spawn `{program}`: {source}")]
    Spawn {
        /// What was being spawned.
        program: String,
        /// Why it failed.
        source: io::Error,
    },
    /// An ioctl or wait failed.
    #[error("pty operation failed: {0}")]
    Io(#[from] io::Error),
    /// A path or variable contained a NUL, which C strings cannot carry.
    #[error("`{what}` contains an interior NUL byte")]
    InteriorNul {
        /// Which value.
        what: String,
    },
}

/// How a child ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitStatus {
    /// It returned this code.
    Code(i32),
    /// A signal killed it.
    Signal(i32),
}

impl ExitStatus {
    /// Whether this counts as success.
    #[must_use]
    pub const fn is_success(self) -> bool {
        matches!(self, Self::Code(0))
    }
}

/// The signals worth naming.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// Interrupt — what Ctrl-C sends.
    Int,
    /// Quit.
    Quit,
    /// Terminate.
    Term,
    /// Kill, which cannot be caught.
    Kill,
    /// Hang up.
    Hup,
}

impl Signal {
    const fn number(self) -> i32 {
        match self {
            Self::Int => libc::SIGINT,
            Self::Quit => libc::SIGQUIT,
            Self::Term => libc::SIGTERM,
            Self::Kill => libc::SIGKILL,
            Self::Hup => libc::SIGHUP,
        }
    }
}

/// A pty with a process on the far end.
#[derive(Debug)]
pub struct Pty {
    master: OwnedFd,
    pid: libc::pid_t,
    size: PtySize,
    reaped: Option<ExitStatus>,
}

impl Pty {
    /// Open a pty and spawn a process on it.
    ///
    /// # Errors
    /// Fails if no pty is available, if the program cannot be executed, or if
    /// any path or variable contains an interior NUL.
    pub fn spawn(config: &PtyConfig) -> Result<Self, PtyError> {
        let (master, slave) = open_pty(config.size)?;

        let program = cstring(config.program.as_os_str(), "program")?;
        let mut argv: Vec<CString> = vec![program.clone()];
        for a in &config.args {
            argv.push(cstring(OsStr::new(a), a)?);
        }
        let envp = build_env(config)?;
        let cwd = config
            .cwd
            .as_ref()
            .map(|p| cstring(p.as_os_str(), "cwd"))
            .transpose()?;

        // Everything that can allocate or fail is done *before* the fork. The
        // child half of a fork in a process that may have other threads may
        // only call async-signal-safe functions; allocating there is how a
        // spawn deadlocks once in a thousand runs and nobody can reproduce it.
        let argv_ptrs: Vec<*const libc::c_char> = argv
            .iter()
            .map(|c| c.as_ptr())
            .chain(std::iter::once(std::ptr::null()))
            .collect();
        let envp_ptrs: Vec<*const libc::c_char> = envp
            .iter()
            .map(|c| c.as_ptr())
            .chain(std::iter::once(std::ptr::null()))
            .collect();

        // SAFETY: fork itself has no preconditions. What follows in the child
        // is restricted to async-signal-safe calls, per the comment above.
        #[allow(unsafe_code, reason = "spawning onto a pty requires fork/exec")]
        let pid = unsafe { libc::fork() };
        if pid < 0 {
            return Err(PtyError::Spawn {
                program: config.program.display().to_string(),
                source: io::Error::last_os_error(),
            });
        }

        if pid == 0 {
            // SAFETY: the child of a fork, calling only async-signal-safe
            // functions, and ending in exec or _exit — it never returns.
            #[allow(unsafe_code, reason = "the child half of fork/exec")]
            unsafe {
                child_exec(
                    slave.as_raw_fd(),
                    master.as_raw_fd(),
                    &program,
                    &argv_ptrs,
                    &envp_ptrs,
                    cwd.as_ref(),
                );
            }
        }

        // Deliberately *not* setpgid(pid, pid) here. The usual double-call
        // trick for closing the fork race is wrong when the child calls
        // setsid: making it a process-group leader first is exactly the
        // condition under which setsid fails, and the child would end up with
        // no controlling terminal at all. A session leader's group id is its
        // pid anyway, so the group the parent would have created is the one
        // the child creates a moment later.

        // The parent has no use for the slave end; holding it open would mean
        // reads never see EOF when the child exits, which is the classic
        // "terminal hangs after the shell quits" bug.
        drop(slave);

        Ok(Self {
            master,
            pid,
            size: config.size,
            reaped: None,
        })
    }

    /// The child's process id.
    #[must_use]
    pub const fn pid(&self) -> i32 {
        self.pid
    }

    /// The master file descriptor, for a poll loop.
    #[must_use]
    pub fn as_raw_fd(&self) -> RawFd {
        self.master.as_raw_fd()
    }

    /// The size the kernel currently believes.
    #[must_use]
    pub const fn size(&self) -> PtySize {
        self.size
    }

    /// Tell the kernel the terminal changed size.
    ///
    /// This is what sends `SIGWINCH` to the foreground process group; there is
    /// no separate step, and doing it any other way would tell a program the
    /// size changed while the kernel still disagreed.
    ///
    /// # Errors
    /// Fails if the ioctl does.
    pub fn resize(&mut self, size: PtySize) -> Result<(), PtyError> {
        let ws = libc::winsize {
            ws_row: size.rows,
            ws_col: size.cols,
            ws_xpixel: size.pixel_width,
            ws_ypixel: size.pixel_height,
        };
        // SAFETY: `master` is an open pty fd owned by self, and `ws` is the
        // struct TIOCSWINSZ expects.
        #[allow(unsafe_code, reason = "TIOCSWINSZ has no safe wrapper in std")]
        let rc = unsafe { libc::ioctl(self.master.as_raw_fd(), TIOCSWINSZ, &raw const ws) };
        if rc != 0 {
            return Err(PtyError::Io(io::Error::last_os_error()));
        }
        self.size = size;
        Ok(())
    }

    /// Send a signal to the child's *process group*.
    ///
    /// The group, not the process: Ctrl-C in a real terminal interrupts the
    /// whole foreground job, and signalling only the shell would leave the
    /// command it is running untouched.
    ///
    /// # Errors
    /// Fails if the child is gone or the signal cannot be delivered.
    pub fn signal(&self, sig: Signal) -> Result<(), PtyError> {
        // SAFETY: killpg with a valid pgid and signal number; failure is
        // reported through errno rather than by any unsafe effect.
        #[allow(unsafe_code, reason = "no safe wrapper for signalling a group")]
        let rc = unsafe { libc::killpg(self.pid, sig.number()) };
        if rc == 0 {
            return Ok(());
        }
        let err = io::Error::last_os_error();
        if err.raw_os_error() != Some(libc::ESRCH) {
            return Err(PtyError::Io(err));
        }
        // The group does not exist yet: the child has been forked but has not
        // reached setsid. Signalling the process directly is the same thing
        // during that window, because it is the only member the group will
        // have. Without this fallback a signal sent immediately after spawn
        // fails for a reason no caller could have anticipated or retried on.
        // SAFETY: kill with a known child pid and a valid signal number.
        #[allow(unsafe_code, reason = "no safe wrapper for signalling a pid")]
        let rc = unsafe { libc::kill(self.pid, sig.number()) };
        if rc != 0 {
            return Err(PtyError::Io(io::Error::last_os_error()));
        }
        Ok(())
    }

    /// Whether the child has exited, without blocking.
    ///
    /// # Errors
    /// Fails if `waitpid` does for a reason other than the child still running.
    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>, PtyError> {
        if let Some(s) = self.reaped {
            // Reaping twice would report a wrong status the second time, since
            // the pid may by then belong to something else entirely.
            return Ok(Some(s));
        }
        let mut status = 0;
        // SAFETY: waitpid with a valid pid and a pointer to local storage.
        #[allow(unsafe_code, reason = "no non-blocking wait in std for a raw pid")]
        let rc = unsafe { libc::waitpid(self.pid, &raw mut status, libc::WNOHANG) };
        match rc {
            0 => Ok(None),
            n if n == self.pid => {
                let s = decode_status(status);
                self.reaped = Some(s);
                Ok(Some(s))
            }
            _ => Err(PtyError::Io(io::Error::last_os_error())),
        }
    }

    /// Wait for the child to exit.
    ///
    /// # Errors
    /// Fails if `waitpid` does.
    pub fn wait(&mut self) -> Result<ExitStatus, PtyError> {
        if let Some(s) = self.reaped {
            return Ok(s);
        }
        let mut status = 0;
        // SAFETY: as `try_wait`, blocking.
        #[allow(unsafe_code, reason = "no blocking wait in std for a raw pid")]
        let rc = unsafe { libc::waitpid(self.pid, &raw mut status, 0) };
        if rc != self.pid {
            return Err(PtyError::Io(io::Error::last_os_error()));
        }
        let s = decode_status(status);
        self.reaped = Some(s);
        Ok(s)
    }

    /// Stop reads from blocking when there is nothing to read.
    ///
    /// Required by any caller that polls rather than dedicating a thread: a
    /// blocking read on an idle session holds the whole loop, so the terminal
    /// stops responding to the keyboard until the program happens to print
    /// something. That is indistinguishable from a hang.
    ///
    /// # Errors
    /// Fails if the descriptor's flags cannot be read or set.
    pub fn set_nonblocking(&self, on: bool) -> Result<(), PtyError> {
        let fd = self.master.as_raw_fd();
        // SAFETY: `fd` is an open descriptor owned by self; F_GETFL takes no
        // further arguments and reports through errno.
        #[allow(unsafe_code, reason = "no safe wrapper for fcntl")]
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 {
            return Err(PtyError::Io(io::Error::last_os_error()));
        }
        let updated = if on {
            flags | libc::O_NONBLOCK
        } else {
            flags & !libc::O_NONBLOCK
        };
        // SAFETY: as above, setting the flags just read back.
        #[allow(unsafe_code, reason = "no safe wrapper for fcntl")]
        let rc = unsafe { libc::fcntl(fd, libc::F_SETFL, updated) };
        if rc < 0 {
            return Err(PtyError::Io(io::Error::last_os_error()));
        }
        Ok(())
    }

    /// A reader over the master end.
    #[must_use]
    pub fn reader(&self) -> PtyIo {
        PtyIo {
            fd: self.master.as_raw_fd(),
        }
    }

    /// A writer over the master end.
    #[must_use]
    pub fn writer(&self) -> PtyIo {
        PtyIo {
            fd: self.master.as_raw_fd(),
        }
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        if self.reaped.is_some() {
            return;
        }

        // Hanging up is what closing a window means. But a `SIGHUP` followed by
        // a single non-blocking wait is not enough: the child has not exited
        // yet at that instant, so it is never reaped, and it stays a zombie for
        // the life of the daemon. A long-running instance that opens and closes
        // sessions all day would accumulate one per session.
        let _ = self.signal(Signal::Hup);

        // Give it a moment to go on its own. Bounded, because dropping a value
        // must not be able to block a session tree for an arbitrary time.
        let deadline = std::time::Instant::now() + HANGUP_GRACE;
        while std::time::Instant::now() < deadline {
            match self.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(2)),
                Err(_) => return,
            }
        }

        // It ignored the hangup, or is still shutting down. `SIGKILL` cannot be
        // caught, so the blocking wait after it is bounded by the kernel rather
        // than by the program's cooperation.
        let _ = self.signal(Signal::Kill);
        let _ = self.wait();
    }
}

/// How long a child gets to exit on its own after a hangup.
///
/// Short: this runs inside `Drop`, and a session tree must not be blocked for
/// an arbitrary time because one program is slow to notice.
const HANGUP_GRACE: std::time::Duration = std::time::Duration::from_millis(200);

/// Read and write halves of the master end.
///
/// Borrowed from the [`Pty`] rather than owning the descriptor, so closing is
/// always the pty's decision and never a stray handle's.
#[derive(Debug, Clone, Copy)]
pub struct PtyIo {
    fd: RawFd,
}

impl Read for PtyIo {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // SAFETY: `fd` is an open pty master borrowed from the Pty, and `buf`
        // is valid for `buf.len()` bytes.
        #[allow(unsafe_code, reason = "reading a borrowed raw fd")]
        let n = unsafe { libc::read(self.fd, buf.as_mut_ptr().cast(), buf.len()) };
        if n < 0 {
            let err = io::Error::last_os_error();
            // When the child exits, the master reports EIO rather than EOF on
            // Linux. Reporting that as an error would make every clean exit
            // look like a failure.
            if err.raw_os_error() == Some(libc::EIO) {
                return Ok(0);
            }
            return Err(err);
        }
        Ok(n as usize)
    }
}

impl Write for PtyIo {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // SAFETY: as `read`, with a shared buffer.
        #[allow(unsafe_code, reason = "writing a borrowed raw fd")]
        let n = unsafe { libc::write(self.fd, buf.as_ptr().cast(), buf.len()) };
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(n as usize)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(target_os = "linux")]
const TIOCSWINSZ: libc::c_ulong = libc::TIOCSWINSZ;
#[cfg(not(target_os = "linux"))]
const TIOCSWINSZ: libc::c_ulong = libc::TIOCSWINSZ as libc::c_ulong;

fn decode_status(status: i32) -> ExitStatus {
    if libc::WIFSIGNALED(status) {
        ExitStatus::Signal(libc::WTERMSIG(status))
    } else {
        ExitStatus::Code(libc::WEXITSTATUS(status))
    }
}

fn cstring(s: &OsStr, what: &str) -> Result<CString, PtyError> {
    CString::new(s.as_bytes()).map_err(|_| PtyError::InteriorNul {
        what: what.to_owned(),
    })
}

fn build_env(config: &PtyConfig) -> Result<Vec<CString>, PtyError> {
    let mut vars: std::collections::BTreeMap<String, String> = std::env::vars().collect();
    for k in &config.env_remove {
        vars.remove(k);
    }
    for (k, v) in &config.env {
        vars.insert(k.clone(), v.clone());
    }
    // A terminal that does not say what it is leaves every curses program
    // guessing, and most guess wrong.
    vars.entry("TERM".to_owned())
        .or_insert_with(|| "xterm-256color".to_owned());
    vars.into_iter()
        .map(|(k, v)| {
            CString::new(format!("{k}={v}")).map_err(|_| PtyError::InteriorNul { what: k })
        })
        .collect()
}

fn open_pty(size: PtySize) -> Result<(OwnedFd, OwnedFd), PtyError> {
    let mut master: RawFd = -1;
    let mut slave: RawFd = -1;
    let mut ws = libc::winsize {
        ws_row: size.rows,
        ws_col: size.cols,
        ws_xpixel: size.pixel_width,
        ws_ypixel: size.pixel_height,
    };
    // SAFETY: openpty writes two descriptors into the locals; the name
    // argument is null because we do not need the slave's path, and the
    // termios argument is null to take the default line discipline.
    #[allow(unsafe_code, reason = "openpty is the portable way to get a pty pair")]
    let rc = unsafe {
        libc::openpty(
            &raw mut master,
            &raw mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &raw mut ws,
        )
    };
    if rc != 0 {
        return Err(PtyError::Open(io::Error::last_os_error()));
    }
    // SAFETY: both are freshly opened descriptors this call now owns.
    #[allow(unsafe_code, reason = "taking ownership of the new descriptors")]
    unsafe {
        Ok((OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)))
    }
}

/// The child half of the fork. Never returns.
///
/// # Safety
/// Must be called only in the child of a `fork`, and calls only
/// async-signal-safe functions.
#[allow(unsafe_code, reason = "the child half of fork/exec")]
unsafe fn child_exec(
    slave: RawFd,
    master: RawFd,
    program: &CString,
    argv: &[*const libc::c_char],
    envp: &[*const libc::c_char],
    cwd: Option<&CString>,
) -> ! {
    unsafe {
        libc::close(master);

        // setsid, TIOCSCTTY and the three dup2s in one call. Doing them by
        // hand works on Linux and quietly does not on macOS, where the
        // controlling-terminal claim has to happen in the order login_tty uses
        // — and a pty with no controlling terminal has no job control at all:
        // no Ctrl-C, no SIGWINCH, no foreground process group. It fails
        // silently, which is the worst way for it to fail.
        if libc::login_tty(slave) != 0 {
            libc::_exit(125);
        }

        // Everything above stderr is closed. A child inherits every descriptor
        // its parent had open, and omt's parent is a daemon holding client
        // sockets and listeners — so a shell spawned here keeps them open on
        // the child's side, and a client that disconnects never produces the
        // EOF the server is waiting for. The server then holds that connection
        // for as long as the shell lives, which is forever.
        //
        // A loop rather than `closefrom`: that is not available on every
        // platform omt builds for, and `close` on a descriptor that was never
        // open simply fails, which is fine here. The bound is the soft limit,
        // because that is the highest number the parent could have opened.
        let mut limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        let highest = if libc::getrlimit(libc::RLIMIT_NOFILE, &raw mut limit) == 0 {
            // Capped: a system with an enormous limit would otherwise spend
            // millions of syscalls here, in the child, before every exec.
            (limit.rlim_cur as libc::c_int).min(4096)
        } else {
            1024
        };
        for fd in 3..highest {
            libc::close(fd);
        }

        if let Some(dir) = cwd
            && libc::chdir(dir.as_ptr()) != 0
        {
            // A missing working directory is the caller's mistake, and
            // starting in an unexpected one silently is worse than not
            // starting.
            libc::_exit(126);
        }

        // Signal dispositions are inherited across exec; a shell that starts
        // with SIGINT ignored can never be interrupted.
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
        libc::signal(libc::SIGINT, libc::SIG_DFL);
        libc::signal(libc::SIGQUIT, libc::SIG_DFL);
        libc::signal(libc::SIGTERM, libc::SIG_DFL);
        libc::signal(libc::SIGCHLD, libc::SIG_DFL);

        libc::execve(program.as_ptr(), argv.as_ptr(), envp.as_ptr());
        // exec only returns on failure. 127 is what a shell reports for
        // "command not found", which is what this is.
        libc::_exit(127);
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {

    #[test]
    fn a_child_does_not_inherit_the_parents_descriptors() {
        // The bug this caught: a shell spawned by the daemon kept the daemon's
        // client sockets open, so a client that disconnected never produced the
        // EOF the server was waiting for — and the server held that connection
        // for as long as the shell lived. Which is forever, for a shell.
        //
        // A pipe is used as the witness. If the child inherits the write end,
        // reading from this end blocks instead of returning EOF.
        let mut fds = [0 as libc::c_int; 2];
        #[allow(unsafe_code, reason = "pipe has no safe wrapper that keeps raw fds")]
        let rc = unsafe { libc::pipe(fds.as_mut_ptr()) };
        assert_eq!(rc, 0, "pipe");
        let (read_end, write_end) = (fds[0], fds[1]);

        let mut pty = Pty::spawn(&sh("sleep 30")).expect("spawn");

        // The parent's own copy goes; only the child could still hold one.
        #[allow(unsafe_code, reason = "closing a raw fd this test opened")]
        unsafe {
            libc::close(write_end);
        }

        // Polled with a deadline rather than read immediately: the child may
        // still be between fork and exec, and a bare non-blocking read would
        // race it and report a descriptor it is about to close.
        let mut poll = libc::pollfd {
            fd: read_end,
            events: libc::POLLIN,
            revents: 0,
        };
        #[allow(unsafe_code, reason = "poll has no safe wrapper for raw fds")]
        let ready = unsafe { libc::poll(&raw mut poll, 1, 2_000) };

        // EOF is readable, so a ready descriptor means the write end is gone
        // everywhere. A timeout means somebody still holds it — which is the
        // failure this test exists for.
        let n = if ready > 0 {
            let mut byte = [0u8; 1];
            #[allow(unsafe_code, reason = "reading a raw fd this test opened")]
            unsafe {
                libc::read(read_end, byte.as_mut_ptr().cast(), 1)
            }
        } else {
            -1
        };

        #[allow(unsafe_code, reason = "closing a raw fd this test opened")]
        unsafe {
            libc::close(read_end);
        }
        pty.signal(Signal::Kill).ok();
        pty.wait().ok();

        assert_eq!(
            n, 0,
            "the child inherited a descriptor: read returned {n} rather than EOF"
        );
    }

    use super::*;
    use std::time::{Duration, Instant};

    fn sh(script: &str) -> PtyConfig {
        PtyConfig {
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".into(), script.into()],
            size: PtySize::new(80, 24),
            ..PtyConfig::default()
        }
    }

    /// Read until the pattern shows up or the deadline passes.
    fn read_until(pty: &Pty, needle: &str, within: Duration) -> String {
        let mut r = pty.reader();
        let mut out = String::new();
        let deadline = Instant::now() + within;
        let mut buf = [0u8; 4096];
        while Instant::now() < deadline {
            match r.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    out.push_str(&String::from_utf8_lossy(&buf[..n]));
                    if out.contains(needle) {
                        break;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        out
    }

    #[test]
    fn a_process_runs_and_its_output_comes_back() {
        let pty = Pty::spawn(&sh("echo hello-from-the-pty")).expect("spawn");
        let out = read_until(&pty, "hello-from-the-pty", Duration::from_secs(5));
        assert!(out.contains("hello-from-the-pty"), "{out:?}");
    }

    #[test]
    fn the_exit_code_comes_back() {
        let mut pty = Pty::spawn(&sh("exit 3")).expect("spawn");
        assert_eq!(pty.wait().expect("wait"), ExitStatus::Code(3));
        assert!(!ExitStatus::Code(3).is_success());
    }

    #[test]
    fn waiting_twice_gives_the_same_answer() {
        // The pid may belong to something else by the second call, so the
        // status has to be remembered rather than asked for again.
        let mut pty = Pty::spawn(&sh("exit 7")).expect("spawn");
        let first = pty.wait().expect("wait");
        let second = pty.wait().expect("wait again");
        assert_eq!(first, second);
        assert_eq!(first, ExitStatus::Code(7));
    }

    #[test]
    fn a_running_child_is_not_reported_as_exited() {
        let mut pty = Pty::spawn(&sh("sleep 5")).expect("spawn");
        assert_eq!(pty.try_wait().expect("try_wait"), None);
        pty.signal(Signal::Kill).expect("kill");
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if pty.try_wait().expect("try_wait").is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the child never exited");
    }

    #[test]
    fn the_child_gets_a_controlling_terminal() {
        // Without it there is no job control: no Ctrl-C, no SIGWINCH, no
        // foreground process group. `tty` says whether the claim worked.
        let pty = Pty::spawn(&sh("tty")).expect("spawn");
        let out = read_until(&pty, "/dev/", Duration::from_secs(5));
        assert!(out.contains("/dev/"), "not a terminal: {out:?}");
        assert!(!out.contains("not a tty"), "{out:?}");
    }

    #[test]
    fn the_size_reaches_the_child() {
        let pty = Pty::spawn(&PtyConfig {
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".into(), "stty size".into()],
            size: PtySize::new(100, 40),
            ..PtyConfig::default()
        })
        .expect("spawn");
        let out = read_until(&pty, "40", Duration::from_secs(5));
        assert!(out.contains("40 100"), "stty said: {out:?}");
    }

    #[test]
    fn resizing_is_visible_to_the_child() {
        // The ioctl is what sends SIGWINCH; there is no separate step, and
        // telling the program before the kernel agreed would be a lie.
        let mut pty = Pty::spawn(&sh(
            "trap 'stty size' WINCH; echo ready; for i in 1 2 3 4 5 6 7 8 9 10; do sleep 0.2; done",
        ))
        .expect("spawn");
        read_until(&pty, "ready", Duration::from_secs(5));
        pty.resize(PtySize::new(120, 50)).expect("resize");
        let out = read_until(&pty, "50 120", Duration::from_secs(5));
        assert!(out.contains("50 120"), "after SIGWINCH: {out:?}");
        assert_eq!(pty.size(), PtySize::new(120, 50));
    }

    #[test]
    fn input_written_to_the_pty_reaches_the_child() {
        let pty = Pty::spawn(&sh("read line; echo got:$line")).expect("spawn");
        pty.writer().write_all(b"typed\n").expect("write");
        let out = read_until(&pty, "got:typed", Duration::from_secs(5));
        assert!(out.contains("got:typed"), "{out:?}");
    }

    #[test]
    fn a_signal_sent_immediately_after_spawn_still_arrives() {
        // The window between fork and setsid: the process group does not exist
        // yet, and a caller that spawned and instantly cancelled would
        // otherwise get ESRCH for a child that is very much alive.
        let mut pty = Pty::spawn(&sh("sleep 30")).expect("spawn");
        pty.signal(Signal::Kill).expect("signal right after spawn");
        assert!(!pty.wait().expect("wait").is_success());
    }

    #[test]
    fn an_interrupt_reaches_the_whole_foreground_group() {
        // Signalling only the shell would leave the command it is running
        // untouched, which is not what Ctrl-C means to anybody.
        let mut pty = Pty::spawn(&sh("echo ready; sleep 30")).expect("spawn");
        read_until(&pty, "ready", Duration::from_secs(5));
        pty.signal(Signal::Term).expect("signal");
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Some(status) = pty.try_wait().expect("try_wait") {
                assert!(!status.is_success(), "{status:?}");
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the sleep survived a SIGTERM to its group");
    }

    #[test]
    fn injected_environment_reaches_the_child() {
        // How the hook finds its way home: the session id is injected here.
        let pty = Pty::spawn(&PtyConfig {
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".into(), "echo v=$OMT_SESSION".into()],
            env: vec![("OMT_SESSION".into(), "s-123".into())],
            ..PtyConfig::default()
        })
        .expect("spawn");
        let out = read_until(&pty, "v=s-123", Duration::from_secs(5));
        assert!(out.contains("v=s-123"), "{out:?}");
    }

    #[test]
    fn the_rest_of_the_environment_survives_injection() {
        // Replacing the environment wholesale gives a shell with no PATH,
        // which can do nothing at all.
        let pty = Pty::spawn(&PtyConfig {
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".into(), "echo path=${PATH:+set}".into()],
            env: vec![("OMT_SESSION".into(), "s-1".into())],
            ..PtyConfig::default()
        })
        .expect("spawn");
        let out = read_until(&pty, "path=set", Duration::from_secs(5));
        assert!(out.contains("path=set"), "{out:?}");
    }

    #[test]
    fn a_variable_can_be_removed() {
        let pty = Pty::spawn(&PtyConfig {
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".into(), "echo term=[${TERM:-unset}]".into()],
            env_remove: vec!["TERM".into()],
            ..PtyConfig::default()
        })
        .expect("spawn");
        let out = read_until(&pty, "term=", Duration::from_secs(5));
        // TERM is re-added with a default, because a terminal that does not
        // say what it is leaves every curses program guessing.
        assert!(out.contains("xterm-256color"), "{out:?}");
    }

    #[test]
    fn the_working_directory_is_honoured() {
        let pty = Pty::spawn(&PtyConfig {
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".into(), "pwd".into()],
            cwd: Some(PathBuf::from("/tmp")),
            ..PtyConfig::default()
        })
        .expect("spawn");
        let out = read_until(&pty, "tmp", Duration::from_secs(5));
        assert!(out.contains("tmp"), "{out:?}");
    }

    #[test]
    fn a_missing_program_exits_with_the_shell_convention() {
        // 127 is what a shell reports for "command not found", and reporting
        // anything else would make a typo look like a crash.
        let mut pty = Pty::spawn(&PtyConfig {
            program: PathBuf::from("/nonexistent/definitely-not-here"),
            ..PtyConfig::default()
        })
        .expect("fork succeeds; the exec is what fails");
        assert_eq!(pty.wait().expect("wait"), ExitStatus::Code(127));
    }

    #[test]
    fn a_missing_working_directory_refuses_to_start() {
        // Starting somewhere unexpected is worse than not starting.
        let mut pty = Pty::spawn(&PtyConfig {
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".into(), "echo should-not-run".into()],
            cwd: Some(PathBuf::from("/nonexistent/nowhere")),
            ..PtyConfig::default()
        })
        .expect("spawn");
        assert_eq!(pty.wait().expect("wait"), ExitStatus::Code(126));
    }

    #[test]
    fn a_path_with_an_interior_nul_is_refused_rather_than_truncated() {
        // Truncating it would run a different program than the caller named.
        use std::os::unix::ffi::OsStringExt;
        let bad = std::ffi::OsString::from_vec(b"/bin/sh\0extra".to_vec());
        let err = Pty::spawn(&PtyConfig {
            program: PathBuf::from(bad),
            ..PtyConfig::default()
        })
        .expect_err("must refuse");
        assert!(matches!(err, PtyError::InteriorNul { .. }), "{err:?}");
    }

    #[test]
    fn a_nonblocking_read_returns_rather_than_waiting_for_output() {
        // The property a polling loop depends on. A blocking read on an idle
        // session holds the loop, and the terminal stops responding to the
        // keyboard until the program happens to print — which is
        // indistinguishable from a hang.
        let pty = Pty::spawn(&sh("sleep 5")).expect("spawn");
        pty.set_nonblocking(true).expect("nonblocking");

        let started = Instant::now();
        let mut r = pty.reader();
        let mut buf = [0u8; 64];
        let result = r.read(&mut buf);
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "the read blocked for {:?}",
            started.elapsed()
        );
        match result {
            Err(e) => assert_eq!(e.kind(), io::ErrorKind::WouldBlock, "{e:?}"),
            Ok(n) => assert_eq!(n, 0, "an idle pty produced {n} bytes"),
        }
    }

    #[test]
    fn a_nonblocking_pty_still_delivers_what_the_program_wrote() {
        let pty = Pty::spawn(&sh("echo present; sleep 5")).expect("spawn");
        pty.set_nonblocking(true).expect("nonblocking");
        let mut r = pty.reader();
        let mut out = String::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline && !out.contains("present") {
            let mut buf = [0u8; 1024];
            match r.read(&mut buf) {
                Ok(n) if n > 0 => out.push_str(&String::from_utf8_lossy(&buf[..n])),
                _ => std::thread::sleep(Duration::from_millis(10)),
            }
        }
        assert!(out.contains("present"), "{out:?}");
    }

    #[test]
    fn reading_after_the_child_exits_reports_end_of_file() {
        // On Linux the master reports EIO here rather than EOF; surfacing that
        // as an error would make every clean exit look like a failure.
        let mut pty = Pty::spawn(&sh("echo bye")).expect("spawn");
        read_until(&pty, "bye", Duration::from_secs(5));
        pty.wait().expect("wait");
        let mut r = pty.reader();
        let mut buf = [0u8; 64];
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match r.read(&mut buf) {
                Ok(0) => return,
                Ok(_) => {}
                Err(e) => panic!("expected EOF, got {e}"),
            }
            assert!(Instant::now() < deadline, "never reached EOF");
        }
    }

    #[test]
    fn dropping_a_pty_reaps_its_child_rather_than_leaving_a_zombie() {
        // A hangup plus one non-blocking wait leaves the child unreaped: it has
        // not exited yet at that instant. A daemon opening and closing sessions
        // all day would accumulate one zombie per session.
        let pid = {
            // Announces itself, so the drop races a genuinely running child the
            // way a real session close does — rather than one still in exec.
            let pty = Pty::spawn(&sh("echo up; sleep 30")).expect("spawn");
            let pid = pty.pid();
            read_until(&pty, "up", Duration::from_secs(5));
            pid
        };

        // SAFETY: signal 0 only probes for the process's existence. A zombie
        // still answers, which is exactly what this is checking for.
        #[allow(unsafe_code, reason = "signal 0 probes for a process's existence")]
        let alive = unsafe { libc::kill(pid, 0) } == 0;
        assert!(!alive, "the child outlived its Pty as a zombie");
    }

    #[test]
    fn dropping_a_pty_whose_child_already_exited_does_nothing() {
        // The common case, and it must not spend the grace period.
        let mut pty = Pty::spawn(&sh("exit 0")).expect("spawn");
        pty.wait().expect("wait");

        // Only the drop is timed. Including the spawn would measure how long
        // this machine takes to fork a shell, which under a loaded test run is
        // easily longer than the grace period being asserted about — a test
        // that fails on a busy laptop and passes on an idle one is worse than
        // no test, because it teaches people to rerun until it is green.
        let started = Instant::now();
        drop(pty);
        assert!(
            started.elapsed() < Duration::from_millis(150),
            "an already-reaped child cost {:?} to drop",
            started.elapsed()
        );
    }

    #[test]
    fn a_size_survives_the_round_trip_through_the_kernel() {
        let mut pty = Pty::spawn(&sh("sleep 5")).expect("spawn");
        pty.resize(PtySize {
            cols: 90,
            rows: 30,
            pixel_width: 720,
            pixel_height: 480,
        })
        .expect("resize");
        assert_eq!(pty.size().pixel_width, 720);
        pty.signal(Signal::Kill).expect("kill");
    }
}
