# VS Code Remote-SSH — Architecture Study

Benchmark study for omt's client/server SSH split. The question this document
answers is not "what is Remote-SSH like" but "what exactly does it do, at the
level of paths, environment variables, handshake frames and timeouts, so omt can
copy the parts that are right and skip the parts that only exist because VS Code
is 200 MB of JavaScript."

**Evidence grading is applied per claim:**

| Tag | Meaning |
|---|---|
| **VERIFIED** | Observed directly on this machine — file contents, log lines, or strings extracted from the shipped server bundle. Paths quoted verbatim. |
| **DOCUMENTED** | Stated in official Microsoft documentation (`code.visualstudio.com/docs/remote/*`). |
| **INFERRED** | Deduced from the above, or from the open-source relatives (`openvscode-server`, `code-server`). Flagged so it is not mistaken for fact. |

**Primary evidence base on this machine.** There is no `~/.vscode-server` here
(no Remote-SSH session has been made from this Mac), but there *is* a
`~/.cursor-server` — Cursor is a fork of VS Code and ships the identical
mechanism with renamed constants. That directory is the CLI/tunnel-server layout
rather than the Remote-SSH layout, but the **server payload underneath is the
same `vscode-reh-*` tarball, the same `server-main.js`, the same handshake, the
same ptyHost, the same `VSCODE_IPC_HOOK_CLI`**. Where a claim comes from Cursor,
the doc says so and gives the VS Code equivalent name.

Related omt docs: [07 — Remote protocol](../architecture/07-remote-protocol.md) ·
[09 — SSH and media](../architecture/09-ssh-and-media.md) ·
[02 — Crate map](../architecture/02-crate-map.md) ·
[03 — Capability catalog](../architecture/03-capability-catalog.md)

---

## 0. The shape in one diagram

```
LOCAL (laptop)                          │ ssh          REMOTE (dev box)
────────────────────────────────────────┼──────────────────────────────────────────
Electron main + renderer (the UI)       │              vscode-server / "REH"
  ├── workbench (monaco, all views)     │                (Remote Extension Host)
  ├── LOCAL extension host (Node)       │                ├── server-main.js  ← HTTP+WS
  │     └── "ui" extensions             │                ├── ManagementConnection
  │        (themes, keymaps, Remote-SSH)│                │     file ops, search, ext mgmt
  ├── web extension host (WebWorker)    │                ├── ExtensionHostConnection
  ├── OS clipboard                      │                │     └── REMOTE extension host
  ├── local file system                 │                │          "workspace" extensions
  └── ssh child process(es) ────────────┼── TCP/UDS ────►├── ptyHost (separate process)
        -L 127.0.0.1:<rand>:<remote>    │                │     └── shell / agent PTYs
                                        │                └── file watcher (parcel/inotify)
```

Everything the user *sees* is local. Everything the workspace *is* — files,
processes, terminals, language servers, debuggers — is remote. The wire in
between carries an RPC protocol, not pixels. That is the single most important
architectural fact, and it is the one omt already agrees with
([07 §4.2](../architecture/07-remote-protocol.md), byte-stream-primary).

---

## 1. The bootstrap

### 1.1 What "Connect to Host" actually does

Sequence (DOCUMENTED for the outline, INFERRED for exact ordering from the
`code-server`/`openvscode-server` sources and the Remote-SSH output channel):

| # | Step | Where |
|---|---|---|
| 1 | Resolve the host from `~/.ssh/config` (the extension *parses* the user's config; it does not replace ssh). | local |
| 2 | Spawn a real `ssh` child process. `remote.SSH.path` overrides the binary; PuTTY is explicitly unsupported. | local |
| 3 | Run a **platform probe** — `uname -s`/`uname -m` (or a PowerShell probe on Windows) — to pick the server build. Cached per host in `remote.SSH.remotePlatform`. | remote |
| 4 | Run an **install script** (a heredoc'd shell script piped over the ssh exec channel) that checks for `~/.vscode-server/bin/<commit>/`, and if absent downloads and untars the server. | remote |
| 5 | Start the server: `~/.vscode-server/bin/<commit>/bin/code-server --start-server --host=127.0.0.1 --port 0 --connection-token <token> --telemetry-level …`. It prints its listening port and token to stdout. | remote |
| 6 | Local extension establishes a **port forward** to that port and connects a WebSocket to it. | both |
| 7 | Client opens the **Management** connection, then the **ExtensionHost** connection, then N **Tunnel** connections for forwarded ports. | both |

**The commit hash is the version key.** The local client knows its own
`product.json.commit` and demands a server built from *exactly* that commit. No
negotiation, no compatibility window. **VERIFIED** — the shipped server's
`product.json` on this machine:

```json
"commit": "7d96c2a03bb088ad367615e9da1a3fe20fbbc6a0",
"quality": "stable",
"version": "2.5.26",
"vscodeVersion": "1.105.1",
"serverApplicationName": "cursor-server",
"serverDataFolderName": ".cursor-server",
"tunnelApplicationName": "cursor-tunnel",
"serverDownloadUrlTemplate":
  "https://cursor.blob.core.windows.net/remote-releases/${commit}/vscode-reh-${os}-${arch}.tar.gz"
```

VS Code's own template is
`https://update.code.visualstudio.com/commit:${commit}/server-${os}-${arch}/stable`
(DOCUMENTED via the FAQ's egress requirements: `update.code.visualstudio.com`
and `vscode.download.prss.microsoft.com` on 443).

**"REH"** in the tarball name stands for *Remote Extension Host* — the internal
name for the server. **VERIFIED** from this machine's CLI log:

```
[2026-03-02 11:52:41] info Downloading Cursor server ->
  /var/folders/2x/hqbz74zs7fvdxf_53693r26h0000gp/T/.tmpyIUnJk/vscode-reh-darwin-arm64.tar.gz
[2026-03-02 11:53:34] info server entrypoint: cursor-server
[2026-03-02 11:53:36] info Starting server...
[2026-03-02 11:53:36] info Server started
```

Note the 53-second gap between "Downloading" and "server entrypoint". That is
the first-connect cost, and it is the single loudest user complaint (§9).

### 1.2 The on-disk layout

Remote-SSH (DOCUMENTED, plus INFERRED for sub-paths from the CLI layout below):

```
~/.vscode-server/
├── bin/
│   └── <commit-40-hex>/          one full server install per client commit
│       ├── bin/
│       │   ├── code-server       bash → node out/server-main.js "$@"
│       │   ├── remote-cli/code   bash → node out/server-cli.js  APP VERSION COMMIT EXEC "$@"
│       │   └── helpers/browser.sh  bash → node out/server-cli.js … --openExternal "$@"
│       ├── node                  a full bundled Node runtime (~110 MB)
│       ├── out/server-main.js    the entire server, one bundle
│       ├── out/server-cli.js     the `code` CLI shim
│       ├── out/bootstrap-fork.js how child processes (extension host, ptyHost) are spawned
│       ├── extensions/           built-in extensions (~48 dirs)
│       ├── node_modules/         ~184 packages incl. native addons (node-pty, spdlog, …)
│       └── product.json
├── data/
│   ├── Machine/                  machine-scoped settings
│   ├── User/                     settings, keybindings, globalStorage, History
│   ├── logs/
│   └── machineid
├── extensions/                   installed *workspace* extensions
└── .<commit>.log                 per-commit bootstrap log
```

The CLI/tunnel layout is different and **VERIFIED** verbatim on this machine:

```
/Users/hanxiao.du/.cursor-server/
├── .cli.7d96c2a03bb088ad367615e9da1a3fe20fbbc6a0.log
├── .cli.b3573281c4775bfc6bba466bf6563d3d498d1070.log
├── cli/servers/
│   ├── lru.json      ["Stable-7d96c2a…", "Stable-b357328…"]
│   ├── Stable-7d96c2a03bb088ad367615e9da1a3fe20fbbc6a0/
│   │   ├── log.txt   (27 KB of server log)
│   │   ├── pid.txt   (5 bytes: the pid)
│   │   └── server/   ← the unpacked vscode-reh tarball, exactly as above
│   └── Stable-b3573281c4775bfc6bba466bf6563d3d498d1070/
├── data/  ├── CachedExtensionVSIXs/.trash/  ├── CachedProfilesData/
│          ├── Machine/  ├── User/  ├── logs/  └── machineid
└── extensions/
```

Two observations worth stealing:

- **`pid.txt` + `log.txt` next to each server install** is how the CLI answers
  "is a server already running for this commit?" without a registry.
  **VERIFIED** from `.cli.<commit>.log`:
  ```
  info Checking …/Stable-7d96c2a…/log.txt and …/Stable-7d96c2a…/pid.txt for a running server...
  info Found running server (pid=50815)
  ```
- **`lru.json` is the GC policy** — a plain array of install directory names,
  most-recent-first. Old entries past a cap are deleted. **VERIFIED** file
  contents above (two entries, matching the two directories present). This is
  about as simple as version GC gets, and it works.

### 1.3 Choosing the right build

**DOCUMENTED requirements** for the prebuilt Linux server, current as of VS Code
1.99+ (a hard bump that broke every CentOS 7 / Ubuntu 18.04 user on the same
day):

| Requirement | Minimum |
|---|---|
| Linux kernel | ≥ 4.18 |
| glibc | ≥ 2.28 |
| libstdc++ | ≥ 3.4.25 |
| Remote host packages | `bash` at `/bin/bash`, `tar`, and `curl` **or** `wget` |
| RAM | 1 GB min, 2 GB + 2 cores recommended |

`${os}-${arch}` resolves to `linux-x64`, `linux-arm64`, `linux-armhf`,
`darwin-x64`, `darwin-arm64`, `win32-x64`, `alpine-x64`, `alpine-arm64`
(INFERRED from the template and the published artifact names). **Alpine is
explicitly unsupported for Remote-SSH** despite an alpine build existing — it is
there for Dev Containers. There is no musl story.

The probe is `uname`, so a host whose `uname -m` reports something the template
does not cover (e.g. `ppc64le`, `riscv64`) simply cannot be connected to. This
is a real limitation, not an oversight: the server ships a **prebuilt Node
binary plus native node_modules**, so "just compile it" is not available to the
user.

### 1.4 A host with no internet

**DOCUMENTED:** "The Remote-SSH extension attempts downloading on the remote
host first, then falls back to downloading locally and transferring it." That
fallback is the entire offline story and it is controlled by
`remote.SSH.localServerDownload` (`auto` | `always` | `off`).

- `auto` — try remote `curl`/`wget`; on failure download on the laptop and
  `scp`/pipe the tarball over the ssh channel.
- `always` — never let the remote reach the internet. This is the setting
  air-gapped and proxy-hostile corporate environments need, and it is not the
  default, which is why the first connect on a locked-down host takes minutes
  and then fails with a curl error.

The manual escape hatch is documented in the community rather than the docs:
download `server-linux-x64.tar.gz` for your commit, `mkdir -p
~/.vscode-server/bin/<commit>`, untar with `--strip-components=1`, `touch
~/.vscode-server/bin/<commit>/0` (the "install complete" sentinel the script
looks for). **INFERRED** from the install-script behaviour; the sentinel file
name has changed across versions and should not be relied on.

### 1.5 Update and garbage collection

- **Update** = the client updated, therefore the commit changed, therefore a
  *whole new server install* is downloaded. There is no in-place patch. A
  monthly VS Code release costs every remote host another ~350 MB of disk and
  another cold-start download.
- **GC** = LRU over install directories (**VERIFIED** `lru.json` above). VS Code
  Server's CLI keeps a small number (3 in recent versions, INFERRED) and deletes
  the rest. Remote-SSH historically kept *everything* and required
  **Remote-SSH: Uninstall VS Code Server from Host…**, which is the documented
  cure (DOCUMENTED):
  ```bash
  kill -9 $(ps aux | grep vscode-server | grep $USER | grep -v grep | awk '{print $2}')
  rm -rf $HOME/.vscode-server
  ```
  The fact that this snippet appears in the *official troubleshooting docs* is
  itself a finding: the design assumes the remote state directory is disposable
  and users will periodically nuke it.
- Extension VSIX caching also GCs. **VERIFIED** on this machine —
  `data/CachedExtensionVSIXs/.trash/` holds two 100 MB Claude Code VSIXs pending
  deletion. A "move to `.trash`, sweep later" pattern, presumably because
  deleting a file that a running extension host has open is a Windows problem.

---

## 2. The transport

### 2.1 It is a WebSocket over an SSH port forward — not stdio

This is the part most people get wrong. The server is a **real HTTP server**
that speaks WebSocket upgrades; ssh is used only to (a) start it and (b) carry
TCP to it.

**VERIFIED** from the shipped bundle's argv parser — the server's own options:

| Flag | Purpose |
|---|---|
| `--start-server` | run as a server rather than a CLI |
| `--port <port \| range>` | listen on TCP; `0` = pick a free port and print it |
| `--socket-path <path>` | listen on a **Unix domain socket** instead |
| `--connection-token <token>` | the shared secret (deprecates `--connectionToken`) |
| `--connection-token-file <path>` | read it from a file (deprecates `--connection-secret`, `--connectionTokenFile`) |
| `--server-base-path <path>` | serve under a URL prefix |
| `--server-data-dir <dir>` | override `~/.vscode-server` |
| `--enable-remote-auto-shutdown` | exit when the last extension host disconnects |
| `--remote-auto-shutdown-without-delay` | …immediately, rather than after a timer |
| `--accept-server-license-terms`, `--telemetry-level`, `--use-host-proxy`, `--without-browser-env-var` | assorted |

Both listen modes are real and both are used:

- **TCP loopback** is the Remote-SSH default. **VERIFIED** from the CLI log:
  `Listening on 127.0.0.1:62397`.
- **Unix socket** is what the CLI/tunnel path uses, and what
  `Remote.SSH: Remote Server Listen On Socket` switches Remote-SSH to.
  **VERIFIED** from the server log on this machine:
  ```
  Server bound to /var/folders/2x/hqbz74zs7fvdxf_53693r26h0000gp/T/cursor-a2f5cafa-4300-46e5-9f44-fbb018fd37a6
  Extension host agent listening on /var/folders/…/cursor-a2f5cafa-…
  ```
  The socket mode exists **for multi-user security**: a loopback TCP port on a
  shared host is reachable by every other user on that box, and the connection
  token is the only thing stopping them. A `0600` Unix socket is not. DOCUMENTED
  as the mitigation for "multi-user security concerns".

**Environment variable `VSCODE_AGENT_FOLDER`** overrides the data root, and the
default falls back to `product.json.serverDataFolderName` or the literal
`.vscode-remote`. **VERIFIED** from the bundle:

```js
ka = kr["server-data-dir"] || process.env.VSCODE_AGENT_FOLDER
   || path.join(os.homedir(), product.serverDataFolderName || ".vscode-remote")
```

### 2.2 Two forwarding strategies, and why there are two

`remote.SSH.useLocalServer` (DOCUMENTED) selects between them:

| Mode | Mechanism | Notes |
|---|---|---|
| `true` (default, Linux/macOS) | The extension runs a **local Node "local server"** which owns the ssh child and does the forwarding in-process. | Better multiplexing, fewer ssh processes, but it is the source of a good fraction of the "connection hangs" bugs. |
| `false` | Plain `ssh -L 127.0.0.1:<random-local>:127.0.0.1:<remote-port> host`, one child process. | The documented workaround for Windows OpenSSH bugs. Simpler, more ssh processes. |

The local port is chosen at random (DOCUMENTED — "the port to tunnel on the
client is selected at random"), which is why there is no way to pin it and why
`Change Local Address Port` exists only for *user* forwarded ports.

**`AllowTcpForwarding yes` in the remote `sshd_config` is a hard requirement.**
The diagnostic is the exact string `open failed: administratively prohibited` in
the Remote-SSH output channel (DOCUMENTED). A hardened bastion that disables
forwarding cannot run Remote-SSH at all. This matters for omt (§Implications).

### 2.3 ControlMaster

**Not automatic.** VS Code does *not* write `ControlMaster` into your ssh
config; it *documents* that you should (DOCUMENTED, verbatim from the
troubleshooting page):

```
Host *
    ControlMaster auto
    ControlPath  ~/.ssh/sockets/%r@%h-%p
    ControlPersist  600
```

with `mkdir -p ~/.ssh/sockets`. Without it, each connection attempt is a fresh
authentication — which for hardware-token or MFA-gated hosts means a prompt per
reconnect, and reconnects are frequent. Note this machine's user has already
solved it by hand for one host — **VERIFIED** in `~/.ssh/config`:

```
Host janus
    ControlMaster auto
    ControlPath /tmp/ssh-janus.sock
    ControlPersist 600
    ServerAliveInterval 15
    ServerAliveCountMax 4
```

That is exactly the config omt's `07 §2.4` already specifies generating. omt is
right and VS Code is wrong here: leaving multiplexing to documentation means
most users never get it.

### 2.4 The handshake — connection token and `signedData`

Three logical connections share one WebSocket endpoint, distinguished by a
handshake. **VERIFIED** from `server-main.js`.

The upgrade request carries its state in the **query string**, not headers:

```
GET /?reconnectionToken=<uuid>&reconnection=true&skipWebSocketFrames=false
    &permessageDeflate=true …
Upgrade: websocket
```

**VERIFIED** parse:
```js
if (typeof d.reconnectionToken == "string") s = d.reconnectionToken;
if (d.reconnection === "true") r = true;
if (d.skipWebSocketFrames === "true") n = true;
```

Then a two-message challenge on the socket:

```
server → client   { "type": "sign", "data": <string>, "signedData": <string> }
client → server   { "type": "connectionType",
                    "signedData": <string>,
                    "commit": "<40-hex>",
                    "desiredConnectionType": 1 | 2 | 3,
                    "args": { … } }
```

Server-side validation, **VERIFIED** (deminified):

```js
const v = m.commit, w = this._productService.commit;
if (v && w && v !== w) { /* log commit mismatch */ }
let S = false;
if (!a) S = true;                                     // no signing service
else if (this._connectionToken.validate(m.signedData)) S = true;   // ← the token
else try { S = (a.validate(m.signedData) === "ok"); } catch {}
if (!S) { if (this._environmentService.isBuilt) return d("…"); }
```

Reading of this:

- **The connection token is a bearer secret.** `signedData` is accepted if it
  *is* the connection token. The extra `a.validate(...)` branch is the Microsoft
  build's signing-service path (the "Web-UI is served" / vscode.dev case) and is
  absent in OSS builds. In a self-built (`!isBuilt`) server, **validation
  failure is logged and the connection is allowed anyway** — a development
  affordance that is also exactly the kind of thing you do not want to ship.
- **Commit mismatch is checked and logged but does not by itself reject.** The
  actual incompatibility bites later, in the RPC layer, which is why version-skew
  failures present as weird runtime errors rather than a clean "wrong version".
- The token is passed as `--connection-token` on the server command line — which
  means it is **visible in `ps` to every user on the host**. This is precisely
  why `--connection-token-file` exists, and why socket mode exists.

`desiredConnectionType`, **VERIFIED** from the dispatch:

| Value | Name | Purpose |
|---|---|---|
| 1 | `ManagementConnection` | file system, search, extension management, the "everything else" RPC channel |
| 2 | `ExtensionHostConnection` | one per remote extension host process; carries the extension RPC |
| 3 | `Tunnel` | `this._createTunnel(socket, args)` — a forwarded port, multiplexed over the same WebSocket |

**Multiplexing is at the WebSocket level, not the SSH level.** N logical
connections, each its own WebSocket, all through the one forwarded TCP port,
which is itself one ssh channel. `permessageDeflate` is negotiated per
connection and `inflateBytes` is passed across a reconnect so the deflate
context survives — **VERIFIED** in the connection-transfer payload:

```js
{ type: "VSCODE_EXTHOST_IPC_SOCKET",
  initialDataChunk: <base64>,
  skipWebSocketFrames: e,
  permessageDeflate: t,
  inflateBytes: <base64> }
```

That message name is worth pausing on: the server can **hand a live socket to a
child process** (the extension host) with the already-buffered bytes and the
compression state. That is how the extension host talks to the client *directly*
after setup rather than proxying every frame through the parent. It is a genuine
piece of engineering and it is the reason extension RPC latency is tolerable.

---

## 3. The process model on the remote

### 3.1 What runs

| Process | Started by | Lifetime | Dies when |
|---|---|---|---|
| `node out/server-main.js` (the server / REH) | the bootstrap script over ssh | **outlives the SSH connection** | grace timer expires, or `--enable-remote-auto-shutdown` fires, or the host reboots, or the user kills it |
| Remote **extension host** (`bootstrap-fork.js`) | server, one per window/workspace | tied to `ExtensionHostConnection` + grace timer | its connection is disposed |
| **ptyHost** | server (via `ptyHostService`) | outlives client disconnects; owns all PTYs | grace timer, or crash (auto-restarted) |
| **File watcher** | inside the extension host / server | per watched workspace | workspace closed |
| Per-terminal shell processes | ptyHost | independent of any connection | user exits the shell, or revive scrollback expires |

**VERIFIED** from a real server log on this machine:

```
Extension host agent started.
[<unknown>][780ab572][ManagementConnection] New connection established.
[<unknown>][8281eca8][ExtensionHostConnection] New connection established.
```

The bracketed hex is `reconnectionToken.substr(0, 8)` — **VERIFIED** from the
log-prefix code. Every log line is keyed by the connection identity, which makes
"which of my four windows is doing this" answerable. Cheap, and omt should do
the same.

### 3.2 The reconnect model — the part omt should copy verbatim

**The server keeps running. The client reattaches to the same objects.** There
is no session restart, no re-spawn, no scrollback loss. Mechanically:

1. Every logical connection is created with a client-generated
   **`reconnectionToken`** (a UUID) that is stable across TCP drops.
2. When the socket closes, the server does **not** tear down. It logs and starts
   a timer. **VERIFIED**:
   ```js
   this.protocol.onDidDispose(() => {
     this._log(`The client has disconnected, will wait for reconnection
                ${ms(this._reconnectionGraceTime)} before disposing...`);
     this._disconnectRunner1.schedule();
   });
   ```
3. A reconnect arrives as a new WebSocket with `reconnection=true` and the
   **same** `reconnectionToken`. The server matches it to the parked connection,
   cancels the timer, replays `initialDataChunk`, and continues.
4. If a *different* client connects for the same resource, the long timer is
   replaced by a short one. **VERIFIED**:
   ```js
   this._log(`Another client has connected, will shorten the wait for reconnection
              ${ms(this._reconnectionShortGraceTime)} before disposing...`);
   this._disconnectRunner2.schedule();
   ```

**The two grace times, VERIFIED as literals in the shipped bundle:**

| Constant | Value | Meaning |
|---|---|---|
| `_reconnectionGraceTime` | `108e5` ms = **10 800 000 ms = 3 hours** | how long a disconnected connection is held open waiting for its client |
| `_reconnectionShortGraceTime` | `3e5` ms = **300 000 ms = 5 minutes** | shortened window once someone *else* has taken over |

Three hours is a deliberate, generous number: it means closing the laptop lid
over lunch and reopening it resumes the *exact* session — same extension host,
same language server state, same terminals, no reindex. This is the single
biggest quality-of-life property of Remote-SSH and it is entirely a consequence
of "the server is not owned by the connection".

The same constants are handed down to the ptyHost as environment. **VERIFIED**:

```js
VSCODE_PIPE_LOGGING: "true",
VSCODE_VERBOSE_LOGGING: "true",
VSCODE_RECONNECT_GRACE_TIME: this._reconnectConstants.graceTime,
VSCODE_RECONNECT_SHORT_GRACE_TIME: this._reconnectConstants.shortGraceTime,
VSCODE_RECONNECT_SCROLLBACK: this._reconnectConstants.scrollback
```

and constructed as **VERIFIED**:

```js
createInstance(ReconnectConstants, {
  graceTime: 108e5,
  shortGraceTime: 3e5,
  scrollback: config.getValue("terminal.integrated.persistentSessionScrollback") ?? 100
});
```

### 3.3 What does *not* survive

- **Host reboot.** Nothing persists to disk. All terminals are gone.
- **Grace expiry.** After 3 hours, `_cleanResources()` disposes the extension
  host and its terminals.
- **`--enable-remote-auto-shutdown`.** **VERIFIED** logic: on the last extension
  host connection closing, `console.log("Last EH closed, waiting before shutting
  down")` then a timer, unless `--remote-auto-shutdown-without-delay`. This is
  how ephemeral/Codespaces-style hosts avoid an immortal server. It is **off**
  for ordinary Remote-SSH, which is why a `~/.vscode-server` node process is
  still running on your dev box from three weeks ago.

---

## 4. The local/remote split

### 4.1 The rule

The declared rule is the manifest field `extensionKind`, an **ordered preference
array** (DOCUMENTED):

| `extensionKind` | Behaviour |
|---|---|
| `["ui"]` | local only. Cannot load in a remote-only scenario. |
| `["workspace"]` | remote only (runs where the workspace is). |
| `["ui", "workspace"]` | prefer local, relocate to remote if it cannot run locally. |
| `["workspace", "ui"]` | prefer remote, fall back to local. |
| *(absent)* | **default is `workspace`** if the extension has a `main`; `ui` if it only contributes declaratively (themes, snippets, grammars, keymaps). |

Resolution order (DOCUMENTED): enumerate available hosts → check what the
extension can run in (Node vs web) → apply `extensionKind` → prefer a Node host
over a web host.

**There is an override layer nobody documents loudly.** `product.json` ships a
hard-coded `extensionKind` map that overrides the extension's own manifest for
extensions Microsoft has decided got it wrong. **VERIFIED** on this machine:

```json
"extensionKind": {
  "Shan.code-settings-sync": ["ui"],
  "shalldie.background": ["ui"],
  "techer.open-in-browser": ["ui"],
  "CoenraadS.bracket-pair-colorizer-…": ["ui"],
  …
},
"extensionPointExtensionKind": { "typescriptServerPlugins": ["workspace"] }
```

And users can override *that* with the `remote.extensionKind` setting. So there
are three layers of the same decision, which is a smell.

### 4.2 The actual split

| Responsibility | Local | Remote | Notes |
|---|---|---|---|
| Workbench UI, editor rendering, all views | ✅ | | Monaco runs on the laptop; only text crosses the wire. |
| Themes, keymaps, snippets, grammars | ✅ | | Declarative — no host needed. |
| Settings sync, window management, `Remote-SSH` itself | ✅ | | Cannot function remotely by definition. |
| OS clipboard | ✅ | | The remote never touches a clipboard. |
| File system provider (`vscode-remote://ssh-remote+host/path`) | | ✅ | Every read/write is an RPC. |
| Search (ripgrep) | | ✅ | Runs on the remote; only results cross. |
| File watching | | ✅ | inotify on the remote — see §7. |
| Terminals | | ✅ | ptyHost, §5. |
| Tasks | | ✅ | `cwd` is remote. |
| Debugging | | ✅ | Debug adapter runs remotely; the UI is local. |
| Language servers | | ✅ | The whole reason for the architecture. |
| Source control (git) | | ✅ | Except credential helpers, which need the local agent → SSH agent forwarding. |
| Port forwarding UI | ✅ | detection ✅ | §6.5. |

### 4.3 Failure mode when an extension guesses wrong

The failure is **silent and confusing**, which is the design's worst property:

- A `ui` extension that actually needed workspace files sees an **empty
  workspace**. `vscode.workspace.workspaceFolders` returns `vscode-remote://`
  URIs it cannot `fs.readFile`. Symptom: "the extension does nothing", no error.
- A `workspace` extension that needed local resources (a locally installed
  binary, the clipboard, a GUI) **fails at runtime** with ENOENT for a tool the
  user can see on their laptop.
- Extensions that use Node `fs` and `path` directly rather than
  `vscode.workspace.fs` break the moment they land remotely. This is the single
  most common remote extension bug.
- The user-visible cue is a **"Install in SSH: host"** button in the extensions
  view. The user is expected to notice that an extension is greyed out on one
  side. Most do not.

Root cause: `extensionKind` is a *declaration of intent* that is never checked
against behaviour. Nothing stops a `ui` extension from calling `fs.readFile` on
a workspace path; it just returns nothing. omt's capability catalog
([03](../architecture/03-capability-catalog.md)) is the right shape to avoid
this, because a capability declares *where it executes* as part of its type
rather than as a hint.

---

## 5. Terminals — the most relevant section for omt

### 5.1 End to end

```
xterm.js (local renderer)
   │  keystrokes as RPC over the Management/ExtHost channel
   ▼
server-main.js  ─ ptyHostService (a proxy) ─┐
                                            │ Node IPC (message port)
                                            ▼
                                        ptyHost process
                                            │ node-pty
                                            ▼
                                       PTY master ── shell / agent
```

`ptyHost` is a **separate process** on purpose: `node-pty` is a native addon and
a PTY-related crash must not take down the file system provider and the
extension host with it. **VERIFIED** — `hl` enum in the bundle:
`{ LocalPty, PtyHost, PtyHostWindow, Logger, Heartbeat }`, and a `Heartbeat`
channel exists specifically to detect a wedged ptyHost. There are also
`terminal.integrated.developer.ptyHost.latency` and
`…ptyHost.startupDelay` settings for deliberately injecting latency to test the
reconnect paths — a nice sign the team took this seriously.

### 5.2 The frames

The terminal protocol is a typed RPC channel, not a byte pipe. Creation, from
the **VERIFIED** call site:

```js
$createProcess({
  shellLaunchConfig, initialCwd, cols, rows, unicodeVersion, env,
  resolverEnv,
  shouldPersistTerminal,       // ← the persistence opt-in
  workspaceId, workspaceName,
  workspaceFolders, activeWorkspaceFolder, activeFileResource,
  options
})
```

Every terminal is addressed by a **`persistentProcessId`** — a small integer
handed back by the ptyHost — and *not* by a connection-scoped handle.
**VERIFIED** from the command-execution path:

```js
this._onExecuteCommand.fire({ reqId, persistentProcessId, commandId, commandArgs })
```

This is the key design decision: **terminal identity is owned by the ptyHost,
not by the client connection**. A client can vanish and come back and ask for
`persistentProcessId: 7` again. Compare omt's `SessionId`, which already has
this property.

The channel carries (INFERRED from `IPtyService` in the open-source tree, names
stable across versions): `$createProcess`, `$start`, `$input`, `$resize`,
`$acknowledgeDataEvent` (flow control), `$shutdown`, `$attachToProcess`,
`$detachFromProcess`, `$listProcesses`, `$getTerminalLayoutInfo` /
`$setTerminalLayoutInfo`, `$orphanQuestionReply`, and events
`$onProcessData`, `$onProcessExit`, `$onProcessReady`,
`$onProcessReplay`, `$onDidChangeProperty`.

**Flow control is explicit and ack-based.** `$acknowledgeDataEvent(id, charCount)`
exists because a fast-scrolling remote process on a slow link would otherwise
buffer without bound. VS Code pauses the pty reader when unacked characters
exceed a high-water mark. omt's `07 §6` backpressure design is the same idea
with a better vocabulary (lossy vs lossless queues).

### 5.3 Resize

`$resize(id, cols, rows)` sets `TIOCSWINSZ`. There is **one** PTY size, owned by
whichever window has the terminal focused. VS Code sidesteps omt's hard
multi-viewport problem ([07 §4.3](../architecture/07-remote-protocol.md#43-the-resize-problem))
by simply not having it — a terminal belongs to one window. When the same
persistent terminal is attached from a second window, the second window's
dimensions win and the first sees a reflow. Users notice.

### 5.4 Reconnect and scrollback restoration — the mechanism

This is the crown jewel and it is simpler than people assume.

1. Terminals created with `shouldPersistTerminal: true` (the default; disabled
   by `terminal.integrated.enablePersistentSessions: false`) are kept alive in
   the ptyHost after the client disconnects.
2. The ptyHost maintains a **bounded recorder** per persistent process: the last
   N lines of *terminal state*, where N =
   `terminal.integrated.persistentSessionScrollback`, **default 100**
   (**VERIFIED**, `?? 100` in the constructor above), passed to the ptyHost as
   `VSCODE_RECONNECT_SCROLLBACK`.
3. On reattach, `$attachToProcess(id)` is followed by a **`$onProcessReplay`**
   event carrying the recorded buffer — as ANSI, plus the current cursor
   position and dimensions — which xterm.js writes into a fresh buffer before
   live data resumes.
4. The recorder is **state, not a byte log**. It records enough to reconstruct
   the visible screen plus N lines of scrollback, not every byte the process ever
   emitted. A process that printed 4 GB replays 100 lines.

Consequences, all real and all complained about:

- **Scrollback above the last 100 lines is lost on reconnect.** Users raise the
  setting to 1000+ and then complain about ptyHost memory.
- **A full-screen TUI (vim, tmux, an agent TUI) replays as whatever it had drawn
  at disconnect time**, which is correct as a screen but has no history. This
  is fine because the alt screen has no scrollback anyway.
- `terminal.integrated.persistentSessionReviveProcess`
  (`onExit` | `onExitAndWindowClose` | `never`, **VERIFIED** as a settings key)
  additionally serializes terminal *state* to disk so terminals reappear —
  **but as dead, non-interactive buffers with a "restart" affordance**, not as
  live processes. That is precisely omt's `SessionState::Orphaned`
  ([05 §8](../architecture/05-session-model.md#8-persistence-and-restore)).
  Converged design, arrived at independently.

### 5.5 Shell integration

VS Code injects a shell rc fragment (`shellIntegration-bash.sh`, `-zsh.zsh`,
`.ps1`, `-rc.zsh`, `-login.zsh`, `.fish`) that emits **OSC 633** — VS Code's
private superset of OSC 133 — marking prompt start `633;A`, command start
`633;B`, pre-execution `633;C`, exit code `633;D;<code>`, cwd `633;P;Cwd=`, and
the full command line `633;E;<cmd>`. This is how command decorations, "rerun
command", and sticky scroll work.

Relevant to omt: **VS Code chose to inject rather than infer**, and it chose a
private OSC because OSC 133 does not carry the command text or exit code
reliably. omt's [P4](../architecture/01-principles.md#p4--native-semantics-observe-never-re-implement)
prefers observing OSC 133 natively — which is the more respectful choice, but it
should be understood that the reason VS Code has richer command blocks is that
it took the impolite option.

---

## 6. Clipboard, files, media and the CLI trick

### 6.1 Clipboard

Copy/paste is **entirely local**. The renderer is local, so `Ctrl-C` in the
editor and `Ctrl-C` in xterm.js both hit the local OS clipboard directly.
Nothing crosses the wire. There is no OSC 52 problem, no tier ladder, no
terminal detection.

This is worth stating plainly because it is the cleanest possible answer and it
is **unavailable to omt in the general case** — omt's renderer is a terminal
inside somebody else's terminal emulator. omt's `09 §5` tier ladder exists
precisely because omt does not own the rendering surface. The one omt
configuration that *does* get VS Code's clean answer is the web/desktop client
and `omt --remote` with the local TUI — which is another argument for pushing
users there.

### 6.2 Opening a remote file "locally"

There is no such thing. A remote file is `vscode-remote://ssh-remote+<host>/path`
and every operation on it is an RPC through the Management connection's file
system provider. The editor never has a local copy. Binary assets are fetched
over an HTTP route on the server, **VERIFIED**:

```js
if (route === "/vscode-remote-resource") {
  const p = query.path;
  if (typeof p !== "string") return respond(400, "Bad request.");
  …
}
```
guarded immediately above by
```js
if (!validateConnectionToken(this._connectionToken, req, parsedUrl))
  return respond(403, "Forbidden.");
```

So: **images in the editor are served over authenticated HTTP through the same
forwarded port, with the connection token checked per request.** That is exactly
omt's `GET /v1/blob/<id>` design ([09 §8](../architecture/09-ssh-and-media.md#8-security)).
Convergent again.

### 6.3 Drag and drop

Dragging a local file onto a remote window uploads it (the file is read locally
and written via the remote FS provider). Dragging out is a download. It works
but is slow for large files because every write is an RPC round trip through a
provider designed for source files, not for 500 MB tarballs. Users are told to
use `scp`.

### 6.4 `VSCODE_IPC_HOOK_CLI` — the trick, precisely

The scenario: you are in an integrated terminal **on the remote host**. You type
`code foo.rs`. A tab opens **in the local window**. No new process, no new ssh
connection, no X forwarding.

Mechanism, **VERIFIED end to end on this machine**:

1. The server generates a per-terminal-process socket path:

   ```js
   function M1() {
     const e = generateUuid();
     if (process.platform === "win32")
       return `\\\\.\\pipe\\vscode-ipc-${e}-sock`;
     const t = (process.platform !== "darwin" && XDG_RUNTIME_DIR)
                 ? XDG_RUNTIME_DIR
                 : os.tmpdir();
     const i = path.join(t, `vscode-ipc-${e}.sock`);
     registerForCleanup(i);
     return i;
   }
   ```

   So on Linux the path is `$XDG_RUNTIME_DIR/vscode-ipc-<uuid>.sock`
   (i.e. `/run/user/$UID/vscode-ipc-*.sock`), on macOS it is under `os.tmpdir()`
   because macOS has no `XDG_RUNTIME_DIR`, and on Windows it is a named pipe.

2. The path is injected into **each terminal's environment at creation time**,
   immediately before `$createProcess`. **VERIFIED**:

   ```js
   const m = M1();
   p.VSCODE_IPC_HOOK_CLI = m;
   const v = await this._ptyHostService.createProcess(
       shellLaunchConfig, initialCwd, cols, rows, unicodeVersion, p /* env */, …);
   ```

   Note it is **per terminal**, not per server — each terminal gets a fresh UUID
   socket bound to the window that created it. That is what makes `code` open a
   tab in *the right window* when you have four windows on the same host.

3. `$PATH` in the terminal is prefixed with
   `~/.vscode-server/bin/<commit>/bin/remote-cli/`, which contains a `code`
   script. **VERIFIED** (Cursor's, byte for byte):

   ```bash
   #!/usr/bin/env bash
   realdir() { … }                       # resolve symlinks
   ROOT="$(dirname "$(dirname "$(realdir "$0")")")"
   APP_NAME="cursor"
   VERSION="2.5.26"
   COMMIT="7d96c2a03bb088ad367615e9da1a3fe20fbbc6a0"
   EXEC_NAME="cursor"
   CLI_SCRIPT="$ROOT/out/server-cli.js"
   "$ROOT/node" "$CLI_SCRIPT" "$APP_NAME" "$VERSION" "$COMMIT" "$EXEC_NAME" "$@"
   ```

4. `server-cli.js` reads `VSCODE_IPC_HOOK_CLI`, connects to that Unix socket,
   and POSTs a JSON command (`{ type: "open", fileURIs, folderURIs, …,
   forceNewWindow, waitMarkerFilePath }`). The server relays it up the
   Management connection to the local window, which opens the tab.

5. **`code --wait`** (for `$EDITOR`, `git commit`) works by the CLI creating a
   marker file and blocking until the local window deletes it on editor close.

**The same trick is reused for `$BROWSER`.** **VERIFIED** —
`bin/helpers/browser.sh` is the identical script with `--openExternal` appended,
and it is exported as `BROWSER` in the terminal environment so that a remote
process calling `xdg-open https://…` opens a tab on *your laptop*.

**Failure mode users hit constantly:** the variable is only set in terminals VS
Code created. In a `tmux` session started before the connection, in a `ssh`
session from a different terminal, or in a detached screen, `code` is either
missing or points at a dead socket. The community workaround is to pick the
newest socket at shell start:

```bash
export VSCODE_IPC_HOOK_CLI=$(ls -t /run/user/$UID/vscode-ipc-*.sock 2>/dev/null | head -1)
```

which is wrong the moment you have two windows open, and stale the moment VS
Code restarts. This is a real, permanent, unfixed wart — see §9 and the omt
implications.

### 6.5 Port forwarding

- **Auto-forward** watches for `Listening on <port>` / `localhost:<port>`-shaped
  output in the integrated terminal and forwards it. `remote.autoForwardPorts`
  (default on), `remote.autoForwardPortsSource` = `process` (scan `/proc/net/tcp`
  for new listeners) | `output` (regex the terminal output) | `hybrid`.
- `remote.portsAttributes` maps ports to behaviours:
  ```json
  { "3000": { "label": "app", "onAutoForward": "openBrowser" },
    "9229": { "onAutoForward": "silent" },
    "*":    { "onAutoForward": "notify" } }
  ```
  `onAutoForward` ∈ `notify` | `openBrowser` | `openBrowserOnce` | `openPreview`
  | `silent` | `ignore`.
- Forwarded ports ride `desiredConnectionType: 3` (Tunnel) over the same
  WebSocket — **VERIFIED** `this._createTunnel(socket, args)` — so a forwarded
  port does **not** cost an ssh channel or an `ssh -L`.
- The Ports view shows local address, remote port, running process, and origin
  (auto / user / from `LocalForward`).

Scanning `/proc/net/tcp` is the good source; regexing terminal output is the
one that misfires (it will forward a port number that happened to appear in a
log line). `hybrid` is the default in recent builds.

---

## 7. Failure modes and diagnostics

| Failure | Symptom | Cause | Fix |
|---|---|---|---|
| **Version skew** | Connection hangs at "Setting up SSH host: downloading VS Code Server", or opens then behaves oddly | Local client updated → new commit → new server download; or a stale server from the old commit is still bound | **Remote-SSH: Kill VS Code Server on Host** (DOCUMENTED as the general-purpose fix) |
| **`~/.vscode-server` corruption** | Interrupted download leaves a half-untarred `bin/<commit>/`; every connect fails identically | The install script's completion sentinel is present but the tree is not | **Remote-SSH: Uninstall VS Code Server from Host…**, or the documented `rm -rf $HOME/.vscode-server` |
| **glibc / arch mismatch** | `/lib64/libc.so.6: version 'GLIBC_2.28' not found`, or the server exits immediately with no message | Prebuilt server needs glibc ≥ 2.28 since 1.99 (DOCUMENTED); CentOS 7, Ubuntu 18.04, Amazon Linux 2 all broke | Pin an older VS Code, or use `code tunnel` with an old CLI, or upgrade the host. There is no supported fix. |
| **Alpine / musl** | Never connects | Unsupported (DOCUMENTED) | Use Dev Containers, or a glibc host |
| **Disk full on remote** | Silent failure mid-untar | Each commit is ~350 MB and nothing GCs them | `rm -rf ~/.vscode-server/bin/*` |
| **`AllowTcpForwarding no`** | `open failed: administratively prohibited` in the Remote-SSH output channel (DOCUMENTED) | Hardened sshd | Enable it, or use Remote-Tunnels (§8) |
| **Slow first connect** | 30 s – 5 min of "downloading" | ~110 MB `node` + ~250 MB of bundle over the remote's link; on this machine, **VERIFIED 53 seconds** just for the download | `remote.SSH.localServerDownload`, or pre-seed the directory |
| **Auth prompt storm** | MFA/token prompt per reconnect | No `ControlMaster` (VS Code does not add it) | Add it by hand (§2.3) |
| **inotify watcher limits** | "Unable to watch for file changes in this large workspace" | `fs.inotify.max_user_watches` (often 8192) exhausted by `node_modules` | `sysctl fs.inotify.max_user_watches=524288`, and `files.watcherExclude` |
| **Shell env resolution timeout** | Terminals start with a wrong `PATH`; extensions can't find tools | VS Code spawns a login shell to snapshot the environment and gives it a deadline. A slow `.zshrc` (nvm, conda, pyenv, corporate mdm) blows it | **VERIFIED on this machine**, repeatedly: `ptyHost was unable to resolve shell environment Error: Unable to resolve your shell environment in a reasonable time. Please review your shell configuration and restart.` Fix: move slow init behind an interactive guard, or set `terminal.integrated.inheritEnv: false` |
| **Extension marketplace unreachable from remote** | `Error getting extensions control manifest Error: Timeout getting extensions control` — **VERIFIED on this machine** | The remote server fetches the extension control manifest itself | Proxy config, or accept the 3 s timeout |
| **Multi-user host** | Another user connects to your server | Connection token on the command line, visible in `ps` | `Remote.SSH: Remote Server Listen On Socket` |

**Diagnostics inventory:**

| Command / artifact | What it gives |
|---|---|
| **Remote-SSH: Show Log** | The local extension's view: ssh invocation, probe, install script output |
| Output → **Remote Server** | The server's own stdout |
| `~/.vscode-server/.<commit>.log` | Bootstrap log (download, entrypoint, start) — **VERIFIED** format §1.1 |
| `~/.vscode-server/data/logs/<ts>/` | Per-session: `remoteagent.log`, `exthost/`, `ptyhost.log`, per-extension logs |
| `pid.txt` next to the install | The running server's pid — **VERIFIED** |
| **Developer: Toggle Developer Tools** | Local renderer console |
| **Developer: Show Running Extensions** | Activation times, split by local/remote host |
| `terminal.integrated.developer.ptyHost.latency` | Inject artificial pty latency to reproduce reconnect bugs |

---

## 8. `code tunnel` / Remote-Tunnels

### 8.1 What it is

A second, independent mechanism: the remote machine makes an **outbound**
connection to a Microsoft-operated relay (the Azure "dev tunnels" service) and
registers itself. The client — VS Code desktop, or `vscode.dev` in a browser —
connects to the same relay. No inbound port, no sshd, no ssh keys.

```
remote host ──outbound TLS──► dev tunnels relay (Azure) ◄──outbound TLS── vscode.dev / desktop
```

**DOCUMENTED:** "VS Code makes outbound connections to a service hosted in
Azure; no firewall changes are generally necessary, and VS Code doesn't set up
any network listeners."

### 8.2 CLI and state

- `code tunnel` — downloads and starts the server, prints a `vscode.dev/tunnel/<name>` URL.
- `code tunnel service install` — run as a background service (systemd user unit / launchd / Windows service).
- `code tunnel --no-sleep` — inhibit machine sleep.
- `code tunnel user login --provider github`.
- State lives in the CLI's own tree, **VERIFIED layout** on this machine at
  `~/.cursor-server/cli/servers/Stable-<commit>/{server,log.txt,pid.txt}` with
  `lru.json` — this is the *tunnel/CLI* layout, which is why it exists on a Mac
  that has never been a Remote-SSH target.

### 8.3 Trust model — and where it is weaker

| | Remote-SSH | Remote-Tunnels |
|---|---|---|
| Identity | SSH keys / host keys. Trust is between two machines you control. | GitHub or Microsoft account, **the same account on both ends** (DOCUMENTED) |
| Third party in the path | none | **Microsoft's relay**, always |
| Inbound firewall | needs port 22 reachable | none |
| E2E confidentiality | SSH | tunnel traffic is encrypted (AES-256-CTR, DOCUMENTED) inside the relay TLS. The relay sees ciphertext but is the rendezvous and the authorization point. |
| Blast radius of account compromise | one host | **every machine you have ever run `code tunnel` on** |
| Auditability | sshd logs | Microsoft's, plus a local CLI log |
| Works behind CGNAT / corporate NAT | no | yes |

The account-shaped trust boundary is the real difference. A compromised GitHub
account is remote shell on every tunneled machine simultaneously. `code tunnel`
is the better answer when there is no inbound reachability and no VPN — and the
worse answer any time SSH already works.

**This is precisely the argument for omt targeting Tailscale instead.** Tailscale
gives the no-inbound-port property (WireGuard, outbound-only, NAT traversal)
*without* introducing a relay that is also the authorization authority: the
relay (DERP) sees only ciphertext it cannot decrypt, keys are per-device, and
ACLs are the user's. omt gets tunnels' reachability with SSH's trust model.
Doing so is strictly better than either VS Code mechanism, and omt should say so.

---

## 9. What is good and what is bad

### 9.1 Genuinely good

1. **The server outlives the client, with a 3-hour grace window.** Close the
   laptop, come back, everything is exactly as you left it — language server
   warm, terminals running, no reindex. This is the whole product. Everything
   else is plumbing.
2. **Reconnection tokens on the logical connection, not the socket.** Identity
   is not the TCP connection. It is a small idea with enormous consequences and
   it costs nothing.
3. **ptyHost as a separate process with an explicit heartbeat.** Native-addon
   crashes do not take the session down; a wedged ptyHost is *detected* rather
   than hanging the UI.
4. **The `VSCODE_IPC_HOOK_CLI` design.** Per-terminal socket, injected into the
   environment, a shim on `$PATH`, and the identical trick reused for `$BROWSER`.
   It makes remote work feel local at almost zero cost.
5. **One HTTP/WS endpoint multiplexing management, N extension hosts, and N
   forwarded ports.** Port forwarding costing nothing extra is why the ports
   feature is usable at all.
6. **Ack-based flow control on terminal data.** The obvious failure — a fast
   remote process starving a slow link — was designed for, not discovered.
7. **The commit hash as the compatibility key.** Brutal, but unambiguous. There
   is never a subtly incompatible pair.
8. **Auto-forward via `/proc/net/tcp`.** Detecting listeners at the OS level
   rather than by regexing output is the correct engineering choice.
9. **`lru.json`.** GC as a text file. Sometimes the right amount of engineering
   is none.
10. **The docs tell you to `rm -rf` the state directory.** The design genuinely
    treats the remote state as disposable, and that is why the recovery story
    works at all.

### 9.2 Genuinely bad — the recurring complaints

1. **First connect is slow and the payload is absurd.** ~350 MB per commit, of
   which ~110 MB is a *bundled Node runtime* (**VERIFIED**: the `node` binary in
   the install is 111 673 840 bytes). Every monthly release re-downloads all of
   it, to every host. On a slow link or a metered VM this is minutes.
2. **Nothing garbage-collects `~/.vscode-server/bin`.** The perennial "my /home
   is full" thread. The CLI path got `lru.json`; Remote-SSH did not.
3. **Exact-commit pinning with no negotiation.** Update the laptop on the train,
   land, and every remote host must re-download before you can work. There is no
   "close enough" mode and no way to pin the client to the server.
4. **The glibc 2.28 cliff (1.99, March 2025).** A minimum-bump in a point
   release stranded every CentOS 7 / Ubuntu 18.04 / Amazon Linux 2 user with an
   unhelpful dynamic-linker error and no supported path forward. Still the
   loudest bug tracker topic.
5. **`VSCODE_IPC_HOOK_CLI` does not survive `tmux`, `screen`, `ssh`, `sudo`, or
   a shell started outside VS Code.** Everyone who uses a multiplexer writes the
   same fragile `ls -t /run/user/$UID/vscode-ipc-*.sock | head -1` hack, and it
   picks the wrong window whenever more than one is open. Given that the target
   audience is terminal users, this is the most consequential design gap.
6. **`extensionKind` is advisory and unverified.** Three override layers
   (manifest, `product.json`, user setting) for one decision, and the failure
   mode is silence — an extension that does nothing, with no diagnostic.
7. **Scrollback on reconnect defaults to 100 lines.** Users are surprised to
   lose history after a lid-close. Raising it trades directly against ptyHost
   memory, and nothing tells you that.
8. **No ControlMaster by default.** Users on hardware tokens get an
   authentication prompt per reconnect, and the fix is buried in a
   troubleshooting page.
9. **Connection token on the argv.** Visible in `ps` on a shared host. Socket
   mode fixes it and is not the default.
10. **`AllowTcpForwarding no` is fatal, and the error is
    `administratively prohibited`** — a message from OpenSSH, three layers below
    where the user is, with nothing connecting it to the cause.
11. **Zombie servers.** `--enable-remote-auto-shutdown` is off for Remote-SSH,
    so a `node` process holding hundreds of MB of RSS survives on the dev box
    indefinitely. On shared build machines this is a running joke.
12. **Diagnostics are scattered** across a local output channel, a bootstrap log,
    a server log, an exthost log and a ptyHost log, in three directories, with
    no single "why did this fail" command.
13. **`remote.SSH.useLocalServer`** exists because the sophisticated path is
    buggy enough to need a documented escape hatch. Two connection
    implementations means bugs in both.

---

## 10. Implications for omt

### 10.1 What a single static Rust binary makes disappear

| VS Code problem | omt |
|---|---|
| ~350 MB payload, ~110 MB of bundled Node | **Gone.** One binary, ~15–30 MB, ~5–10 MB compressed. Uploading it over the ssh channel is seconds, not minutes. |
| ~184 `node_modules` with native addons per platform | **Gone.** Statically linked. |
| `bash` + `tar` + (`curl`\|`wget`) required on the remote | **Gone** if omt uploads over the ssh channel itself (§10.4). |
| Untar-into-a-tree that can be left half-written | **Gone.** Write to `omt.tmp`, `fsync`, `rename(2)`. Atomic install is one syscall. |
| glibc ≥ 2.28 cliff | **Solved by target choice**: ship `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl` as the default remote artifacts. Fully static, no libc dependency, works on Alpine — which Remote-SSH still cannot do at all. Keep glibc builds for the local/desktop case where `getaddrinfo`/NSS matters. |
| Exact-commit pinning, no negotiation | **Should be gone** — omt already has a real negotiation mechanism (`proto` versions, feature strings, `catalog_hash` in [07 §3.3](../architecture/07-remote-protocol.md#33-handshake-and-capability-negotiation)) and a catalog-intersection rule ([07 §1.5](../architecture/07-remote-protocol.md#15-federating-across-versions)). Use it. |

### 10.2 What does *not* disappear

1. **Arch/libc detection.** Still needed to pick `x86_64` vs `aarch64` vs
   `armv7`, and musl vs glibc. Do it the way VS Code does — `uname -sm` over the
   ssh exec channel — but do it in the **same** ssh invocation that does
   everything else, not as a separate connection.
2. **Delivery to a host with no internet.** omt's answer is strictly better and
   should be the *default*, not the fallback: pipe the local binary over the
   existing ssh channel. See §10.4.
3. **Version skew.** Does not disappear, it just becomes survivable instead of
   fatal. It still needs a policy and a UI.
4. **Upgrade and GC of remote installs.** A 20 MB binary GCs itself trivially,
   but *state* (session logs, blobs, the store) does not. Blob TTLs and the
   store's compaction ([09 §2](../architecture/09-ssh-and-media.md#2-the-blob-store))
   already cover it; the remote install needs the same discipline.
5. **`AllowTcpForwarding no`.** omt is *more* exposed here than VS Code, because
   `09 §5.2`'s reverse socket wants `ssh -R` with Unix-domain sockets, which
   corporate sshd disables even more often than `-L`. This is already
   [09 OPEN QUESTION 7](../architecture/09-ssh-and-media.md#9-open-questions) and
   this research raises its priority.
6. **Slow, weird, and hostile SSH configurations.** ProxyJump chains, `Match`
   blocks, hardware tokens, `Include`d configs. omt inherits every one of these
   the moment it shells out to `ssh` — which it should still do, for exactly the
   reason `07 §2.4` gives.

### 10.3 What omt should copy

**1. The server outlives the client — with omt's own grace timers.**
This is the finding. VS Code's numbers are `108e5` ms (3 h) and `3e5` ms (5 min).
omt's daemon already outlives clients by construction (it is a daemon), so omt
gets the *better* version for free: the grace question is not "how long do I keep
the process" but "how long do I keep the **replay window and subscription
state**". Recommendation:

```toml
[remote]
detach_grace       = "3h"     # hold subscription + replay state for a vanished client
detach_grace_short = "5m"     # shortened once another client attaches for the same actor
```

and log every connection with its resume token prefix, exactly as VS Code does
(`[<addr>][780ab572][ManagementConnection]`). omt's `07 §5.2` replay window
(4 MiB / 4096 events) is the analogue of VS Code's 100-line scrollback recorder,
and it is a better mechanism because it is a *sequence* window rather than a
line count — resume is exact rather than approximate. Keep it.

**2. Reconnection identity that is not the socket.** omt has this
(`session_token` + `since_seq`). VS Code validates the design.

**3. Connection tokens — but not on the argv.** VS Code's mistake is instructive.
omt should:
- default the remote instance to a **Unix socket at `0600`**, never a loopback
  TCP port, whenever the transport is ssh (this makes multi-user hosts safe by
  construction, where VS Code made it an opt-in setting);
- if a TCP port is used, pass the token via a **file** or **stdin**, never argv,
  and never an environment variable inherited by user shells;
- reuse `13 §4.1`'s `CredentialScope` so an `omt ssh` token is narrowly scoped
  and expiring, rather than a full-power bearer.

**4. The `code`-from-the-remote-shell trick — done properly.**
This is directly applicable and omt can beat VS Code, because VS Code's version
is broken in exactly the environment omt lives in (tmux, screen, nested ssh).
Design:

- The remote omt instance exports into every pane it spawns:
  ```
  OMT_SESSION=s_4b2f
  OMT_INSTANCE=3f2a…
  OMT_SOCK=$XDG_RUNTIME_DIR/omt/<instance>.sock
  ```
  Note `OMT_SOCK` is **per instance**, not per pane — the pane is identified by
  `OMT_SESSION`. This is the fix for VS Code's problem: a stale or
  wrongly-guessed socket path cannot open a tab in the wrong window, because the
  target is named explicitly, and a socket that belongs to a *dead* instance is
  simply not connectable.
- `omt` invoked with `OMT_SOCK` set does **not** start a new instance. It becomes
  a thin CLI against the running one — `omt split`, `omt open <file>`,
  `omt paste`, `omt run`, `omt notify` all become RPCs, in the pane the user is
  actually in.
- **Survive the multiplexer.** If `OMT_SOCK` is unset (tmux started before omt,
  a fresh `ssh`), do not guess-by-mtime. Fall back to a **deterministic**
  discovery path: `$XDG_RUNTIME_DIR/omt/` contains one socket per instance with
  the instance id in the name; if exactly one is live, use it; if several, prompt
  or require `--instance`. Deterministic, explainable, never silently wrong.
- **`--wait` for `$EDITOR`.** Copy the marker-file mechanism verbatim; it is the
  right design and `git commit` depends on it.
- **`$BROWSER` too.** VS Code's `helpers/browser.sh` is the same shim with
  `--openExternal`. omt should export `BROWSER=omt-open` so a remote process
  calling `xdg-open` opens on the laptop. Two lines of code, disproportionate
  delight.

**5. Auto-forwarding, from `/proc/net/tcp` not from output.** VS Code offers
`process` | `output` | `hybrid`; the process source is right and the output
source misfires. omt should implement the process source (`/proc/net/tcp{,6}`
diff on Linux, `lsof`-equivalent via `libproc` on macOS), scoped to processes
descended from omt-managed panes so it does not forward the whole machine. Reuse
`remote.portsAttributes`-shaped config. Since omt already has a multiplexed
control channel, a forwarded port should be a **stream on the existing
connection** — exactly VS Code's `desiredConnectionType: 3` — not a new ssh
channel.

**6. The local/remote responsibility split, but enforced by types.**
VS Code's `extensionKind` fails because it is an unverified hint. omt's
capability catalog can make locality **part of the capability's declaration**,
checked at dispatch:

```rust
capability! { name = "media.clipboard.read", …, locality = Locality::ClientLocal }
capability! { name = "session.spawn",        …, locality = Locality::InstanceLocal }
capability! { name = "media.blob.begin",     …, locality = Locality::Either }
```

A `ClientLocal` capability invoked on the remote instance returns
`unsupported { reason: "runs on the client" }` — a *diagnosable* failure, which
is precisely what `extensionKind` never produces. This is a small addition to
`03` and it eliminates VS Code's single worst class of bug.

**7. Per-connection log prefixes and a `pid.txt`.** Cheap, and it makes "is
something already running for this host" answerable without a registry.

**8. `--auto-shutdown`.** VS Code leaves zombie servers because it defaults the
flag off. omt should ship `[remote] idle_shutdown = "off" | <duration>` and
**default it to off for interactive `omt ssh`** (a multiplexer's whole point is
persistence) but **on** for `omt serve --stdio` invoked without any persistent
session, so a probe does not leave a daemon behind.

### 10.4 What omt should do differently

| VS Code | omt | Why |
|---|---|---|
| Download on the remote, fall back to local | **Upload from local by default** | The binary is 20 MB, the ssh channel already exists and is already authenticated, and it makes air-gapped and proxy-hostile hosts work on the first try instead of after a five-minute timeout. VS Code cannot do this because its payload is 350 MB. |
| Exact commit match, no negotiation | **Proto version + capability intersection** ([07 §1.5](../architecture/07-remote-protocol.md#15-federating-across-versions)) | omt already has the machinery. A 0.4.1 client against a 0.4.0 remote should work with two buttons greyed out, not fail. |
| Loopback TCP by default, socket opt-in | **Unix socket by default over ssh** | Multi-user hosts are the common case for dev boxes and the token-in-`ps` problem is real. |
| `ControlMaster` documented, not configured | **Configured**, per `07 §2.4` | Most users never read the troubleshooting page. |
| 100 lines of replay scrollback | **Sequence-indexed replay window** + `session.scrollback.get` for history | Exact resume, and unbounded history from the store rather than a magic line count. |
| Inject OSC 633 into the user's shell rc | **Observe OSC 133 natively** ([P4](../architecture/01-principles.md#p4--native-semantics-observe-never-re-implement)) | Correct principle. Accept that block metadata is thinner, and offer an *opt-in* omt shell integration for users who want exit codes and command text — never installed silently. |
| One PTY size, last window wins | **`ViewportPolicy`** ([07 §4.3](../architecture/07-remote-protocol.md#43-the-resize-problem)) | omt genuinely has the multi-viewport problem VS Code avoided; it is already designed for. |
| Server survives, PTYs die on host reboot | Same, plus **`SessionState::Orphaned`** with agent-native resume | Better than VS Code's `persistentSessionReviveProcess`, which restores a dead buffer with no resume path. |
| Microsoft relay for the no-inbound case | **Tailscale** | Same reachability; the relay cannot decrypt and is not the authorization authority. Strictly better trust model, and it is already omt's direction. |
| Five log files in three directories | **`omt doctor`** — one command, per-link diagnosis | VS Code's diagnostics are its weakest area; this is a cheap differentiator. |

### 10.5 Concrete recommendation for `omt ssh` bootstrap

**Command shape.** `omt ssh <host>` is a *thin local client* driving a remote
instance. `omt --remote <host>` (already in `07 §2.4`) is the same thing; unify
them — `omt ssh` should be the friendly spelling and `--remote` the flag form.

**The bootstrap, in one ssh connection.**

```
omt ssh box
 1. Reuse or create a ControlMaster:
      ssh -o ControlMaster=auto \
          -o ControlPath=$XDG_RUNTIME_DIR/omt/ssh-%C \
          -o ControlPersist=600 \
          -o ServerAliveInterval=15 -o ServerAliveCountMax=4 box
 2. Probe + attempt in ONE exec:
      ssh box -- 'omt serve --stdio --proto 1 2>/dev/null || \
                  { printf "OMT-MISSING "; uname -sm; ldd --version 2>&1 | head -1; }'
 3a. Success  → stdio framing (07 §2.3), handshake, done. Typical case: ~1 RTT.
 3b. OMT-MISSING → the probe already told us os/arch/libc. Pick the artifact,
      then upload over the SAME multiplexed ssh connection:
        ssh box -- 'cat > ~/.local/bin/omt.tmp && chmod 755 ~/.local/bin/omt.tmp \
                    && mv -f ~/.local/bin/omt.tmp ~/.local/bin/omt'  < <(zstd -c omt-<triple>)
      (decompress on the local side if the remote has no zstd; the probe can
       report that too). Then go to 3a.
```

Design notes:

- **One connection, one authentication.** The probe, the install and the serve
  all ride the ControlMaster. VS Code's multi-phase bootstrap is why it prompts
  for MFA repeatedly.
- **Try before you probe.** The overwhelmingly common case is "omt is already
  there and the right version". Optimize for it: the probe costs nothing when it
  succeeds.
- **`mv -f` is the install.** Atomic, and it replaces a running binary safely on
  Unix (the running process keeps its inode).
- **Delivery policy** as three modes, defaulting to the middle one:

  | `[remote].bootstrap` | Behaviour |
  |---|---|
  | `assume-installed` | Never install. Fail with the exact `uname` and the download URL. For locked-down hosts and CI. |
  | `upload` *(default)* | Push the binary over the existing ssh channel. Works offline, works behind proxies, needs no `curl`/`tar`/`bash` on the remote. |
  | `download` | Ask the remote to fetch from a release URL. Only useful when the remote's link is much faster than the laptop's. |

  Never make `download` the default; VS Code's remote-first ordering is the
  direct cause of its worst first-connect experience.
- **Ask before installing.** Writing a binary to someone's `~/.local/bin` is a
  side effect. Prompt once per host, remember the answer in
  `[remote.hosts."box"]`. `--yes` for scripts.

**Version negotiation.**

- The stdio handshake already carries `proto: [1]` and returns
  `catalog_hash`. Extend `Welcome` with the remote's `version` (it has it) and
  apply `07 §1.5`'s intersection rule.
- Policy ladder:

  | Situation | Action |
  |---|---|
  | Same `proto`, same `catalog_hash` | Connect. Nothing to say. |
  | Same `proto`, different catalog | Connect. Grey out non-intersecting capabilities with "not supported on `box` (0.3.2)". **This is the case VS Code fails and omt should not.** |
  | Remote `proto` older but supported | Connect, badge the session `proto 1 (remote 0.3.2)`. |
  | No common `proto` | Refuse, and offer **one** action: "upgrade omt on `box` to 0.4.1" — which is the same upload path as the bootstrap. |
  | Remote newer than local | Connect if `proto` intersects; never auto-downgrade the remote. Downgrading a shared dev box because one laptop is old is user-hostile. |

- **Never** auto-upgrade the remote on version mismatch alone. A remote omt may
  be serving another user's attached sessions; replacing its binary is not a
  local decision. Offer, log, and require confirmation.

**Where state lives.**

| State | Location | Rationale |
|---|---|---|
| Sessions, PTYs, scrollback, the event log | **Remote only**, `$XDG_STATE_HOME/omt/` | The instance is authoritative for its own sessions ([07 §1.1](../architecture/07-remote-protocol.md#11-the-shape)). No mirroring, no sync, no conflict resolution. |
| Blob store | **Remote**, `$XDG_RUNTIME_DIR/omt/<instance>/blobs` | Already specified in `09 §2`. Media follows the agent that consumes it. |
| Control socket | **Remote**, `$XDG_RUNTIME_DIR/omt/<instance>.sock`, `0600` | Per `07 §2.3`. |
| Binary | **Remote**, `~/.local/bin/omt` | One file. No versioned tree, therefore no GC problem and no `lru.json`. |
| Host records, credentials, per-host bootstrap consent | **Local**, `$XDG_CONFIG_HOME/omt/` | Client-side federation ([07 §1.1](../architecture/07-remote-protocol.md#11-the-shape)) — the client holds the list of instances. |
| Clipboard, downloads, the local terminal's capabilities | **Local** | The only things that inherently cannot be remote. |
| UI preferences (keymap, theme, layout) | **Local**, with remote defaults as a fallback | VS Code splits these into `User` (synced) and `Machine` scope; omt should do the same and be explicit about which settings are machine-scoped. |

The single rule, stated once: **state lives where the process it describes
lives.** A session's history belongs to the machine running the process; a host
list belongs to the machine you are sitting at. There is nothing in the middle,
and therefore nothing to synchronize.

---

## 11. Open questions for omt

1. **Does `ssh -R` with Unix-domain sockets survive real corporate sshd?**
   `09 OQ7` already asks this. This research says: `AllowTcpForwarding no` is
   common enough that VS Code documents its error string. Measure before
   committing to the reverse socket as tier 1.
2. **musl vs glibc as the default remote artifact.** musl's `getaddrinfo` and
   NSS behaviour differ (no `nsswitch.conf`, no LDAP/SSSD users). If the remote
   omt needs to resolve the invoking user via NSS, a fully static musl build may
   see a different `/etc/passwd` view than the login shell. Needs a test on an
   SSSD-joined host.
3. **Binary upload throughput over an ssh exec channel.** 20 MB should be
   seconds, but ssh's own windowing may make it worse than `scp`. Measure; if it
   is bad, use `scp`/`sftp` over the same ControlMaster instead of `cat`.
4. **Does replacing `~/.local/bin/omt` while a remote instance is serving other
   clients cause a version-split?** New sessions get the new binary, existing
   ones keep the old. Probably fine, but the daemon should report both.
5. **`OMT_SOCK` discovery when several instances run as the same user.** The
   deterministic rule in §10.3 needs a concrete tiebreak — probably "the instance
   that owns the pane whose pty is my controlling terminal", derived from
   `/proc/self/stat`'s tty on Linux. Unclear on macOS.
6. **Auto-forward scoping.** Diffing `/proc/net/tcp` finds every listener on the
   host, including other users'. Scoping to omt-descended processes needs a
   pid→inode→socket join, which is cheap on Linux (`/proc/<pid>/fd`) and awkward
   on macOS.
7. **Whether `omt ssh` should also be a pty wrapper.** `09 §5.3` tier 2a needs
   omt in the byte path; the thin-client design of `07 §2.4` does not put it
   there. These are two different programs wearing one name. Decide which
   `omt ssh` is, and name the other one.
