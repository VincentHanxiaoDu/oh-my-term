# Remote Protocol, Transport and Auth

How an `omt` instance talks to a remote client. This document specifies
`omt-proto` (the wire contract), `omt-transport` (the byte pipes) and the
protocol-facing half of `omt-server`.

Related: [02 — Crate map](02-crate-map.md) ·
[03 — Capability catalog](03-capability-catalog.md) ·
[05 — Session model](05-session-model.md) ·
[08 — Web client](08-web-client.md) ·
[12 — Collaboration](12-collaboration.md) ·
[13 — Security model](13-security.md) ·
[23 — Identity and devices](23-identity-and-devices.md)

---

## 1. Topology and federation

Identity, devices and the optional `home` registry role are owned by
[23 — Identity and devices](23-identity-and-devices.md). This section owns the
*session* topology; 23 owns *who may connect and how that is proved and
revoked*.

### 1.1 The shape

```
   phone (web client)                         laptop (web client / TUI)
        │  │  │                                       │
        │  │  └────── wss ───────► instance C (cloud dev box)
        │  └───────── wss ───────► instance B (workstation, via tailnet)
        └──────────── wss ───────► instance A (laptop, via tailnet)
                                        ▲
                                   unix socket
                                        │
                                  omt CLI, omt-hook, local TUI
```

Two facts define everything else:

- **Each instance is authoritative for its own sessions.** No instance proxies,
  mirrors or depends on another instance for **session or terminal state**.
  There is no cluster, no leader election, no shared store for the work itself,
  and that is precisely what makes the federation client-side. An instance that
  is offline simply contributes nothing.

  **The one exception, stated precisely.** Instances that share an owner's
  identity registry replicate **revocations and registry epochs only**
  ([23 §3.2](23-identity-and-devices.md#32-how-other-instances-learn-about-it),
  [23 §6.1](23-identity-and-devices.md#61-revoking-one-device)) — a small,
  signed, monotonic, best-effort channel. No session operation depends on it: an
  instance that never syncs still serves every session it owns, and still
  verifies device grants offline against the enrolled identity root key. When
  the channel fails, the failure is reported as *partial*
  (`applied_on` / `pending_on` on `device.revoke`), never hidden. Nothing else
  crosses instance boundaries.
- **The client federates.** The web client holds a list of *instance
  connections*, each with its own credential, its own connection state, its own
  event sequence space, and its own catalog. The unified session list is a
  client-side merge.

This is the opposite of another tool's model, where the first client to attach becomes
the store owner. It costs the client some complexity and buys: no single point
of failure, no cross-machine trust requirement, and a natural Tailscale story
(each machine publishes itself; the phone collects them).

### 1.2 Instance identity

```rust
/// Stable, generated once at first daemon start, persisted in the state dir.
pub struct InstanceId(Uuid);

pub struct InstanceDescriptor {
    pub id: InstanceId,
    pub name: String,             // user-editable, defaults to hostname
    pub version: semver::Version, // omt build version
    pub proto: ProtoVersion,      // wire protocol version
    pub catalog_hash: [u8; 32],   // blake3 of the sorted capability name+schema list
    pub platform: Platform,       // os, arch
    pub started_at: OffsetDateTime,
}
```

`id` is the join key everywhere: session ids are only unique *within* an
instance, so the client's global key for a session is `(InstanceId, SessionId)`.
Wire messages never carry `InstanceId` — the connection determines it. This
keeps frames small and makes it impossible to spoof another instance's id over
an authenticated connection.

### 1.3 Adding an instance

Three paths, all producing the same client-side record:

| Path | Flow | Use |
|---|---|---|
| **Invite link** | `omt invite --role operator --ttl 24h` prints `https://host:7878/#/join?i=<b64 invite>`. Opening it on the phone adds the instance and exchanges the invite for a long-lived credential. | first setup, sharing with a colleague |
| **Manual** | user enters URL + bearer token, or URL + username/password | scripted / password managers |
| **Tailnet discovery** | client queries the local tailnet for peers advertising `_omt._tcp` (or, when the client runs on a tailnet device, the Tailscale LocalAPI peer list) and offers a one-tap "add" that authenticates via tailnet identity | the common case once Tailscale is in play |

An invite is a signed, expiring, scoped token; it is *not* the credential. The
first successful use exchanges it for a per-device credential bound to a device
public key, so a leaked link is time-boxed and revocable per device. Full
mechanics in [13 §3](13-security.md).

```json
{
  "t": "join.exchange",
  "id": "req_1",
  "invite": "eyJ2IjoxLCJpbnN0Ijoi…",
  "device": {
    "name": "Vincent's iPhone",
    "pubkey": "ed25519:9f3a…",
    "platform": "ios/safari"
  }
}
```

```json
{
  "t": "join.credential",
  "id": "req_1",
  "instance": { "id": "3f2a…", "name": "workstation", "version": "0.4.1", "proto": 1 },
  "credential": { "kind": "bearer", "token": "omt_c_9K3…", "role": "operator", "expires_at": null }
}
```

### 1.4 Per-instance connection state

The client models each instance as a small state machine, surfaced in the UI —
never hidden, because "is my laptop reachable" is a question the user asks
constantly.

```
Unconfigured → Connecting → Handshaking → Authenticating
                   ▲                            │
                   │                            ▼
              Backoff ◄── Disconnected ◄─── Ready ──► Degraded (resynced / partial)
                                  ▲                       │
                                  └───────────────────────┘
   terminal states: Unauthorized (credential rejected/revoked),
                    Incompatible (proto version unsupported)
```

`Unauthorized` and `Incompatible` do **not** retry; everything else backs off.
Each instance row in the session list carries its state so a stale list is
visibly stale rather than quietly wrong.

### 1.5 Federating across versions

Per [03 §7](03-capability-catalog.md#7-versioning), the handshake returns the
instance's actual capability list. The client keeps a per-instance
`Set<CapabilityName>` and computes:

- **Per-session actions** are gated on *that session's instance* — not on the
  intersection. A session on a new instance keeps its new buttons.
- **Multi-select / bulk actions** across sessions from several instances are
  gated on the **intersection** of the selected instances' catalogs. Actions
  outside the intersection are shown disabled with "not supported on
  `workstation` (0.3.2)".
- **Unknown event payload variants** are ignored by the client (all `Payload`
  enums are `#[serde(other)]`-tolerant and the TS types carry a catch-all), so a
  newer instance can emit events an older client cannot render without breaking
  the stream.

Rendering the intersection and greying out the rest is the rule; failing is
never the rule.

### 1.6 The unified session list

The client merges per-instance session lists into one view sorted by
*attention*, not by instance:

1. sessions with an open `Interaction` (blocked agent — needs a human),
2. sessions in `AgentState::Blocked` with no interaction id (omt can see the
   block but cannot render it),
3. `Working`,
4. `Idle`, most recently active first.

`AgentState` variant names are owned by
[06 §4](06-agent-layer.md#4-merging-confidence-tiers-not-voting); there is no
separate `busy`/`needs_attention` vocabulary.

Instance is a subtitle and a filter, not the primary grouping. This is a
deliberate product decision: the user's question is "what needs me?", and the
answer should not be spread across four collapsed sections.

---

## 2. Transport layer

### 2.1 The trait

`omt-transport` is framing only. No auth, no routing, no protocol semantics —
those live in `omt-proto` and `omt-server` respectively (P1).

```rust
/// A bidirectional, ordered, reliable message pipe carrying `Frame`s.
#[async_trait]
pub trait Transport: Send + 'static {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Receive the next frame. `Ok(None)` means the peer closed cleanly.
    async fn recv(&mut self) -> Result<Option<Frame>, Self::Error>;

    /// Send one frame. Implementations must be cancel-safe.
    async fn send(&mut self, frame: Frame) -> Result<(), Self::Error>;

    /// Best-effort flush; used before a deliberate close.
    async fn flush(&mut self) -> Result<(), Self::Error>;

    fn peer(&self) -> PeerInfo;      // socket addr, uid for unix, tailnet identity if any
    fn kind(&self) -> TransportKind; // WebSocket | UnixSocket | SshStdio
}

/// Exactly two frame kinds. Everything above is built on this.
pub enum Frame {
    /// UTF-8 JSON control message (`ProtoMessage`).
    Text(Bytes),
    /// Length-delimited binary payload with a 24-byte header (see §3.6).
    Binary(Bytes),
}
```

Ordering and reliability are assumed *within* a connection; the protocol layer
provides recovery *across* connections (§5). Transports must not reorder, must
not merge frames, and must not deliver partial frames.

### 2.2 WebSocket — primary

`wss://<host>:7878/v1/ws`, subprotocol `omt.v1`. Native WebSocket framing
carries `Frame` directly (`Text` → text frame, `Binary` → binary frame); no
extra length prefix is needed. Limits:

| Limit | Value | Rationale |
|---|---|---|
| max control frame | 1 MiB | schemas are small; a bigger control frame is a bug or an attack |
| max binary frame | 8 MiB | one image/screenshot; larger uploads are chunked (§3.6) |
| max in-flight unacked terminal bytes | 256 KiB per subscription | backpressure trigger (§6) |
| permessage-deflate | **off** for terminal/binary, on for control | terminal output is already compressed by coalescing; deflate on every keystroke costs latency and CPU for nothing |

### 2.3 Unix socket — local CLI, TUI fallback, hooks

`$XDG_RUNTIME_DIR/omt/<instance>.sock`, mode `0600`. Framing is a 4-byte
little-endian length prefix plus a 1-byte kind tag (`0` text, `1` binary), then
the payload. Identical `ProtoMessage` catalogue — this is the point: the local
CLI exercises the same protocol the phone does, so a bug in the remote path
shows up in `omt session list`.

Unlike another tool, the socket is **not** implicitly trusted. `SO_PEERCRED`
(Linux) / `LOCAL_PEERCRED` (macOS) is checked, and a peer whose uid differs from
the daemon's uid is rejected before the handshake. Same-uid peers get the
`Local` actor and `Admin` role by default, configurable down. See
[13 §2](13-security.md).

`omt-hook` uses this socket and nothing else; it never speaks WebSocket. Its two
messages are `HookEvent` and `HookAck`, specified in §3.8.

### 2.4 SSH stdio bridge — `omt ssh <target>`

`omt ssh workbox` does **not** invent a network protocol. It runs:

```
ssh -T <target> -- omt serve --stdio --proto 1
```

and speaks the length-prefixed framing of §2.3 over the ssh subprocess's
stdin/stdout. This gives, for free: existing SSH auth and host-key trust,
jump hosts, agent forwarding, and corporate policy compliance. Concretely:

- omt generates a temporary SSH config that includes the user's config first,
  then adds `ServerAliveInterval 15`, `ServerAliveCountMax 4`, and a private
  `ControlPath` so a second `--remote` to the same host reuses the connection.
  `[remote].manage_ssh_config = false` opts out entirely.
- Remote binary resolution: `PATH` → known install prefixes → prompt to install
  a version-matched binary to `~/.local/bin/omt`. `OMT_REMOTE_BINARY` overrides
  for development.
- Version skew is handled by the same catalog-intersection rule as §1.5, so a
  slightly older remote is usable, not fatal.
- stderr from the remote process is captured and surfaced as diagnostics; it is
  never interleaved into the frame stream (a classic stdio-bridge bug).

Authentication over the stdio bridge is transport-level: the peer already proved
SSH access to the account that owns the daemon. The bridge therefore skips the
`auth.*` exchange and is assigned `Admin` unless configured otherwise.

### 2.5 Keepalive, reconnect, backoff

- **Keepalive**: the server sends a `ping` control message every 20 s if the
  connection has been idle; the client must answer `pong` within 10 s. WebSocket
  protocol-level pings are also used where available, but the application-level
  ping is authoritative because intermediaries answer protocol pings.
- **Death detection**: 2 missed pongs, or a transport error, closes the
  connection. On mobile this is deliberately generous — radios sleep.
- **Reconnect** (client side) uses full-jitter exponential backoff:

```rust
fn backoff(attempt: u32) -> Duration {
    let base = Duration::from_millis(250);
    let cap  = Duration::from_secs(30);
    let exp  = cap.min(base * 2u32.saturating_pow(attempt.min(10)));
    Duration::from_millis(rand::thread_rng().gen_range(0..=exp.as_millis() as u64))
}
```

Full jitter (not "exp/2 + jitter") because the pathological case is a laptop
waking up and every one of its instance connections retrying in lockstep.
Attempt counter resets after 60 s of a healthy connection, not on connect, so a
flapping link does not reset into a hot loop.

- **Foreground override**: when the tab becomes visible or the network changes
  (`online` event / `navigator.connection` change), the client resets the
  attempt counter and reconnects immediately. Users tapping into an app expect
  it to try *now*.

---

## 3. Message protocol

### 3.1 Encoding decisions

| Channel | Encoding | Why |
|---|---|---|
| control (handshake, auth, capability calls, events) | **JSON** text frames | debuggable with `websocat`, one schema source (`schemars`) shared with REST and the TS client, negligible volume |
| terminal output | **binary** frames, raw bytes with a 24-byte header | this is the high-rate path; base64 in JSON costs 33 % bandwidth and a parse per frame |
| audio (STT upload) | **binary** frames, Opus | obvious |
| images / files | **binary** frames, chunked | ditto |

Rejected: a single binary encoding (bincode/msgpack) for everything, as another tool
does. It saves a few percent on a channel that is not the bottleneck, and costs
the ability to read the wire in a browser devtools panel — which for a product
whose primary client is a browser is a bad trade. The terminal and media paths,
where it actually matters, *are* binary.

### 3.2 The envelope

```rust
#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ProtoMessage {
    // ---- connection lifecycle ----
    Hello(Hello),                       // C→S, first message
    Welcome(Welcome),                   // S→C
    AuthChallenge(AuthChallenge),       // S→C
    Auth(Auth),                         // C→S
    AuthOk(AuthOk),                     // S→C
    Error(ProtoError),                  // either direction, may be unsolicited
    Ping { nonce: u64 },
    Pong { nonce: u64 },
    Close { code: CloseCode, reason: String },

    // ---- capability RPC ----
    Call(Call),                         // C→S
    Result(CallResult),                 // S→C
    Cancel { id: RequestId },           // C→S

    // ---- events ----
    Subscribe(Subscribe),               // C→S
    Unsubscribe { sub: SubId },         // C→S
    Event(EventFrame),                  // S→C
    Resync(Resync),                     // S→C, unsolicited
    Lagged(Lagged),                     // S→C, unsolicited

    // ---- terminal ----
    TermAttach(TermAttach),             // C→S
    TermDetach { session: SessionId },  // C→S
    TermInput(TermInput),               // C→S (small inputs; large paste uses binary)
    TermResize(TermResize),             // C→S
    TermSnapshotMeta(TermSnapshotMeta), // S→C, precedes a binary snapshot payload

    // ---- binary channel bookkeeping ----
    BlobBegin(BlobBegin),               // either direction
    BlobAbort { blob: BlobId, reason: String },
    BlobDone { blob: BlobId, sha256: String },

    // ---- agent hook ingress (unix socket only, §3.8) ----
    HookEvent(HookEvent),               // omt-hook → daemon
    HookAck(HookAck),                   // daemon → omt-hook
}
```

Every request-bearing message carries `id: RequestId`; every response echoes it.
Unsolicited server messages carry no `id`. `RequestId` is **stable across
connections** — see §3.5.

### 3.3 Handshake and capability negotiation

```json
{ "t": "hello", "id": "req_0",
  "proto": [1],
  "client": { "name": "omt-web", "version": "0.4.1", "kind": "web" },
  "device": { "id": "dev_7Qa…", "name": "iPhone", "platform": "ios/safari" },
  "features": ["term.binary", "blob.chunked", "stt.opus"],
  "resume": { "session_token": "omt_s_5tR…" }
}
```

```json
{ "t": "welcome", "id": "req_0",
  "proto": 1,
  "instance": { "id": "3f2a…", "name": "workstation", "version": "0.4.1",
                "platform": { "os": "linux", "arch": "aarch64" },
                "catalog_hash": "b3:7c1e…" },
  "auth": { "required": true, "methods": ["bearer", "password", "invite", "tailnet", "device_grant"] },
  "features": ["term.binary", "blob.chunked", "stt.opus", "term.ack"],
  "limits": { "max_control_frame": 1048576, "max_binary_frame": 8388608,
              "replay_window_bytes": 4194304, "replay_window_events": 4096 }
}
```

`proto` in `Hello` is the list of versions the client supports; the server picks
the highest it also supports and states it in `Welcome`. No match →
`Error{code:"unsupported_proto"}` then `Close`. Feature strings are additive
capability flags for things too fine-grained to be a proto version bump; both
sides intersect them and neither may use a feature the other did not list.

The **catalog** itself is fetched with a normal capability call after auth
(`instance.catalog`), keyed by `catalog_hash` so the client can cache it across
reconnects and across sessions:

```json
{ "t": "call", "id": "req_2", "name": "instance.catalog",
  "input": { "known_hash": "b3:7c1e…" } }
```

```json
{ "t": "result", "id": "req_2", "ok": true,
  "output": { "unchanged": true } }
```

### 3.4 Auth

```json
{ "t": "auth_challenge", "id": null,
  "methods": ["bearer", "password", "invite", "tailnet", "device_grant"],
  "nonce": "n_9f2c…" }
```

```json
{ "t": "auth", "id": "req_1", "method": "bearer",
  "bearer": { "token": "omt_c_9K3…" },
  "device_sig": "ed25519:5b7e…" }
```

```json
{ "t": "auth_ok", "id": "req_1",
  "actor": { "id": "act_12", "label": "iPhone (Vincent)", "kind": "remote" },
  "role": "operator",
  "scope": { "visibility": { "kind": "all_sessions" }, "capabilities": null },
  "session_token": "omt_s_5tR…",
  "expires_at": "2026-08-04T09:11:00Z" }
```

`session_token` is a short-lived resume token (default 12 h) so a reconnect can
skip the credential round-trip; it is bound to the device key and to the
credential, and is invalidated when the credential is revoked. `scope` is the
`CredentialScope` from [13 §4.1](13-security.md#41-credential-scope) — the
client uses it to grey out affordances it is not allowed to use, and the server
enforces it in dispatch regardless.

There is no per-credential *interaction policy*: an `Operator` may resolve any
interaction the agent posed, and a `Viewer` may resolve none. omt adds no gate
over the agent's own permission semantics
([D1](decisions.md#d1--omt-adds-no-policy-layer-over-an-agents-permission-semantics)),
and an authenticated remote client is equivalent to the local TUI
([D2](decisions.md#d2--remote-is-exactly-equivalent-to-local)).

### 3.5 Capability call / result

The protocol is a thin envelope over the catalog. `omt-proto` owns no policy and
no capability knowledge; it forwards `(name, input_json)` to
`CapabilityRegistry::dispatch`.

```json
{ "t": "call", "id": "req_31", "name": "interaction.resolve",
  "input": {
    "interaction": "int_88",
    "response": { "type": "choices",
                  "answers": [ { "labels": ["Use Postgres"],
                                 "other": null, "comment": null } ] }
  },
  "deadline_ms": 10000 }
```

```json
{ "t": "result", "id": "req_31", "ok": true,
  "output": { "resolved_by": "act_12", "seq": 91422 } }
```

```json
{ "t": "result", "id": "req_31", "ok": false,
  "error": { "code": "conflict",
             "message": "Interaction int_88 was already resolved by the local TUI",
             "detail": { "resolved_by": "local", "at": "2026-08-03T18:22:04.113Z" } } }
```

Error codes are exactly the closed catalog enum (`not_found`, `conflict`,
`unauthorized`, `precondition_failed`, `unsupported`, `internal`) plus
protocol-level `unsupported_proto`, `auth_failed`, `rate_limited`,
`frame_too_large`. Clients switch on `code`, never on `message`.

Calls are concurrent and out-of-order by design; `Cancel` requests best-effort
abortion and is honoured for queries and for commands that have not yet taken
effect.

#### `RequestId` is stable across connections, and dispatch caches results

```rust
pub struct RequestId { pub device: DeviceId, pub n: u64 }   // wire: "dev_9a:41827"
```

`n` is a **monotonic counter persisted client-side**, not a per-connection
sequence. The earlier definition — *"client-generated, unique per connection"* —
made `RequestId` useless for the one job it exists to do: a client whose socket
dies mid-call reconnects with a fresh counter, so it can never ask "did `req_31`
apply?" and can never learn whether its `interaction.resolve` took effect. It
must then either re-send blind or give up, and neither is acceptable for a
mutation. `DeviceId` supplies cross-connection uniqueness without coordination;
the persisted counter supplies stable identity.

`CapabilityRegistry::dispatch` therefore keeps a **bounded recent-results
cache** keyed by `RequestId`: on a repeat it replays the stored result verbatim
and does not execute the capability a second time. The cache is bounded by count
and age (default 1024 entries / 10 minutes per device) and is memory-only — an
eviction or a daemon restart turns a repeat into a normal call, which is why the
mechanism only makes the *retry-safe* classes of
[D15](decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism)
work and is explicitly **not** relied on for the byte-stream or
externally-confirmed classes. Those must never be replayed at all: an injected
answer is at-most-once ([06 §5.1](06-agent-layer.md#51-lifecycle)), and a raw
write is resumed by `ack`, not repeated (§3.6).

> **Choice made here.** The cache is memory-only rather than persisted. A
> persisted cache would have to be `fsync`ed on the critical path of every
> mutation to be worth anything, and the classes that need durability across a
> restart already get it from their own ledger CAS. Recorded so the tradeoff is
> visible rather than assumed.

### 3.6 Binary payloads

A binary frame is a 24-byte header followed by the payload:

```
 0        1        2                4                8                          16                          24
 +--------+--------+----------------+----------------+---------------------------+---------------------------+
 | ver(1) | kind(1)|   stream(u16)  |  reserved(u32) |      seq_or_off(u64)      |          ack(u64)         | payload…
 +--------+--------+----------------+----------------+---------------------------+---------------------------+
 kind: 1 = terminal output   2 = terminal input   3 = blob chunk
       4 = audio chunk       5 = terminal snapshot
 all integers little-endian; `reserved` MUST be zero and is rejected otherwise
```

`stream` is a small integer handed out by the server (for terminal streams, at
`TermAttach` time) — 2 bytes rather than a 16-byte UUID per frame, which at 60
frames/s matters. `seq_or_off` is the per-stream sequence for terminal and audio
and the byte offset for blob chunks.

**`seq_or_off` is `u64`, matching `Seq` everywhere else — choice recorded.**
An earlier draft carried it as `u32`, which silently truncated a value the rest
of the system holds in 64 bits
([04 §4.1](04-terminal-core.md#41-what-a-renderer-gets)'s `Snapshot.seq`,
[§5.1](#51-sequence-spaces)'s per-session space,
[glossary `Seq`](glossary.md)). Three options were considered — widen; keep
`u32` as an offset within an explicit epoch; keep `u32` with a defined
wrap-around window — and **widening wins on cost**. A `u32` sequence wraps after
~4.3 × 10⁹ frames or bytes, and the failure that produces is the worst kind: a
client resuming at `since_seq` after a wrap rejoins at a point the server also
considers valid, so it believes it is caught up and is not, with no error on
either side. The two `u32`-preserving options both buy 8 bytes by introducing a
second sequence-space concept (an epoch, or a wrap window) into a protocol whose
whole resume story rests on there being exactly three sequence spaces and
nothing else — a permanent complexity cost against a one-time byte cost.

The byte cost is negligible against this document's own targets: [§6.2](#62-coalescing-terminal-frames)
coalesces terminal output on a 16 ms timer with a 32 KiB early-flush, so a
saturated stream carries **one** header per up-to-32 KiB payload — 24 bytes
against 32768 is 0.07 %. The worst case is uncoalesced input frames (`kind=2`,
exempt from coalescing), where a 1-byte keystroke costs 24 bytes of header
instead of 12; at human typing rates that is tens of bytes per second. Neither
figure is visible against [§7](#7-latency-budget)'s budget.

**`u64` does not wrap in any reachable regime** — at 10⁹ increments/s it lasts
~580 years — so there is no wrap behaviour to define, and that is the point of
the choice. A client that nonetheless observes `seq_or_off` **decrease** on a
stream treats it as a server-side sequence reset, not a wrap: it discards its
resume point and expects the `Resync{reason: "sequence_reset"}` that
[§5.2](#52-replay-window)'s last row already defines for the "client from the
future" case. There is no other legal way for a sequence to go backwards.
`since_seq` ([§3.7](#37-subscriptions), [§5](#5-resume-and-reliability)) and the
replay window are therefore unchanged: `since_seq` is a `u64` in the same space
as the header field, comparisons are plain ordering with no modular arithmetic,
and the replay window's bounds (`min(4 MiB, 4096 events)`) are far too small for
the question to arise.

**`ack` is `u64` for the same reason, and its space is *not* bounded.** It is
[D15](decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism)
consequence 7's client-input sequence, counted in **bytes consumed into the
PTY** — and input is not only typing. A pasted file, a `blob` push replayed as
input, or an agent driving a session pushes megabytes through that counter, so
"a human can't type 4 GiB" is not an argument that holds. Since the whole
purpose of `ack` is that the byte-stream class has *no* safe replay, an
ambiguous ack is unrecoverable by construction, which is exactly the case not to
economise on. Same rule as above: `ack` never wraps, and a decrease means reset.

**`ack: u64` is reserved now, before the wire freezes.** On a `kind=1` terminal
output frame it carries **the highest client input sequence the server has
consumed** into the PTY. It has two independent rationales and both are recorded
here so it cannot be value-engineered out as "a v2 feature":

1. **Predictive echo.** [`remote-continuity §10.3`](../design/remote-continuity.md#103-model-additions)
   already requires it: mosh-style local echo cannot confirm or revert a
   prediction without knowing which of its own keystrokes the far end has
   actually consumed.
2. **Durability — the load-bearing one.** `session.write_bytes` is
   [D15](decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism)'s
   **raw byte stream** class, which **must never be replayed**: re-sending
   keystrokes into whatever the terminal is doing now is arbitrary damage, not a
   retry. A consumed-offset ack is therefore the *only* safe resumption
   mechanism available to that class. On reconnect the client resumes from
   `ack`, sends nothing that was already consumed, and reports the ambiguous tail
   (written but unacknowledged) to the user rather than silently re-sending it.
   Without `ack` in the header, the byte-stream class has no correct recovery at
   all, and a reconnect after a dropped link either loses input or duplicates it.

Zero is the "no information" value; a client that sees only zeros disables
prediction and treats every reconnect tail as ambiguous. This costs 8 bytes per
frame against a coalesced frame budget ([§6.2](#62-coalescing-terminal-frames))
measured in kilobytes.

Blobs (images, file push/pull, snapshots) are negotiated in JSON and carried in
binary:

```json
{ "t": "blob_begin", "id": "req_44", "blob": "blob_7", "stream": 12,
  "purpose": "image_paste", "session": "s_4b2f",
  "mime": "image/png", "bytes": 481203, "sha256": "9c1f…" }
```

then N `kind=3` binary frames with ascending offsets, then
`{"t":"blob_done","blob":"blob_7","sha256":"9c1f…"}`. The receiver verifies size
and digest, and rejects on mismatch. Quotas and temp-file lifecycle are
[09 — SSH and media](09-ssh-and-media.md)'s business.

### 3.7 Subscriptions

```json
{ "t": "subscribe", "id": "req_5", "sub": "sub_1",
  "filter": { "sessions": ["s_4b2f", "s_9de1"], "workspaces": ["w_9f3c"],
              "kinds": ["agent", "interaction", "presence"] },
  "since_seq": { "s_4b2f": 91380, "s_9de1": 4021, "w_9f3c": 41208 },
  "policy": { "on_lag": "resync", "max_buffered_events": 2048 } }
```

Filters are coarse on purpose (session set × event-kind set). Fine-grained
server-side filtering is a footgun: it makes resume semantics ambiguous, because
`since_seq` must mean "everything you would have received", which depends on the
filter. Coarse filters keep that mapping obvious.

`workspaces` is the workspace-scoped analogue of `sessions`: workspace-scoped
events carry `workspace` instead of `session` in the envelope and have their own
`Seq` space, so `since_seq` is keyed uniformly by whichever id the event carries.
Subscribing to `workspace_fs` does **not** by itself create a watcher — that is
`workspace.files.watch`'s job (15 §6).

#### 3.7.1 The kinds, and the payload each carries

`kinds` is the top-level grouping of `omt-events`. Ten, closed:

```rust
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Terminal, Agent, Interaction, SessionTree, Presence,
    Config, Plugin, Audit, WorkspaceFs, Instance,
}
```

> **Choice made here.** The set was nine; `instance` is added. Without it,
> [22 §4.4](22-operations.md#44-per-session-fault-isolation-r7)'s
> `instance.degraded` — the event that tells a client the daemon has stopped
> persisting — has no kind, and an event with no kind is unsubscribable through
> `filter.kinds`. `instance` is also the natural home for the third sequence
> space (below), which `config`, `plugin` and `audit` share.

**Scope decides which id and which `Seq` space an event carries**, per
[03 §4](03-capability-catalog.md#4-events-are-the-read-side-twin):

| Scope | Envelope carries | `since_seq` keyed by |
|---|---|---|
| session | `session`, `workspace` = `null` | `SessionId` |
| workspace | `workspace`, `session` = `null` | `WorkspaceId` |
| instance | neither | the reserved id `s_instance` |

The instance space is the third one: `config`, `plugin`, `audit` and `instance`
events belong to no session and no workspace, and before this they had nowhere
to be counted.

**It is keyed by the reserved id `s_instance`, not by an `InstanceId`**, for the
reason given in [§1.2](#12-instance-identity): a connection is already
bound to exactly one instance, so wire messages never carry an `InstanceId` —
sending one would be redundant at best and a second, contradictable source of
truth at worst. `s_instance` is a reserved value in the same id space as
`SessionId` and `WorkspaceId`, which keeps `since_seq` a single uniformly-typed
key across all three scopes rather than a union. [§5.1](#51-sequence-spaces) and
[§5.2](#52-replay-window) use the same reserved id.

```rust
/// The payload of the protocol `Event` envelope. Tagged `type`; the `kind` in
/// `Subscribe.filter.kinds` is the group, not a field on the wire — a client
/// switches on `payload.type`, and the server uses the group to filter.
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventPayload {
    // ============ kind: terminal — session-scoped ============
    // Block, cwd, title and bell facts from `omt-term` (05 §10.4).
    BlockOpened   { block: BlockId, origin: BlockOrigin, at: Position },
    BlockClosed   { block: BlockId, state: BlockState, exit: Option<i32>,
                    duration_ms: Option<u64>, attribution: Attribution },
    BlockUpdated  { block: BlockId, state: BlockState },
    CwdChanged    { cwd: PathBuf },
    TitleChanged  { title: String },
    Bell          {},
    /// 05 §9. Emitted on block closure, so it belongs with the block events.
    HistoryAppended { entry: HistoryEntry },
    /// 05 §10.4 / 17 §3.4. The negotiated size changed, and why.
    SessionResized  { size: GridSize, policy: SizePolicy, reason: ResizeReason },

    // ============ kind: agent — session-scoped ============
    /// The whole of [06 §8.1](06-agent-layer.md#81-agentevent--the-envelope).
    /// The envelope's `session`, `seq`, `ts` and `source` are copies of the
    /// inner event's, never independently computed (06 §8.1).
    AgentEvent    { event: AgentEvent },
    BindingStarted { binding: AgentBinding },
    BindingEnded   { binding: BindingId, at: Timestamp, reason: String },
    /// 06 §4. Carried separately from `AgentEvent` because state is a *merge
    /// result* over several sources, not something any one source emits.
    AgentStateChanged { binding: BindingId, from: AgentState, to: AgentState,
                        deciding_tier: Tier },

    // ============ kind: interaction — session-scoped ============
    /// The full [06 §5](06-agent-layer.md#5-interactions--the-flagship-path)
    /// object, including `state`, `responder` and `viewers`.
    InteractionOpened { interaction: Interaction },
    /// Every transition after the open, including the terminal ones. Clients
    /// switch on `state.type` — see the note below.
    InteractionStateChanged { interaction: InteractionId, state: InteractionState },
    /// Advisory only; the ledger still decides (12 §4.4).
    InteractionViewersChanged { interaction: InteractionId, viewers: Vec<ActorId> },

    // ============ kind: session_tree ============
    WorkspaceOpened   { workspace: Workspace },                 // workspace-scoped
    WorkspaceClosed   {},                                       // workspace-scoped
    WorkspaceRenamed  { name: String },                         // workspace-scoped
    ViewCreated       { view: LayoutView },                     // workspace-scoped
    ViewClosed        { view: ViewId },                         // workspace-scoped
    ViewSelected      { view: ViewId, by: Actor },              // workspace-scoped
    /// 05 §10.4 / 17. Workspace-scoped because a `Layout` belongs to a
    /// `LayoutView`, which belongs to a workspace — not to a session.
    LayoutChanged     { view: ViewId, layout: Layout, geometry_hint: Option<Geometry> },
    SessionCreated    { session: Session },                     // workspace-scoped
    SessionClosed     { session: SessionId },                   // workspace-scoped
    SessionRenamed    { name: String },                         // session-scoped
    SessionStateChanged { from: SessionState, to: SessionState }, // session-scoped
    FocusChanged      { view: ViewId, pane: Option<PaneId>, by: Actor }, // workspace-scoped

    // ============ kind: presence — session-scoped ============
    PresenceChanged   { presence: Presence },
    /// 05 §10.4, semantics in 12 §3. Grouped with presence rather than with
    /// session_tree: "who may type" is the same question as "who is here", and
    /// a client that renders one always renders the other.
    WriterChanged             { token: Option<WriterToken>, reason: WriterChangeReason },
    WriterTakeoverRequested   { by: Actor, expires_at: Timestamp },
    WriterTakeoverResolved    { by: Actor, granted: bool },

    // ============ kind: config — instance- or workspace-scoped ============
    /// 10 §4. `keys` are setting paths, never values — a value may be a secret.
    ConfigChanged { keys: Vec<String>, scope: ConfigScope, source: ConfigSource,
                    by: Actor, reload: ReloadKind },
    /// 10 §5.3. A reload that failed validation; the old config stays live.
    ConfigInvalid { diagnostics: Vec<Diagnostic> },

    // ============ kind: plugin — instance-scoped ============
    PluginLoaded   { plugin: PluginId, version: String, granted: Vec<CapabilityPattern> },
    PluginUnloaded { plugin: PluginId, reason: String },
    PluginFailed   { plugin: PluginId, diagnostic: Diagnostic },

    // ============ kind: audit — instance-scoped, Admin only ============
    /// 13 §6. Subscribing requires `Admin`; the bus filters per subscription,
    /// so a non-admin subscription to `audit` yields nothing rather than an
    /// error, exactly as a filtered-out session does.
    AuditAppended { entry: AuditEntry },

    // ============ kind: workspace_fs — workspace-scoped ============
    /// 15 §4.6. Coalesced by the watcher; `truncated` says so honestly.
    FilesChanged { changes: Vec<FileEvent>, truncated: bool },
    VcsChanged   { summary: VcsSummary },
    /// The watcher stopped (descriptor exhaustion, a rescan, an unmount).
    /// A dropped watch is reported, never silently mistaken for "no changes".
    WatchDropped { reason: String, rescan_required: bool },

    // ============ kind: instance — instance-scoped ============
    /// 22 §4.4. Added or cleared as instance-level capability degrades.
    InstanceDegraded  { degradation: InstanceDegradation },
    InstanceRecovered { kind: String },
    InstanceShuttingDown { reason: String, grace_ms: u32 },
}
```

**Inner types are owned elsewhere and referenced, never restated here.**
`AgentEvent` is [06 §8.1](06-agent-layer.md#81-agentevent--the-envelope);
`Interaction`, `InteractionState`, `AgentBinding`, `AgentState`, `Tier` are
[06](06-agent-layer.md); `Block*`, `Attribution`, `Position` are
[04](04-terminal-core.md); `Session`, `Workspace`, `SessionState`,
`HistoryEntry`, `Presence`, `WriterToken` are [05](05-session-model.md) with
writer/presence semantics in [12](12-collaboration.md); `Layout`, `LayoutView`,
`Geometry`, `SizePolicy` are [17](17-panes-and-layout.md); `Diagnostic`,
`ConfigSource` are [10](10-configuration.md); `AuditEntry` is
[13](13-security.md); `FileEvent`, `VcsSummary` are
[15](15-workspace-explorer.md); `InstanceDegradation` is
[22 §4.4](22-operations.md#44-per-session-fault-isolation-r7).

> **Choice: one transition event, not four.** `interaction` could have had
> `interaction_resolved`, `interaction_cancelled`, `interaction_submitted` and
> `interaction_undelivered` as separate variants. It has one
> `interaction_state_changed` carrying [06 §5](06-agent-layer.md#5-interactions--the-flagship-path)'s
> `InteractionState`, because that enum is already the authoritative state
> machine ([12 §4.1](12-collaboration.md#41-the-invariant) owns its
> transitions) and a second, parallel vocabulary on the wire would let the two
> disagree. `Submitted` and `Undelivered` ([D15](decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism))
> are states in that enum, so they need no wire work — which is the point.

#### 3.7.2 One example per kind

The `interaction` example is the corrected form of the one that used to stand
here alone: it carries the whole `Interaction`, including the `state`,
`responder` and `viewers` that 06 §5 requires and the old illustration omitted.

```json
{ "t": "event", "sub": "sub_1", "session": "s_4b2f", "workspace": null,
  "seq": 91380, "ts": "2026-08-03T18:21:58.400Z", "source": "core",
  "caused_by": null,
  "payload": { "type": "block_closed", "block": "blk_31",
               "state": "finished", "exit": 0, "duration_ms": 812,
               "attribution": "agent" } }
```

```json
{ "t": "event", "sub": "sub_1", "session": "s_4b2f", "workspace": null,
  "seq": 91381, "ts": "2026-08-03T18:21:59.100Z", "source": "hook",
  "caused_by": null,
  "payload": { "type": "agent_event",
               "event": { "session": "s_4b2f", "binding": "bnd_5",
                          "agent": "claude_code", "agent_version": "2.1.4",
                          "agent_session": "1f0c…", 
                          "thread": { "id": "th_0", "parent": null,
                                      "is_subagent": false, "label": null },
                          "seq": 91381, "ts": "2026-08-03T18:21:59.100Z",
                          "tier": "hook", "source": "hook_bridge",
                          "cwd": "/home/v/src/omt", "git_branch": "main",
                          "payload": { "type": "tool_call", "turn": "t_12",
                                       "call": "toolu_01A…", "name": "Edit",
                                       "input": { "file_path": "…" },
                                       "status": "running", "parent": null } } } }
```

```json
{ "t": "event", "sub": "sub_1", "session": "s_4b2f", "workspace": null,
  "seq": 91382, "ts": "2026-08-03T18:21:59.882Z", "source": "hook",
  "caused_by": null,
  "payload": { "type": "interaction_opened",
               "interaction": {
                 "id": "int_88", "session": "s_4b2f", "binding": "bnd_5",
                 "kind": { "type": "choice",
                           "questions": [{ "question": "Which database should I use?",
                                           "header": "Database",
                                           "multi_select": false,
                                           "allow_free_text": true,
                                           "options": [{ "label": "Postgres", "description": "…" },
                                                       { "label": "SQLite",   "description": "…" }] }] },
                 "opened_at": "2026-08-03T18:21:59.882Z",
                 "timeout_at": "2026-08-03T18:31:59.882Z",
                 "state": { "type": "open" },
                 "responder": { "fidelity": "synthetic",
                                "state_dependence": "independent",
                                "supports_edit": false },
                 "viewers": [] } } }
```

```json
{ "t": "event", "sub": "sub_1", "session": null, "workspace": "w_9f3c",
  "seq": 41209, "ts": "2026-08-03T18:22:03.010Z", "source": "core",
  "caused_by": "dev_9a:41827",
  "payload": { "type": "layout_changed", "view": "vw_1",
               "layout": { "…": "17 §1.3" }, "geometry_hint": null } }
```

```json
{ "t": "event", "sub": "sub_1", "session": "s_4b2f", "workspace": null,
  "seq": 91383, "ts": "2026-08-03T18:22:04.113Z", "source": "core",
  "caused_by": "dev_9a:41828",
  "payload": { "type": "writer_changed",
               "token": { "holder": { "id": "act_12", "kind": "remote" },
                          "acquired_at": "2026-08-03T18:22:04.113Z",
                          "epoch": 7 },
               "reason": "auto_acquired" } }
```

```json
{ "t": "event", "sub": "sub_2", "session": null, "workspace": null,
  "seq": 5512, "ts": "2026-08-03T18:22:10.000Z", "source": "core",
  "caused_by": "dev_9a:41830",
  "payload": { "type": "config_changed", "keys": ["agent.confirm_window_ms"],
               "scope": "instance", "source": "user_file", 
               "by": { "id": "act_12", "kind": "remote" }, "reload": "live" } }
```

```json
{ "t": "event", "sub": "sub_2", "session": null, "workspace": null,
  "seq": 5513, "ts": "2026-08-03T18:22:11.400Z", "source": "plugin",
  "caused_by": null,
  "payload": { "type": "plugin_failed", "plugin": "ntfy-notifier",
               "diagnostic": { "code": "OMT-P012", "message": "…" } } }
```

```json
{ "t": "event", "sub": "sub_3", "session": null, "workspace": null,
  "seq": 5514, "ts": "2026-08-03T18:22:12.900Z", "source": "core",
  "caused_by": "dev_9a:41831",
  "payload": { "type": "audit_appended",
               "entry": { "at": "…", "actor": "act_12", "device": "dev_9a",
                          "capability": "interaction.resolve",
                          "effects": [], "outcome": "ok" } } }
```

```json
{ "t": "event", "sub": "sub_4", "session": null, "workspace": "w_9f3c",
  "seq": 41210, "ts": "2026-08-03T18:22:14.220Z", "source": "fs",
  "caused_by": null,
  "payload": { "type": "files_changed", "truncated": false,
               "changes": [{ "rel": "crates/omt-agent/src/lib.rs", "change": "modified" }] } }
```

```json
{ "t": "event", "sub": "sub_2", "session": null, "workspace": null,
  "seq": 5515, "ts": "2026-08-03T18:22:20.000Z", "source": "core",
  "caused_by": null,
  "payload": { "type": "instance_degraded",
               "degradation": { "kind": "not_persisting",
                                "since": "2026-08-03T18:22:19.980Z",
                                "detail": "store: no space left on device",
                                "remedy": "free space under $XDG_STATE_HOME/omt" } } }
```

#### 3.7.3 The `source` vocabulary — one closed set

Two documents disagreed. [06 §3](06-agent-layer.md#3-source-model) has
`Tier = Heuristic | Process | Marker | Transcript | Hook | Protocol`;
[08 §2.1](08-web-client.md#21-what-codegen-emits)'s generated `EventSourceTag`
renamed `heuristic` → `pty` and added `workspace_fs` and `system`. Both claimed
to be generated from `omt-events`, which cannot be true of both.

The diagnosis: the two lists answer different questions that were conflated. A
*tier* is a confidence ranking of agent-observation sources and exists only for
`kind: agent`. A *source tag* is the producer class of **any** event, and most
events have no tier at all — a `layout_changed` is not more or less confident
than a `files_changed`. 08's additions were the symptom: `workspace_fs` and
`system` are producers with no tier, so they had to be smuggled into a tier
enum.

**The single closed set**, which `omt-events` generates and both 06 and 08 defer
to:

```rust
#[serde(rename_all = "snake_case")]
pub enum EventSourceTag {
    // the six agent-observation tiers, spelled exactly as `Tier`'s variants
    Heuristic, Process, Marker, Transcript, Hook, Protocol,
    // producers that are not agent observations and have no tier
    Core,    // the daemon's own state machines: session tree, terminal,
             // presence, writer token, config, instance health
    Fs,      // the filesystem watcher (15 §4.6)
    Plugin,  // a plugin, via the plugin host (11)
}
```

Rules:

1. For `kind: agent`, `source` **is** the inner `AgentEvent::tier`, lower-cased.
   It is one of the first six, always, and codegen asserts the equality
   (06 §8.1).
2. For every other kind, `source` is one of `core`, `fs`, `plugin` — never a
   tier name. An `interaction` event is the exception that proves the rule: it
   carries the tier that observed the interaction, because it *is* an agent
   observation, merely on its own kind.
3. `pty` is **not** in the set. It was 08's rename of `heuristic`, and the tier
   is named for what it is (a guess) rather than for where the bytes came from.
   `Tier::Heuristic` in 06 is the owner; 08 regenerates.
4. `workspace_fs` and `system` are **not** in the set as sources. `workspace_fs`
   is an `EventKind`; `system` was standing in for what is now `core`.

### 3.8 The `omt-hook` wire messages

`omt-hook` is the tier-4 ingress: the agent's own hook system executes it, it
reads the agent's JSON document on stdin, and it reports to the daemon over the
unix socket of §2.3. This is the flagship observation path
([D11](decisions.md#d11--omt-mirrors-the-agents-own-card-it-does-not-intercept-or-replace-it):
the `PreToolUse` hook fires *before* the agent draws its card and carries
`tool_input` verbatim, which is everything needed to mirror the interaction
remotely), so its encoding is specified here rather than left to the
implementation.

#### 3.8.1 Not a capability call — a distinct `ProtoMessage` pair

**Decision: `HookEvent` / `HookAck` are `ProtoMessage` variants, and they do
not go through `CapabilityRegistry::dispatch`.**

The rejected alternative was an `agent.hook.report` capability, which is
attractive because dispatch is where authorization and auditing live
([03 §3](03-capability-catalog.md#35-the-dispatch-path)). It is wrong for three reasons:

1. **A hook is not an actor requesting a mutation.** The capability catalog is
   the surface through which an *actor* changes state, and every entry carries a
   `Role`, an `Effects` set and an `Intent` class. A hook has no role, requests
   no mutation and has no intent to deliver — it reports an observation, exactly
   as `TranscriptTail` and `AcpClient` do, and neither of those is a capability
   call either. Making one observation source RPC-shaped because it happens to
   arrive over a socket would put the tier ladder on two different footings.
2. **The authorization that matters already happened, at the right layer.** §2.3
   checks `SO_PEERCRED`/`LOCAL_PEERCRED` and rejects any peer whose uid differs
   from the daemon's before the handshake. That is precisely the right check for
   a local process the agent spawned as the daemon's user. Dispatch would add a
   role check with no meaningful answer.
3. **Latency.** Claude Code alone has 30 hook events, several per tool call, on
   a single-digit-millisecond budget (§3.8.4). Dispatch's per-call machinery —
   the recent-results cache keyed by `RequestId`, effects refinement, the audit
   append — is the wrong cost on that path, and the `RequestId` it is keyed by
   is `(DeviceId, u64)`, which a hook does not have.

**Auditing is preserved, in the right log.** [13 §6](13-security.md)'s audit log
records actor-initiated capability calls; a hook event is not one. What it *is*
is durably recorded, in full, in the agent event log
([21 §1](21-data-lifecycle.md) row 7) once normalized into an `AgentEvent`, and
`agent.explain` (06 §4) reports the `HookBridge` source's health, freshness and
last event. So "what did the hook tell us and when" is answerable; it is simply
not an audit question.

#### 3.8.2 The messages

```rust
pub struct HookEvent {
    /// Per-hook-process nonce, not a `RequestId` — a hook has no `DeviceId`
    /// and lives for milliseconds. Echoed on the ack; the hook makes exactly
    /// one call in its life, so uniqueness within the connection suffices.
    pub nonce: u64,
    /// Version of *this* message pair, negotiated separately from `proto`
    /// because the hook binary is installed into the agent's config and may
    /// be older or newer than the daemon (22 reports the skew).
    pub hook_proto: u16,

    // ---- who ----
    pub agent: AgentKind,
    pub agent_version: Option<String>,
    /// The agent's own session id, when the payload or environ carries one.
    pub agent_session: Option<AgentSessionId>,

    // ---- what ----
    /// The agent's own hook event name, **verbatim and un-normalized**:
    /// `"PreToolUse"`, `"beforeShellExecution"`, `"AfterTool"`. The hook does
    /// not map it; the daemon's per-agent normalizer does, and keeping the raw
    /// name means an unrecognized event is loggable rather than lost.
    pub event: String,
    pub tool_name: Option<String>,
    /// The agent's own id for this tool invocation, when it supplies one.
    /// This is what correlates a `PreToolUse` with its `PostToolUse`, and
    /// therefore what makes D15's confirm-by-observation possible.
    pub tool_use_id: Option<String>,
    /// **Verbatim.** The exact `tool_input` document, unmodified and
    /// unredacted on the wire (the socket is local, same-uid, 0600).
    /// Redaction happens on the daemon side before any write
    /// ([21 §2.1](21-data-lifecycle.md#21-the-placement-rule)).
    pub tool_input: Option<serde_json::Value>,
    pub tool_response: Option<serde_json::Value>,
    /// The whole stdin document, verbatim, for fields this schema does not
    /// name. Bounded (§3.8.3); a hook never truncates silently.
    pub raw: serde_json::Value,

    // ---- where ----
    pub correlation: HookCorrelation,

    /// How long the hook is willing to wait for the ack before it fails open
    /// (§3.8.4). Advisory: it tells the daemon whether a slow path is worth
    /// starting, and the hook enforces it regardless of the answer.
    pub deadline_ms: u32,
}

/// 06 §7.2. `OMT_SESSION` and `OMT_INSTANCE` are injected into every PTY omt
/// spawns, so a hook already knows which pane it belongs to and no
/// "match the transcript to the pane" heuristic runs.
pub struct HookCorrelation {
    pub instance: Option<InstanceId>,   // $OMT_INSTANCE
    pub session: Option<SessionId>,     // $OMT_SESSION
    pub pid: u32,                       // the agent process, = the hook's ppid
    pub ppid: u32,
    pub cwd: PathBuf,
}

pub struct HookAck {
    pub nonce: u64,
    /// The daemon recorded the event. `false` means it could not (unknown
    /// agent, malformed payload) and is informational only — the hook's
    /// behaviour is identical either way.
    pub recorded: bool,
    pub directive: HookDirective,
}

#[serde(tag = "type", rename_all = "snake_case")]
pub enum HookDirective {
    /// The only value sent in v1. D11: omt observes and gets out of the way.
    Proceed,
    /// **Reserved, never sent in v1.** The opt-in deferral path of
    /// [06 §5.3](06-agent-layer.md#53-the-deferral-mechanism--demoted-to-an-optional-optimization),
    /// which takes the local user's native card away and is therefore
    /// per-agent opt-in if it ever ships.
    Defer { budget_ms: u32 },
    /// **Reserved, never sent in v1.** Would be omt denying a tool call, which
    /// [D1](decisions.md#d1--omt-adds-no-policy-layer-over-an-agents-permission-semantics)
    /// forbids: omt adds no policy layer over the agent's own permission
    /// semantics. Present so the wire does not have to change if a *user*
    /// ever configures a deny that the agent itself would have offered.
    Deny { reason: String },
}
```

**Correlation when omt did not spawn the agent.** `OMT_SESSION` is absent, so
`correlation.session` is `None` and the daemon correlates by `agent_session`
against its bindings, then by `(cwd, pid)` proximity — the last-resort path 06
§7.2 already marks low-confidence in `agent.explain`. The hook never guesses a
session id.

#### 3.8.3 Fail-open, in wire terms

The rule is unconditional and predates this section
([06 §5.3](06-agent-layer.md#53-the-deferral-mechanism--demoted-to-an-optional-optimization)
point 3): **an agent must never hang because omt is slow or dead.**

| Situation | Hook behaviour |
|---|---|
| `OMT_SOCK` unset, or the socket does not exist | exit 0 immediately, empty document on stdout. No connect attempt. This is the guard clause at the top of the hook (06 §7.1) |
| connect refused / socket stale | same, without retry |
| `deadline_ms` elapses with no `HookAck` | abandon the read, empty document, exit 0. The partially-sent `HookEvent` is the daemon's problem, not the agent's |
| `HookAck` malformed, or `directive` unknown to this hook version | treat as `Proceed` |
| `HookAck { directive: Proceed }` | empty document, exit 0 — the normal path |
| any panic in the hook binary | a `catch_unwind` at `main` emits the empty document and exits 0 |

The hook's exit status is **always 0** except where the agent itself defines a
non-zero status as meaningful and omt is not using that meaning — which today is
nowhere.

**Who renders the agent-appropriate stdout: the hook binary.** [06 §5.3](06-agent-layer.md#53-the-deferral-mechanism--demoted-to-an-optional-optimization)
says the default on any error is to return `{}`, which is Claude Code's empty
document and not universal. The mapping from `HookDirective` to the bytes a
given agent expects on stdout lives in **`omt-hook`**, keyed by the `AgentKind`
it was installed for (`omt-hook --agent claude-code`, with `OMT_HOOK_AGENT` as a
fallback):

```
Proceed  →  claude-code : {}
            codex       : {}
            cursor      : {"permission": "allow"}      ← per-agent, in the hook
            gemini/qwen : {}
```

> **Choice made here.** The alternative — the daemon returns the literal stdout
> bytes and the hook echoes them — is rejected for one decisive reason: **the
> fail-open path is exactly the path on which the daemon is unreachable.** A
> hook that cannot render its own agent's empty document has nothing to print
> when the socket is gone, which is the case the whole contract exists for. A
> secondary reason: the table is per-agent knowledge, and per-agent knowledge
> lives next to the agent (`AgentAdapter`, 06 §7) or in the binary the adapter
> installs — not in the transport. The cost is that adding an agent means
> shipping a new `omt-hook`; `omt-integration-version` (06 §7.1) already stamps
> installs so a stale one is detectable.
>
> The exact per-agent documents are the adapter's to verify — 06 §10 question 2
> ("do Codex/Cursor/Gemini hook payloads match the Claude-Code shape?") covers
> the input side of the same question, and the output side rides with it.

**Bounds.** A `HookEvent` is a control message and is subject to §2.2's 1 MiB
control-frame limit. A `tool_input` that would exceed it (a large `Write`) has
its `raw` field replaced by `{"omt_truncated": true, "bytes": N}` with
`tool_input` retained if it alone fits, and the daemon marks the resulting
`AgentEvent` as truncated. The hook never sends a frame the daemon will reject,
and never silently drops the field without saying so.

#### 3.8.4 Timing — which number applies to what

Three numbers exist in three documents and they measure three different things.
Stated plainly, because as previously written `omt doctor` would flag a
correctly-behaving hook:

| Operation | Budget | Owned by |
|---|---|---|
| `omt-hook` process start (exec → first byte written to the socket) | **single-digit milliseconds** | [02 — crate map](02-crate-map.md#omt-hook). It is a separate binary precisely to hit this |
| socket connect + `HookEvent` + `HookAck`, on a healthy daemon | **target ≤ 10 ms, p99 ≤ 20 ms** | this section |
| `omt doctor`'s `agents` check: full spawn → connect → ack → exit | **warns above 50 ms** | [22 §3.1](22-operations.md#31-the-checks) |
| `omt-hook`'s wait for an ack before it gives up and fails open | **`deadline_ms`, default 250 ms** | [06 §5.3](06-agent-layer.md#53-the-deferral-mechanism--demoted-to-an-optional-optimization) |

The reconciliation: doctor measures the **first three lines summed**, which for
a healthy hook is roughly 5 + 10 ms, comfortably inside its 50 ms threshold. It
does **not** measure the fourth. The 250 ms figure is an *abort deadline on the
pathological path* — the time after which a hook stops waiting for a daemon that
is not answering — and it is never an expected latency. A hook that actually
reaches its deadline is by definition already degraded, and doctor reporting
that as a failure is correct behaviour, not a false positive.

Two consequences worth stating:

- **Doctor must measure a real round trip, not a synthetic one.** Its check
  spawns the installed `omt-hook` binary against the live socket with a
  synthetic `SessionStart` event, so it measures the path the agent uses,
  including the binary's startup. Measuring only the socket round trip would
  miss the failure 02's budget exists to prevent (a hook that got slow to
  start).
- **`deadline_ms` is never raised to accommodate a slow daemon.** If the daemon
  cannot ack inside 250 ms it is unhealthy, and the honest outcome is a
  degraded tier-4 source reported by `agent.explain` — not an agent that
  stutters on every tool call.

---

## 4. Terminal streaming

### 4.1 The three options

| | (a) Raw byte passthrough | (b) Server-side grid diffs | (c) Hybrid |
|---|---|---|---|
| Server cost | none (tee the PTY) | full render + diff per client | render only when needed |
| Bandwidth | high on redraws, low on typing | low and bounded | bounded |
| Fidelity | perfect (xterm.js is a real emulator) | limited to what the diff format encodes | perfect |
| Resume | requires replaying a byte window | trivial (send a grid) | trivial |
| Multi-size clients | broken (one PTY size for all) | solved (render per client) | solved |
| Client complexity | low | medium (custom renderer) | medium |

### 4.2 The decision: (c) hybrid, byte-stream-primary

**Steady state is raw bytes.** After attach, the server tees PTY output to each
attached client as `kind=1` binary frames, coalesced (§6.2). xterm.js is a
correct emulator; re-implementing its job server-side and shipping a lossy diff
format would be a fidelity regression for no benefit on the common path (a phone
watching an agent scroll text).

**Snapshots are grid state.** On attach, on resume outside the replay window,
and after any resync, the server sends a *snapshot*: the authoritative grid from
`omt-term`, serialized, followed by byte frames from the snapshot's sequence
onward. The snapshot is exactly what another tool's `RenderEncoding::TerminalAnsi`
almost is, but explicit and typed rather than pre-diffed ANSI:

```json
{ "t": "term_snapshot_meta", "session": "s_4b2f", "stream": 12,
  "seq": 91422, "cols": 120, "rows": 40,
  "encoding": "grid_v1", "bytes": 24816,
  "cursor": { "row": 12, "col": 4, "visible": true, "shape": "block" },
  "modes": { "alt_screen": true, "bracketed_paste": true, "app_cursor": false },
  "scrollback_available": 12000 }
```

followed by one `kind=5` binary frame. `grid_v1` is a compact run-length
encoding of cells (codepoint, style id, width) plus a style table — the same
structure `omt-term` already maintains for damage tracking, so producing it is
cheap and it is exactly reconstructible into an xterm.js buffer via
`writeln`-free direct buffer population.

So: **snapshot to establish state, bytes to keep it.** Grid diffs are used in
exactly one place — the *block view* on mobile, which renders structured command
blocks (OSC 133) rather than a terminal, and therefore consumes semantic
`terminal.block.*` events, not bytes at all. That is where mobile bandwidth is
actually won: a phone in block view receives no PTY bytes.

### 4.3 The resize problem

Two clients attached to one session with different viewports is the genuinely
hard case, because a PTY has exactly one `TIOCSWINSZ` and the program inside
(especially a full-screen TUI agent) renders for that one size.

Rejected: **per-client server-side render.** It requires a full emulator
instance per client per session and reflow that no full-screen TUI cooperates
with — an agent that drew a box at 120 columns cannot be re-rendered at 40; the
information is gone. It only works for reflowable line-oriented output, which is
precisely the case where it is least needed.

**Decision: one authoritative PTY size per session, owned by the writer, with
fit-to-view for everyone else, plus an explicit resize handoff.**

```rust
pub struct ViewportPolicy {
    /// The size actually applied to the PTY. Set by the writer's viewport,
    /// or pinned by the user.
    pub authoritative: TermSize,
    pub owner: SizeOwner,     // Writer | Pinned { by: ActorId, size: TermSize } | Smallest
}

pub enum SizeOwner {
    /// Default. The current writer's viewport drives the PTY.
    Writer,
    /// A client explicitly pinned a size ("keep this session at 120x40").
    Pinned { by: ActorId, size: TermSize },
    /// Opt-in: PTY is set to the smallest attached viewport, so nobody is cropped.
    Smallest,
}
```

Non-authoritative clients receive the full grid and render it **scaled to fit
width, letterboxed vertically**, with a visible badge: `120×40 · driven by
laptop`. A phone therefore sees the real screen, small but complete and
correct — never a cropped window into a layout it cannot understand, and never
a reflow that mangles a TUI.

Consequences, stated plainly because they are user-visible:

- Taking the writer token *may resize the PTY*, which for a full-screen agent
  causes a redraw. The client warns before the first takeover of a session whose
  size would change by more than 20 %, and offers "take input without resizing"
  (which acquires the writer token with `keep_size: true`, leaving
  `authoritative` pinned).
- `Smallest` exists for genuine pair-programming, where a resize storm is worse
  than a small terminal.
- The phone can always *read* at full fidelity; the letterbox affects legibility,
  not correctness. Combined with the block view (§4.2) as the default mobile
  surface, the small-text case is the exception rather than the norm.

```json
{ "t": "term_attach", "id": "req_9", "session": "s_4b2f",
  "viewport": { "cols": 52, "rows": 24, "dpr": 3 },
  "want": "grid_then_bytes",
  "since_seq": 91380 }
```

```json
{ "t": "term_resize", "id": "req_12", "session": "s_4b2f",
  "viewport": { "cols": 60, "rows": 30 },
  "request_authoritative": false }
```

A client always reports its viewport (used for presence, for `Smallest`, and to
decide letterboxing); `request_authoritative` is the explicit ask, and is
refused with `precondition_failed` if the client does not hold the writer token.

### 4.4 Native sessions

Everything above §4.4 describes a `pty` session. A `native` session
([D8](decisions.md#d8--two-session-modes-pty-default-and-native-acp)) has
no PTY and no terminal at all, and the protocol says so rather than degrading
quietly.

- **The mode is on the wire, before attach.** A session's `SessionMode`
  ([05 §1.5](05-session-model.md#1-the-object-model)) is carried in
  `session.get`, in `session.list` (and therefore in every row of the unified
  session list, §1.6), and in the `TermAttach` reply. It is **not** in
  `Welcome`: `Welcome` is per-*connection* and carries no session data at all
  (§3.3), so a document claiming otherwise gives clients a field that does not
  exist. A client therefore knows whether a grid exists
  *before* it decides how to render the session, and never has to infer it from
  the absence of bytes.
- **No terminal surface exists.** For a `native` session there are no terminal
  byte frames (`kind=1`/`kind=2`), no grid snapshot (`kind=5`,
  `TermSnapshotMeta`), and no `ViewportPolicy` — there is no PTY to size.
  `TermAttach` and `TermResize` return `unsupported`. Clients still report a
  viewport, for presence only ([12 §2](12-collaboration.md#2-presence-is-first-class-state)).
- **§6's lossy/lossless split degenerates.** A native session's stream is
  entirely lossless, so `LagPolicy::Strict` (never drop; close the connection on
  overflow) is the **only legal policy** for it. This is not a preference: §6.2's
  fallback is "collapse to state", and a grid *is* a lossless summary of
  arbitrary byte history, whereas a list of tool calls and messages is not.
  There is nothing structured agent events can be collapsed into, so they must
  not be dropped.
- **§7's latency budget does not apply.** It measures a keystroke round trip,
  and a native session has no keystrokes — the unit is a submitted prompt or a
  permission answer, whose budget is dominated by the agent. Local echo is
  disabled for the same reason.
- **JSON only.** The binary frame `kind` set (§3.6) stays closed; native
  sessions introduce no new binary kinds and are carried entirely as typed
  events on the control channel. Blobs (images in a prompt) use the existing
  `kind=3` path unchanged.

---

## 5. Resume and reliability

### 5.1 Sequence spaces

- Every event carries `(session_id, seq)`, `seq` strictly monotonic per session,
  assigned by the state layer at mutation time (never by the transport).
- Terminal byte frames carry their own per-stream `seq` in the binary header,
  in the **same sequence space** as that session's events. This is the key
  design point: a client that resumes at `seq = N` gets both events and terminal
  bytes from `N` onward, correctly interleaved, so a `Interaction` event and the
  PTY bytes that drew its box cannot cross.
- Instance-scoped events (config, plugin, peers) use the reserved session id
  `s_instance` with its own sequence.
- **Workspace-scoped events** (`workspace_fs` and anything else keyed to a
  workspace rather than a session, §3.7) carry `workspace` instead of `session`
  in the envelope and have their own per-workspace `Seq` space. So there are
  exactly three sequence spaces — per session, per workspace, and the single
  `s_instance` space — and `since_seq` is keyed uniformly by whichever id the
  event carries.

### 5.2 Replay window

The daemon keeps, per session, a ring buffer of the last `min(4 MiB, 4096
events)` (both configurable, both reported in `Welcome.limits`). On
`Subscribe{since_seq}` or `TermAttach{since_seq}`:

**`since_seq` shape, stated once because the two messages differ deliberately.**
In `Subscribe` it is a **map** keyed by session id, workspace id or
`s_instance`, because one subscription covers many of them (§3.7, §5.1). In
`TermAttach` it is a **scalar**, because a terminal attach covers exactly one
session. Neither form is a shorthand for the other, and a client that sends the
wrong shape gets `Error{code:"precondition_failed"}` rather than a silently
ignored field.

| Scope | Window kept | On `since_seq` older than the window |
|---|---|---|
| session (events + terminal bytes) | `min(4 MiB, 4096 events)` | `Resync` + grid snapshot, then live |
| workspace (`workspace_fs` and other workspace-scoped events) | last 1024 events, no byte cap — these events are small and there is no snapshot to send | `Resync` with `snapshot_follows: false`; the client re-reads current state via `workspace.files.*` |
| `s_instance` (config, plugin, peers) | last 1024 events | `Resync` with `snapshot_follows: false`; the client refetches the catalog and config |

| Condition | Response |
|---|---|
| `since_seq` inside the window | replay from `since_seq + 1`; nothing else |
| `since_seq` older than the window | `Resync` + snapshot, then live |
| `since_seq` newer than current (client from the future — e.g. daemon restarted and re-seeded) | `Resync` with `reason: "sequence_reset"` |
| session unknown | `Error{not_found}` |

```json
{ "t": "resync", "sub": "sub_1", "session": "s_4b2f",
  "reason": "window_exceeded",
  "from_seq": 91380, "now_seq": 104882,
  "dropped_events": 8213,
  "snapshot_follows": true }
```

The client **must** treat `Resync` as "discard local state for this session and
rebuild". It is a normal, expected message, not an error — a phone that slept
for an hour will always get one.

**Rebuilding is not just the snapshot.** Discarding and re-reading live state
recovers what *is*, not what *happened* — and under
[D12](decisions.md#d12--no-push-notifications-in-v1-open-and-replay-instead)
open-and-replay is the only discovery path there is. An interaction that opened
**and reached a terminal state** entirely inside the gap is in no snapshot and
in no replayed event, so a client that stops at the snapshot presents an idle
session and the user never learns their agent asked and gave up. Therefore,
after any `Resync`, before it ranks its home screen, the client **must** refetch:

1. `interaction.list { since_read_mark: true, include_terminal: true }` — the
   durable attention log ([20 §12.5](20-recall-and-usage.md#125-attention-and-the-durable-attention-log)),
   which is the only source for interactions that came and went while
   disconnected;
2. the current attention state for every session (`attention.get` / the
   session-list attention fields, §1.6).

Only then does [`remote-continuity §2.3`](../design/remote-continuity.md#23-the-continuity-ranking)'s
ranking run. Ranking on live state alone silently drops exactly the events the
user most needed to see.

**Canonical name, binding.** The message is `Resync`, tagged `"t": "resync"` on
the wire (§3.2). It is *not* `resync_required`; any document or catalog entry
using that spelling is wrong and should be corrected to match this one.

### 5.3 Daemon restart

Sessions survive daemon restart via `omt-store` (append-only log + snapshots).
Sequence numbers survive too: the store records the high-water `seq` per session
and the daemon resumes from `high_water + 1`, never restarting at zero. That is
what makes a client's `since_seq` meaningful across a restart.

What does *not* survive: the replay window (in memory), open subscriptions, and
writer tokens (all released on restart — see
[12 §3](12-collaboration.md)). So the first reconnect after a daemon restart is
always a resync. The client is told explicitly:

```json
{ "t": "welcome", "instance": { "started_at": "2026-08-03T18:40:11Z", "…": "…" },
  "restart": { "since_last_seen": true, "reason": "process_restart" } }
```

**PTYs themselves do not survive a daemon restart in v1.** Per
[05 §8](05-session-model.md#8-persistence-and-restore), restored sessions come
back as `SessionState::Orphaned`: content is readable, searchable and copyable,
writes return `precondition_failed`, and the UI offers **"restart"** — re-spawn
the same argv in the same cwd with the same injected env, keeping the old
scrollback above a separator. For an agent session the agent-native resume path
(`claude --resume <uuid>`, `codex resume`) is offered as the one-tap action.
Re-parenting PTYs to a supervisor process so they survive an upgrade is
[05 §13.1](05-session-model.md#13-open-questions), deferred.

A **`native` session** (§4.4) has no PTY and therefore never enters
`SessionState::Orphaned`: its restart path is the agent's own ACP session
resume, re-establishing the JSON-RPC connection and resuming the agent-side
session id, with the typed event history replayed from `omt-store`.

### 5.4 Mobile specifics

| Situation | Behaviour |
|---|---|
| **Tab backgrounded** | client sends `Subscribe`-level `policy.background: true`, which downgrades terminal subscriptions to *suspended* (server stops sending bytes, keeps advancing `seq`) while leaving `agent`/`interaction`/`presence` events flowing. Resume on visibility sends `since_seq` and typically gets a snapshot. |
| **iOS suspends the socket** | detected on `visibilitychange` → immediate reconnect with the resume `session_token`; no credential round-trip, so warm reconnect is one RTT. |
| **Network switch (Wi-Fi → cellular)** | connection error → immediate reconnect (attempt counter reset, §2.5), new TCP/TLS handshake, resume by `session_token` + `since_seq`. |
| **Long sleep (hours)** | `session_token` may have expired → full auth; `since_seq` far outside the window → resync. Both are one round trip each and the UI shows "catching up" rather than an error. |
| **Metered connection** | client sets `policy.terminal: "blocks_only"`, receiving structured block events and no PTY bytes until the user opens the full terminal view. |

---

## 6. Backpressure

Backpressure is per **subscription**, not per connection, because a phone that
cannot keep up with a firehose session must not lose its `interaction` events —
those are the product.

### 6.1 Policy

```rust
pub struct SubscriptionPolicy {
    /// What to do when this subscription's buffer is full.
    pub on_lag: LagPolicy,
    pub max_buffered_events: usize,   // default 2048
    pub max_buffered_bytes: usize,    // default 1 MiB of terminal payload
}

pub enum LagPolicy {
    /// Default. Drop the oldest terminal frames first, then coalesce, then —
    /// only if still behind — drop everything for the session and send `Resync`.
    Resync,
    /// Never drop; apply flow control upstream. Only legal for local transports,
    /// because a slow remote peer would otherwise stall the PTY reader.
    Block,
    /// Drop terminal frames, never drop non-terminal events; if non-terminal
    /// buffer overflows, close the connection with `overloaded`.
    Strict,
}
```

**Event classes are not equal.** The server maintains two queues per
subscription: *lossy* (terminal bytes) and *lossless* (everything else).
Terminal bytes are dropped and coalesced freely. Lossless events are never
silently dropped — if the lossless queue overflows, the session is resynced
(which is itself an announced state), and if resync cannot be delivered the
connection is closed with `overloaded`. There is no code path where a client
misses an `Interaction` and does not know it.

### 6.2 Coalescing terminal frames

The terminal fan-out task per subscription:

1. accumulates PTY output in a per-subscription buffer with a **flush timer of
   16 ms** (one display frame),
2. flushes early if the buffer exceeds 32 KiB,
3. if the buffer exceeds `max_buffered_bytes` before the socket drains, feeds
   the accumulated bytes through `omt-term` for that session's grid state and
   replaces the whole buffer with a **snapshot** — which is strictly smaller
   than a screenful of redraws and is exactly correct.

Step 3 is the elegant part: the fallback under load is not "drop and hope" but
"collapse to state", which is the one thing a terminal stream can always be
collapsed into. A phone on a bad link therefore sees fewer intermediate frames
but never a corrupted screen.

Typing is exempt from coalescing: input frames (`kind=2`) are sent immediately,
unbuffered. Latency of the local echo path dominates perceived quality.

### 6.3 How the client learns

Every drop is announced. A client's terminal view can be in exactly three
states, all visible:

- `live` — receiving bytes continuously,
- `coalesced` — receiving snapshots under load (badge: "catching up"),
- `resynced` — state was rebuilt from a snapshot, scrollback before the snapshot
  came from `session.scrollback.get`, not from the stream.

```json
{ "t": "lagged", "sub": "sub_1", "session": "s_4b2f",
  "class": "terminal", "dropped_bytes": 1841200,
  "action": "snapshot_sent", "seq": 104901 }
```

---

## 7. Latency budget

Target: **a keystroke from a phone on a tailnet appears on the remote screen and
comes back within 120 ms p50, 250 ms p95.** Budget for a phone on LTE →
Tailscale DERP-less direct path → laptop:

| Segment | Budget (p50) | Notes |
|---|---|---|
| touch → JS keydown | 16 ms | one frame |
| JS → WebSocket frame on the wire | 2 ms | no batching for input |
| network RTT/2 (WireGuard direct) | 25 ms | 60 ms if relayed via DERP |
| server: decode, authorize, write to PTY | 1 ms | authorization is a role compare, precomputed at auth time |
| PTY → program → PTY echo | 5–40 ms | the agent's own cost, not ours |
| server: read, coalesce (first-byte flush) | 0–16 ms | first byte after idle flushes immediately |
| network RTT/2 | 25 ms | |
| xterm.js parse + paint | 8 ms | |

Optimizations that matter, in order of impact:

1. **Flush immediately when the coalescing buffer was empty.** The 16 ms timer
   applies to the *second* byte onward. This alone is most of the perceived
   difference.
2. **No permessage-deflate on the terminal path** (§2.2) — deflate adds a
   context-flush per message and CPU on both ends for output that is already
   sparse when typing.
3. **Local echo for plain printable keys when the session is not in alt-screen
   and bracketed-paste is off.** xterm.js renders the character optimistically
   and reconciles against the server byte stream; a mismatch within 250 ms
   reverts. This is the single largest perceived win on a relayed link, and it
   is safe precisely because it is bounded to the case where the remote program
   is known to be a line-oriented shell. See
   [12 §7](12-collaboration.md#7-optimistic-ui) for the optimistic-UI rules.
4. **Prefer direct WireGuard over DERP**: the client surfaces relay status,
   because a relayed tailnet path roughly doubles the budget and users can
   usually fix it.
5. **Coalesce input on paste only**: a paste becomes one binary frame with
   bracketed-paste markers, not 4000 keystroke frames.
6. **Never block the PTY reader on a slow client** (§6.1 — `Block` is
   local-only).

---

## 8. Notifications to a closed tab — none in v1

**omt ships no notification backend.** When a client is not connected, it is not
notified. This section exists to record the decision, to keep the extension
point specified, and to state the property that falls out of it.

### 8.1 The decision

[D12](decisions.md#d12--no-push-notifications-in-v1-open-and-replay-instead)
removes push from v1 outright — not "off by default", not "opt-in": **zero
backends ship**. A browser with its tab closed cannot be reached except through
the browser vendor's relay (FCM for Chrome/Android, `web.push.apple.com` for
Safari/iOS). That means the daemon opening an outbound connection to a third
party, and it leaks the metadata *"this machine needs its owner, now"* even
though the Web Push payload is encrypted end to end. For a tool whose stated
position is no cloud, no telemetry and no required egress
([00 §8](00-overview.md#8-what-omt-is-not)), that is a contradiction as a
default and a maintenance burden as an option. The self-hosted alternative users
would actually be pointed at — `ntfy` on a tailnet — adds an app and a
deployment step to onboarding.

**The property this buys: omt makes no outbound network connections at all.**
Plain, checkable, and no longer caveated. The daemon accepts connections; it
never initiates one.

**The capability given up, stated plainly:** the "agent blocks, the phone buzzes
in your pocket" journey is not in v1. Users learn that an agent needs them when
they next open a client. This must be said in the README and in onboarding, not
discovered.

### 8.2 What replaces it: open-and-replay

Discovery is entirely **open → reconnect → replay → rank**. §5.2 specifies the
mechanics and makes the two mandatory refetches after a `Resync` — the durable
attention log and current attention state — a protocol requirement rather than a
client nicety, because without them an interaction that opened and went terminal
inside an offline gap is invisible forever. The user experience is designed in
[`../design/remote-continuity.md` §5](../design/remote-continuity.md#5-open-and-replay--from-cold-start-to-the-right-screen).

Because this is now the *only* discovery path, its quality is not a convenience:
cold-start latency, the continuity ranking, and complete recovery of what was
missed are primary features.

### 8.3 The reserved extension point

Nothing in the design may assume notifications never exist
([P2](01-principles.md#p2--pluggable-extension-without-modification)). The
`Notifier` trait and its call sites are **specified and reserved**, with zero
implementations in tree:

```rust
#[async_trait]
pub trait Notifier: Send + Sync {
    fn id(&self) -> &str;
    /// An addressable pointer plus a short, non-secret title and body derived
    /// from the session and the interaction kind. Never the question text,
    /// the agent's output, tool arguments, or scrollback.
    async fn notify(&self, n: &NotificationPointer) -> Result<(), NotifyError>;
}

pub struct NotificationPointer {
    pub instance: InstanceId,
    pub session: SessionId,
    pub kind: NotificationKind,          // interaction | turn_ended | error | session_died
    pub interaction: Option<InteractionId>,
    pub title: String,                   // "claude · api-gateway"
    pub body: String,                    // "Needs a decision"
}
```

Call sites fire on the same triggers a backend would have wanted (interaction
opened, agent turn ended after > N seconds, agent error, session died, writer
takeover requested), and with no backend registered they are no-ops. Two future
consumers are anticipated and neither is core's to build:

- a **native iOS/Android app**, which has a first-party push channel that does
  not route through a browser vendor;
- a **user or third-party plugin** ([11 — Plugins](11-plugins.md)) — ntfy,
  Telegram, Bark, a webhook — shipped without touching core. A plugin that makes
  an outbound connection is the *user's* choice and is disclosed as such; it does
  not change what omt itself does.

`notification.push.subscribe` is **not** a v1 capability and does not appear in
the catalog. The coalescing rule a future backend will need (at most one per
session per 30 s, replaced in place by `tag: "<instance>:<session>"`) is recorded
here so it is not rediscovered.

---

## 9. OPEN QUESTIONS

1. **Grid snapshot format.** `grid_v1` needs a concrete encoding and a
   round-trip fuzz test against `omt-term`. Open: whether to encode styles as a
   per-snapshot table (smaller) or as an interned global table shared across
   snapshots on a connection (smaller still, but stateful and therefore harder
   to resume). Owner: `omt-term` + `omt-proto`.
2. **Does `since_seq` for terminal bytes hold up in practice?** Putting PTY
   bytes and events in one sequence space is elegant but means the sequence
   allocator is on the PTY hot path. Needs a benchmark at 10 MB/s of output
   before it is committed to.
3. **Writer-token resize warning threshold** (20 %) is a guess. Needs user
   testing; may need to be per-agent (a full-screen TUI cares, a shell does not).
4. **`Smallest` size policy vs. agents that refuse to render below 80 columns.**
   Several agent TUIs degrade badly under ~80 cols. Do we clamp the authoritative
   size to a per-agent minimum reported by the adapter? Coordinate with
   [06 — Agent layer](06-agent-layer.md).
5. ~~**iOS PWA push reliability.**~~ **Retired.** [D12](decisions.md#d12--no-push-notifications-in-v1-open-and-replay-instead)
   ships no notification backend, so the experiment has nothing to measure and
   the "should `ntfy` be the default?" question has no v1 subject.
   [13 §11 Q7](13-security.md#11-open-questions) deferred to this entry and
   should be closed with it. What replaces it as a measurable risk is
   **cold-start-to-useful-screen latency** for open-and-replay (§8.2), which is
   owned by [`../design/remote-continuity.md` §5](../design/remote-continuity.md#5-open-and-replay--from-cold-start-to-the-right-screen).
6. **Blob chunk size** (currently unspecified; likely 256 KiB) interacts with
   backpressure — a large image upload from a phone must not starve the
   interaction path. Probably needs its own queue class alongside lossy/lossless.
7. **SSH bridge and binary frames**: the stdio path carries the same framing,
   but ssh's own windowing may interact badly with 8 MiB frames. May need a
   smaller `max_binary_frame` negotiated per transport kind.
8. **Federated search/actions across instances** (e.g. "find the session that
   touched `auth.rs`") is currently client-side fan-out. Whether that stays
   acceptable at ~10 instances is unmeasured.
