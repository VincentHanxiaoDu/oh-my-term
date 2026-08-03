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
| **(c)** | User is on the web client (phone) attached to a remote omt and attaches a photo | Photo uploads over the existing WebSocket into the blob store; agent gets a path | `media.image.upload` over the control channel (§4.2); the mobile capture paths, preprocessing and share-target flow are §4.3 | **Fully solved.** This is the easiest case and, not coincidentally, the universal fallback for (b). |
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

**Quota.** These are the canonical `[media]` configuration keys; this document
owns them and [10 §7.9](10-configuration.md#79-media) tabulates them.
Per-instance defaults:

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
              effects = [Effects::WRITES_FS] }

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

### 4.3 Attachments — any file, from any surface

§4.2 says "the browser has the bytes" and moves on. That sentence hides two
problems. On a phone there is **no screenshot in the clipboard** — the image
comes from the camera roll, from the camera right now, or from the OS share
sheet, and on iOS it arrives as HEIC at 4032×3024 with GPS attached. And images
are not the whole story: users attach logs, CSVs, diffs, PDFs, config files and
whole directories, and **an opaque binary upload is the wrong handling for most
of them**.

This section specifies attachments generally: what kinds exist, what each agent
can actually accept, every source path, and the single pipeline they all
converge on. It does not redefine the SSH tier ladder (§5) — a tier is a way of
getting bytes to the instance, and everything below happens after that.

Prior art worth naming: opencode's `serve` mode carries structured attachment
parts over its HTTP API
([agent-clis §3.2](../research/agent-clis.md#32-machine-readable-modes--the-richest-of-any-cli-here)),
which is why its web UI can attach a file without any terminal involvement. That
is the shape below; omt's advantage is that one blob store also serves the TUI
and every SSH tier.

#### 4.3.1 Content taxonomy — the decision table

Treating everything as an opaque blob is the tempting simplification and it is
wrong, because the *best* thing to hand an agent differs by kind. An agent given
a path to a 3 KB log will read it with its own tool and waste a turn; an agent
given 200 KB of inlined CSV will blow its context. The decision is made once, in
`omt-media`, from the sniffed type and the size.

| Content kind | Detection | What omt does | What the agent receives |
|---|---|---|---|
| **Image** (PNG/JPEG/WebP/GIF/HEIC) | `infer` + successful decode with the `image` crate | Preprocess (§4.3.5), store, materialize | Structured image block where the agent has one; else the materialized path in the agent's reference syntax (§4.3.7) |
| **Text, small** (< `inline_max`, default **32 KiB**) | sniffed non-binary **and** valid UTF-8 (or transcodable, §4.3.5) | Store the blob *and* inline the content in a fenced block with the filename as the info string | The text itself, in the prompt. No tool call needed, no round trip. |
| **Text, large** (≥ 32 KiB) | as above | Store, materialize, **do not inline** | The materialized path. Every agent in the covered set has a file-read tool; making it read 400 KB itself is correct and is how the agent controls its own context. |
| **Diff / patch** (`.diff`, `.patch`, or content matching a unified-diff header) | content sniff | Treated as text, but rendered as a diff in every omt surface via the shared renderer ([15 §7.4](15-workspace-explorer.md#74-reading-a-diff-on-a-phone)) | Same as text, with the fence tagged `diff` |
| **Structured data** (JSON/YAML/TOML/CSV/TSV) | extension + parse probe | Text rules apply. CSV additionally gets a **head/tail preview** (first 20 + last 5 rows) inlined when the file is large, with the path | Preview + path, so the agent knows the shape before deciding to read it all |
| **PDF** | magic bytes `%PDF-` | Store, materialize; **never** re-encode | Path. Claude Code reads PDFs natively (§4.3.2); others read them via their own tools or fail honestly. Page count is surfaced in the tray. |
| **Office documents** (`docx`, `xlsx`, `pptx`) | zip container + OOXML part probe | Store, materialize | Path. omt does **not** convert to text: a lossy in-house docx→markdown step would be omt inventing semantics ([P4](01-principles.md#p4--native-semantics-observe-never-re-implement)), and several agents have real converters already. |
| **Audio / video** | `infer` | Store, materialize, **never transcode, never truncate** | Path plus an explicit note that most agents cannot read it. The tray labels it *"most agents can't read this"* before sending, not after. Audio destined for *transcription* is a different feature — `stt.*` ([08 §7](08-web-client.md#7-voice-input)) — and the tray offers "transcribe instead" for audio. |
| **Archives** (`zip`, `tar`, `tar.gz`, `tar.zst`) | magic bytes | Store, materialize. **Never auto-extract.** | Path, plus an inlined listing of up to 200 entries (name, size) so the agent can decide what to ask for. Auto-extraction would write N unreviewed files into the user's workspace from a blob; the listing gives the same usefulness with none of the risk. |
| **Directory / multi-file selection** | source path reports a directory, or > 1 file selected | Two modes, user-chosen in the tray, defaulting by count and size | **≤ 8 files, ≤ 1 MiB total** → each attached individually (the common case: "these three files"). **Otherwise** → one `tar.zst` blob plus an inlined tree listing, with the archive materialized. Same rule as `media.file.pull`'s directory handling (§7.4), so there is one archive path. |
| **Executable / unknown binary** | fails every probe | Store, materialize | Path, plus a tray warning. omt neither refuses it nor pretends it is useful. |
| **Empty file** | `len == 0` | Refuse at staging | Nothing. An empty attachment is always a mistake, and finding out after the prompt is sent is worse. |

Two rules govern the whole table:

- **Inlining is a size decision, never a type decision.** A 2 KB `.py` and a
  2 KB `.log` are both inlined; a 2 MB `.py` is a path. The threshold is one
  config key (`media.inline_max_bytes`, default 32 KiB) rather than a per-type
  policy nobody can predict.
- **omt never converts content into a different format** except where the target
  cannot accept the source at all (HEIC → JPEG, §4.3.5) or where the user asked
  (transcribe this audio). Handing an agent a file the user did not produce is
  the kind of helpfulness that costs an hour of debugging.

```rust
/// The classification, computed once, server-side, from sniffed bytes.
pub enum AttachmentClass {
    Image { width: u32, height: u32 },
    Text { encoding: Encoding, lines: u64, inline: bool },
    Diff { files: u32 },
    Data { format: DataFormat, preview: Option<String> },
    Pdf { pages: Option<u32> },
    Office { kind: OfficeKind },
    Media { duration: Option<Duration>, transcribable: bool },
    Archive { entries: Option<u32>, listing: Option<String> },
    Binary,
}
```

#### 4.3.2 What each agent CLI actually accepts

**This table is the most load-bearing artifact in the section**, because
`AgentAdapter::attachment_reference()` is generated from it. Evidence tiers
follow [P4](01-principles.md#p4--native-semantics-observe-never-re-implement)'s
discipline and the convention in
[agent-clis](../research/agent-clis.md): **VERIFIED** = observed on a running
install or in its on-disk artifacts; **DOCUMENTED** = stated in official docs;
**UNCERTAIN** = inferred, or reported only by third parties.

| Agent | Image paste in its TUI | File reference syntax | PDF / non-image binary | Documented size limits | Evidence |
|---|---|---|---|---|---|
| **Claude Code** | **Yes.** Paste (`Ctrl-V`, not `Cmd-V`, on macOS) and drag-and-drop. Mechanism, observed: the file is copied to `~/.claude/uploads/<session-uuid>/<8hex>-<original-name>` and the **composer text becomes `@"<absolute path>" …`**; the transcript then carries a real `{"type":"image","source":{"type":"base64","media_type":"image/png"}}` content block. So the TUI's own paste is *path-insertion plus agent-side inlining*, not a terminal image protocol. | `@<path>`, and `@"<path with spaces>"`. **Absolute paths outside the project root work** — Claude Code itself writes them. | **Yes.** Its `Read` tool takes a `pages` parameter for PDFs (max 20 pages per request; required above 10 pages) and reads Jupyter notebooks as cells. Non-images generally: the same `uploads/` dir accepts them (a `.zip` was observed there). | Images: JPEG, PNG, GIF, WebP (DOCUMENTED). Byte limits not documented for the CLI. | **VERIFIED** on v2.1.220 from `~/.claude/uploads`, `~/.claude/paste-cache`, transcript JSONL, and the `Read` tool contract |
| **Codex CLI** | Launch-time only, as far as the CLI surface shows: `-i, --image <FILE>...` — *"Optional image(s) to attach to the initial prompt"*. No documented mid-session attach. | Absolute path in the prompt; its own read tools. | UNCERTAIN. `-i` is image-specific; other files go via path. | Not documented | `-i` flag **VERIFIED** from `codex exec --help` on the installed build; mid-session behaviour **UNCERTAIN** |
| **opencode** | Not via a CLI flag — `--help` exposes no image option. Its **server API and ACP mode carry structured attachment parts**, and the `part` type census in its SQLite store includes a `file` type. | `@`-style file mentions in the TUI (UNCERTAIN on exact syntax); structured parts over the API are the real path. | Likely yes via structured parts, by MIME. UNCERTAIN. | Not documented | `--help` surface **VERIFIED**; `file` part type **VERIFIED** from [agent-clis §3.3](../research/agent-clis.md#33-session-storage); attachment semantics **UNCERTAIN** |
| **Gemini CLI** | **No image flag exists** in `--help` (checked on the installed build): no `-i`, no `--image`. | `@<path>` file inclusion, plus `--include-directories` for extra workspace roots. | UNCERTAIN; assume path-only. | Not documented | `--help` **VERIFIED**; `@` syntax **DOCUMENTED** |
| **Cursor CLI** (`cursor-agent`) | **Partial and platform-dependent.** Clipboard image paste is reported not to work on Windows, and pasting from inside Cursor's own integrated terminal is reported broken; the working path is a pre-existing file referenced by path or `@`. | Path / `@` mention. | UNCERTAIN | Not documented | **UNCERTAIN** — community reports only; `--help` on the installed build emits a JS dump rather than usable help, so nothing could be verified locally |
| **Amp** | **Yes, by reference**: paste the file path, drag a file into the terminal, or press `@` to fuzzy-find. | `@` mention. | UNCERTAIN | Up to **3 reference images** for style guidance (DOCUMENTED); no byte limit stated | **DOCUMENTED** |
| **Aider** | **Yes**: `/paste` pastes an image *or* text from the clipboard. Also `/add <image-file>` in chat, and `aider <image-file>` at launch. The most explicit clipboard story of the set. | `/add <path>`, then reference it by name. | UNCERTAIN; images are the documented case. | Not documented | **DOCUMENTED** |
| **Goose** | **No CLI attach path.** Multimodal input is supported by the model layer, but adding a file or image to the context from the CLI is an open feature request (`@`-file tagging exists in the desktop UI, not the CLI). | None in the CLI today. | No | — | **DOCUMENTED** (open issues) |
| **Qwen Code** | Gemini-CLI-derived; assume Gemini's behaviour (`@` inclusion, no image flag). | `@<path>` | UNCERTAIN | — | **UNCERTAIN** — not separately verified |
| **Crush** | Drag-and-drop of files and large pastes are handled explicitly, and attachments are removable in the composer. Clipboard *image* paste: UNCERTAIN. | UNCERTAIN | UNCERTAIN | — | **DOCUMENTED** for drag-drop/attachment UX; specifics **UNCERTAIN** |
| **ACP-generic** | n/a (no TUI of its own) | `session/prompt` content blocks carry typed resources — the cleanest target in the set. | Yes, by MIME, subject to the agent behind it | — | **DOCUMENTED** ([agent-clis §11](../research/agent-clis.md#11-cross-cutting-acp-agent-client-protocol)) |

**Four conclusions omt should design on, and one that changes an existing
decision:**

1. **The absolute path is a *good* fallback, not a consolation prize.** Every
   agent in this set has a file-read tool. Handing it an absolute path and a
   sentence is a first-class interaction: the agent reads what it needs, when it
   needs it, and controls its own context. The only kinds where a path is
   genuinely worse than a structured attachment are images (a path costs the
   agent a read turn and some agents will not read image bytes at all) and text
   small enough to inline. That is why §4.3.1's table inlines small text and
   structures images, and paths everything else — it is not a compromise, it is
   the right answer for those kinds.
2. **`@"<absolute path>"` outside the project root is proven.** Claude Code's own
   TUI writes exactly that for a pasted image, pointing at `~/.claude/uploads/`,
   which is outside every workspace. **This resolves OPEN QUESTION §9.4 in this
   document**: omt does *not* need to materialize into the workspace's
   `.omt/media/` to stay on the well-trodden path, and therefore does not need to
   litter the user's repo or write a `.gitignore` entry. Materializing under the
   instance's managed root (§2) is correct. The quoting form matters — omt must
   emit `@"…"` whenever the path contains a space.
3. **Large *text* pastes already spill to a file in the wild.** Claude Code
   keeps `~/.claude/paste-cache/<16-hex>.txt`, content-hash-named, rather than
   inlining a huge paste into the composer. omt's 32 KiB inline threshold is the
   same instinct, and the convergence is reassuring rather than coincidental.
4. **Nobody strips EXIF.** The images in `~/.claude/uploads/` retain full Exif —
   camera orientation, device software string, and, on a phone that records it,
   GPS. An agent handed that file, with a network tool available, can read the
   user's coordinates. omt stripping metadata (§4.3.5, §8) is therefore not
   belt-and-braces; it is the only place in this chain where it happens.
5. **Image paste in a foreign terminal is unreliable across the board** —
   Cursor's Windows and integrated-terminal reports are the visible tip of the
   same problem §5 spends five tiers on. This is a point in favour of omt's
   design, not a gap in it: omt reads the OS clipboard directly (§4.3.3) instead
   of asking a terminal to deliver bytes it has no protocol for.

#### 4.3.3 Every source path, evaluated

One row per source: what is technically possible, and what omt does.

**Desktop web**

| Source | Possible? | omt's behaviour |
|---|---|---|
| Clipboard **paste** | Yes — `ClipboardEvent.clipboardData.files` and `.items` carry real `image/png` bytes and, for a file copied in the OS file manager, a file list. **The browser can do what a TUI cannot**, because the user's paste gesture *is* the consent, with no permission prompt. | Bound on the composer. Image items are staged directly; text items above the inline threshold become a text attachment rather than a wall of composer text. |
| `navigator.clipboard.read()` | Yes, but prompts | Only behind an explicit "paste from clipboard" button, for the case where there is no keyboard. The `paste` event is always preferred. |
| **Drag and drop** | Yes — `DataTransfer.files`, and `webkitGetAsEntry()` for **directories** | Dropping onto the composer stages; dropping onto a block offers "attach to this agent turn" (§7.4). Directories are walked (depth-capped at 6, 500 entries) and go through §4.3.1's directory rule. |
| **File picker** | Yes | `<input type="file" multiple>`, no `accept` restriction beyond a deny-list, because the whole point is "any file". |
| **Web Share Target** | Chromium desktop only | Registered; see §4.3.4. |

**Mobile web / PWA**

| Source | Possible? | omt's behaviour |
|---|---|---|
| **Photo library** | Yes | `<input type="file" accept="image/*" multiple>` → the system photo picker. |
| **Live camera** | Yes | `<input type="file" accept="image/*" capture="environment">` — a **separate control**, not a mode of the first. `capture` forces the camera and removes the library option, so one button that sometimes skips the picker would be a bad surprise. Two icons: `⧉` library, `⌾` camera. |
| **Files app / document picker** | Yes — this is the one people forget | A third control with **no `accept` filter**, opening iOS Files / Android SAF. This is how a log, a PDF or a CSV gets attached from a phone, and without it "attach any file" is a desktop-only claim. |
| **Share sheet → PWA** | Android/Chromium yes; **iOS Safari: no** | §4.3.4, including the honest platform statement. |
| Clipboard paste | Partial — mobile keyboards do deliver `paste` events with image data | Bound; not advertised, because it is inconsistent across keyboards. |

**Native TUI, running locally**

| Source | Possible? | omt's behaviour |
|---|---|---|
| **OS clipboard, read directly** | **Yes, trivially** | omt is a local process. It reads `NSPasteboard` / X11 / Wayland / Win32 directly (§4.1) — including `public.file-url` and `text/uri-list`, which is how a file *copied in Finder* arrives. This is dramatically better than any terminal escape protocol and it is why §4.1 is a one-liner while §5 is five tiers. |
| **Drag a file onto the terminal window** | Yes, but note what arrives | Every terminal emulator inserts **a path as text**, not the bytes — often shell-quoted or backslash-escaped, sometimes as a `file://` URL. omt therefore detects a pasted-path-shaped line in the composer: if the text resolves to an existing readable file, the composer offers *"attach this file?"* as an inline chip rather than sending a path the agent may or may not understand. Unquoting handles `'…'`, `"…"`, backslash escapes and `file://` percent-decoding. **This is the single highest-value TUI affordance in this section** and it costs a path-resolve. |
| **A file picker inside omt** | Yes | `media.picker.open` opens omt's own fuzzy file browser over the workspace (reusing the explorer's index, [15](15-workspace-explorer.md)), bound to the attach key. Necessary because a TUI has no OS file dialog, and because attaching a *workspace* file is the most common non-image case. |
| **`@`-completion in the composer** | Yes | Typing `@` in omt's composer completes workspace paths and stages the match as an attachment. Deliberately the same character the agents themselves use, so muscle memory transfers. |

**Native TUI over SSH** — cross-reference only. The tier ladder in §5 decides how
the *bytes* reach the instance (tier 0 `omt --remote`, tier 1 reverse socket,
tier 2 OSC bridge, tier 3 terminal-native, tier 4 out-of-band). Once they arrive,
everything in this section applies unchanged. Note that the drag-a-file case
above is *free* over SSH when the file is on the remote and merely *hard* when it
is local — which is exactly the §5 problem, and is why the composer's
attach-this-path chip says which side of the connection the path resolved on.

**`omt ssh` / `omt --remote` thin client** — the recommended case. omt owns both
ends, reads the local clipboard natively, and streams over the existing control
channel with no base64 and no pty (§6.1).

**CLI** — so scripting and "I am already in a shell" work:

```
omt attach ./crash.log --to <instance>:<session>        # stage in the tray
omt attach ./crash.log --to <instance>:<session> --send  # stage and submit
omt attach ~/Downloads/*.png --to :current               # globs; :current = focused session
omt paste --to <instance>:<session>                      # read THIS machine's clipboard
cat report.csv | omt attach - --name report.csv --to :current
```

`omt attach` is `media.blob.begin`/chunks/`commit` plus a tray insert — the same
three calls as every other source. `omt paste` (already named in §5.5) is
`omt attach` with the clipboard as the source. Both are ordinary catalog
capabilities, so they exist on all three surfaces by construction
([03 §5](03-capability-catalog.md#5-the-parity-contract)).

#### 4.3.4 Mobile capture specifics

```tsx
// web/src/views/composer/Attach.tsx — the whole capture surface.
<input ref={library}  type="file" accept="image/*" multiple hidden
       onChange={e => ingest(e.currentTarget.files, "picker")} />
<input ref={camera}   type="file" accept="image/*" capture="environment" hidden
       onChange={e => ingest(e.currentTarget.files, "camera")} />
<input ref={anyFile}  type="file" multiple hidden
       onChange={e => ingest(e.currentTarget.files, "files_app")} />
```

All sources — these three, drag, paste, share target, and the CLI — call one
function:

```ts
async function ingest(files: FileList | File[] | null, origin: SubOrigin) {
  for (const f of Array.from(files ?? [])) {
    const staged = await stage(f, origin);   // classify + preprocess, off the main thread
    tray.push(staged);                       // §4.3.8 — visible immediately
    void upload(staged);                     // §4.3.6 — resumable, cancellable
  }
}
```

The tray entry appears **before** the upload starts. A phone photo takes seconds
on LTE, and a composer that looks empty for three seconds after you pick a file
reads as broken.

**Web Share Target — sharing a file *into* omt.** The delightful path, and the
one that makes the PWA a citizen of the OS: screenshot, Share, omt. No app
switch, no picker, no navigating to a session first. It is not images-only —
`accept` includes documents, so sharing a PDF from a mail client works too.

```json
// web/manifest.webmanifest (excerpt)
{
  "share_target": {
    "action": "/#/share", "method": "POST", "enctype": "multipart/form-data",
    "params": {
      "title": "title", "text": "text",
      "files": [{ "name": "media",
                  "accept": ["image/*", "application/pdf", "text/*",
                             "application/json", "application/zip"] }]
    }
  }
}
```

The `POST` is intercepted by the service worker, because there is no server to
post to — an omt instance serves a static bundle and a WebSocket, and the share
target must work before any instance is reachable.

```ts
// service-worker.ts
self.addEventListener("fetch", (e: FetchEvent) => {
  const url = new URL(e.request.url);
  if (e.request.method === "POST" && url.pathname === "/share") {
    e.respondWith((async () => {
      const form  = await e.request.formData();
      const files = form.getAll("media") as File[];
      const id = crypto.randomUUID();
      const inbox = await caches.open("share-inbox");
      await Promise.all(files.map((f, i) => inbox.put(`/share/${id}/${i}`, new Response(f))));
      return Response.redirect(`/#/share?id=${id}&n=${files.length}`, 303);
    })());
  }
});
```

**Where does it go?** Not a picker, in the common case — the **continuity
ranking**:

1. If exactly one session is plausibly current (the app was open on it within
   5 minutes, or exactly one session is in `needs you`), the share lands in that
   session's tray, with a header chip naming it and a one-tap **"not this one"**.
   Guessing with a cheap correction beats a mandatory picker.
2. Otherwise: the ranked session list, filtered to sessions with an agent
   binding, with the file already staged and preprocessed. One tap starts the
   upload.
3. Shared `text`/`title` seed the composer, so a screenshot shared from a browser
   arrives with the page URL as context.

Ranking and "current session" are specified in
[remote continuity §2.3](../design/remote-continuity.md#23-the-continuity-ranking).

**Platform honesty.** Web Share Target is implemented in Chromium (Android and
desktop) and is **not** available in iOS Safari at the time of writing. omt does
not advertise a share target it could not register — `navigator.share` presence
is *not* a proxy, that is share-*from*, not share-*to* — and the onboarding tip
appears only where registration succeeded. On iOS the Files picker and the photo
picker are the answer. §9.10 tracks it.

#### 4.3.5 Preprocessing — images, text, and the things omt must not touch

Preprocessing runs in a `Worker` with `OffscreenCanvas` on the web, and in
`omt-media` for the TUI and CLI paths, so one policy serves all sources. Where
`OffscreenCanvas` is unavailable the main-thread path runs with a visible
"preparing…" state rather than silently blocking.

```ts
export interface StagedAttachment {
  id: string;                   // staging id, distinct from BlobId
  blob: Blob;                   // the bytes actually uploaded
  class: AttachmentClass;       // §4.3.1, computed client-side, re-derived server-side
  mime: string;
  bytes: number;
  hash: string;                 // BLAKE3 (WASM) of `blob`, for §4.2 dedup
  original: { name: string; mime: string; bytes: number };
  transforms: ("downscale" | "reencode" | "heic_decode" | "exif_strip" | "orient"
             | "transcode_utf8" | "tar_zst")[];
  preview: PreviewKind;         // thumbnail URL, first lines, page count, entry listing
  insertion: string;            // exactly what will go into the prompt (§4.3.8)
}
```

**Images — the budget, with concrete numbers.** A 12 MB, 4032×3024 HEIC helps no
agent and costs the user a minute on LTE.

| Knob | Value | Rationale |
|---|---|---|
| Max longest edge | **1568 px** | Above this no vision model in the covered set gains accuracy, and it is the point where a screenshot's text is still legible after re-encode. A 4032 px photo becomes 1568×1176. |
| Re-encode target | **JPEG q=0.82**, or **PNG** when the source is PNG *and* the decoded image has ≤256 distinct colours or an alpha channel | Screenshots are PNG-shaped (flat colour, sharp text, where JPEG ringing is visibly bad); photos are JPEG-shaped. Deciding by content rather than extension is a few lines and avoids both failure modes. |
| Soft size target | **≤ 1.5 MB** | On overflow, quality steps 0.82 → 0.72 → 0.62, then the longest edge halves once. Two passes maximum; this is not a search. |
| Hard refusal | **> 32 MiB source** | Matches `max_blob_bytes` (§2). Refused before decode, with the limit named. |
| Skip entirely | ≤ 1568 px **and** ≤ 512 KiB **and** not HEIC | Re-encoding a small PNG screenshot only loses fidelity. Strip metadata, upload as-is. |
| Animated GIF/WebP | pass through, capped at 8 MiB | Downscaling an animation in a browser means decoding every frame; rarely what an agent needs. |

**EXIF and privacy — and §4.3.2 finding 4 is why this matters.** Re-encoding
through `ImageBitmap` drops metadata as a *side effect*, which is the desired
outcome but must not be the mechanism, because the skip-entirely path does not
re-encode. Therefore:

- **Orientation is applied to the pixels**
  (`createImageBitmap(file, { imageOrientation: "from-image" })`), then the tag
  is gone. A sideways photo handed to an agent is a support ticket.
- **All other metadata is stripped, GPS explicitly.** On the pass-through path a
  minimal chunk stripper runs (JPEG APPn, PNG `tEXt`/`iTXt`/`eXIf`) rather than
  the step being skipped.
- **The instance strips again** (§8) regardless of what the client claims. The
  client-side strip is for bandwidth and defence in depth; the server-side strip
  is the guarantee. Never trust the client, including our own.

**HEIC.** iOS photos are `image/heic`/`heif` and several agents in §4.3.2's table
will not accept them. In order: (1) **`createImageBitmap(file)`** — Safari on
iOS 17+ decodes HEIC natively, so the platform that produces HEIC also solves it,
for free, and this is the path taken in practice; (2) a lazily-loaded `libheif`
WASM build (~700 KiB), on first HEIC only, never in the initial bundle; (3)
upload untouched and let the instance decode with the `image` crate — last
resort, because the point of preprocessing is to *not* send 8 MB. A HEIC never
reaches an agent.

**Text.** Encoding is sniffed; UTF-16/Latin-1 is transcoded to UTF-8 and the
transform is recorded, because an agent handed UTF-16 sees mojibake. A BOM is
stripped. CRLF is **preserved** — rewriting line endings in a file the agent may
edit is a real way to produce a spurious diff. Files that fail UTF-8 validation
after transcoding are reclassified `Binary` and never inlined.

**Everything else is untouched.** PDFs, office documents, archives, audio and
video are uploaded byte-exact. Their hash is their identity, and re-encoding
would break both dedup and the user's expectations.

#### 4.3.6 One pipeline

Every source above converges on the pipeline this document already specifies.
**A per-source code path is explicitly rejected**: five ingress paths with five
quota checks is five places to forget one, and §8's guarantee ("enforced in
`omt-media`, not in the callers") only holds if there is one road.

```
  desktop paste ┐
  drag & drop   │
  file picker   │
  share target  │      ┌───────────┐   ┌──────────┐   ┌──────────────┐   ┌───────────┐
  camera roll   ├────► │  stage    ├──►│  admit   ├──►│  transfer    ├──►│  commit   │
  live camera   │      │ classify  │   │ quota    │   │ chunked      │   │ sniff     │
  Files app     │      │ preprocess│   │ dedup by │   │ resumable    │   │ decode-   │
  TUI clipboard │      │ hash      │   │ hash     │   │ progress     │   │ verify    │
  TUI drag-path │      └───────────┘   └──────────┘   └──────────────┘   │ EXIF stri │
  omt picker    │            §4.3.5          §2            §4.2/§3.6      │ materiali │
  omt attach CLI│                                                         └─────┬─────┘
  SSH tiers 0–4 ┘                                                               │
                                                                                ▼
                                                                      ┌──────────────────┐
                                                                      │ tray → insertion │
                                                                      │  §4.3.7 / §4.3.8 │
                                                                      └──────────────────┘
```

The wire is §4.2's three calls, unchanged, with the fields the general case
needs:

```ts
await instance.rpc("media.blob.begin", {
  len: staged.bytes, mime: staged.mime, filename: staged.original.name,
  hash: staged.hash, session: sessionId, purpose: "prompt_attachment",
  origin: staged.origin,          // picker | camera | files_app | share_target
                                  // | paste | drop | tui_clipboard | cli | osc_bridge
});
// → { have: true }                       ⇒ send nothing; done in one round trip
// → { have: false, transfer, chunk_bytes, resume_from: 0 }
```

- **Dedup first.** The hash is computed during staging, so re-attaching the same
  screenshot or the same log to a second session costs exactly one round trip
  (§2). The same file shared twice is free.
- **Chunking at 256 KiB** as `kind=3` binary frames
  ([07 §3.6](07-remote-protocol.md#36-binary-payloads)), with the server naming
  `chunk_bytes` so it is tunable per transport without a client change.
- **Resumability.** The instance retains a partial for **10 minutes**, keyed by
  `(hash, len)`. A reconnect re-issues `media.blob.begin` with the same hash and
  gets `resume_from: <bytes stored>`; the client seeks and continues. Restarting
  an 8 MB upload from zero because a train entered a tunnel is the difference
  between a feature people use and one they avoid.
- **Progress** rides the existing `media.transfer.progress` event (§5.3.1), so
  the tray, the TUI and a second device show the same bar. Start an upload on the
  phone, open the laptop, see it in flight.
- **Cancel** is `media.blob.abort`; the partial is unlinked immediately. Removing
  a tray entry cancels an in-flight transfer.
- **Priority.** Media frames are the low-priority logical stream (§6.1). An
  upload must never delay an `interaction` event —
  [07 §9 Q6](07-remote-protocol.md#9-open-questions) flags that a large upload
  from a phone needs its own queue class, and this is precisely why.
- **Offline.** A staged attachment with no connection stays in the draft
  ([remote continuity §2.4](../design/remote-continuity.md#24-drafts)) and uploads
  on reconnect. The tray shows `⇧ waiting for network`, and the prompt cannot be
  sent until every attachment has committed — sending a prompt that references a
  file which does not exist yet is worse than waiting.

#### 4.3.7 What the agent finally receives

The blob is materialized exactly as §2 specifies —
`$root/refs/<session>/<short>-<sanitized-name>.<ext>`, a hard link to the
content-addressed blob — and referenced through the adapter. §4.3.2 finding 2
means the managed root is fine; materializing inside the workspace is no longer
required.

```rust
pub trait AgentAdapter {
    /// How this agent wants to be handed an attachment that exists on disk.
    /// `None` = this agent cannot accept this class at all, in a running session.
    fn attachment_reference(&self, path: &Path, meta: &BlobMeta, class: &AttachmentClass)
        -> Option<AttachmentReference>;

    /// True when the agent accepts this class only at launch (e.g. `codex -i`).
    fn attachment_at_launch_only(&self, class: &AttachmentClass) -> bool { false }
}

pub enum AttachmentReference {
    /// Inline into the prompt text at the cursor. Carries the exact rendered
    /// string so the tray can show the user what will be sent (§4.3.8).
    PromptText(String),
    /// Inline the *content*, not a path — small text (§4.3.1).
    InlineContent { fence: String, body: String, name: String },
    /// Run a slash command first, then reference it.
    Command { command: String, then: String },
    /// Structured attachment over the agent's own protocol. Always preferred.
    Structured(serde_json::Value),
}
```

Per-agent rendering, derived from §4.3.2:

| Agent | Image | Small text | Everything else |
|---|---|---|---|
| Claude Code | `PromptText("@\"<abs>\"")` — its own verified form, quoted | `InlineContent` | `PromptText("@\"<abs>\"")` |
| ACP-generic / opencode | `Structured` resource block | `Structured` | `Structured` where the MIME is accepted, else path |
| Codex | `PromptText(abs)`; `attachment_at_launch_only` → offer `-i` on restart | `InlineContent` | `PromptText(abs)` |
| Gemini / Qwen | `PromptText("@<abs>")` | `InlineContent` | `PromptText("@<abs>")` |
| Aider | `Command { "/add <abs>", then: "…" }` | `InlineContent` | `Command` |
| Amp | `PromptText("@<abs>")` | `InlineContent` | `PromptText("@<abs>")` |
| Cursor / Crush | `PromptText(abs)` | `InlineContent` | `PromptText(abs)` |
| Goose, and anything unknown | `None` for images | `InlineContent` (always safe — it is just text) | `PromptText(abs)` with a lead-in sentence |

`Structured` is always preferred where available, per
[P4](01-principles.md#p4--native-semantics-observe-never-re-implement) — answers
go back the way they came.

**Agents that accept nothing.** Handled honestly rather than by uploading and
hoping. `None` → the tray entry is marked before send: *"`goose` can't take an
image. Save it to the workspace instead?"* — offering `media.file.push`, which is
a real answer. The control is **disabled with a reason, never hidden**
([08 §3.4](08-web-client.md#34-graceful-degradation-across-catalog-versions)).
`attachment_at_launch_only` → the blob is stored and the composer offers
*"restart this session with the file attached"*, which makes §9.5's requirement
concrete instead of a surprise at send time.

Either way **the blob is never silently dropped**: it is in the store, it is in
the session's attachment list, and it has a path the user can copy.

#### 4.3.8 The attachment tray

Attachments are **staged, not sent instantly**. The tray is where the user sees
what they have, what it will cost, and — critically — **exactly what will be
inserted into the prompt**. That last column is the honesty requirement: a user
who cannot see that omt is about to paste `@"/run/user/1000/omt/…"` into their
prompt cannot predict what the agent will do.

Parity is required on all three surfaces
([P3](01-principles.md#p3--parity-one-capability-three-surfaces)); the shape
differs, the operations do not.

**Common model:**

```ts
interface TrayEntry {
  id: string;
  name: string;                     // original filename
  class: AttachmentClass;           // drives the icon and the preview
  bytes: number; originalBytes: number;
  state: "staging" | "uploading" | "committed" | "failed" | "waiting_network";
  progress: number;                 // 0..1
  insertion: string;                // EXACTLY what goes into the prompt
  warning: string | null;           // "most agents can't read this", "EXIF stripped", …
}
```

Operations, identical everywhere: **reorder**, **remove**, **retry**, **preview**,
**copy path**, **change disposition** (inline ↔ path, where both are legal).

**Desktop web / mobile web**

```
┌ attachments (3) ──────────────────────────── 1.9 MB ┐
│ ▣ IMG_4417.jpg   1568×1176  412 KB  ✓   @"/…/a1-IMG_4417.jpg"      ×│
│ ▤ crash.log      2 140 lines 96 KB  ▓▓▓░ 62%   → inlined as ```log ×│
│ ▦ spec.pdf       28 pages   1.4 MB  ✓   @"/…/9f-spec.pdf"          ×│
└──────────────────────────────────────────────────────┘
  [ message claude…                                            ] [↑]
```

- A horizontal strip of 64 px thumbnails (images) or type icons above the text
  field on mobile; a vertical list on desktop where there is room for the
  insertion column.
- Tap opens a preview: lightbox for images, first 200 lines for text, page count
  and first page for PDF, entry listing for archives.
- **Long-press-and-drag reorders**, and **order is preserved into the prompt** —
  "the first screenshot shows the error, the second shows the fix" is meaningful
  and silently reordering it would be a lie.
- Removing cancels an in-flight transfer, drops the reference and decrements the
  blob's refcount. It does **not** delete the blob: content is shared and TTL'd
  (§2), and another session may reference the same bytes.
- Each entry's state is independent; one failure does not block the others. The
  failed chip offers **Retry**, and the prompt cannot be sent while any entry is
  `failed`, `uploading` or `waiting_network`.

**TUI** — the same tray, drawn above the composer, no modal:

```
 ▣ IMG_4417.jpg  412K  ✓   ▤ crash.log  96K ▓▓▓░ 62%   ▦ spec.pdf  1.4M  ✓
 ─────────────────────────────────────────────────────────────────────────
 > @"…/a1-IMG_4417.jpg" the login screen is misaligned▏
```

`a` opens omt's file picker, `Ctrl-V` reads the OS clipboard, `Tab` cycles tray
entries, `x` removes the selected one, `Enter` on an entry previews it (image via
`GraphicsProtocol` §7.2, text in a pager), `<`/`>` reorders. The insertion text
is visible in the composer itself, which is the TUI's natural advantage and the
reason the web tray shows an insertion column to match.

**Limits.** **8 attachments per prompt**, refused at the 9th with the limit
named. No agent in the covered set handles more usefully, and a phone user has
not deliberately picked nine.

#### 4.3.9 Limits and security

Consistent with §8, which is where these are enforced. Restated only where the
general-attachment path adds something.

| Control | Value | Enforced |
|---|---|---|
| Size cap | 32 MiB source (`max_blob_bytes`) | Client refuses early with the reason; the **instance** re-checks before allocating (§8). |
| Inline cap | 32 KiB (`media.inline_max_bytes`) | Server-side. The inlined body counts against nothing but the agent's context, which is exactly why the cap exists. |
| MIME | Sniffed with `infer`; images must additionally decode | Server side, always. The client's `mime` and the `accept` attribute are hints; `accept` is trivially bypassed, and a `.png` from a phone may be anything. |
| Filename | One path component, hash-prefixed, Windows reserved names suffixed | §8. |
| Quota | `max_blobs_per_session` = 200, `max_total_bytes` = 512 MiB | §2. A camera roll is large and a determined thumb can fill a disk. |
| TTL | 24 h, 7 d once referenced by an agent | §2. |
| EXIF/GPS | Stripped client-side *and* server-side | §4.3.5, §8. §4.3.2 finding 4: no agent does this for us. |
| Decode limits | `image::Limits`, 256 MiB alloc, 16384² dims | §8. Untrusted images are decoded for thumbnailing and validation only. |
| Archives | Listed, **never extracted** | §4.3.1. No zip-slip surface, because nothing is unpacked. |
| Directory walk | depth ≤ 6, ≤ 500 entries, symlinks not followed | Client and server. A dropped `node_modules` must fail fast and clearly. |
| Deny-list | none by extension | Deliberate: refusing `.sh` or `.env` by extension is theatre. Secret *content* redaction is [13 §8](13-security.md#8-secret-redaction)'s job and applies to what is inlined. |
| Audit | origin + sub-origin, size, sniffed type, session, actor | §8. "Which device uploaded what, when" stays answerable — hence `BlobOrigin::WebUpload` gaining a sub-origin (`picker | camera | files_app | share_target | paste | drop`), because "came from the OS share sheet" is a meaningfully different provenance from "the user pasted it". |

One mobile-specific addition: **the staging cache is cleaned.** The service
worker's `share-inbox` entries are deleted the moment the page reads them, and
swept on every SW activation. A shared file must not persist in a browser cache
after delivery.

Inlined text is passed through [13 §8](13-security.md#8-secret-redaction)'s
redaction before it enters a prompt, on the same rule as the audit log — a user
who drops a `.env` into the tray should see the redaction in the insertion
preview *before* they send it, which is precisely what §4.3.8's insertion column
is for.

#### 4.3.10 The reverse direction — files the agent produces

A plot from a script, a screenshot from a browser tool, a generated report. The
phone must see it, and the mechanism must not be a second one.

1. **Detection**, in confidence order: the agent's own structured output (an ACP
   resource block, an opencode `file` part — the `part` census in
   [agent-clis §3.3](../research/agent-clis.md#33-session-storage) shows these
   exist); a tool call whose result names a written path under the workspace,
   correlated by the same rule as block attribution
   ([08 §4.2](08-web-client.md#42-block-view)); and an inline image sequence in
   the PTY stream (kitty `APC _G`, iTerm2 `OSC 1337;File=`) which `omt-term`
   already parses (§7.2).
2. **Ingestion.** Whichever source, the bytes land in the same blob store with
   `BlobOrigin::AgentOutput`, classified by §4.3.1 and attached to the block they
   occurred in. No copy, no second index.
3. **In the TUI**: images drawn inline via `GraphicsProtocol` (§7.2); everything
   else as a bordered card with name, type, size and `o` to open / `w` to view in
   the web client. Never nothing.
4. **On the phone**: images render from the server-generated WebP thumbnail
   (longest edge 512) with full resolution on tap (§7.3); text and diffs render
   in the shared renderer; PDFs get a page-1 thumbnail and an open action. A
   phone on LTE loads a page of attachments fast and pays for full bytes only
   when asked.
5. **Getting it out.** **Save** (a normal download from the authenticated
   `GET /v1/blob/<id>`, landing in Photos or Files) and **Share** via
   `navigator.share({ files: [...] })` — the exact inverse of §4.3.4, and the
   reason the loop feels closed: an agent draws a chart, you share it into a
   chat, without a laptop entering the story. On desktop, dragging out uses
   `DataTransfer`'s `DownloadURL` entry (§7.4).
6. **Correlation is never guessed.** Where the source is a heuristic path match
   and correlation is ambiguous, the file attaches to the *session* rather than
   the block and is not claimed to be "produced by" that tool call. Same
   discipline as [P4](01-principles.md#p4--native-semantics-observe-never-re-implement).
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
  would on TCP: `omt ssh` mints a **scoped, expiring token** — role `Viewer` with
  `CredentialScope::capabilities = {media.clipboard.*, media.blob.*}`
  ([13 §4.1](13-security.md#41-credential-scope)) — and passes it via
  `OMT_LOCAL_TOKEN` in the remote environment. There is no `Media` role: the role
  ladder is exactly `Viewer < Operator < Admin`, and narrowing is done with
  scope.
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
| **Claude Code** | `@"<abs-path>"` in the prompt text — the exact form Claude Code's own TUI writes for a pasted image ([§4.3.2](#432-what-each-agent-cli-actually-accepts)). Its `Read` tool handles images, PDFs and notebooks, so the path form is reliably picked up. omt adds a short lead-in when the user typed one. | High, and now **VERIFIED**: absolute paths outside the workspace expand, so omt materializes under its own managed root and no longer needs `.omt/media/` inside the repo. |
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
              effects = [Effects::WRITES_FS] }

capability! { name = "media.file.pull", group = "media", verb = "file-pull",
              kind = Command, role = Role::Operator,
              input  = FilePull { path: PathBuf, max_bytes: Option<u64> },
              output = FilePullAck { blob: BlobMeta },
              effects = [Effects::READS_FS] }
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
machine into the user's laptop. It is opt-in per host, carried by a `Viewer`
credential whose capability scope is `media.clipboard.*` + `media.blob.*`,
token-bound to the ssh session, prompted on first clipboard read per host, and
logged. See [13 §4.1](13-security.md#41-credential-scope).

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
4. ~~**Claude Code's `@` expansion for absolute paths outside the project
   root.**~~ **RESOLVED — see [§4.3.2](#432-what-each-agent-cli-actually-accepts)
   finding 2.** Claude Code's own TUI writes `@"<absolute path>"` pointing into
   `~/.claude/uploads/`, which is outside every workspace, so absolute paths are
   expanded and omt does **not** need to materialize into `.omt/media/` inside
   the repo. The `.gitignore`-writing mitigation is dropped. What remains open is
   narrower: whether the quoting form (`@"…"`) is required only for paths
   containing spaces or is always accepted — omt emits it unconditionally, which
   is safe if the second is true and needs a check if not.
5. **Whether a running Claude Code session accepts an image at all without a
   restart.** Partially answered: pasting mid-session works in Claude Code's own
   TUI (§4.3.2, VERIFIED), so the launch-time constraint is not universal. It
   remains open for **Codex**, whose only documented image surface is the
   launch-time `-i, --image` flag; if mid-session attach is unsupported there,
   `attachment_at_launch_only` (§4.3.7) is the honest path and needs testing.
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
10. **Web Share Target on iOS** (§4.3.2). Not supported in Safari today, which
    removes the most delightful capture path from the platform whose screenshots
    are most likely to be shared. Needs re-checking per iOS release; if it never
    arrives, the fallback is a documented Shortcuts recipe that posts to the
    instance, which is worse but real.
11. **HEIC decode coverage** (§4.3.3). The design assumes Safari/iOS decodes HEIC
    via `createImageBitmap`. Verified in documentation, not on a device, and the
    fallback (`libheif` WASM, ~700 KiB lazy) is a real bundle cost if the
    assumption is wrong on older iOS.
12. **The 1568 px longest-edge budget** (§4.3.3) is taken from vision-model
    guidance, not measured against the agents in [D5](decisions.md#d5--initial-agent-coverage).
    If an agent re-encodes or re-tiles anyway, a larger upload is pure waste and
    the number should drop.
13. **Screenshot-vs-photo detection** for the PNG/JPEG re-encode choice
    (§4.3.3) uses a colour-count and alpha heuristic. It will misfire on
    screenshots of photos. The failure is mild (a slightly larger or slightly
    softer file), but it is a heuristic in a document that is otherwise hostile
    to them, and it should be measured before it is defended.
14. **Partial-transfer retention of 10 minutes** (§4.3.4) is a guess, and it
    interacts with the blob quota: an abandoned 8 MB partial counts against
    something for ten minutes. Whether partials are quota'd separately is
    unspecified.
15. **Reverse-direction correlation** (§4.3.8) for agents with no structured file
    output relies on path matching in tool results, which is the same class of
    inference [06 §4](06-agent-layer.md#4-merging-confidence-tiers-not-voting)
    treats as low confidence. The conservative fallback (attach to session, not
    block) may be too conservative to be useful in practice.
16. **Clipboard *monitoring*.** A tempting feature ("omt notices you copied an
    image and offers to attach it") is deliberately not designed here: polling
    the OS clipboard is a privacy hazard and, on macOS, triggers system paste
    notifications. If it is ever built it must be explicitly enabled and visibly
    indicated. Recorded here so the idea is rejected on purpose rather than
    rediscovered.
