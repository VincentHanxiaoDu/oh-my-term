# Operations: running omt, and finding out why it is misbehaving

Everything else in `docs/architecture/` describes what omt does when it works.
This document covers the rest of its life: starting at boot, sharing a machine
with other people, telling a user why their terminal feels laggy, surviving a
panic in one session without taking the others down, upgrading across five
hosts, getting installed onto a box that has never heard of omt, and being
driven by a CI script with no TTY.

It owns gaps **G5** (observability of omt itself), **G13** (running as a service,
multi-user machines), **G14** (automation and non-interactive use) and the
operational half of **G12** (upgrade) from
[docs/design/scenarios.md](../design/scenarios.md), and requirements R7, R38,
R39, R41, R43, R44, R67, R69, R70, R71.

Related: [01 — Principles](01-principles.md) (P5, P3) ·
[03 — Capability catalog](03-capability-catalog.md) ·
[05 — Session model](05-session-model.md) ·
[07 — Remote protocol](07-remote-protocol.md) ·
[09 — SSH and media](09-ssh-and-media.md) ·
[10 — Configuration](10-configuration.md) ·
[13 — Security](13-security.md) ·
[21 — Data lifecycle](21-data-lifecycle.md) (the redactor and the store, reused
throughout) · [20 — Recall and usage](20-recall-and-usage.md) ·
Research: [vscode-remote §10](../research/vscode-remote.md#10-implications-for-omt).

> **The position in one paragraph.** omt is a **per-user daemon**. There is no
> system-wide multi-user omt, because a system-wide daemon would be a privilege
> boundary omt does not implement
> ([13 §1.3](13-security.md#1-threat-model)). It starts on demand by default and
> can be installed as a user service. It is diagnosable with one command,
> `omt doctor`, whose every failure carries a remedy. One bad session degrades
> that session and nothing else. And every capability is reachable from a script
> with `--json` and no TTY, because a terminal multiplexer that cannot be
> scripted is a strange thing to ship.

---

## 1. Daemon lifecycle

### 1.1 Start on demand is the default

Running `omt` starts a daemon if one is not running, in the background, and
attaches the TUI to it as a client. There is no separate "start the server"
step, and no `omt: server not running` error — that error is a design failure,
not a message to write well.

```
$ omt
  no instance for uid 501 → spawning daemon
  instance 3f2a91c4  ·  socket $XDG_RUNTIME_DIR/omt/3f2a91c4.sock (0600)
  restored 6 sessions from the store (4 orphaned — press ⟨leader⟩ R to restart)
```

Discovery is deterministic, never mtime-guessed
([vscode-remote §10.3](../research/vscode-remote.md#103-what-omt-should-copy)):

1. `$OMT_SOCK` if set (this is what omt itself injects into every pane it
   spawns, so `omt split` from inside a pane always finds *its* instance);
2. otherwise scan `$XDG_RUNTIME_DIR/omt/*.sock` and `connect()` to each — a
   socket that does not accept is a stale file and is unlinked;
3. exactly one live → use it; several → require `--instance`, listing them; none
   → spawn.

The daemon double-forks, `setsid`s, closes inherited descriptors, writes
`$XDG_RUNTIME_DIR/omt/<instance>/pid.txt`, and holds an exclusive `flock` on it
for its lifetime — so "is one already running for this uid" is answerable
without a registry, and a crashed daemon's pid file is provably stale.

**Idle shutdown is off by default** for interactive use — persistence is the
entire point of a multiplexer — but on for a probe:

```toml
[daemon]
idle_shutdown = "off"        # off | <duration>; applies only with zero sessions
                             # and zero attached clients
```

`omt serve --stdio` invoked with no session created defaults to
`idle_shutdown = "5m"`, so an `omt ssh` probe against a host never leaves a
daemon behind ([vscode-remote §10.3](../research/vscode-remote.md#103-what-omt-should-copy)).

### 1.2 `omt service install`

For "start at boot, survive logout, be there when I ssh in". Both are **user**
services. omt never installs a system-level unit; `omt service install --system`
exits with an error explaining why (§2).

```
$ omt service install
  platform: macOS 15.6 → launchd (user domain, gui/501)
  wrote  ~/Library/LaunchAgents/com.oh-my-term.omt.plist
  loaded com.oh-my-term.omt   (RunAtLoad, KeepAlive on crash only)

  omt will now start at login. Sessions are restored from the store; PTYs are
  not (see docs/architecture/05-session-model.md §8).

  omt service status      show it
  omt service uninstall   remove it
```

**launchd** — `~/Library/LaunchAgents/com.oh-my-term.omt.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>                 <string>com.oh-my-term.omt</string>
  <key>ProgramArguments</key>
  <array>
    <string>/opt/homebrew/bin/omt</string>
    <string>daemon</string>
    <string>--foreground</string>
  </array>
  <key>RunAtLoad</key>             <true/>
  <!-- Restart on crash, but NOT on a clean exit: `omt instance shutdown`
       must actually stop it, and a config error must not become a crash loop. -->
  <key>KeepAlive</key>
  <dict>
    <key>SuccessfulExit</key>      <false/>
    <key>Crashed</key>             <true/>
  </dict>
  <key>ThrottleInterval</key>      <integer>10</integer>
  <key>ProcessType</key>           <string>Interactive</string>
  <key>EnvironmentVariables</key>
  <dict>
    <!-- launchd gives a minimal environment; omt does not inherit the login
         shell's PATH, which is why agent CLIs go missing under a service.
         `omt doctor agents` diagnoses exactly this. -->
    <key>PATH</key>
    <string>/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
  </dict>
  <key>StandardOutPath</key>       <string>/Users/ada/.local/state/omt/service.out.log</string>
  <key>StandardErrorPath</key>     <string>/Users/ada/.local/state/omt/service.err.log</string>
  <key>SoftResourceLimits</key>
  <dict><key>NumberOfFiles</key>   <integer>8192</integer></dict>
</dict>
</plist>
```

**systemd** — `~/.config/systemd/user/omt.service`:

```ini
[Unit]
Description=oh-my-term daemon
Documentation=https://github.com/oh-my-term/omt
After=default.target

[Service]
Type=notify
NotifyAccess=main
ExecStart=/usr/local/bin/omt daemon --foreground
# Graceful: SIGTERM asks omt to flush the store and close sockets. Agents get
# the full window to finish; see §1.3.
KillSignal=SIGTERM
TimeoutStopSec=45
KillMode=mixed
Restart=on-failure
RestartSec=10
LimitNOFILE=8192
# omt is not a privilege boundary against its own user (13 §1.3), so hardening
# here buys defence in depth, not a boundary. These are the ones that do not
# break the product:
NoNewPrivileges=no          # agents legitimately run sudo
PrivateTmp=no               # the blob store and ssh ControlPaths must be shared
ProtectSystem=no            # agents write to the user's filesystem by design
Environment=OMT_SERVICE=1

[Install]
WantedBy=default.target
```

Notes that matter more than the files:

- **`Type=notify`.** The daemon calls `sd_notify(READY=1)` only after the store
  is opened, migrations have run and the socket is listening — so
  `systemctl --user start omt` returning success means omt is actually usable.
  On macOS the equivalent is that `omt daemon --foreground` does not write
  `pid.txt` until the same point, and the CLI waits for it.
- **`loginctl enable-linger`** is required for a `systemd --user` service to
  survive logout and start at boot. `omt service install` detects that lingering
  is off and prints the exact command (`sudo loginctl enable-linger ada`) rather
  than silently installing a unit that will not start until the user logs in.
- **`Restart=on-failure`, never `always`.** A clean `omt instance shutdown` must
  stay shut down.
- **`omt service status`** shows the platform's own status plus omt's view side
  by side, because "systemd says active but omt says no instance" is a real
  state and diagnosing it should not require two commands.

```
$ omt service status
launchd  com.oh-my-term.omt   loaded, pid 41288, 0 exits, last start 3 d ago
omt      instance 3f2a91c4    up 3 d 4 h · 11 sessions (3 working) · 2 clients
socket   $XDG_RUNTIME_DIR/omt/3f2a91c4.sock  0600  ada:staff
```

### 1.3 Graceful shutdown with agents running

Shutting down a daemon that is running four agents mid-task is destructive, and
the sequence is specified rather than left to signal defaults.

```
SIGTERM (or `omt instance shutdown`)
 │
 ├─ 1. Refuse new connections; broadcast `instance.shutting_down { grace }`
 │     so every surface shows a countdown banner immediately.
 │
 ├─ 2. If any session's agent is `Working` or has an open Interaction:
 │        interactive → refuse, list them, require --force  (this is R41)
 │        service     → wait up to [daemon] shutdown_grace (default 60 s)
 │                      for them to reach Idle, reporting progress
 │
 ├─ 3. Flush: scrollback chunks, block log, agent events, tree snapshot,
 │     index commit. Interactions and credentials are already durable
 │     (21 §6.3). Write the clean-shutdown marker.
 │
 ├─ 4. SIGHUP every PTY, wait `close_grace` (3 s), SIGKILL survivors.
 │
 └─ 5. Unlink the socket and pid file. Exit 0.
```

```
$ omt instance shutdown
refusing: 3 sessions are working and 1 has an open interaction

  s_4b2f  ~/code/omt        claude   pty      working    4 m 12 s  "refactor the parser"
  s_91cc  ~/code/omt        codex    pty      working    41 s
  s_2d0e  ~/code/infra      claude   pty      BLOCKED    2 m 03 s  Bash(terraform apply)
  s_77a1  ~/code/client-x   opencode native   working    18 m

The `mode` column is not decoration: killing a `native` session ends a JSON-RPC
transport and abandons whatever it was holding, which is a different loss from
killing a PTY, and D8 requires the mode visible wherever a session is listed.

  omt instance shutdown --wait          wait for them to go idle (then shut down)
  omt instance shutdown --force         stop now; running agents are killed
```

**`SIGKILL` of the daemon** loses at most the window described in
[21 §6.2](21-data-lifecycle.md#62-what-kill--9-loses); the store's
append-log design is what makes that acceptable, and the next start reports
`RestoreOutcome::Recovered { lost_tail }` honestly.

### 1.4 Restart with sessions preserved

Restart preserves *state*, not *processes* ([05 §8](05-session-model.md#8-persistence-and-restore)).
What that means operationally:

| Preserved | Lost |
|---|---|
| Workspace/session/pane tree, layouts, titles, focus | The PTY processes themselves |
| Scrollback, blocks, history, agent events, interactions | The in-memory replay window ([07 §5.2](07-remote-protocol.md#52-replay-window)) |
| Per-session `seq` high-water (so `since_seq` still means something) | Open subscriptions, presence, **all writer tokens** |
| The argv, cwd and injected env needed to respawn | — |

Restored sessions come back `Orphaned`, and the restart affordance is one key —
`⟨leader⟩ R` in the TUI, a button on the web session header, `omt session restart
<sid>` in the CLI. For an agent session it offers the agent's own resume
(`claude --resume <uuid>`, `codex resume`) as the primary action, because
respawning `claude` in the same directory without resuming loses the
conversation, which is the thing the user actually cared about.

This limitation is stated in the README, not discovered
([G12](../design/scenarios.md#g12--session-survival-across-daemon-restart-and-upgrade--owned-but-the-answer-is-weak)).
The PTY-supervisor design that would fix it is
[05 §13.1](05-session-model.md#13-open-questions) and remains deferred.

---

## 2. Multi-user machines

Requirement **R44**, scenario **J48**: two developers ssh into a shared build
server and both run `omt`.

### 2.1 The rule

> **omt is per-uid, entirely. Two uids on one machine run two independent
> daemons that share no socket, no state directory, no port, no blob store and
> no log. There is no system-wide daemon, and `omt service install --system` is
> refused with a reason.**

Refused, because a system daemon serving several users would have to authenticate
users and switch uid to run their PTYs — that is a privilege boundary, and
[13 §1.3](13-security.md#1-threat-model) states plainly that omt does not
implement one. Building half of it would be worse than not building it.

### 2.2 The per-uid resources

| Resource | Path / value | Isolation mechanism |
|---|---|---|
| Control socket | `$XDG_RUNTIME_DIR/omt/<instance>.sock`, mode **0600** | `$XDG_RUNTIME_DIR` is `/run/user/$UID` (mode 0700, owned by the uid). Where it is unset — macOS, some containers — omt uses `$TMPDIR/omt-<uid>/` and **creates it 0700, verifying the owner**, refusing to proceed if it exists owned by anyone else |
| State dir | `$XDG_STATE_HOME/omt/`, dir 0700 | Home directory ownership; modes checked and corrected at startup ([13 §5.1](13-security.md#51-at-rest)) |
| Config dir | `$XDG_CONFIG_HOME/omt/` | same |
| Blob store | `$XDG_RUNTIME_DIR/omt/<instance>/blobs/`, dir 0700 | [09 §2](09-ssh-and-media.md#2-the-blob-store) |
| Web/API port | **ephemeral by default** (bind port 0), actual port in `instance.info` and `pid.txt` | Two users never collide. A fixed `[server] port` is opt-in |
| ssh ControlPath | `$XDG_RUNTIME_DIR/omt/ssh-%C` | Inside the per-uid runtime dir; a shared `ControlPath` would let another user ride an authenticated connection |
| Log | `$XDG_STATE_HOME/omt/omt.log`, 0600 | |
| Instance id | UUIDv7 per daemon | Two uids' instances are distinct objects to a federating client |

**Peer credential check.** Every Unix-socket connection is authenticated by
`SO_PEERCRED` / `LOCAL_PEERCRED` before the handshake: a connection whose peer
uid is not the daemon's uid is closed and audited with its pid, uid and argv
([13 §2](13-security.md)). The 0600 mode is the lock; the peer check is the
proof, and it is what makes a mode that got widened by a careless `chmod` a
logged event rather than a breach.

### 2.3 The hazards, specifically

Each of these is a real way this gets it wrong, and each has a countermeasure
omt implements:

1. **A world-readable socket is total compromise.** Whoever can `connect()` to
   the socket can write to your PTYs and approve your agents' tool calls
   ([13 §1.1](13-security.md#1-threat-model)) — it is the highest-authority
   object omt creates. Countermeasure: 0600 + a 0700 parent + the peer-uid check
   + a startup check that *corrects* the mode and logs it. `omt doctor` fails
   loudly if any of the three is wrong.

2. **A shared `/tmp` blob directory.** If `$XDG_RUNTIME_DIR` is unset and omt
   fell back to a predictable `/tmp/omt/`, a hostile local user could
   pre-create the directory and read every screenshot pasted into an agent
   prompt, or plant a symlink and have omt write through it. Countermeasure: the
   fallback path is `$TMPDIR/omt-<uid>/`, created with `mkdir(0700)` and the
   result `fstat`ed for owner and mode; `O_NOFOLLOW` on every open under it; and
   omt **refuses to start** rather than using a directory it does not own.

3. **A TCP port another user can reach.** Loopback is not a boundary on a
   multi-user box: `127.0.0.1:7878` is reachable by every uid on the machine.
   This is the **VS Code connection-token lesson**
   ([vscode-remote §2.4, §10.3](../research/vscode-remote.md#103-what-omt-should-copy)):
   VS Code's remote server listens on a loopback TCP port protected by a
   connection token, and passed that token **on the command line**, where
   `ps aux` made it readable by every other user on the host. Countermeasures, in
   order:
   - **Unix socket by default whenever the transport is ssh.** `omt serve
     --stdio` never opens a TCP port at all; the ssh channel is the transport.
   - When a TCP listener is genuinely wanted, it binds loopback, defaults to an
     **ephemeral port**, and **requires an auth backend**
     ([P8](01-principles.md#p8--security-by-default-no-ambient-trust)).
   - **No secret ever appears in argv or in an inherited environment
     variable.** Tokens are passed by file (0600) or on stdin. `omt doctor net`
     asserts this by reading the daemon's own `/proc/self/cmdline` and
     `environ` and failing if anything credential-shaped is there — a check that
     would have caught the VS Code bug.

4. **`OMT_SOCK` inherited into a user's shell.** omt injects `OMT_SOCK`,
   `OMT_INSTANCE` and `OMT_SESSION` into every pane so `omt open`, `omt split`
   and `$EDITOR` shims work. That is fine — the socket is 0600 and knowing its
   path grants nothing — but it means **`OMT_TOKEN` is never injected into a
   pane**. A pane's authority comes from being on the other side of a socket the
   kernel already checked, not from a bearer token a subprocess could exfiltrate.

5. **A shared `ControlMaster` socket.** `ssh -o ControlPath=/tmp/ssh-%C` shared
   across uids lets another user open channels on your authenticated connection.
   omt's ControlPath is inside the per-uid runtime dir, always.

6. **`umask`.** A daemon started from a shell with `umask 000` would create
   0666 files. omt sets its own `umask(0o077)` at startup, before opening
   anything, and does not rely on inheritance.

### 2.4 What must never be shared

Stated as a list, because it is the review checklist: the control socket, the
state directory, the blob store, the ssh ControlPath, the log, credentials, the
instance key, and any TCP listener. If a future feature needs cross-uid
anything, it needs a privilege boundary design first, and that is a decision
record, not a patch.

---

## 3. `omt doctor`

One command, per-link diagnosis, every failure carrying a remedy. This is the
umbrella that subsumes `omt doctor keys` ([16 §7.5](16-input-and-keymap.md)) and
the media doctoring in [09](09-ssh-and-media.md), and it is requirement **R67**.

```
omt doctor                 everything
omt doctor term            terminal capabilities, TERM/terminfo
omt doctor shell           shell integration
omt doctor agents          agent CLIs, versions, hook integrations
omt doctor keys            keybinding conflicts (16 §7.5)
omt doctor media           clipboard/image paths (09)
omt doctor store           store health, disk, retention, redaction, FDE
omt doctor net             socket, listeners, remotes, version skew, clock
omt doctor service         launchd/systemd unit health
```

### 3.1 The checks

| Group | Check | Failure looks like | Remedy printed |
|---|---|---|---|
| `term` | `TERM` is set and the terminfo entry exists | `TERM=xterm-omt` with no compiled entry | `omt integrate terminfo` (installs to `~/.terminfo`) |
| `term` | truecolor (`COLORTERM`), 256-color, italics, undercurl, sixel/kitty graphics, kitty keyboard protocol, synchronized output (DECSET 2026), OSC 52 write, OSC 8 hyperlinks, bracketed paste | detected by query-and-timeout, reported as a capability table | names the terminal and the setting to change |
| `term` | ambiguous-width setting matches the outer terminal | box-drawing misalignment (J50) | `omt config set term.ambiguous_width wide` |
| `shell` | integration installed in the user's actual shell rc, and the rc is the one being sourced | installed in `.bashrc` but the login shell is zsh | exact file and line to add |
| `shell` | integration is *working* — OSC 133 A/B/C/D observed in a live session in the last hour | installed but not emitting (a `precmd` overwritten by a later plugin) | "load omt's hook last; add it after `oh-my-zsh.sh`" |
| `shell` | `tmux set -g set-clipboard on` when running under tmux ([09 §3.1](09-ssh-and-media.md#31-writing-to-the-local-clipboard-remote--local)) | OSC 52 silently dropped | the exact tmux line |
| `agents` | each configured agent CLI found on `PATH`, with its version | not found under a launchd service (§1.2's PATH trap) | "launchd does not inherit your shell PATH; `omt service install` again, or set `[agents.claude_code].command`" |
| `agents` | **ACP adapter availability** — for each detected agent, whether a `native` mode is reachable ([D8](decisions.md#d8--two-session-modes-pty-default-and-native-acp)): the ACP subcommand or adapter binary exists, launches, and completes an `initialize` handshake, with the negotiated protocol version | `omt claude --native` fails at spawn, or negotiates nothing | names the adapter package and the install command; states that `pty` mode is unaffected. For Claude it also states that the adapter wraps the **Agent SDK, not the Claude Code CLI** |
| `agents` | agent version is within the tested range for its adapter | `claude 2.9.0` vs adapter tested to `2.7.x` | "observation may degrade to the transcript tier; `omt agent explain <sid>` shows which tier is live" |
| `agents` | hook integration installed / stale / broken — compares the installed hook block's checksum against the current binary's | stale after an omt upgrade | `omt integrate install --agent claude-code` |
| `agents` | `omt-hook` binary present, executable, and its round trip completes. **The budget is single-digit milliseconds** ([02 — crate map](02-crate-map.md)); the check's 50 ms is a *failure* threshold, not the budget — a hook that takes 40 ms has already lost its budget and is reported as a warning with its measured time | hook installed but binary missing after a `cargo install` into a different prefix; or a round trip over 50 ms | absolute path it expected, and the fix |
| `agents` | hook can reach the socket (the hook's own failure mode, [06 §9](06-agent-layer.md#9-failure-modes-and-their-handling)) | hook exits 0 but observation falls back to heuristics | "the daemon was not running when the agent started; restart the agent session" |
| `net` | socket exists, is 0600, parent is 0700, owner is us, and `connect()` succeeds | any of those wrong | `chmod`/`chown` command, or "another user owns this path — refusing" |
| `net` | no credential-shaped string in the daemon's argv or environ (§2.3) | a token in argv | names the offending argument |
| `net` | listeners: what is bound, on what interface, with what auth backend | `0.0.0.0` with no auth | refuses to run in that state at all ([13 §2](13-security.md)); doctor explains |
| `net` | each configured remote instance reachable, its round-trip time, and its version | `box` unreachable | the ssh command it tried and the error |
| `net` | **version skew** against each remote: proto intersection and catalog diff | `box` runs 0.3.2; 4 capabilities missing | lists them, offers `omt upgrade --host box` |
| `net` | **clock skew** against each remote (`Welcome.ts` vs local) | > 5 s | "signed invite links and credential expiry depend on this; run `chronyc`/enable NTP on `box`" |
| `store` | free space vs. §21 §3.3 thresholds, and usage vs. retention | 800 MiB free | `omt store purge --before 30d`, with the byte count it would reclaim |
| `store` | sweeper completed a full pass in the last 24 h | stuck sweeper | `omt store sweep --full`, and the log line to look at |
| `store` | integrity: store version, pending migrations, quarantine contents, last `RestoreOutcome` | `Partial` restore not acknowledged | `omt store repair` |
| `store` | redaction is on, and its measured false-positive rate on the corpus | disabled in a project config | names the workspace and the file |
| `store` | full-disk encryption status; state dir not inside a synced folder ([21 §1.2](21-data-lifecycle.md#12-the-one-line-answer-for-backup-exclusion)) | state dir under `~/Dropbox` | the exclusion instruction for that sync client |
| `service` | unit installed, loaded, not crash-looping; lingering enabled (systemd) | `Restart` counter > 3 in an hour | the last 20 log lines and the likely cause |
| `keys` | keybinding conflicts ([16 §7.5](16-input-and-keymap.md)) | | |
| `media` | clipboard/image tiers available for this terminal ([09 §5.6](09-ssh-and-media.md)) | | |

### 3.2 A healthy machine

```
$ omt doctor
omt 0.4.1  ·  instance 3f2a91c4  ·  uid 501 (ada)  ·  macOS 15.6 arm64

terminal ─────────────────────────────────────────────────────────────────────
  ✓ TERM=xterm-ghostty, terminfo found (/Users/ada/.terminfo/78/xterm-ghostty)
  ✓ truecolor · 256 colour · italics · undercurl · OSC 8 · bracketed paste
  ✓ kitty keyboard protocol (full) · synchronized output (DECSET 2026)
  ✓ graphics: kitty protocol · OSC 52 write accepted
  ✓ ambiguous_width=narrow matches the terminal's own setting

shell ────────────────────────────────────────────────────────────────────────
  ✓ zsh 5.9, integration installed in ~/.zshrc:214, loaded last
  ✓ OSC 133 A/B/C/D seen in 4 sessions in the last hour
  ✓ not running under tmux/screen

agents ───────────────────────────────────────────────────────────────────────
  ✓ claude-code   2.7.4    /opt/homebrew/bin/claude   hooks: installed, current
  ✓ codex         0.31.0   /Users/ada/.cargo/bin/codex hooks: installed, current
  ✓ opencode      0.4.9    /opt/homebrew/bin/opencode  ACP: available
  ✓ omt-hook      0.4.1    /opt/homebrew/bin/omt-hook  round-trip 3.1 ms

network ──────────────────────────────────────────────────────────────────────
  ✓ socket $XDG_RUNTIME_DIR/omt/3f2a91c4.sock  0600 ada:staff  connect ok
  ✓ no credential-shaped values in argv or environ
  ✓ listener 127.0.0.1:52411 (ephemeral)  auth: bearer + invite
  ✓ remote box       0.4.1  rtt 18 ms  catalog identical  clock +0.2 s
  ✓ remote pi        0.4.1  rtt 41 ms  catalog identical  clock -1.1 s

store ────────────────────────────────────────────────────────────────────────
  ✓ 6.2 GiB used, 231 GiB free (healthy)
  ✓ store version 5, no pending migrations, no quarantine
  ✓ last full sweep 3 h ago, reclaimed 402 MiB in 11.3 s
  ✓ redaction on (entropy on) · corpus recall 97.5% · false positives 0.8%
  ✓ FileVault enabled · state dir not inside a synced folder

service ──────────────────────────────────────────────────────────────────────
  ✓ launchd com.oh-my-term.omt loaded, 0 restarts, up 3 d 4 h

keys · media ─────────────────────────────────────────────────────────────────
  ✓ no keybinding conflicts (142 bindings)
  ✓ clipboard: direct OS access · image paste: local (tier 1)

42 checks, 42 passed.
```

### 3.3 An unhealthy machine

```
$ omt doctor
omt 0.4.1  ·  instance 8811c0de  ·  uid 1000 (dev)  ·  Ubuntu 22.04 x86_64

terminal ─────────────────────────────────────────────────────────────────────
  ✗ TERM=xterm-omt but no terminfo entry was found
      Programs using terminfo (vim, less, ncurses) will misbehave.
      → omt integrate terminfo          installs to ~/.terminfo, no sudo needed
      → or: omt config set term.term_name xterm-256color   (fewer features)
  ! undercurl not detected; omt will use a straight underline for diagnostics
      → your terminal (Terminal.app) does not support it. No action available.

shell ────────────────────────────────────────────────────────────────────────
  ✗ shell integration installed but not emitting OSC 133
      Found in ~/.bashrc:88, but `precmd` was overwritten by starship at :140.
      Without it, blocks are heuristic: no command text, no exit codes, and
      command history (05 §9) stays empty.
      → move omt's line AFTER starship's init:
          eval "$(starship init bash)"
          source ~/.local/share/omt/shell/omt.bash    ← must be last
  ! running under tmux 3.3a with set-clipboard=external
      → tmux set -g set-clipboard on        (otherwise OSC 52 copy is dropped)

agents ───────────────────────────────────────────────────────────────────────
  ✗ claude-code not found on PATH
      omt was started by systemd, which does not inherit your login shell PATH.
      Your shell finds it at /home/dev/.local/bin/claude.
      → systemctl --user set-environment PATH="$PATH" && omt service install
      → or: omt config set agents.claude_code.command /home/dev/.local/bin/claude
  ✗ codex hooks are STALE
      Installed hook points at /usr/local/bin/omt-hook (0.3.2); this omt is 0.4.1.
      Deferred approvals will fail and fall back to the transcript tier.
      → omt integrate install --agent codex
  ! opencode 0.5.1 is newer than the range this adapter was tested against
      (0.4.0–0.4.9). ACP is still negotiated; if it breaks, omt degrades to
      heuristics and says so in `omt agent explain`.

network ──────────────────────────────────────────────────────────────────────
  ✗ socket mode is 0666 (expected 0600)
      Any user on this machine could write to your terminals and approve your
      agents' tool calls. omt has corrected it and audited the event.
      → nothing further needed; check who has shell access on this host
  ✗ remote box: no common protocol version
      box runs omt 0.2.8 (proto 0); this client speaks proto 1.
      → omt upgrade --host box            uploads 0.4.1 over ssh (§7)
  ! clock skew against pi is +14 s
      Invite-link expiry and credential TTLs will behave unpredictably.
      → on pi:  sudo timedatectl set-ntp true

store ────────────────────────────────────────────────────────────────────────
  ✗ 640 MiB free on /home — omt is in `Pressure` and has dropped its index
      Retention would reclaim 3.1 GiB if run now; the sweeper last completed
      6 days ago and appears stuck (see omt.log:41288 "sweep aborted: lag").
      → omt store sweep --full
      → omt store purge --before 30d      (dry run first: add --dry-run)
  ✗ last restore was Partial and has not been acknowledged
      quarantine/2026-07-28T03-11-02Z-history.db  (18 MiB)
      → omt store repair
  ! state dir is inside ~/Dropbox
      Every byte in docs/architecture/21-data-lifecycle.md §1 is being synced
      to a third party, including redacted-but-still-sensitive scrollback.
      → move it: OMT_STATE_DIR=~/.local/state/omt, or exclude the folder
  ! full-disk encryption is OFF on /home
      → sudo cryptsetup … (see your distribution's documentation)

service ──────────────────────────────────────────────────────────────────────
  ! systemd user unit installed but lingering is off — omt will not start
    until you log in graphically.
      → sudo loginctl enable-linger dev

42 checks, 30 passed, 7 failed, 5 warnings.
Exit code 1.  Machine-readable: omt doctor --json
```

Design rules for the output, each of which is a decision:

- **Every `✗` carries a `→`.** A check with no remedy is a check that should not
  exist; the parity test for doctor asserts every failure variant has a
  non-empty remedy string.
- **The consequence is stated before the fix.** "Blocks are heuristic: no command
  text, no exit codes" is why the user should care; "move the line" is what to
  do. Users skip fixes whose cost they cannot see.
- **`!` is a warning omt cannot fix** (a terminal that lacks undercurl) or a
  policy question (FDE). It does not affect the exit code by default;
  `--strict` promotes warnings to failures for CI.
- **Exit codes**: 0 all passed, 1 at least one failure, 2 usage error. `--json`
  emits the same checks as structured data (§8).
- **Doctor is read-only except for `--fix`**, which applies only the remedies
  marked `auto_fixable` (mode corrections, terminfo install, hook reinstall) and
  prints each one before doing it.

---

## 4. Health, metrics, logs and fault isolation

### 4.1 `system.health`

The structured twin of `omt doctor`: cheap, always available, and the thing a
diagnostics panel and a monitoring script both read. It is what answers J53
("why is this slow") without a profiler.

```rust
pub struct Health {
    pub instance:  InstanceHealth,
    pub sessions:  Vec<SessionHealth>,
    pub clients:   Vec<ClientHealth>,
    pub store:     StoreHealth,
    pub sources:   Vec<SourceHealth>,     // per observation source, per 06
    pub egress:    Vec<EgressStatus>,     // 13 §9.1 — what can leave this machine
}

pub struct InstanceHealth {
    pub version: String,
    pub started_at: Timestamp,
    pub uptime: Duration,
    pub pid: u32,
    pub rss_bytes: u64,
    pub cpu_percent_1m: f32,
    pub open_fds: (u32, u32),             // used, soft limit
    pub threads: u32,
    pub event_loop_lag_p50_p99: (Duration, Duration),
    pub bus_subscribers: u32,
    pub bus_dropped_events: u64,           // 07 §6 backpressure drops, cumulative
    pub degraded: Vec<InstanceDegradation>, // §4.4
}

pub struct SessionHealth {
    pub session: SessionId,
    pub state: SessionState,
    pub mode: SessionMode,                // D8 — Pty | Native; 05 §1.3
    pub rss_estimate_bytes: u64,
    pub agent: Option<AgentHealth>,       // tier live, staleness, last event
    pub faults: Vec<SessionFault>,        // §4.4

    /// `pty` sessions only; `None` for `Native` — there is no PTY, no VT
    /// parser and no grid to measure.
    pub pty: Option<PtySessionHealth>,
    /// `native` sessions only; `None` for `Pty`.
    pub native: Option<NativeSessionHealth>,
}

pub struct PtySessionHealth {
    pub pty_bytes_per_s_1m: u64,
    pub parser_ns_per_kib_p99: u64,
    pub grid_cells: u64,
    pub scrollback_bytes: u64,
    pub damage_frames_per_s: f32,
}

pub struct NativeSessionHealth {
    pub events_per_s_1m: f32,             // JSON-RPC notifications inbound
    pub transcript_bytes: u64,            // omt-rendered transcript, the scrollback analogue
    pub rpc_rtt_p99: Duration,            // request/response latency to the adapter
    pub pending_requests: u32,
    pub acp_version: String,
}

pub struct ClientHealth {
    pub client: ClientId,
    pub kind: ClientKind,
    pub rtt_p50: Duration,
    pub send_queue_bytes: u64,
    pub backpressure: BackpressureState,  // 07 §6
    pub subscriptions: u32,
}

/// The store row of the diagnostics panel (§5.1), plus what `doctor store` needs.
pub struct StoreHealth {
    pub path: PathBuf,
    pub bytes_used: u64,
    pub bytes_free: u64,                  // on the filesystem holding `path`
    pub format_version: u32,              // 21 — the on-disk store format
    pub last_sweep: Option<Timestamp>,    // retention sweep, 21 §3
    pub write_latency_p99: Duration,
    pub gaps: u32,                        // `store.gap` markers written, 21 §3.3
}

/// One observation source for one session, per [06](06-agent-layer.md).
pub struct SourceHealth {
    pub session: SessionId,
    pub source: SourceId,
    pub tier: ObservationTier,            // Hook | Protocol | Heuristic, 06
    pub state: SourceState,               // Working | Blocked | Unknown
    pub last_event_at: Option<Timestamp>,
    pub staleness: Option<Duration>,       // now − last_event_at
    pub events_1m: u32,
}

/// One row of [13 §9.1](13-security.md#9-network-egress-and-supply-chain)'s
/// egress table, evaluated against this instance's live config. One entry per
/// feature that *can* open an outbound connection, enabled or not.
pub struct EgressStatus {
    pub feature: EgressFeature,
    /// `None` for daemon-initiated paths (Web Push, webhooks, the revocation
    /// poll) — 13 §9.1 lists those with no capability by design.
    pub capability: Option<CapabilityName>,
    pub destination: EgressDestination,   // resolved host/URL, or "not configured"
    pub class: EgressClass,               // ThirdParty | OwnRegistry — 13 §9.1's two tables
    pub enabled: bool,
    /// `None` for on-demand paths; `Some` only for the 15-minute revocation poll,
    /// the single recurring outbound connection in the product.
    pub interval: Option<Duration>,
}

/// An instance-level capability that is currently degraded. Added and cleared by
/// the `instance.degraded` event (§10). Distinct from a `SessionFault` (§4.4),
/// which is scoped to one session, and from the layout `Degradation`
/// ([17 §2.3](17-panes-and-layout.md#23-minimums-and-what-happens-when-they-cannot-be-met)).
pub struct InstanceDegradation {
    pub kind: InstanceDegradationKind,    // NotPersisting | MetricsOff | RegistryUnreachable | ClockSkew | HooksStale
    pub since: Timestamp,
    pub detail: String,                   // one line, shown in the panel banner
    pub remedy: Option<String>,           // the doctor remedy, when there is one
}

/// Returned by `upgrade.apply` (§10, §6.1): *when* the daemon will restart, and
/// what is holding it.
pub struct RestartPlan {
    pub when: RestartWhen,                // Now | WhenIdle | Manual — `--force`, `--wait`, `--binary-only`
    pub binary_installed: bool,           // the new inode is on disk already
    pub store_migration: Option<(u32, u32)>,
    pub blocked_by: Vec<SessionSummary>,  // agents still working, §6.1's refusal
}

/// One bound endpoint, as `doctor net` and `omt service status` display it
/// (§3.1, §5.1). The Unix control socket is always present; a TCP entry exists
/// only when one was explicitly configured ([13 §2](13-security.md)).
pub struct Listener {
    pub transport: ListenerTransport,     // Unix { path, mode, owner } | Tcp { addr }
    pub auth: AuthBackend,                // 13 §2 — peer-cred, bearer + invite, …
    pub ephemeral: bool,                  // port chosen at bind time
    pub connections: u32,
}
```

**`Health.egress` is the same data `instance.health` reports.** `system.health`
is the structured model — the full picture, for the diagnostics panel and for a
monitoring script. `instance.health` ([13 §9.1](13-security.md#9-network-egress-and-supply-chain))
is retained as the narrow, cheap query for one question — *what can leave this
machine, and is any of it enabled* — because a warning banner should not have to
compute session and client health to render. Two capabilities, one source of
truth, and the reference catalog carries both with exactly that distinction.

`system.health` is a `Query` at `Viewer`, because knowing the daemon is healthy
is not a privileged fact and a phone should be able to show it.

### 4.2 Structured logging

- **`tracing` with spans**, not lines. Every capability call is a span carrying
  `capability`, `actor`, `request_id`, `session`, and its outcome and duration;
  every PTY read/write batch, every store flush and every agent-source event is a
  child span. That is what makes "why is this slow" answerable — the p99 lives in
  the span durations, not in a text search.
- **Levels**: `error` (something failed and the user will notice), `warn`
  (degraded but working — a stale hook, a dropped event), `info` (lifecycle:
  start, stop, session create/close, client attach, migration), `debug`
  (per-capability calls), `trace` (per-frame, per-record; expensive and refuses
  to log env at all, [13 §8](13-security.md#8-secret-redaction)).
- **Destination**: `$XDG_STATE_HOME/omt/omt.log`, 0600, rotated at 32 MiB, 5
  files kept ([10 §7.12](10-configuration.md#712-log)). Under systemd, also
  journald via `--log-format=journal` when `OMT_SERVICE=1`, because that is where
  a Linux admin will look. Under launchd, stdout/stderr go to
  `service.out.log`/`service.err.log`, which are separate files precisely so a
  crash message that never reached the tracing layer is still on disk.
- **Redaction is not optional**: the redactor is a `Layer` in the stack
  ([13 §8](13-security.md#8-secret-redaction), [21 §2](21-data-lifecycle.md#2-redaction-before-write))
  and `log.redact_secrets` cannot be set to false.
- **Per-connection prefixes** — `[3f2a91c4][c_780ab5][web]` — so a
  multi-client incident is greppable
  ([vscode-remote §10.3](../research/vscode-remote.md#103-what-omt-should-copy)).
- **Log level is hot-reloadable** ([10 §6](10-configuration.md)) and settable per
  target: `omt config set --ephemeral log.level "info,omt_term=trace"`. Asking a
  user to restart the daemon to reproduce a bug loses the bug.

### 4.3 Metrics

Optional, off by default, no egress ([13 §9.1](13-security.md#9-network-egress-and-supply-chain)):

```toml
[metrics]
enabled = false
# When enabled, a Prometheus text endpoint on the existing loopback listener,
# behind the same auth. omt never pushes anywhere.
path    = "/metrics"
```

The exported series are exactly the fields of `Health` — there is no second
metrics model to drift. Names: `omt_session_pty_bytes_total`,
`omt_parser_seconds_bucket`, `omt_event_bus_dropped_total`,
`omt_store_bytes`, `omt_store_sweep_seconds`, `omt_client_rtt_seconds`,
`omt_agent_state{kind,state}`, `omt_session_faults_total{class}`.

### 4.4 Per-session fault isolation (R7)

> **Invariant: a fault originating in one session degrades that session and
> nothing else. No single session can panic, starve, or exhaust the instance.**

This is scenario J54, and it needs a mechanism, not a promise.

**What "bad" means, and what happens:**

| Fault class | Trigger | Mechanism | Result |
|---|---|---|---|
| `ParserPanic` | a panic anywhere in the per-session pipeline (`omt-term`, block tracker, an adapter's normalizer) on hostile bytes | every session's byte-processing step runs inside `catch_unwind` at the **session task boundary**, with the panic hook capturing the backtrace and the last 4 KiB of input | session → `Degraded { ParserPanic }`: the PTY keeps running and its raw bytes keep being persisted, but grid state is frozen and the pane shows a banner with a "reset terminal state" action. The offending bytes are quarantined to `state/omt/crashes/<ts>/` and `omt bug-report` picks them up. **The other 14 sessions are untouched.** |
| `RunawayOutput` | sustained output above `session.max_bytes_per_s` (default 8 MiB/s) for > 5 s — `yes`, a `find /`, an accidental binary `cat` | per-session token-bucket read scheduler; the reader stops draining and lets the PTY's own buffer apply backpressure to the writer | session → `Degraded { Throttled }` with a visible "throttled — 8 MiB/s" badge and a one-key "let it run" override. The event bus is protected because the coalescer ([07 §6.2](07-remote-protocol.md#62-coalescing-terminal-frames)) already caps frame rate |
| `ScrollbackExhaustion` | a session grows past `store.max_scrollback_bytes` | oldest chunks evicted first, per session, never instance-wide | that session loses its oldest history; nobody else does |
| `NativeTransportClosed` (`native` only) | the ACP adapter exits, or the JSON-RPC transport closes | there is **no PTY and therefore no EOF to observe**: the fault is a closed stdio transport, plus any in-flight request completing with a transport error. Pending `session/request_permission` calls are the ones that matter — they are blocking requests with no timeout, so a transport close is the only signal they will ever get | session → `Exited { transport_closed }`; open interactions become `Abandoned` and are marked as such on every surface rather than silently disappearing ([06 §9](06-agent-layer.md#9-failure-modes-and-their-handling)). Recovery is the **agent's own resume** (ACP `session/load` against its session id), never a synthetic replay by omt |
| `AgentOom` / `AgentCrash` (`pty` only) | the agent CLI is OOM-killed or exits non-zero mid-turn | the PTY read returns EOF; the process reaper reads the exit status and `dmesg`/`/proc` for an OOM signature where available | session → `Exited { status }` with a diagnosis line: *"claude was killed by the OOM killer (rss 6.1 GiB)"*, and a restart affordance. omt's own memory is unaffected — the agent is a child process, not a plugin |
| `SlowConsumer` | a client cannot keep up | per-subscription backpressure ([07 §6](07-remote-protocol.md#6-backpressure)) drops that subscription's terminal frames, never `interaction` events, and never affects other clients | the slow client resyncs |
| `StoreError` | a write fails (ENOSPC, EIO) | the store returns an error to that session's writer; the session enters `Degraded { NotPersisting }` and a `store.gap` marker is written ([21 §3.3](21-data-lifecycle.md#33-disk-pressure)) | sessions keep running; content stops being written |
| `PluginFault` | an out-of-process plugin hangs or crashes | already isolated by process boundary ([11](11-plugins.md)) | plugin disabled, reported |

The mechanisms behind the invariant:

1. **One task per session**, with its own bounded input channel. A session that
   stops draining fills its own channel and no other.
2. **`catch_unwind` at the session boundary**, plus `panic = "unwind"` in the
   release profile — deliberately not `abort`, because abort would make the
   invariant impossible to hold. Any panic caught is an `error!` log, a crash
   record and a `session.faulted` event (§10's event table; dotted lowercase is
   the convention throughout the corpus); it is never swallowed.
3. **No shared mutable state on the hot path.** `omt-term` is a pure state
   machine per session with no globals ([02](02-crate-map.md)), which is what
   makes per-session isolation achievable at all — this invariant was bought by
   the crate layering, not bolted on.
4. **Bounded everything**: per-session byte budget, per-session scrollback
   budget, per-subscription queue, per-connection send queue. An unbounded queue
   anywhere is how one session becomes an instance-wide OOM.
5. **Tested adversarially**: the VT fuzz corpus ([P5](01-principles.md#p5--production-grade-from-the-first-commit))
   is replayed into one session of a 20-session instance, and the test asserts
   the other 19 keep processing and the instance's `Health` stays nominal.

`Degraded` sessions are listed in `system.health.sessions[].faults`, surfaced in
the diagnostics panel, and shown as a badge on the session row on every surface.
A degraded session that a user has not acknowledged keeps its banner —
degradation that goes unnoticed is indistinguishable from a bug.

Degradation of the *instance* rather than one session — the store not
persisting, the registry unreachable, hooks gone stale — is a separate list:
`InstanceHealth.degraded: Vec<InstanceDegradation>` (§4.1), maintained by the
`instance.degraded` event (§10). It is `InstanceDegradation`, not `Degradation`:
the bare name is [17 §2.3](17-panes-and-layout.md#23-minimums-and-what-happens-when-they-cannot-be-met)'s
layout degradation and the two are unrelated.

---

## 5. Diagnostics panel and `omt bug-report`

### 5.1 The panel (P3 — all three surfaces)

`⟨leader⟩ ⇧D` in the TUI, *Instance → Diagnostics* in the web client, and
`omt doctor --watch` in the CLI. All three render the same `system.health` plus a
live `doctor` summary; there is no TUI-only diagnostic view
([P3](01-principles.md#p3--parity-one-capability-three-surfaces)).

```
┌ diagnostics ─ instance 3f2a91c4 ─ omt 0.4.1 ─ up 3d 4h ────────────────────┐
│ daemon   rss 412 MiB   cpu 3%   fds 214/8192   loop lag p50 0.4ms p99 3ms  │
│ bus      2 subscribers   0 dropped                                        │
│ store    6.2 GiB   231 GiB free   sweep 3h ago   writes p99 1.2ms         │
│                                                                            │
│ session   mode    state     out/s   parser p99  scroll   agent   tier  faults │
│ s_4b2f    pty     live      41 KiB   18 µs/KiB  6.1 MiB  working hook   —    │
│ s_91cc    pty     live     2.1 MiB   22 µs/KiB  8.0 MiB  working proto  THROT │
│ s_2d0e    pty     live       0 B      —         1.2 MiB  blocked hook   —    │
│ s_77a1    pty     degraded   —        —         4.4 MiB  unknown heur   PANIC │
│ s_0c31    native  live      12 ev/s   n/a       2.8 MiB  working acp    —    │
│                                                                            │
│ client       kind    rtt    queue   backpressure   viewing                 │
│ c_780ab5     web     41ms   0 B     ok             s_4b2f (blocks)         │
│ c_local      tui     —      —       ok             workspace ~/code/omt    │
│                                                                            │
│ [d] run doctor   [l] tail log   [b] bug report   [r] reset s_77a1          │
│ native rows show events/s and transcript bytes; parser and grid are n/a    │
└────────────────────────────────────────────────────────────────────────────┘
```

### 5.2 `omt bug-report`

```
$ omt bug-report
Collecting… this bundle stays on your machine until you choose to send it.

  ✓ versions            omt 0.4.1 (git 9a41c2e), rustc 1.89, macOS 15.6 arm64
  ✓ doctor output       42 checks (7 failed)
  ✓ system.health       full snapshot
  ✓ config              config.toml, keybindings.toml   [secrets stripped]
  ✓ log tail            last 5,000 lines of omt.log     [redacted]
  ✓ crash records       1 record: 2026-08-03T18:22 ParserPanic in s_77a1
  ✓ quarantined input   4,096 B of PTY bytes from that panic  ⚠ see below
  ✓ agent versions      claude 2.7.4, codex 0.31.0, opencode 0.4.9
  ✓ terminal probe      ghostty 1.2.1, capability table
  ✗ core dump           never included (docs 21 §2.6 — it contains unredacted
                        scrollback from the daemon's memory)
  ✗ scrollback          never included; add specific blocks with --block <id>
  ✗ credentials         never included

  Redaction applied: 61 findings (env 12, sk 1, entropy 48).
  Estimated size: 812 KiB.

  ⚠ The quarantined PTY bytes are the input that caused the panic. They are
    redacted but they are still your terminal's output. Review them:

      omt bug-report --review          open the bundle in $PAGER, file by file
      omt bug-report --exclude quarantined-input

Write the bundle? [y/N] y
Wrote ~/omt-bug-report-2026-08-03T18-52-11Z.tar.zst  (804 KiB)

Nothing has been sent anywhere. Attach this file to an issue at
https://github.com/oh-my-term/omt/issues after reviewing it.
```

Rules:

- **omt never sends it.** There is no upload endpoint, no crash reporter, no
  "report this automatically" checkbox
  ([13 §9.1](13-security.md#9-network-egress-and-supply-chain)). The user gets a
  file.
- **Redaction is [21 §2](21-data-lifecycle.md#2-redaction-before-write)'s
  redactor**, the same one, applied again on collection — belt and braces, since
  some inputs (live config, a `--block` inclusion) did not pass through the write
  path.
- **`--review` is a first-class flow, not a footnote.** The bundle is a directory
  of plain text before it is an archive, and the user can page through every file
  and delete any of them before it is sealed. A bug report the user did not read
  is one they should not send.
- **Deterministic manifest.** `MANIFEST.json` lists every file, its size, its
  redaction findings by class, and the exclusions — so a maintainer knows what
  they are *not* looking at.

---

## 6. Upgrade

### 6.1 `omt upgrade`

```
$ omt upgrade
current 0.4.1  ·  available 0.5.0  (checked: you asked; omt never checks on its own)

changes affecting you:
  · store format 5 → 6 (block attribution index). A backup of the small
    databases is taken automatically; ~9,200 blocks, est. 4 s.
  · agent hooks must be reinstalled: `omt integrate install` runs after upgrade.

refusing to restart the daemon: 3 sessions are working, 1 is blocked
  s_4b2f  claude   working    4 m 12 s
  s_91cc  codex    working    41 s
  s_2d0e  claude   BLOCKED    Bash(terraform apply)
  s_77a1  claude   working    18 m

  omt upgrade --wait     install now, restart when every agent is idle
  omt upgrade --force    restart now; running agents are killed
  omt upgrade --binary-only   install the binary, restart later yourself
```

This is requirement **R41**, and the refusal is the point: killing an agent
mid-`terraform apply` because a version was available is the single worst thing
an upgrade can do. `--wait` installs the binary immediately (which is safe — the
running daemon keeps its inode) and restarts at the first moment every agent is
`Idle` with no open interactions, or when the user says so.

**omt never checks for updates on its own.** `omt upgrade` reaches the network
because the user typed it; there is no background check, consistent with
[13 §9.1](13-security.md#9-network-egress-and-supply-chain). Where omt was
installed by a package manager, `omt upgrade` detects it (`brew`, `cargo
install`, `apt`, a plain binary) and either delegates or refuses with the right
command, rather than overwriting a package manager's file.

### 6.2 Version skew

Three skew axes, and each already has machinery
([03 §7](03-capability-catalog.md#7-versioning),
[07 §1.5](07-remote-protocol.md#15-federating-across-versions)):

| Axis | Rule |
|---|---|
| **CLI ↔ local daemon** | Must match exactly. A `0.5.0` CLI against a `0.4.1` daemon prints *"your omt binary was upgraded but the running daemon is 0.4.1; `omt instance restart` when convenient"* and then proceeds if the catalogs intersect. The socket handshake carries the version, so this is detected, never guessed |
| **Client ↔ remote instance** | The negotiated-catalog rule from [03 §7](03-capability-catalog.md#7-versioning): the client renders the **intersection** and greys out the rest with *"not supported on `box` (0.3.2)"*. Never a hard failure while a common proto exists |
| **Proto** | No common proto version → refuse, with exactly one offered action: upgrade the remote (§7's upload path). This is the only hard failure |

Degradation is visible, not silent: a session on an older remote carries a
`proto 1 (remote 0.3.2)` badge, and the greyed-out actions say *why* on hover or
long-press. A user who cannot see why a button is dead files a bug about the
button.

### 6.3 Multi-host upgrade (R39)

Someone with a laptop, two dev boxes, a Pi and a CI runner:

```
$ omt upgrade --all-hosts
host          current   sessions   working   plan
  local       0.4.1      11         3        restart deferred (--wait)
  box         0.3.2       4         0        upload 0.5.0, restart
  pi          0.4.1       1         0        upload 0.5.0, restart
  ci-runner   0.4.1       0         0        upload 0.5.0, restart
  vpn-box     —           —         —        UNREACHABLE (ssh: timeout)

Proceed with 3 remote upgrades? [y/N] y

box        uploaded 21.4 MiB in 3.1 s · installed · restarted · 0.5.0 ✓
pi         uploaded 19.8 MiB in 26.4 s · installed · restarted · 0.5.0 ✓
ci-runner  refused: 1 session is working (started 2 m ago by ci-token)
           → omt upgrade --host ci-runner --wait

local      binary installed; daemon restarts when 3 agents go idle
```

**Honest scope limits, stated in the docs and in `--help`:**

- It is a **loop over the hosts the client already knows**, from
  `instances.toml`. It is not fleet management: no inventory, no groups, no
  rollout policy, no canary, no rollback orchestration.
- It **stops at the first failure per host** and continues to the next host.
  Hosts are independent; there is no transaction across them.
- It **will not upgrade a host serving another user's sessions**. If the remote
  instance reports attached clients whose actor is not the caller, omt refuses
  and says so — replacing a shared dev box's binary is not one laptop's decision
  ([vscode-remote §10.5](../research/vscode-remote.md#105-concrete-recommendation-for-omt-ssh-bootstrap)).
- **Downgrade is not orchestrated.** `omt upgrade --host box --version 0.4.1`
  works for the binary, but if `box` has already migrated its store to a newer
  format, the older binary will refuse to open it
  ([21 §7.1](21-data-lifecycle.md#71-the-versioned-store-rule)) — which is
  correct and is printed as a warning *before* the downgrade, not discovered
  after.
- **More than ~10 hosts is not the target.** At that point the right tool is
  the user's existing configuration management, and omt's answer is "we are a
  single static binary; `ansible copy` works fine", which is a better answer than
  a half-built fleet tool.

---

## 7. Bootstrap onto a host with no omt (R38)

The full design is [vscode-remote §10.4–10.5](../research/vscode-remote.md#104-what-omt-should-do-differently)
and the transport is [07 §2.4](07-remote-protocol.md#24-ssh-stdio-bridge--omt-ssh-target);
this section is the operational specification.

### 7.1 One ssh connection

```
$ omt ssh box
  ssh box (ControlMaster: reused, $XDG_RUNTIME_DIR/omt/ssh-%C)
  probe: OMT-MISSING  Linux aarch64  glibc 2.31
  → box has no omt. Install 0.5.0 to ~/.local/bin/omt? (21.4 MiB over this
    ssh connection; nothing is downloaded on box) [y/N] y
  uploading aarch64-unknown-linux-musl … 21.4 MiB in 3.1 s
  installed (atomic rename) · starting · handshake ok · 0 sessions
```

The probe and the attempt are **one ssh exec**, so the common case ("omt is
already there") costs nothing and a host with MFA prompts once:

```sh
ssh box -- 'omt serve --stdio --proto 1 2>/dev/null || \
            { printf "OMT-MISSING "; uname -sm; \
              (ldd --version 2>&1 | head -1) || echo musl; \
              command -v zstd >/dev/null && echo zstd || echo nozstd; }'
```

### 7.2 Arch and libc detection

`uname -sm` gives `Linux aarch64` / `Darwin arm64` / `Linux x86_64`; the `ldd`
line distinguishes glibc from musl (Alpine's `ldd` prints `musl libc`, and a
missing `ldd` implies musl or a minimal container). Mapping:

| Probe | Artifact |
|---|---|
| `Linux x86_64` (any libc) | `x86_64-unknown-linux-musl` |
| `Linux aarch64` (any libc) | `aarch64-unknown-linux-musl` |
| `Linux armv7l` | `armv7-unknown-linux-musleabihf` |
| `Darwin arm64` / `x86_64` | the matching macOS target |
| anything else | refuse, print the `uname` and the release URL |

**musl is the default remote artifact regardless of the host's libc.** Fully
static, no glibc-version cliff, works on Alpine and on a 2018 CentOS box alike —
which is precisely the class of host where VS Code's Remote-SSH still fails
outright. glibc builds exist for the local desktop case where NSS/`getaddrinfo`
matters.

### 7.3 Delivery, install, and consent

- **Upload over the existing ssh channel is the default**, not a fallback. The
  binary is ~20 MiB, the channel is already authenticated, and this works on
  air-gapped hosts and behind hostile proxies on the first try. `zstd -c` on the
  local side when the remote reported `zstd`, plain otherwise.
- **Install location** `~/.local/bin/omt`, no sudo, no versioned tree, and
  therefore **no GC problem** — a single file replaced in place is the whole
  lifecycle. `--install-prefix` overrides it; a system-wide install is the
  user's own business and omt does not attempt it.
- **Atomic install**: `cat > omt.tmp && chmod 755 omt.tmp && mv -f omt.tmp omt`.
  `mv` is a `rename(2)`, which safely replaces a running binary on Unix (the
  running process keeps its inode). There is no half-written state.
- **Consent per host, remembered.** Writing an executable into someone's
  `~/.local/bin` is a side effect and is prompted once, recorded in
  `[remote.hosts."box"] bootstrap_consent = true`. `--yes` for scripts;
  `[remote] bootstrap = "assume-installed"` for locked-down hosts. When omt is
  then *not* there, the session does not fail: it falls back to the pty-wrapper
  rung of [09 §5.1](09-ssh-and-media.md#51-tier-overview)'s ladder, so the user
  still gets a working terminal, and the diagnostic that would have been the
  failure — the exact `uname` and the download URL — is shown as a notice
  instead. Someone who typed `omt ssh box` wants to work on box; refusing to
  give them a terminal because a media tier is unavailable would be the wrong
  trade.
- **Verification.** The uploaded artifact's BLAKE3 is checked on the remote by
  the freshly installed binary itself (`omt --print-self-hash`) against the
  local value before it is used. A truncated upload is caught immediately rather
  than becoming a confusing runtime failure.

### 7.4 Remote-install GC

The binary needs none. The **state** does
([vscode-remote §10.2](../research/vscode-remote.md#102-what-does-not-disappear)):
a remote instance accumulates exactly the inventory in
[21 §1](21-data-lifecycle.md#1-the-inventory), under the same retention and the
same sweeper, on the remote host. So:

- `omt store usage --host box` and `omt store purge --host box …` work over the
  federated connection, because they are ordinary capabilities
  ([21 §8](21-data-lifecycle.md#8-capabilities)) and remote is equivalent to
  local ([D2](decisions.md#d2--remote-is-exactly-equivalent-to-local)).
- `omt doctor` reports store pressure on every configured remote, not only
  locally — a Pi that filled its SD card is the failure a user will otherwise
  discover at the worst moment.
- `omt ssh box --uninstall` removes the binary, stops the daemon, and offers
  `store.purge --everything` on that host with the manifest, so a host can be
  left exactly as it was found. It is the remote form of
  [19 §8](19-onboarding.md#8-uninstall-and-rollback)'s `uninstall.apply`, and is
  distinct from `service.uninstall` (§10), which only removes a service unit.

---

## 8. Automation and CI (G14)

### 8.1 The contract

Four rules, and they hold for **every** capability because the CLI is generated
from the catalog ([03 §1](03-capability-catalog.md#1-the-idea)):

1. **`--json` on everything.** The schema is the capability's own output schema,
   so it is documented and stable by [03 §7](03-capability-catalog.md#7-versioning).
2. **stdout is data, stderr is diagnostics.** Progress bars, warnings and prompts
   go to stderr; `omt session list --json | jq` is always safe.
3. **No TTY required, ever.** Anything that would prompt fails with exit code 3
   and a message naming the flag that would have avoided the prompt (`--yes`,
   `--confirm <name>`, `--instance`).
4. **Documented exit codes:**

| Code | Meaning |
|---|---|
| 0 | success |
| 1 | the operation ran and failed (`not_found`, `conflict`, `precondition_failed`) |
| 2 | usage error — bad flag, bad input schema |
| 3 | a prompt was required and there is no TTY |
| 4 | authentication or authorization failure |
| 5 | cannot reach the instance (no daemon, socket gone, remote unreachable) |
| 6 | timeout (`agent.wait`, `--deadline`) |
| 7 | the remote operation failed *on the remote* — distinguished from 1 so a script can tell "box said no" from "I could not ask box" |

### 8.2 Non-interactive auth (R70)

```sh
export OMT_INSTANCE=box              # a name from instances.toml, or a URL
export OMT_TOKEN=$(cat /run/secrets/omt-ci)   # a bearer credential, 13 §4
omt session list --json
```

- `OMT_TOKEN` is read once at start and never re-exported into a child. It is
  **not** injected into panes omt spawns (§2.3 hazard 4).
- `OMT_TOKEN_FILE` is preferred in CI systems that mount secrets as files, since
  it keeps the value out of the environment entirely.
- When neither is set and `$OMT_SOCK` exists, the local socket's peer-uid check
  *is* the authentication — a script running as you on your own machine needs no
  token, which is the common case and should not require ceremony.
- A CI credential should be minted narrow:
  `omt token create --role operator --scope 'session.*,agent.*,interaction.*'
  --expires 90d --label ci` ([13 §4](13-security.md#4-roles-and-their-mapping-onto-the-catalog)).
  Its actions are audited under that credential's actor, which is how J45's
  unattended resolutions stay attributable.

### 8.3 The primitives scripts need (R71)

| Capability | Why a script needs it |
|---|---|
| `session.capture` | scrollback or a block range → text/ANSI/JSONL. The `tmux capture-pane` equivalent, and the thing every migrating script asks for first |
| `agent.wait` | **the key primitive.** Block until a session reaches a state, with a timeout. Without it every script is a `sleep` loop |
| `interaction.list` / `interaction.resolve` | answer a permission card from a script (J45) |
| `session.export` | a whole session as JSONL for archival, per [21 §4.2](21-data-lifecycle.md#42-storeexport--the-archive-format) |
| `events.subscribe --json` | stream events as newline-delimited JSON for a long-running watcher |

```rust
capability! {
    /// Block until a session's agent reaches one of the requested states.
    name  = "agent.wait",
    group = "agent",
    verb  = "wait",
    kind  = Query,                      // it mutates nothing; it observes
    role  = Role::Viewer,
    input = AgentWait {
        session: SessionId,
        /// Any of these ends the wait.
        ///
        /// `Blocked` is an **agent state**: it matches whenever the agent is
        /// blocked on a human, including at the moment of the call when
        /// `immediate` is set. `Interaction` is an **edge**: it matches only
        /// when a *new* interaction opens after the wait began, and carries
        /// that interaction in the result's `interaction` field. A script that asks "is
        /// anyone needed here?" wants `Blocked`; a watcher that wants each new
        /// card exactly once wants `Interaction`, because `Blocked` would fire
        /// immediately on a card it has already handled.
        until: Vec<WaitCondition>,      // Idle | Blocked | Working | Exited | Interaction
        timeout: Duration,
        /// Resolve immediately if the condition already holds. Default true —
        /// the alternative is a guaranteed race.
        immediate: bool,
    },
    output = AgentWaitResult {
        matched: Option<WaitCondition>,
        state: AgentState,
        interaction: Option<InteractionSummary>,
        waited: Duration,
        timed_out: bool,
    },
    effects = [],
}
```

`agent.wait` is implemented over the event bus with the same `since_seq`
semantics as any subscription, so it cannot miss a transition that happened
between the call being issued and the subscription starting. That is the whole
reason it is a capability instead of a client-side polling loop.

### 8.4 Real scripts

**Unattended agent run in CI (J44):**

```sh
#!/usr/bin/env bash
set -euo pipefail
export OMT_INSTANCE=ci-runner
export OMT_TOKEN_FILE=/run/secrets/omt-ci

sid=$(omt session create --workspace "$PWD" --command claude --json | jq -r .session)
trap 'omt session close --session "$sid" --force >/dev/null' EXIT

omt session send-text --session "$sid" --text "fix the failing test in src/parser.rs" --submit

# Wait for the agent to finish OR to need a human. Never sleep-poll.
result=$(omt agent wait --session "$sid" --until idle --until blocked --timeout 30m --json)

case "$(jq -r .matched <<<"$result")" in
  idle)
    omt session capture --session "$sid" --format jsonl > run.jsonl
    ;;
  blocked)
    echo "agent needs a human:" >&2
    jq -r '.interaction | "\(.kind): \(.summary)"' <<<"$result" >&2
    exit 1
    ;;
  *)
    echo "timed out after 30m" >&2       # exit code 6 from omt
    exit 6
    ;;
esac
```

**A watcher that notifies on any blocked agent, across every host:**

```sh
omt events subscribe --kinds agent,interaction --json \
| jq -c --unbuffered 'select(.payload.t == "interaction_opened")' \
| while read -r ev; do
    notify-send "omt: $(jq -r '.payload.summary' <<<"$ev")"
  done
```

**Nightly hygiene:**

```sh
omt store usage --json | jq -e '.free_bytes > 5e9' \
  || omt store purge --before 30d --yes
omt doctor --json --strict || omt bug-report --output /var/log/omt-bugreport.tar.zst
```

### 8.5 What omt refuses to automate

Stated plainly, because someone will ask for each of these:

1. **omt never auto-answers an interaction.** There is no `auto_approve = true`,
   no allow-list, no danger classifier, no "approve everything from this agent"
   — per [D1](decisions.md#d1--omt-adds-no-policy-layer-over-an-agents-permission-semantics).
   `interaction.resolve` is exposed so *your* script can decide, under *your*
   credential, and every such resolution is audited as coming from that
   credential. Policy lives in your script or a plugin ([11](11-plugins.md)),
   never in `config.toml`. This is a documented no, not an unimplemented
   feature.
2. **omt never auto-upgrades a remote** on version mismatch alone (§6.3).
3. **`store.purge` never runs unconfirmed** in a non-TTY context without an
   explicit `--yes`; the dry-run manifest is the default there
   ([21 §4.3](21-data-lifecycle.md#43-storepurge--destruction-with-a-manifest)).
4. **omt never sends a bug report** anywhere (§5.2).
5. **omt never synthesizes an answer it had to infer from the screen**
   ([D3](decisions.md#d3--synthetic-input-is-bounded-by-state-dependence-not-by-tool-danger)) —
   automation does not relax that bound; if anything it tightens the case for it,
   because nobody is watching.

---

## 9. Performance and resource budgets

Budgets, not aspirations: each is asserted by a benchmark in CI, and a
regression past the ceiling fails the build
([P5](01-principles.md#p5--production-grade-from-the-first-commit)).

| Resource | Budget | Notes |
|---|---|---|
| Daemon baseline RSS | **< 40 MiB** with zero sessions | store handles, registries, the runtime |
| Per idle session | **< 2 MiB** + scrollback | grid (~80×24 styled cells ≈ 60 KiB) + block index + adapter state |
| Per session scrollback in memory | **≤ 8 MiB**, compressed chunks beyond the hot window | the on-disk copy is the archive; memory holds the hot tail |
| **50 sessions, idle** | **< 250 MiB RSS**, < 1 % CPU | the design target; below tmux's per-pane cost at the same scrollback depth |
| **50 sessions, 5 producing 1 MiB/s** | < 600 MiB, < 60 % of one core | parser throughput is the binding constraint |
| Parser throughput | **> 60 MiB/s/core** on the plain-text corpus, > 20 MiB/s on the escape-heavy corpus | [04](04-terminal-core.md)'s number |
| Event-loop lag | p99 **< 5 ms** under the 50-session load | above this, typing feels laggy — this is the metric J53 is really about |
| File descriptors | **3 per session** (pty master, log/chunk writer, adapter channel) + 2 per client + ~20 fixed. 50 sessions ≈ 190 fds | `LimitNOFILE=8192` in the unit leaves three orders of headroom; omt raises its own soft limit toward the hard limit at startup and reports the result in `Health` |
| Threads | tokio worker count = min(cores, 8), plus one blocking pool for store I/O | not one thread per session |
| Store write | p99 **< 5 ms** for `Bulk`, < 15 ms for `Critical` fsync on NVMe | [21 §6.3](21-data-lifecycle.md#63-fsync-policy-and-what-it-costs) |
| Startup | **< 200 ms** to a usable TUI with 50 restored sessions | scrollback is loaded lazily per pane; only the tree and the visible grids are read eagerly |

**What happens at 50 sessions**, specifically, because that is the number a
power user reaches and the answer should not be "we do not know":

- Memory is ~250 MiB idle and is dominated by scrollback, which is exactly what
  `store.max_scrollback_bytes` bounds.
- The TUI renders only visible panes; a workspace with 40 hidden sessions costs
  nothing to draw. Damage tracking is per session
  ([04 §9.3](04-terminal-core.md#93-batching-and-coalescing)).
- The event bus coalesces terminal frames per subscription
  ([07 §6.2](07-remote-protocol.md#62-coalescing-terminal-frames)), so N sessions
  producing output do not produce N × frame-rate messages to one client.
- Above `[daemon] max_sessions` (default **200**), `session.create` returns
  `precondition_failed` naming the limit rather than degrading everything. A
  refusal with a number is better than an instance that swaps.
- `omt doctor` reports when any of these budgets is being exceeded, with the
  session that is responsible — which turns "omt is slow" into "s_91cc is
  producing 2.1 MiB/s and is throttled".

---

## 10. Capabilities

### 10.1 Jobs — the shape long-running commands return

A command that can take longer than a user will hold a phone still returns a
`JobId` rather than blocking: `upgrade.apply`, `remote.bootstrap`,
`store.export` ([21 §8](21-data-lifecycle.md#8-capabilities)) and
`search.reindex` ([20 §12](20-recall-and-usage.md#12-capabilities)). Progress
arrives as `instance`-scoped events, so a client that reconnects mid-job learns
where it got to instead of waiting on a call it can no longer hear the answer
to.

| Capability | Kind | Role | Input | Output | Effects |
|---|---|---|---|---|---|
| `job.list` | Query | V | `{ include_finished? }` | `[{ id, capability, actor, started_at, progress, state }]` | — |
| `job.get` | Query | V | `{ job }` | `{ …, outcome? }` | — |
| `job.cancel` | Command | O | `{ job }` | `{ state }` | — |

**Cancellation is cooperative and says so.** A job reaches its next checkpoint
and stops; one that has passed its point of no return — an install that has
begun the atomic rename ([§7](#7-bootstrap-onto-a-host-with-no-omt-r38)) —
returns `precondition_failed` with the reason rather than reporting a success it
cannot deliver. `job.cancel` carries a binding rather than living in the palette
alone
([16 §11](16-input-and-keymap.md#11-capabilities-introduced-here),
[D17](decisions.md#d17--parity-is-a-floor-against-unreachability-not-a-promise-of-good-affordances)):
stopping something is reached for under pressure.

Declared per [03 §2](03-capability-catalog.md#2-declaring-a-capability). Roles:
`V`iewer < `O`perator < `A`dmin.

| Capability | Kind | Role | Input | Output | Effects |
|---|---|---|---|---|---|
| `system.health` | Query | V | `{ include: [Sessions\|Clients\|Store\|Sources] }` | `Health` (§4.1) | — |
| `system.doctor` | Query | V | `{ groups: Vec<DoctorGroup>, strict: bool }` | `{ checks: [Check { id, group, status, detail, remedy, auto_fixable }], passed, failed, warnings }` | `READS_FS` |
| `system.doctor.fix` | Command | A | `{ checks: Vec<CheckId> }` | `{ fixed: [CheckId], failed: [(CheckId, String)] }` | `WRITES_FS` |
| `system.log.tail` | Query | A | `{ lines, follow: bool, filter: Option<String> }` | stream of `{ ts, level, target, span, message }` | `READS_FS` |
| `system.log.level` | Command | A | `{ directive: String }` | `{ applied: String, previous: String }` | — |
| `system.metrics` | Query | A | `{}` | Prometheus text (when `[metrics].enabled`) | — |
| `system.bug_report` | Command | A | `{ include: BugReportParts, dest: PathBuf, review: bool }` | `{ path, bytes, manifest, redaction_findings }` | `READS_FS`, `WRITES_FS` |
| `instance.info` | Query | V | `{}` | `{ id, version, pid, socket, listeners: [Listener], uptime, uid }` | — |
| `instance.shutdown` | Command | A | `{ force: bool, wait: bool, grace: Duration }` | `{ blocked_by: [SessionSummary], shutting_down: bool }` | `DESTRUCTIVE` |
| `instance.restart` | Command | A | `{ force: bool, wait: bool }` | `{ blocked_by: [SessionSummary] }` | `DESTRUCTIVE` |
| `service.install` | Command | A | `{ platform: Auto\|Launchd\|Systemd, autostart: bool }` | `{ path, loaded, notes: [String] }` | `WRITES_FS`, `SPAWNS_PROCESS` |
| `service.status` | Query | V | `{}` | `{ installed, loaded, restarts, since, lingering: Option<bool> }` | — |
| `service.uninstall` | Command | A | `{}` | `{ removed: PathBuf }` | `WRITES_FS`, `DESTRUCTIVE` |
| `upgrade.check` | Query | O | `{ channel }` | `{ current, available, notes, store_migration: Option<(u32,u32)> }` | `NETWORK` |
| `upgrade.apply` | Command | A | `{ version?, force: bool, wait: bool, binary_only: bool }` | `{ installed, restart: RestartPlan, blocked_by: [SessionSummary] }` | `NETWORK`, `WRITES_FS`, `SPAWNS_PROCESS`, `DESTRUCTIVE` |
| `remote.bootstrap` | Command | O | `{ host, consent: bool, mode: Upload\|Download\|AssumeInstalled, prefix: PathBuf }` | `{ installed_version, artifact, bytes, duration }` | `NETWORK`, `WRITES_FS`, `SPAWNS_PROCESS` |
| `remote.probe` | Query | O | `{ host }` | `{ present: bool, version?, os, arch, libc, has_zstd }` | `NETWORK` |
| `agent.wait` | Query | V | §8.3 | `AgentWaitResult` | — |
| `session.capture` | Query | V | `{ session, range: Blocks\|Lines\|All, format: Text\|Ansi\|Jsonl, max_bytes }` | `{ content, truncated, from, to }` | `READS_FS` |

`session.capture`'s `Ansi` format returns `unsupported` for a `native` session
([D8](decisions.md#d8--two-session-modes-pty-default-and-native-acp)): there is
no VT stream to reproduce — omt rendered that session itself from typed events,
so the faithful renderings are `Text` and `Jsonl`, and inventing escape
sequences omt never received would be a fabrication, not a capture.

**`system.doctor` is both a leaf capability and a namespace prefix** — it is
callable itself, and `system.doctor.fix` sits underneath it. It is the only such
name in the catalog, and the catalog **permits it**: names are resolved as whole
strings, never by walking a prefix tree, so `system.doctor` and
`system.doctor.fix` are two unrelated entries that happen to share a stem. The
alternative, `system_doctor_fix`, buys a rule nobody was enforcing at the cost of
a name no user would guess. The CLI renders them `omt doctor` and
`omt doctor fix`.

**`service.uninstall` removes the service unit and nothing else** — omt stays
installed and start-on-demand keeps working (§1.1). It is not
`uninstall.plan`/`uninstall.apply`, which remove omt's whole footprint
([19 §8](19-onboarding.md#8-uninstall-and-rollback)), and not `plugin.uninstall`,
which removes one plugin ([11](11-plugins.md)). `omt uninstall` calls
`service.uninstall` as one of its steps.

**`remote.probe` is `Operator`, not `Viewer`**, matching its sibling
`remote.bootstrap`. It declares `NETWORK` because it opens an outbound SSH
connection to a host the caller named, and [13 §4](13-security.md#4-roles-and-their-mapping-onto-the-catalog)
makes `Viewer` + `NETWORK` a CI failure with no available carve-out — the
read-only-subprocess exemption covers `SPAWNS_PROCESS` with `READS_FS` only, and
deliberately not `NETWORK`. A read-only shared link must not be able to make this
machine connect somewhere.

`system.doctor` being a `Query` at `Viewer` is deliberate: diagnosing your own
instance from your phone is exactly the situation where diagnosis is hardest to
get otherwise, and it reads nothing a `Viewer` cannot already see.
`system.log.tail` is `Admin` because a log tail, even redacted, is closer to the
raw record than anything else in the catalog.

Events emitted:

| Event | When |
|---|---|
| `instance.shutting_down` | shutdown initiated, with the grace window |
| `instance.degraded` | an `InstanceDegradation` (§4.1) was added to or cleared from `InstanceHealth.degraded` (§4.4) |
| `session.faulted` | a session entered `Degraded`, with the fault class |
| `session.fault_cleared` | a degraded session was reset |
| `health.threshold` | a budget in §9 was crossed, naming the session responsible |
| `service.state_changed` | the unit was installed, loaded or removed |
| `upgrade.available` | only after an explicit `upgrade.check` — never from a background poll |

---

## 11. OPEN QUESTIONS

1. **OPEN QUESTION — `catch_unwind` versus `panic = "abort"`.** §4.4's isolation
   invariant requires unwinding, but unwinding after a panic in code that was
   holding a lock or mid-mutation of the grid leaves state whose validity we
   assert but cannot prove. The mitigation is that a caught panic marks the
   session `Degraded` and freezes its grid rather than continuing to use it — but
   "freeze" still means the structure stays allocated and readable. The
   alternative is a process per session (real isolation, ~1 MB and a socket each,
   and a substantially different architecture). Needs a decision with the
   `omt-term` owner before the fault-isolation test is written, because the test
   encodes the answer.

2. **OPEN QUESTION — should the daemon self-restart after N panics?** A session
   that panics the parser every second produces a crash record every second. A
   circuit breaker (disable the session's parser entirely after 3 panics in 60 s)
   is obviously right; whether the *instance* should ever restart itself is less
   clear, because a restart kills every PTY (§1.4) — the cure is worse than
   almost any disease. Lean: never self-restart, always circuit-break the
   session.

3. **OPEN QUESTION — `omt doctor` for remote hosts by default.** §7.4 says
   doctor reports remote store pressure. Doing that on every `omt doctor` means
   the command's latency is the slowest ssh round trip, which will make people
   stop running it. Options: `--remote` opt-in (fast default, but the Pi's full
   SD card stays undiscovered), or run remote checks in parallel with a 2 s
   budget and report `(not checked: timed out)`. Lean: the latter, but it needs
   the `remote.probe` cost measured on a real slow link.

4. **OPEN QUESTION — metrics without a scrape target.** §4.3 exposes Prometheus
   text on the loopback listener. A user running omt on a laptop has no
   Prometheus, so the metrics are effectively write-only, while the people who
   *do* have one are running omt on servers where they would rather scrape it
   over the tailnet than over loopback. That implies binding the metrics endpoint
   separately from the API, which multiplies the listener/auth surface for a
   feature almost nobody will enable. Possibly the right answer is no metrics
   endpoint at all and `system.health --json` on a cron, which is one fewer
   listener. Needs a user, not an opinion.

5. **OPEN QUESTION — service install and the PATH problem.** §1.2 hard-codes a
   `PATH` into the launchd plist and asks systemd users to import theirs. Both
   are fragile: a user who installs an agent CLI with a new package manager six
   months later gets "claude not found" from a service they configured once and
   forgot. Alternatives: have the daemon source the user's login shell once at
   startup (`$SHELL -lic 'echo $PATH'` — slow, and it executes the user's rc as a
   side effect of starting a daemon), or resolve agent binaries through a
   `[agents.*].command` that `omt doctor` keeps fresh. Lean: keep the plist
   minimal, make `omt doctor agents` the standing fix, and reconsider if this
   becomes the top support question.

6. **OPEN QUESTION — how much of `omt doctor` belongs in `system.health`.** They
   overlap: health is continuous and cheap, doctor is on-demand and probes
   (terminal capability queries, ssh round trips, `integrity_check`). The current
   split is "health never blocks, doctor may", which is principled but means two
   places to add a check and two places to forget one. A single check registry
   where each check declares its cost, with health running only the `Cheap` ones,
   would be better and is probably worth doing before the check list grows past
   ~50. Needs a small design pass, not a decision.
