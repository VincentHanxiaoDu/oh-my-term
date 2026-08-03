# SSH, Clipboard and the Image Bridge

`omt-media` is a small crate with a hard problem. Everything else in omt runs on
one machine or over a connection omt controls. Media does not: the bytes a user
wants to move often start in a **foreign terminal emulator on a laptop** and need
to land in a **temp file on a remote box** so an agent can read them, with a
`ssh` session and possibly a `tmux` in between — none of which omt wrote.

This document enumerates the scenarios precisely, designs the transfer
mechanism, and is explicit about which paths are genuinely achievable and which
are wishful. The short version, stated up front so nothing below reads as
overselling:

> **omt can make image paste over SSH work reliably when omt is on both ends
> (`omt --remote`, or a local omt daemon with a forwarded socket). Through a
> foreign terminal emulator, automatic clipboard-image capture is achievable
> only on kitty today, semi-automatic on iTerm2 and WezTerm, and not at all on
> the rest. For those, omt degrades to an explicit, one-tap-from-the-phone
> upload rather than pretending.**

- [00 — Overview](00-overview.md) · [02 — Crate map](02-crate-map.md)
- [04 — Terminal core](04-terminal-core.md) — the OSC/DCS parser this builds on
- [07 — Remote protocol](07-remote-protocol.md) — the control channel
- [08 — Web client](08-web-client.md) — the upload/drag-drop surface
- [13 — Security](13-security.md)
- Research: [iTerm2 §4–5](../research/iterm2.md#4-inline-images--imgcat-wire-format)

---

## 1. The scenarios

Four cases, one row each. "Mechanism" names the section that specifies it.

| # | Situation | What must happen | Mechanism | Verdict |
|---|---|---|---|---|
| **(a)** | omt runs locally; user pastes an image from the local clipboard into an agent prompt | Image is read from the OS clipboard, written to the instance's blob store, and the agent receives a path in its own reference syntax | Direct OS clipboard read (§4.1) | **Fully solved.** No terminal involvement at all — omt owns the process. |
| **(b)** | User SSHes into a remote box, runs omt there, pastes an image that lives in the **local** machine's clipboard | The bytes must cross from the local machine into the remote instance's blob store, through the ssh tty or a side channel | Tiered: reverse socket (§5.2) → in-band OSC bridge (§5.3) → terminal-specific clipboard read (§5.4) → phone/web fallback (§5.5) | **Partially solved, tier-dependent.** See §5.6 for the honest matrix. |
| **(c)** | User is on the web client (phone) attached to a remote omt and attaches a photo | Photo uploads over the existing WebSocket into the blob store; agent gets a path | `media.image.upload` over the control channel (§4.2) | **Fully solved.** This is the easiest case and, not coincidentally, the universal fallback for (b). |
| **(d)** | User copies text or a file **out of** a remote omt back to the local clipboard | Text lands in the local OS clipboard; files land in a local directory | OSC 52 out (§3), `media.file.pull` for the web client (§7), chunked OSC 52 for large text (§3.3) | **Text: solved with caveats.** **Files: solved for omt-controlled ends, best-effort otherwise.** |

The design principle across all four: **there is exactly one blob store and one
reference format.** Every path above is a different way of getting bytes *into*
it. Once bytes are in, everything downstream — agent reference syntax, TUI
display, web display, TTL, quota — is identical.

---

## 2. The blob store

```rust
/// Content-addressed, per-instance, quota'd, TTL'd. The single landing zone.
pub struct BlobStore {
    root: PathBuf,        // $XDG_RUNTIME_DIR/omt/<instance>/blobs, 0700
    quota: QuotaConfig,
    index: BlobIndex,     // sled/sqlite: hash → metadata, refcounts, expiry
}

pub struct BlobId(pub [u8; 32]);   // BLAKE3 of the *decoded* content

pub struct BlobMeta {
    pub id: BlobId,
    pub len: u64,
    pub mime: Mime,                 // sniffed, never trusted from the sender
    pub kind: BlobKind,             // Image { w, h } | File | Text
    pub filename: Option<String>,   // sanitized to a single path component
    pub created: SystemTime,
    pub expires: SystemTime,
    pub origin: BlobOrigin,         // LocalClipboard | WebUpload | OscBridge | ReverseSocket | AgentOutput
    pub session: Option<SessionId>, // for quota accounting and cleanup
    pub refs: u32,                  // >0 pins it past TTL
}

pub enum BlobKind { Image { width: u32, height: u32 }, File, Text }
```

**Layout.** `blobs/<hh>/<hash>` where `hh` is the first hash byte in hex, plus
`blobs/<hh>/<hash>.meta.json`. Files are `0600`, the root is `0700`.

**Materialized path.** Agents need a path with a plausible extension, not a
64-hex blob. On first reference omt creates
`$root/refs/<session>/<short>-<sanitized-name>.<ext>` as a **hard link** to the
blob (falling back to a copy across filesystems). Content is stored once; N
sessions referencing the same screenshot cost one copy.

**Deduplication** is by BLAKE3 of decoded content. This is not a micro-
optimization: the same screenshot gets pasted repeatedly during a debugging
session, and re-transferring 2 MB over a pty at 9600-equivalent throughput is
the difference between usable and not. §5.3's protocol therefore *offers the
hash first* and skips the body when the receiver already has it.

**Quota** (`media.quota` in config, per-instance defaults):

| Knob | Default | Behavior on breach |
|---|---|---|
| `max_blob_bytes` | 32 MiB | Reject with `precondition_failed`, message names the limit |
| `max_total_bytes` | 512 MiB | Evict expired first, then LRU with `refs == 0`; if still over, reject |
| `max_blobs_per_session` | 200 | Reject with a clear message |
| `ttl` | 24 h | Background sweeper every 5 min |
| `ttl_referenced` | 7 d | A blob handed to an agent is pinned longer |

**Cleanup** runs on a timer, on instance start (sweeping the whole root), and on
session close (dropping that session's `refs/` directory). A crashed instance
leaves at most one TTL window of garbage, and the next start collects it.

---

## 3. Clipboard text

### 3.1 Writing to the local clipboard (remote → local)

**OSC 52** is the mechanism, emitted by `omt-media` into the session's PTY
output stream so it traverses ssh and reaches the outer emulator:

```
ESC ] 52 ; c ; <base64 of UTF-8 text> BEL
```

Inside tmux/screen it must be wrapped, keying off `TERM` (which survives ssh)
rather than `$TMUX` — the same rule iTerm2's utilities use:

```rust
fn wrap_osc(payload: &str, term: &str) -> String {
    if term.starts_with("screen") || term.starts_with("tmux") {
        // DCS tmux; <ESC-doubled payload> ST — tmux only accepts ESC \ as ST
        format!("\x1bPtmux;\x1b{}\x1b\\", payload.replace('\x1b', "\x1b\x1b"))
    } else {
        payload.to_string()
    }
}
```

`tmux` additionally requires `set -g set-clipboard on` in the user's config;
`omt doctor` checks for it and prints the exact fix.

**Size limits — the real constraint.** OSC 52 has no limit in the spec and
several in practice:

| Layer | Practical cap | Note |
|---|---|---|
| xterm | ~1 MB, but only with `allowWindowOps`-adjacent resources set | Often disabled by distro defaults |
| tmux | `buffer-limit` and an internal ~74 KB per-escape cap historically | The most common truncation point |
| screen | ~1 KB per OSC in older versions | Effectively unusable for anything large |
| iTerm2 | Truncates "very long OSC" in the parser; write is trust-gated | Has `it2copy`'s chunked `OSC 1337;Copy=` as the escape hatch |
| kitty | Configurable, `clipboard_max_size` default 512 KiB | Refuses silently above it |
| Terminal.app | Does not implement OSC 52 at all | Zero |

**omt's policy:** attempt OSC 52 for payloads under a configured
`clipboard.osc52_max_bytes` (default **64 KiB**, chosen to stay under tmux's
historical cap after base64 expansion), and fall back (§3.3) above it. omt does
**not** attempt to detect success — OSC 52 has no acknowledgement, and inferring
it from terminal behavior is exactly the kind of heuristic
[P4](01-principles.md#p4--native-semantics-observe-never-re-implement) forbids
from producing structured claims. Instead the UI says *"sent to your local
clipboard via OSC 52"* and offers the fallback one tap away.

### 3.2 Reading the local clipboard (local → remote)

**OSC 52 read (`ESC ] 52 ; c ; ? BEL`) is dead.** iTerm2's decoder refuses it
outright; xterm gates it behind a non-default resource; kitty requires explicit
`clipboard_control read-clipboard` opt-in and prompts; most others never
implemented it. Designing around it would produce a feature that works on one
machine in the office.

omt therefore treats clipboard *read* through a foreign terminal as
**unavailable by default** and reaches for §5's tiers. Where a terminal does
support it and the user has enabled it, omt uses it opportunistically — it is a
fast path, never the design.

### 3.3 Fallback path for large text

In order of preference, all reachable from one "Copy" action:

1. **Reverse socket** (§5.2), if a local omt instance is reachable: the remote
   calls `media.clipboard.write` on the *local* instance, which uses the real OS
   clipboard. No size limit, no terminal involvement. This is the good path.
2. **Chunked OSC**, when the outer terminal is known to support a chunked
   clipboard protocol. iTerm2's segmented `OSC 1337;Copy=2/3/4;<uid>` form is a
   documented interface fact and omt emits it when it has detected iTerm2
   ([iTerm2 §5.2](../research/iterm2.md#52-osc-1337-copy--chunked-clipboard-it2copy)).
   kitty's chunked OSC 52 (repeated `52;c;<chunk>` with a leading `!` reset) is
   emitted when kitty is detected.
3. **Web client**: the text is placed in a blob and the phone/browser copies it
   with `navigator.clipboard.writeText`. If your laptop's terminal will not
   cooperate, your browser will.
4. **A file on the remote** with a printed path, and a one-line
   `omt file pull <id>` command. Never lose the data.

### 3.4 Capabilities

```rust
capability! { name = "media.clipboard.write", group = "media", verb = "clipboard-write",
              kind = Command, role = Role::Operator,
              input  = ClipboardWrite { text: String, target: ClipboardTarget /* Auto|Osc52|Local|Blob */ },
              output = ClipboardWriteAck { method: ClipboardMethod, truncated: bool, blob: Option<BlobId> },
              effects = [Effects::TOUCHES_FS] }

capability! { name = "media.clipboard.read", group = "media", verb = "clipboard-read",
              kind = Query, role = Role::Operator,
              input  = ClipboardRead { prefer: Vec<Mime> },
              output = ClipboardContents { text: Option<String>, blobs: Vec<BlobMeta>, source: ClipboardSource },
              effects = [] }
```

`media.clipboard.read` succeeds trivially in case (a) and (c); in case (b) it
returns `unsupported` with a `source` naming the reason, which the surfaces
render as the tier-degradation message (§5.6).

---

## 4. Getting images in: the easy cases

### 4.1 Case (a) — local omt, local clipboard

`omt-media` reads the OS clipboard directly. No terminal, no escape sequences.

- **macOS**: `NSPasteboard` via `objc2`, requesting `public.png`, `public.tiff`,
  `public.jpeg`, then `public.file-url` (a copied *file* is the common case for
  screenshots dragged from Finder).
- **Linux/X11**: `x11-clipboard` targets `image/png`, `image/jpeg`,
  `text/uri-list`.
- **Linux/Wayland**: `wl-clipboard`'s protocol via `wayland-client`, same MIME
  order. If neither compositor path is available, shell out to `wl-paste
  --list-types` / `wl-paste -t` (present on effectively every Wayland desktop).
- **Windows**: `CF_DIBV5`/`CF_HDROP` via `windows-rs`, re-encoded to PNG.

The image is sniffed, normalized (EXIF orientation applied, EXIF metadata
including GPS **stripped** — see §8), stored, and referenced.

**Trigger.** The TUI binds a key (default `Ctrl-V` when the focused session has
an agent binding) to `media.image.paste`, which is the composed capability:
read clipboard → store blob → materialize path → inject the reference into the
agent per §6. There is no separate "upload" step in the user's model.

### 4.2 Case (c) — web client upload

Already designed in [08 §5](08-web-client.md): the browser has the bytes and an
open WebSocket to the instance. The upload is chunked over the same connection.

```ts
// client → instance, binary frames on the control channel
await instance.rpc("media.blob.begin", {
  len: file.size, mime: file.type, filename: file.name,
  hash: await blake3(file),              // dedup: instance may answer `have: true`
});
// if !have: send N binary frames tagged with the transfer id
await instance.rpc("media.blob.commit", { transfer: id });
```

If `media.blob.begin` returns `{ have: true }` the client sends nothing. A photo
re-attached to a second session costs one round trip.

Drag-and-drop, the file picker, the camera (`<input capture>`) and
`paste` events (`ClipboardEvent.clipboardData.files`) all funnel into the same
three calls.

**This is also the universal fallback for case (b).** If nothing else works, the
user opens the web client — which they already have, on the phone in their
pocket — and drops the image there. It lands in the same blob store, in the same
session. That is a genuinely good answer, not a consolation prize.

---

## 5. Case (b): image paste over SSH — the core mechanism

The situation: the user is at a laptop, in *their* terminal emulator
(iTerm2/kitty/WezTerm/Ghostty/Terminal.app/Windows Terminal), running
`ssh box`, running `omt` there. An image is in the laptop's clipboard. The agent
runs on `box`.

There is no standard by which a terminal emulator delivers a pasted image to the
foreground application. Bracketed paste carries text. That is the whole problem,
and every mechanism below is a way around it.

### 5.1 Tier overview

```
Tier 0  omt --remote <target>        omt on both ends, own control channel        §6 of this doc / [07]
Tier 1  reverse socket               local omt daemon reachable from remote        §5.2
Tier 2  in-band OSC bridge           omt's own OSC channel + a local companion     §5.3
Tier 3  terminal-native              kitty OSC 5522 / iTerm2 RequestUpload         §5.4
Tier 4  out-of-band                  phone/web upload, QR handoff                  §5.5
```

omt probes downward at session start and records the achieved tier in the
session state, so every surface can say exactly *why* paste behaves the way it
does. A tier is never claimed without a positive handshake.

### 5.2 Tier 1 — the reverse socket (the recommended answer)

If the user runs omt on their laptop too — which is the expected case, since omt
*is* their terminal multiplexer — then the laptop has an instance with a real OS
clipboard, and all we need is a path from the remote instance back to it.

`ssh` already has one: **remote port/socket forwarding**.

```
omt ssh box            # thin wrapper, or the user's own ~/.ssh/config
  ⇢ ssh -R /run/user/1000/omt-local.sock:$XDG_RUNTIME_DIR/omt/<local>/ctl.sock box
  ⇢ exports OMT_LOCAL_SOCK=/run/user/1000/omt-local.sock in the remote environment
```

The remote instance, on paste, does:

```rust
// omt-media, remote side
let peer = LocalPeer::from_env()?;                   // OMT_LOCAL_SOCK
let contents = peer.call("media.clipboard.read", ClipboardRead {
    prefer: vec![mime::IMAGE_PNG, mime::IMAGE_JPEG],
}).await?;                                           // ← runs on the laptop
for meta in contents.blobs {
    let bytes = peer.pull_blob(meta.id).await?;      // streamed over the forwarded socket
    self.store.put(bytes, meta.origin(ReverseSocket))?;
}
```

Properties: full duplex, no base64 expansion, no pty contention, works with
tmux/nested shells because it does not touch the tty at all, and it gives case
(d) file *pull* for free (the remote pushes a blob to the laptop, which saves it
to `~/Downloads` and can reveal it in the file manager).

**Authentication.** The forwarded socket is a full omt control channel and must
not be a hole. Rules:

- The local instance requires a credential on the forwarded socket exactly as it
  would on TCP: `omt ssh` mints a **scoped, expiring token** (role `Media`, a
  role below `Viewer` that permits only `media.clipboard.*` and `media.blob.*`)
  and passes it via `OMT_LOCAL_TOKEN` in the remote environment.
- The token is bound to the ssh session's lifetime and revoked on exit.
- The socket is `0600` and lives in the remote user's runtime dir. Anyone who
  can read it is already the remote user, but the token scope means even then
  the blast radius is "can read my clipboard", not "can drive my laptop". That
  is still a real disclosure, so the reverse socket is **opt-in per host**
  (`media.reverse_socket.hosts` in config), never on by default.
- **A clipboard read from the remote is a prompt on the laptop** the first time
  per host, with a "always allow for `box`" option — modeled on the browser
  clipboard permission, for the same reason.

`ssh -R` with a Unix socket requires `StreamLocalBindUnlink yes` on the remote
sshd (or omt unlinking a stale socket itself, which it does). Older sshd without
`StreamLocalBindUnlink` still works if omt cleans up on exit; a TCP loopback
forward is the fallback when Unix-socket forwarding is disabled entirely.

### 5.3 Tier 2 — the in-band OSC bridge

When there is no reverse socket (sshd forbids forwarding, the user did not use
`omt ssh`, a jump host is in the way), omt falls back to moving bytes through
the **tty itself** — the same trick iTerm2's `it2ul` uses, but with omt on both
ends instead of a terminal emulator.

The local end is `omt clip-agent`: a tiny process the user's local omt instance
runs (or that `omt ssh` starts), which watches the *outer* terminal's output for
omt's OSC requests and replies by writing into the pty as if typed.

Wait — in tier 2 the outer terminal is foreign, so omt cannot see its output. So
tier 2 requires the local end to be *in the pipe*. Two supported shapes:

**(2a) `omt ssh` as a pty wrapper.** `omt ssh box` spawns ssh inside a local
omt-managed PTY. omt now sees every byte in both directions and can implement the
bridge without any cooperation from the outer emulator. The outer emulator is
just rendering omt's local session. This is the shape that makes tier 2 work,
and it is one command away from tier 1.

**(2b) A pre-existing ssh in a foreign terminal.** omt is not in the pipe. Tier 2
is **not achievable**; omt drops to tier 3 or 4. This is stated plainly rather
than papered over.

#### 5.3.1 Wire format

omt reserves **OSC 5837** (`ESC ] 5837 ; <verb> ; <k>=<v> … [ : <payload> ] ST`)
as its private control namespace, registered in
[04 — Terminal core](04-terminal-core.md) alongside the OSC 133/52/1337 handling.
The same `TERM`-sniffing tmux wrapping as §3.1 applies to every emission.

Request/response, remote-initiated:

```
remote → local :  OSC 5837 ; media.req ; id=<u64> ; want=image/png,image/jpeg ; max=33554432 ST
local  → remote:  (written into the pty as input)
                  OMT5837 <id> offer <hash-hex> <len> <mime> <b64-filename>\n
remote → local :  OSC 5837 ; media.pull ; id=<u64> ; from=0 ; chunk=3072 ST    # or media.skip if deduped
local  → remote:  OMT5837 <id> part <seq> <base64-chunk>\n     (repeated)
                  OMT5837 <id> end <blake3-hex>\n
remote → local :  OSC 5837 ; media.ack ; id=<u64> ; got=<len> ST
```

Design points, each with a reason:

- **The offer carries the hash before the body.** If the remote already has that
  blob (the same screenshot pasted twice), it answers `media.skip` and the
  transfer costs two lines. Deduplication is not an optimization here — it is
  what makes repeated pasting tolerable at pty throughput.
- **Responses go on the *input* side.** The remote reads them from its own stdin,
  exactly like `it2ul`'s `read status` / `read_base64_stanza`. That is why this
  works through ssh, docker exec, nested shells and tmux: anything that carries a
  tty carries this.
- **Line-oriented, `OMT5837 ` prefixed.** The remote reader is a small state
  machine in `omt-media` that consumes stdin lines beginning with the sentinel
  and passes everything else through untouched. A stray line in the user's typing
  that starts with `OMT5837 ` and fails the transfer's id check is discarded.
- **Chunking at 3072 base64 chars (~2.25 KiB decoded).** Below typical pty line
  discipline limits with margin, and small enough that a canceled transfer stops
  promptly. The remote sets `chunk` in `media.pull` so it can adapt.
- **Explicit terminal echo suppression.** The remote sets the pty to `-echo`
  for the duration; without it a 2 MB base64 blob is painted onto the user's
  screen. This is the single most important implementation detail and it is
  restored in a guard/`Drop`.
- **Flow control.** `media.ack` every 64 chunks; the sender pauses if
  unacknowledged bytes exceed 256 KiB. A pty has no backpressure signal that
  reaches us, so we invent one.
- **Progress.** Each `part` updates a `media.transfer.progress` event, so the TUI
  and the web client show a real progress bar. At pty throughput a 2 MB image is
  a several-second operation and silence would read as a hang.
- **Cancellation.** `OSC 5837 ; media.cancel ; id=<u64>` from either direction;
  the receiver discards the partial and unlinks it.
- **Hard limits.** `max` is advertised by the receiver, clamped to
  `media.osc_bridge.max_bytes` (default **8 MiB** — a quarter of the blob-store
  limit, because pty transport is slow and a 32 MiB paste over a tty is a
  mistake, not a feature). Above it, omt refuses and suggests tier 4.

#### 5.3.2 Where this differs from iTerm2, and why

iTerm2's `it2ul` puts the **terminal emulator** on the local end: the emulator
shows a file picker, tars the selection, and types base64 back
([iTerm2 §5.4](../research/iterm2.md#54-local--remote-upload-it2ul-the-interesting-one)).
That is an elegant design and omt borrows its *shape* — request over OSC,
response as synthetic input — because that shape is what survives ssh.

omt differs in three ways that matter:

| | iTerm2 | omt |
|---|---|---|
| Who is the local end | The terminal emulator | omt itself (2a), or a local omt daemon (tier 1) |
| What the user does | Picks a file in a modal | Nothing — the clipboard already has the image |
| Transfer of choice | Always through the pty | Through the pty only as **tier 2**; tier 1 uses a real socket |

The third point is the real advantage and it comes directly from omt's
architecture: **omt already has a control channel**
([07](07-remote-protocol.md)) and a capability catalog that both ends speak. A
terminal emulator has only the pty, so it must be clever; omt has an out-of-band
path whenever both ends are omt, so it should only be clever as a fallback.
Building the OSC bridge as the *primary* mechanism would be reimplementing a
constraint we do not have.

The other borrowed interface fact is `it2dl`'s simplification: **a download is an
upload with `inline=0`** — one transport, two behaviors. omt's `media.file.push`
and `media.file.pull` are likewise one blob protocol with a direction bit, not
two protocols.

### 5.4 Tier 3 — terminal-native clipboard access

When omt is not in the pipe, the only remaining in-band hope is the outer
emulator's own protocol. Honest inventory:

| Terminal | Can the app read an **image** from the local clipboard? | Mechanism | omt support |
|---|---|---|---|
| **kitty** | **Yes** | kitty's clipboard protocol (`OSC 5522`) supports arbitrary MIME types; `kitten clipboard --get-filename` / `--mime image/png` reads them. Requires `clipboard_control` to permit `read-clipboard` (not the default; kitty prompts or requires config). | Implemented. Detected via `TERM=xterm-kitty` + `XTERM_VERSION`/kitty query. |
| **iTerm2** | **No, not the clipboard.** But `OSC 1337;RequestUpload=format=tgz` opens a **file picker** and streams the selection back through the pty. | User picks the file; if the image is only in the clipboard they must save it first. | Implemented as a "pick a file" affordance, correctly labelled — it is not clipboard paste. |
| **WezTerm** | **Partial / unverified.** WezTerm implements OSC 52 write and gated read, and has an extensible action system, but no documented image-MIME clipboard read. | — | Text read attempted where enabled; image path falls through to tier 4. **OPEN QUESTION.** |
| **Ghostty** | **No.** OSC 52 text only, read gated by `clipboard-read` config. | — | Text only. |
| **Alacritty** | **No.** OSC 52 write only. | — | Tier 4. |
| **Terminal.app** | **No.** No OSC 52 at all. | — | Tier 4. |
| **Windows Terminal** | **No** image path; OSC 52 write supported. | — | Tier 4. |
| **tmux/screen in between** | Degrades everything: caps OSC size, requires DCS wrapping, and swallows read replies unless `allow-passthrough on`. | — | omt detects and warns. |

So tier 3 is, realistically, **kitty**. That is one terminal, with a
non-default config flag. It is worth implementing because kitty users are
disproportionately represented in this audience, and because the code path is
small — but it is not a general answer and this document does not present it as
one.

### 5.5 Tier 4 — out-of-band, and why it is fine

When tiers 0–3 all fail, omt does not silently do nothing. Pressing paste with
an image-shaped clipboard that omt cannot reach produces:

```
┌ omt ────────────────────────────────────────────────────┐
│ This terminal can't hand omt an image from your          │
│ clipboard (Terminal.app, no clipboard protocol).         │
│                                                          │
│   ▸ Open on your phone      ██▀▄█ ▀▄  (QR)               │
│     https://box.tail1234.ts.net/u/7f2a1c                 │
│   ▸ Or run locally:  omt paste --to box:sess-3           │
│   ▸ Or:              omt ssh box   (enables paste)       │
└──────────────────────────────────────────────────────────┘
```

- The **QR** encodes a short-lived (5 min), single-use, upload-scoped URL into
  the remote instance's web surface, pre-bound to the target session. The user
  points a phone at their screen and drops the image; it lands in the right
  session. On a Tailscale deployment this is genuinely a five-second operation.
- `omt paste --to <instance>:<session>` runs on the **laptop**, reads the local
  clipboard directly, and pushes over the ordinary remote protocol. For users
  who have omt locally but did not use `omt ssh`, this is the one-liner.
- The message always names the *reason* and the *specific* remedy for the
  detected terminal. "Not supported" with no diagnosis is the failure mode this
  whole section exists to avoid.

### 5.6 The honest feasibility matrix

| Setup | Automatic clipboard-image paste? | Notes |
|---|---|---|
| omt local only (case a) | **Yes** | Direct OS clipboard. |
| Web client (case c) | **Yes** | Native browser paste/drop/camera. |
| `omt --remote box` | **Yes** | omt owns both ends; §6. |
| `omt ssh box` (omt wraps ssh) | **Yes** | Tier 1 socket, or tier 2 OSC bridge. |
| Foreign terminal + local omt daemon + `ssh -R` configured by hand | **Yes** | Tier 1. Requires user config. |
| kitty + plain `ssh`, `clipboard_control` allows read | **Yes** | Tier 3; kitty may prompt per read. |
| iTerm2 + plain `ssh` | **No — semi-automatic** | File picker via `RequestUpload`. Clipboard-only images need a manual save first. |
| WezTerm + plain `ssh` | **Unverified, assume no** | See OPEN QUESTIONS. |
| Ghostty / Alacritty / Terminal.app / Windows Terminal + plain `ssh` | **No** | Tier 4 (QR / `omt paste` / switch to `omt ssh`). |
| Any of the above with tmux in between and `allow-passthrough off` | **No** | Even tier 3 breaks. omt detects and says so. |

The product consequence: **`omt ssh` and `omt --remote` are the documented way
to get this feature**, and the first-run experience nudges toward them. Chasing
universal foreign-terminal clipboard access is a losing engineering bet against
software omt does not control.

---

## 6. `omt --remote <ssh-target>` — the thin client

The easy case, and the one omt should push users toward.

```
$ omt --remote box
```

1. omt spawns `ssh box omt serve --stdio` (or `--stdio --exec` to start the
   daemon if it is not running). The remote `omt serve --stdio` speaks the
   ordinary omt wire protocol on stdin/stdout — it is the `SshStdio` `Transport`
   from [`omt-transport`](02-crate-map.md#omt-transport), no different in kind
   from the WebSocket one.
2. The **local** process runs only `omt-tui`. It holds no session state; it is a
   renderer over the remote catalog, exactly as the web client is.
3. Because the local process is a full omt, it also has: the local OS clipboard,
   the local filesystem, and knowledge of the local terminal emulator.

### 6.1 The media path in thin-client mode

Everything in §5 collapses:

```
paste keypress in omt-tui (local)
  → local omt-media reads the OS clipboard          (native, §4.1)
  → media.blob.begin { hash, len, mime } over the ssh-stdio control channel
  → remote answers { have: false }
  → binary frames over the same multiplexed channel  (no base64, no pty)
  → media.blob.commit
  → remote materializes refs/<session>/<name>.png
  → agent.prompt with the reference (§7.1)
```

No OSC, no synthetic input, no echo suppression, no base64 expansion, no tmux
wrapping, no terminal detection. Throughput is ssh's, which is the network's.
Case (d) is the same in reverse: `media.file.pull` streams the blob to the local
process, which writes it to `~/Downloads` and copies its path to the local
clipboard.

The control channel is multiplexed ([07](07-remote-protocol.md)), so a 20 MB
transfer does not stall keystrokes: media frames are on a separate logical
stream with lower priority than input and terminal output.

### 6.2 Degrading case (b) gracefully

A user in a foreign terminal who runs bare `omt` on the remote gets, at session
start:

- a one-line status in the session header: `paste: tier 4 (Terminal.app) —
  press ? for options`, never a modal;
- the §5.5 card on the first failed paste, with the reason and the three
  remedies;
- `omt doctor` reporting the full chain (terminal, tmux passthrough, sshd
  forwarding, local omt presence) with a specific fix per broken link.

What omt does **not** do: silently degrade to a worse-quality path, retry
something that has already failed, or block the paste keystroke waiting for a
timeout. A failed paste is instantaneous and explained.

---

## 7. Image display and file transfer

### 7.1 Handing the image to the agent

Once the blob is materialized, omt injects a reference in the agent's own
syntax. This is per-adapter data on `AgentAdapter`, not a global format:

| Agent | Reference form | Confidence |
|---|---|---|
| **Claude Code** | A path in the prompt text; `@<path>` for an explicit file reference, which Claude Code expands. The `Read` tool handles image files, so an absolute path in the prompt is reliably picked up. omt injects `@<abs-path>` plus a short lead-in when the user typed one. | High for the path form; the exact `@`-expansion rules for absolute paths outside the workspace are **UNCERTAIN** — omt materializes into the workspace's `.omt/media/` when the blob store's root is outside it, to stay on the well-trodden path. |
| **Codex CLI** | `codex -i <path>` at launch; for a running session, an absolute path in the prompt. | Medium |
| **Gemini CLI / Qwen** | `@<path>` file reference, same as their file-inclusion syntax. | Medium |
| **Aider** | `/add <path>` then reference it. | High (documented command) |
| **opencode** | Attachment parts over its HTTP API — the only agent here with a real structured attachment channel. | High |
| **ACP-generic** | `session/prompt` content blocks support typed resources; the image goes as a resource block, not a path. | High |
| Anything else | Absolute path in the prompt, prefixed with a human sentence. | Fallback |

```rust
pub trait AgentAdapter {
    /// How this agent wants to be handed an image that already exists on disk.
    fn image_reference(&self, path: &Path, meta: &BlobMeta) -> ImageReference;
}

pub enum ImageReference {
    /// Inline into the prompt text at the cursor.
    PromptText(String),
    /// Run a slash command first, then reference it.
    Command { command: String, then: String },
    /// Structured attachment over the agent's own protocol.
    Structured(serde_json::Value),
}
```

`Structured` is always preferred when available, per
[P4](01-principles.md#p4--native-semantics-observe-never-re-implement) — answers
go back the way they came.

### 7.2 Inline display in the TUI

omt is a terminal application, so displaying an image means asking the *outer*
terminal to draw it. Detection and negotiation:

```rust
pub enum GraphicsProtocol {
    Kitty,      // APC _G …  — best: placement control, unicode placeholders, deletion
    ITerm2,     // OSC 1337;File=inline=1 — good, widely supported (iTerm2, WezTerm, Konsole, mintty)
    Sixel,      // DCS q — universal-ish fallback, lossy palette, no placement control
    None,       // render a placeholder card
}
```

Detection order, all with short timeouts and all falling through on no reply:

1. **Kitty**: send `APC _Gi=<id>,s=1,v=1,a=q;<1px base64> ST` and look for the
   `OK` response. This is the only reliable positive test.
2. **iTerm2/compatible**: `OSC 1337;ReportVariable=…` or the `DA1`+`XTVERSION`
   (`CSI > 0 q`) response naming iTerm2/WezTerm/Konsole/mintty.
3. **Sixel**: primary device attributes (`CSI c`) reply containing `4`.
4. Otherwise `None`.

Results are cached per `(TERM, TERM_PROGRAM, TERM_PROGRAM_VERSION)` and
invalidated on those changing. Under tmux, every emission is DCS-wrapped and
requires `allow-passthrough on`; omt detects tmux and, if passthrough is off,
reports `None` rather than emitting sequences that will be swallowed and leave
garbage on screen.

Rendering rules:

- Images occupy **real grid cells** so they scroll, reflow and select like text
  — the model iTerm2 uses
  ([iTerm2 §4.2](../research/iterm2.md#42-terminal-side-argument-parsing)) and
  the one [`omt-term`](04-terminal-core.md) implements. Cell dimensions come
  from the pixel size reported in `session.resize`.
- Size is clamped to `min(image_cols, viewport_cols)` preserving aspect ratio,
  with a configurable max height in rows (default 20) beyond which the image is
  downscaled — a full-screen screenshot should not evict the conversation.
- `GraphicsProtocol::None` renders a bordered placeholder with filename,
  dimensions, size, and a hint (`o` to open, `w` to view in the web client).
  Never nothing.
- Emission is **chunked** (`m=1` continuation for kitty, `MultipartFile=` /
  `FilePart=` / `FileEnd` for the iTerm2 protocol) so a large image does not
  produce one enormous escape sequence.

### 7.3 Display in the web client

Always available. The blob is fetched over the control channel (or a
short-lived authenticated `GET /v1/blob/<id>` for browser-native
`<img>` caching), rendered inline in block view and in interaction cards, with
tap-to-zoom in a lightbox. Thumbnails are generated server-side by `omt-media`
(`image` crate, longest edge 512, WebP) so a phone on LTE loads a grid of
attachments quickly and fetches full resolution on demand.

### 7.4 File push and pull

```rust
capability! { name = "media.file.push", group = "media", verb = "file-push",
              kind = Command, role = Role::Operator,
              input  = FilePush { blob: BlobId, dest: PathBuf, overwrite: bool, mode: Option<u32> },
              output = FilePushAck { path: PathBuf, bytes: u64 },
              effects = [Effects::TOUCHES_FS] }

capability! { name = "media.file.pull", group = "media", verb = "file-pull",
              kind = Command, role = Role::Operator,
              input  = FilePull { path: PathBuf, max_bytes: Option<u64> },
              output = FilePullAck { blob: BlobMeta },
              effects = [] }
```

- **`push`** writes a blob to a path *on the instance*, resolved against the
  session's workspace root and rejected if it escapes it (§8) unless the caller
  is `Admin` and `allow_outside_workspace` is set.
- **`pull`** reads a path on the instance into a blob; the client then downloads
  it. Directories are `tar+zstd`'d first (iTerm2's `it2dl` uses `tar -czf`; zstd
  is the same idea with a better ratio and no compatibility obligation since
  both ends are omt).
- **Web drag-and-drop** is push: dropping files onto a session view uploads them
  (§4.2) and then pushes them to the workspace root, showing a destination
  picker when more than one file or when a name collides. Dropping onto a
  *block* in block view offers "attach to this agent turn" instead.
- **Dragging out** of the web client uses `DataTransfer` with a `DownloadURL`
  entry pointing at the authenticated blob URL, so a file can be dragged from
  the browser to the desktop on Chromium. Elsewhere it is a download button.

---

## 8. Security

Media is the largest untrusted-input surface in omt after the VT parser, and it
writes to disk. Every rule below is enforced in `omt-media`, not in the callers,
so no surface can bypass it.

**Path traversal.**

```rust
fn resolve_in_root(root: &Path, requested: &Path) -> Result<PathBuf, MediaError> {
    let root = root.canonicalize()?;
    let joined = root.join(requested);
    // Canonicalize the deepest existing ancestor, then re-append; this handles
    // creation of new files while still resolving symlinks in the parent chain.
    let resolved = canonicalize_lexically_then_deepest(&joined)?;
    if !resolved.starts_with(&root) { return Err(MediaError::EscapesRoot { requested: requested.into() }); }
    Ok(resolved)
}
```

- Filenames from any peer are reduced to a **single path component** with
  `..`, `/`, `\`, NUL, and control characters stripped, then truncated to 255
  bytes on a char boundary, then prefixed with the blob's short hash to
  guarantee uniqueness. Windows reserved names (`CON`, `PRN`, `NUL`, `AUX`,
  `COM1`…) are suffixed.
- Blobs are opened with `O_NOFOLLOW` where available; the store never follows a
  symlink it did not create.
- `media.file.pull` refuses to read outside the workspace root (or an explicit
  `media.allowed_roots` list), refuses device files, fifos and sockets, and
  refuses files whose size exceeds `max_blob_bytes` before reading a byte.

**Content-type sniffing.** The sender's `mime` is a *hint*, never trusted.
`omt-media` sniffs with `infer`, and for anything claiming to be an image it
must additionally decode successfully with the `image` crate before being
classified `BlobKind::Image`. A mismatch is not an error — the blob is stored as
`BlobKind::File` with the sniffed type and a `mime_mismatch` flag surfaced in the
UI. Extensions on materialized paths are derived from the **sniffed** type.

Decoding untrusted images is itself risk: decode runs with limits
(`image::Limits` capping allocation to 256 MiB and dimensions to 16384²), and
decoding happens only for thumbnailing and validation — never as a prerequisite
for storing bytes.

**EXIF.** Images are re-encoded or stripped of all metadata except orientation
(which is applied to the pixels). A phone photo carries GPS coordinates, and
handing an agent a file whose metadata reveals the user's home address is an
exfiltration path nobody asked for.

**Quota exhaustion.** Every ingress path — clipboard, OSC bridge, web upload,
reverse socket — goes through one admission check *before* allocating:
`len` must be declared up front, must be ≤ `max_blob_bytes`, and the session must
be under its blob count and the instance under `max_total_bytes` after
projection. Streaming ingress is counted as it arrives and aborted the moment it
exceeds the declared `len` (a lying sender cannot stream forever). The OSC bridge
has its own tighter cap (§5.3.1) because the pty is shared with the user's
actual work.

**The managed root is absolute.** There is exactly one function that produces a
writable path in `omt-media`, and it is `resolve_in_root`. `media.file.push`
outside the workspace requires `Admin` **and** an explicit config opt-in **and**
is recorded in the audit log with the resolved path. There is no other writer.

**Blob URLs.** `GET /v1/blob/<id>` requires the same auth as every other route
and additionally checks that the credential's role can see the owning session.
Blob ids are content hashes, so they are unguessable, but that is defense in
depth, not the access control.

**The reverse socket** is the sharpest edge: it is a channel from a remote
machine into the user's laptop. It is opt-in per host, scoped to a `Media` role,
token-bound to the ssh session, prompted on first clipboard read per host, and
logged. See [13 — Security](13-security.md) for the role definition.

**Audit.** Every blob ingress and egress emits an event with origin, size,
sniffed type, session, and actor. "Which machine read my clipboard, when" must
be answerable.

---

## 9. OPEN QUESTIONS

1. **WezTerm image clipboard.** WezTerm has a rich internal clipboard model and
   an OSC 52 implementation with configurable read permission, but we have not
   verified whether any protocol lets the *application* retrieve `image/png`
   from the local clipboard. If it does, WezTerm joins kitty in tier 3. Needs a
   hands-on test, not documentation reading.
2. **kitty's read-clipboard ergonomics.** kitty's `clipboard_control` gating for
   reads may prompt per access or require a config change. If it prompts on
   every paste, tier 3 is technically working and practically annoying, and we
   should say so in the docs rather than advertise kitty as "supported".
3. **iTerm2 clipboard→file.** Does iTerm2 expose any way for the application to
   ask for the *clipboard's* image (as opposed to a file picker)? `RequestUpload`
   is a picker. If not, the honest phrasing is "iTerm2 supports file upload, not
   clipboard image paste", which is what §5.4 currently says.
4. **Claude Code's `@` expansion for absolute paths outside the project root.**
   Whether `@/run/user/1000/omt/blobs/…` is expanded, or only workspace-relative
   paths are, decides whether omt must always materialize inside the workspace
   (`.omt/media/`). Current design materializes inside the workspace to be safe,
   at the cost of littering the repo — mitigated with a `.gitignore` entry omt
   writes on first use, which is itself mildly presumptuous. Needs a decision
   after testing.
5. **Whether a running Claude Code session accepts an image at all without a
   restart.** The `-i` style flags are launch-time for several CLIs. If a given
   agent only accepts images at launch, omt's paste must be honest about that
   and offer "restart with this image attached".
6. **Multiple images in one paste.** Clipboards can hold several; the OSC bridge
   protocol handles it (N offers under one request id) but the agent reference
   syntax for multiple files is per-agent and only verified for a couple.
7. **`ssh -R` Unix-socket forwarding availability in practice.** Corporate sshd
   configs frequently disable forwarding entirely. If tier 1 fails often in the
   real world, tier 2 (`omt ssh` pty wrapper) becomes the primary path and
   deserves proportionally more engineering.
8. **Throughput of the OSC bridge in tmux.** Base64 through a pty through tmux's
   passthrough, with echo disabled, at 2 MB — unmeasured. If it is worse than
   ~200 KB/s the 8 MiB cap should drop and tier 4 should be offered earlier.
9. **Kitty graphics under tmux.** tmux's passthrough support for the kitty
   graphics protocol has historically been incomplete (placement and deletion in
   particular). We may need to force `GraphicsProtocol::ITerm2` or `Sixel` under
   tmux even when kitty is detected outside it.
10. **Clipboard *monitoring*.** A tempting feature ("omt notices you copied an
    image and offers to attach it") is deliberately not designed here: polling
    the OS clipboard is a privacy hazard and, on macOS, triggers system paste
    notifications. If it is ever built it must be explicitly enabled and visibly
    indicated. Recorded here so the idea is rejected on purpose rather than
    rediscovered.
