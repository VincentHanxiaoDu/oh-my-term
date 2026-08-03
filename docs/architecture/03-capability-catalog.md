# Capability Catalog and Surface Parity

This is the mechanism behind principle **P3**: *anything the native TUI can do,
the API exposes, and the web client can do*. It is the most load-bearing piece
of the architecture, because every other subsystem is reached through it.

---

## 1. The idea

A **capability** is a single named operation on an omt instance: a *command*
(mutates) or a *query* (reads). Capabilities are declared once, in one place,
with their types. From that single declaration omt derives:

1. the in-process dispatch used by the TUI,
2. the HTTP/WebSocket routes used by remote clients,
3. the JSON Schema for inputs and outputs,
4. the TypeScript client the web app imports,
5. the `omt <group> <verb>` CLI subcommand tree,
6. the reference documentation,
7. the parity test matrix.

Nothing is hand-written twice, so nothing can drift.

```
                      ┌───────────────────────┐
                      │  capability catalog   │   declared once
                      └───────────┬───────────┘
        ┌─────────────┬───────────┼───────────┬─────────────┐
        ▼             ▼           ▼           ▼             ▼
   in-process     HTTP/WS     JSON Schema   TS client    CLI tree
   dispatch        routes                                 + docs
        │             │                         │
     omt-tui      omt-server                 web client
```

## 2. Declaring a capability

Capabilities are declared with a macro that produces both a type and a registry
entry. The macro exists so the declaration is data — it can be enumerated at
build time for codegen and at run time for the parity test.

```rust
capability! {
    /// Send text to a session's PTY as if typed by the user.
    name  = "session.send_text",
    group = "session",
    verb  = "send-text",
    kind  = Command,
    role  = Role::Operator,
    input  = SendText { session: SessionId, text: String, submit: bool },
    output = SendTextAck { seq: Seq },
    /// Human-facing name. Shown in the command palette and the CLI help.
    title = "Send text",
    aliases = ["type"],
    // hidden = false is the default; `hidden = true` requires `hidden_reason`.
    /// Effects declared for auditing and for the mobile UI's confirmation rules.
    /// This is the *maximum* set; see "conditional effects" below.
    effects = [Effects::WRITES_PTY],
    /// D15: which delivery class this mutation belongs to, and therefore
    /// whether dispatch may retry it.
    intent = Intent::RawStream,
}
```

Fields:

| Field | Purpose |
|---|---|
| `name` | stable dotted id; the wire name; never renamed without a deprecation shim |
| `group`/`verb` | derive the CLI (`omt session send-text`) and the REST path (`POST /v1/session/send-text`) |
| `kind` | `Command` or `Query`; queries are cacheable and safe to retry |
| `role` | minimum role: `Viewer` < `Operator` < `Admin` |
| `input`/`output` | `serde` + `schemars` types; the schema source of truth |
| `effects` | declared side-effect bits, the **maximum** over all inputs; the closed set is `WRITES_PTY`, `SPAWNS_PROCESS`, `READS_FS`, `WRITES_FS`, `NETWORK`, `DESTRUCTIVE` |
| `refine_effects` | optional `fn(&Input) -> EffectBits`, narrowing `effects` for one call. See below |
| `intent` | D15 delivery class; determines whether a retry is safe. Required for `Command`, forbidden for `Query` |
| `parity` | `Parity::Full` (the default, omitted in practice) or `Parity::Exempt { surfaces, reason }`; see §5 |
| `title` | short human-facing name; **imperative, ≤ 40 chars** ("Send text", not "Session Send-Text"). Required unless `hidden` |
| `aliases` | extra strings the command palette and CLI match on |
| `hidden` | omit from the palette and from `--help`; still callable and still parity-tested. Requires `hidden_reason` |
| `hidden_reason` | why it is hidden; enumerated in the generated docs so hiding is never silent |
| `since` | version introduced; drives compatibility docs |

**`title` is required for every non-`hidden` capability, not derived.** A derived
title (title-casing `group` + `verb`) yields "Session Send-Text" and "Layout
Apply Saved", which is what a command palette must never show — the palette is
[19 — Onboarding](19-onboarding.md)'s primary discovery surface and its whole
value is that entries read like things a person wants to do. Deriving it also
makes the palette silently worse every time a `verb` is chosen for CLI ergonomics
rather than for prose. The parity test (§5) enforces the requirement, so the cost
of writing one is paid once, at declaration, by the person with the most context.
`aliases` exist because users search for the word they know ("type", "paste",
"maximize") rather than the word omt chose, and they are matched but not
displayed. `hidden` removes a capability from the palette and from `--help`
without removing it from the catalog, the API, or the parity test — it is a
presentation flag, never an access control.

`effects` describes what **omt** does when the capability runs. It matters for
surfaces — the mobile client uses `DESTRUCTIVE` to require a confirm gesture —
and for the audit log, which records effects per call. It is deliberately *not*
a permission input: authorization is the `role` compare plus credential scope,
and per [D1](decisions.md#d1--omt-adds-no-policy-layer-over-an-agents-permission-semantics)
`effects` never describes an agent's tool call, only omt's own operation. The
`Viewer`+`WRITES_FS`/`DESTRUCTIVE` consistency check is
[13 §4](13-security.md#4-roles-and-their-mapping-onto-the-catalog).

### 2.1 Conditional effects

Several capabilities' effects genuinely depend on their input.
`pane.split { session: None }` spawns a process and `pane.split { session: Some }`
does not; `layout.apply_saved` spawns only for the panes it must create;
`pane.close { close_session: true }` is destructive and `close_session: false`
is not. A purely static `effects` field forces a bad choice between two wrong
answers: under-declare and the audit log is false, or always declare and a
`DESTRUCTIVE` confirm gesture fires on the harmless nine calls out of ten, which
teaches users to tap through confirmations — the failure that makes every
confirmation in the product worthless.

**Decision: `effects` becomes a declared maximum, narrowable per call by an
optional pure `refine_effects`.**

```rust
capability! {
    name = "pane.split",
    /// The maximum. `pane.split` may spawn a process.
    effects = [Effects::SPAWNS_PROCESS],
    /// Evaluated on the validated input, before the handler runs.
    /// MUST be pure, MUST NOT widen: `refine_effects(i) ⊆ effects`.
    refine_effects = |i: &PaneSplit| {
        if i.session.is_none() { Effects::SPAWNS_PROCESS } else { Effects::empty() }
    },
    intent = Intent::Cas,
}
```

Rules:

- **`refine_effects` may only remove bits, never add them.** Asserted in debug
  builds and checked for every capability by a property test over generated
  inputs, so the declared maximum stays a sound upper bound.
- **Static consumers use `effects`.** The generated docs, the
  `Viewer`+`DESTRUCTIVE` consistency check ([13 §4](13-security.md)) and
  credential scoping all read the maximum, because they reason about a capability
  without an input in hand. A capability that *can* be destructive is treated as
  destructive for authorization; refinement is never an authorization bypass.
- **Per-call consumers use the refined set.** The confirm gesture and **the audit
  log** record what this call actually did. The audit-log consequence, stated
  explicitly: an entry's `effects` is the refined set, and an investigator reading
  "`pane.split`, effects: `{}`" learns that call spawned nothing. Both are
  recorded when they differ (`effects` and `effects_declared`), so a reviewer can
  always see that a narrowing happened rather than inferring it.
- **The refinement runs before the handler**, on validated input, so a surface
  can ask "would this call need a confirm?" without performing it. That query is
  `instance.catalog`'s `dry_run_effects`.

### 2.2 Intent class (D15)

Every capability of `kind = Command` declares an `intent`, naming which of
[D15](decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism)'s
five classes it belongs to. Dispatch reads it to decide whether a repeated
`RequestId` may be served from the recent-results cache, replayed, or must be
rejected — so retry safety is a declared property of each capability rather than
a judgement call in each transport.

| `Intent` | Mechanism | On a repeat |
|---|---|---|
| `Cas` | CAS on version-or-state, plus `(identity, intent_id)` | **return the original result** |
| `Append { dedup }` | client `intent_id` + bounded server dedup cache | **return the original entry**, do not append |
| `RawStream` | writer `epoch` + consumed-offset `ack` | **reject loudly**; never replayed |
| `ExternallyConfirmed` | at-most-once write, confirmed by observation, `Undelivered` on timeout | **never retry**, by any actor except a human who can see the screen |
| `Lww` | CAS on `version`, visible loser | return the current value |

`Query` capabilities declare no `intent`; they are safe to retry by definition.
A `Command` with no `intent` fails the build — the omission is exactly the defect
D15 was written to fix, and defaulting it would reintroduce the defect silently.
`intent` is orthogonal to `effects`: the first says how to deliver a mutation,
the second says what the mutation does.

### 2.3 The declaration-level types

Every name used above is a real type in `omt-catalog`, given here so the
contracts implementer has a signature to write rather than a description to
interpret. These are the *whole* set; a declaration mentions nothing else.

#### `Effects` — a closed bit set

```rust
bitflags::bitflags! {
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize, JsonSchema)]
    #[serde(transparent)]
    pub struct Effects: u8 {
        const WRITES_PTY     = 1 << 0;
        const SPAWNS_PROCESS = 1 << 1;
        const READS_FS       = 1 << 2;
        const WRITES_FS      = 1 << 3;
        const NETWORK        = 1 << 4;
        const DESTRUCTIVE    = 1 << 5;
    }
}

/// The name used where a *value* rather than the type is meant (`refine_effects`
/// returns one). A plain alias, not a second type — the two spellings in this
/// document and in 13 refer to the same thing.
pub type EffectBits = Effects;
```

Bits 6 and 7 are reserved and MUST be zero; a declaration cannot invent a bit,
which is what "closed set" means. On the wire and in the audit log an `Effects`
value is a **sorted array of lower-snake strings**
(`["spawns_process","writes_fs"]`), not an integer — the audit log is read by
humans years later and a bitmask there would be undecodable once the numbering
moved.

#### `Intent` — D15's five classes

```rust
pub enum Intent {
    /// D15 class 1. CAS on version-or-state plus `(identity, intent_id)`.
    Cas,
    /// D15 class 2. Appended to a log; a repeat returns the original entry.
    Append { dedup: Dedup },
    /// D15 class 3. Writer `epoch` + consumed-offset `ack`. Never replayed.
    RawStream,
    /// D15 class 4. At-most-once write into a UI omt does not own, confirmed by
    /// observation. `Undelivered` if unconfirmed within `confirm_within`.
    ExternallyConfirmed { confirm_within: Duration },
    /// D15 class 5. CAS on `version`, visible loser.
    Lww,
}
```

`Append`'s `dedup` is **both** of the things it was ambiguous between — which
fields form the key, and how long the server remembers them — so it is a struct
rather than either one alone:

```rust
pub struct Dedup {
    /// Which of the input's fields join `intent_id` in the dedup key.
    /// `IntentIdOnly` is correct whenever the client mints a fresh `intent_id`
    /// per user action, which is the normal case.
    pub key: DedupKey,
    /// How long the bounded server-side cache remembers an entry. After this,
    /// a repeat appends a second row — so it must exceed the longest reconnect
    /// window the class is expected to survive. Default 10 minutes, matching
    /// dispatch's recent-results cache ([07 §3.5](07-remote-protocol.md#35-capability-call--result)).
    pub window: Duration,
    /// Bound on entries per `(identity, capability)`. Default 256.
    pub max_entries: u32,
}

pub enum DedupKey {
    IntentIdOnly,
    /// `intent_id` plus the named input fields, by JSON pointer into the
    /// validated input. Used where a client may legitimately reuse an
    /// `intent_id` across distinct targets.
    IntentIdAnd(&'static [&'static str]),
}
```

`ExternallyConfirmed`'s `confirm_within` is declared rather than global because
the observation window differs by agent and by card
([D16](decisions.md#d16--remote-answering-is-per-card-type-and-the-preconditions-are-empirical));
when it elapses the intent goes to `Undelivered { reason, response }`
([D15](decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism)
consequence 1), never to a retry.

#### `Parity`

```rust
pub enum Parity {
    Full,
    Exempt { surfaces: ExemptSurfaces, reason: &'static str },
}

pub enum ExemptSurfaces { All, Only(&'static [Surface]) }

pub enum Surface { Tui, Web, Cli, Docs }
```

`surfaces` exists because the real exemptions are not all total: `instance.shutdown`
is exempt everywhere, while `continuity.notification.ack` is exempt on the CLI
only. `reason` is a static string, reproduced verbatim in the generated docs and
matched against the committed allow-list (§5).

#### `Role`

```rust
#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub enum Role { Viewer, Operator, Admin }   // ordering is the authorization compare
```

Defined in `omt-types` (two crates name it), re-exported by `omt-catalog`.

#### `RequestId`

Per [D15](decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism)
consequence 5 and [07 §3.5](07-remote-protocol.md#35-capability-call--result):

```rust
pub struct RequestId { pub device: DeviceId, pub n: u64 }
```

`n` is a monotonic counter **persisted client-side**, not a per-connection
sequence. Wire encoding is the single string `"<device>:<n>"` (`"dev_9a:41827"`)
with `n` in decimal — one string rather than an object so it can be a JSON key
in the recent-results cache and a log field without nesting. `Actor::Local`
(the TUI) uses the instance's own `DeviceId`.

#### `CapabilityError`

```rust
pub struct CapabilityError {
    pub code: ErrorCode,
    /// Human-facing, already localized-neutral. Clients switch on `code`, never
    /// on this ([07 §3.5](07-remote-protocol.md#35-capability-call--result)).
    pub message: String,
    pub detail: Option<ErrorDetail>,
}

pub enum ErrorCode {
    NotFound, Conflict, Unauthorized, PreconditionFailed, Unsupported, Internal,
}
```

`detail` is **typed, not a free-form map**, because it is load-bearing:
[D15](decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism)
consequence 10 requires a phone to distinguish "someone else answered" from "the
agent gave up", and a `serde_json::Value` cannot be exhaustively matched in the
generated TypeScript.

```rust
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ErrorDetail {
    /// The D15 c10 discriminator. `LedgerError`'s three cases stop collapsing
    /// onto one `conflict`, and each gets its own rendering on every surface.
    Interaction {
        state: InteractionErrorState,
        resolved_by: Option<ActorRef>,
        at: Option<Timestamp>,
    },
    /// A CAS loser: `config.set`, `continuity.draft.set`, `Lww` generally.
    Cas { expected: Version, actual: Version },
    /// `unsupported` with a machine-readable cause, so a client can say *why*
    /// rather than greying a button out silently.
    Unsupported { because: UnsupportedReason },
    /// Input validation, keyed by JSON pointer into the input.
    Validation { errors: Vec<FieldError> },
}

pub enum InteractionErrorState { AlreadyResolved, Cancelled, Abandoned }

pub enum UnsupportedReason {
    NativeSession,        // 05: send_text/send_keys/write_bytes on a `native` session
    NotDeliverable,       // D13: the card is `deliverable: None`
    CapabilityDisabled,   // config or plugin state
    PlatformUnsupported,  // D10
}
```

Adding an `ErrorDetail` variant is a minor-version change; adding an `ErrorCode`
is not permitted — the code set is frozen, and a new failure mode is expressed
as a new `detail`, which is exactly what keeps every existing client rendering
it as something rather than as nothing.

## 3. The machinery

§2 described a declaration. This section is the mechanism that turns it into the
seven artifacts of §1: what the macro expands to, how the declarations are
collected, what codegen reads, and how a call is dispatched.

### 3.1 What `capability!` expands to

One declaration produces one zero-sized marker type implementing `Capability`,
one `&'static Decl` of pure metadata, and one registration of that `Decl` into a
distributed slice (§3.3). It produces **no** handler — handlers are written by
hand in `omt-daemon` and bound to the marker type by `register::<C>()`.

```rust
/// The declared metadata. Entirely `const`-constructible: no allocation, no
/// `Instant`, nothing that can fail. This is what codegen consumes.
pub struct Decl {
    pub name:          &'static str,     // "session.send_text"
    pub group:         &'static str,
    pub verb:          &'static str,
    pub kind:          Kind,             // Command | Query
    pub role:          Role,
    pub effects:       Effects,          // the declared maximum (§2.1)
    pub intent:        Option<Intent>,   // Some for Command, None for Query
    pub parity:        Parity,
    pub title:         Option<&'static str>,   // None only when `hidden`
    pub aliases:       &'static [&'static str],
    pub hidden:        Option<&'static str>,   // Some(hidden_reason)
    pub since:         &'static str,
    /// Schema *generators*, not schemas: a `schemars::SchemaGenerator` is not
    /// `const`, so the schema is produced on demand by codegen and by
    /// `instance.catalog`, and never stored in the binary as text.
    pub input_schema:  fn(&mut SchemaGenerator) -> Schema,
    pub output_schema: fn(&mut SchemaGenerator) -> Schema,
}

pub trait Capability: Sized + 'static {
    type Input:  DeserializeOwned + JsonSchema + Send + 'static;
    type Output: Serialize + JsonSchema + Send + 'static;

    /// The single source of metadata. `DECL.name` is the wire name.
    const DECL: &'static Decl;

    /// §2.1. The default is the declared maximum — a capability whose effects
    /// do not vary writes nothing. `capability!` overrides this when the
    /// declaration supplies `refine_effects`.
    ///
    /// MUST be pure and MUST NOT widen: `refine_effects(i) ⊆ DECL.effects`.
    fn refine_effects(_input: &Self::Input) -> Effects { Self::DECL.effects }
}
```

The marker type is what gives the whole system a compile-time handle on a
capability: `register::<SessionSendText>(handler)` cannot be written for a
capability that was never declared, and `omt-input` resolving a keybinding to a
capability *name* (02) is the one place a string is used instead — which is
precisely why parity artifact 2 checks that direction mechanically (§5).

### 3.2 Handlers and the erased form

```rust
#[async_trait]
pub trait CapabilityHandler<C: Capability>: Send + Sync + 'static {
    async fn call(&self, ctx: &CallContext, input: C::Input)
        -> Result<C::Output, CapabilityError>;
}

pub struct CallContext {
    pub actor: Actor,          // which client/credential, or Local for the TUI
    pub role: Role,
    pub request_id: RequestId,
    pub intent_id: Option<IntentId>,   // client-minted; required for Cas/Append
    pub deadline: Option<Instant>,
}
```

The registry cannot store `Box<dyn CapabilityHandler<C>>` — `C` differs per
entry — so it stores the erased form. `ErasedHandler` is the *only* trait object
in the catalog, and it is deliberately JSON-in/JSON-out: every transport already
carries JSON, and a bespoke intermediate representation would be a third
serialization to keep in sync with `schemars` and the wire.

```rust
#[async_trait]
pub trait ErasedHandler: Send + Sync {
    fn decl(&self) -> &'static Decl;
    /// Deserialize → refine → call → serialize. Deserialization failure is
    /// `ErrorCode::PreconditionFailed` with `ErrorDetail::Validation`.
    async fn call_json(&self, ctx: &CallContext, input: Value)
        -> Result<Value, CapabilityError>;
    /// §2.1's `dry_run_effects` without performing the call: deserialize,
    /// validate, refine, return. Never touches the handler.
    fn refine_json(&self, input: &Value) -> Result<Effects, CapabilityError>;
}

/// The one adapter, generic over both, written once.
struct Erased<C: Capability, H: CapabilityHandler<C>> { handler: H, _c: PhantomData<C> }
impl<C: Capability, H: CapabilityHandler<C>> ErasedHandler for Erased<C, H> { /* … */ }

pub struct CapabilityRegistry { entries: HashMap<&'static str, Box<dyn ErasedHandler>> }

impl CapabilityRegistry {
    /// Starts empty of handlers but *aware of every declaration* (§3.3).
    pub fn new() -> Self;
    pub fn register<C: Capability>(&mut self, h: impl CapabilityHandler<C>);
    /// Fails if any declared capability has no handler, or any handler names a
    /// capability outside the declared set. Called once by `omt-daemon` at
    /// startup, so "declared but unimplemented" is a boot failure naming the
    /// capability rather than a `not_found` at 3 a.m.
    pub fn seal(self) -> Result<SealedRegistry, SealError>;
}
```

### 3.3 How declarations are collected

**Decision: a `linkme` distributed slice for the runtime list, and codegen reads
that same list by *running a binary that links it*, never by parsing source.**

```rust
#[distributed_slice]
pub static DECLS: [&'static Decl];      // in omt-catalog

// emitted by `capability!` in the declaring crate:
#[distributed_slice(DECLS)]
static DECL_SESSION_SEND_TEXT: &'static Decl = &SessionSendText::DECL_INNER;
```

The tension is real and must not be waved away: **a distributed slice is a
link-time construct, so its contents exist only inside a linked binary — and
codegen needs the list at build time.** The resolution is that `cargo xtask
codegen` does not try to inspect a slice from outside. It *builds and runs* a
binary in which the slice is fully populated, and reads its stdout:

```
cargo xtask codegen
  └─ cargo run -q -p omt -- debug catalog-dump --json
       └─ CapabilityRegistry::new() → sorted by name → serde_json to stdout
  └─ writes the five generated artifacts from that JSON
```

`omt` is the right binary because it transitively depends on every crate that
declares a capability, so its link unit *is* the complete catalog by
construction. `catalog-dump` is a hidden CLI subcommand, not a capability — it
must work with no daemon running.

This buys the property that matters: **codegen's input is exactly the list the
running process will register**, byte for byte, because it is produced by the
same code path. A build-script or proc-macro scan cannot promise that — it would
re-derive the list by parsing text, and every conditional compilation, feature
flag or macro-generated declaration becomes a way for the generated client to
describe a catalog the binary does not have. That divergence is the exact failure
the catalog exists to prevent, so paying one `cargo run` per codegen invocation
is the cheap side of the trade.

Why `linkme` over the alternatives:

| Option | Why not |
|---|---|
| `inventory` | Same link-time collection, but it registers via life-before-`main` constructors. Order is unspecified, it interacts badly with static linking and with `--gc-sections`, and a silently dropped entry means a capability that vanishes from the catalog with no error. `linkme` places entries in a section as `const` data with no runtime initialization. |
| build-script source scan | Re-derives the list by parsing Rust text; diverges from the linked binary as described above. |
| explicit central registration list | One `register_all()` listing every declaration. Correct, but it is a second place to edit, and forgetting a line is invisible — the catalog is smaller by one and nothing fails. `seal()` catches a missing *handler*; nothing would catch a missing *declaration*. |

Ordering is not left to the linker: `CapabilityRegistry::new()` sorts `DECLS` by
`name` and rejects duplicates, so codegen output is deterministic and a name
collision is a boot failure rather than a shadowed entry.

### 3.4 What `cargo xtask codegen` reads and writes

Input: the `catalog-dump` JSON above (declarations plus generated JSON Schema
for every `Input`/`Output`).

Outputs, all committed to the repository:

| Path | Content |
|---|---|
| `schemas/catalog.v1.json` | every input/output schema, plus the declaration metadata; the file `instance.catalog` serves and whose hash is `catalog_hash` ([07 §3.3](07-remote-protocol.md#33-handshake-and-capability-negotiation)) |
| `web/src/generated/capabilities.ts` | `CapabilityName` union, per-capability input/output types, the typed client, and the `handlers` exhaustiveness map (§5) |
| `crates/omt-server/src/generated/routes.rs` | the `POST /v1/<group>/<verb>` route table over `dispatch` |
| `crates/omt/src/generated/cli.rs` | the `omt <group> <verb>` clap tree, including `aliases`, and skipping `hidden` in `--help` |
| `docs/reference/capabilities.md` | parity artifact 4, including the exemption and `hidden_reason` tables |

CI runs `cargo xtask codegen --check`, which regenerates into a temporary
directory and diffs against the committed files. A difference fails the job and
prints the changed paths and the offending capability names; the fix is to run
`cargo xtask codegen` and commit. The generated files are committed rather than
built on demand so that the web package, which does not run `cargo`, has types
without a Rust toolchain, and so that a change to the catalog is visible in
review as a diff.

### 3.5 The dispatch path

There is exactly one:

```
caller → SealedRegistry::dispatch(name, input_json, ctx) → handler → output
```

and it performs these steps in this order:

1. **Resolve** `name` → `Decl` + `ErasedHandler`. Unknown → `not_found`.
2. **Authorize.** `ctx.role >= decl.role`, then the credential's capability
   scope, if it has one. Failure → `unauthorized`. Authorization reads
   `decl.effects` — the declared **maximum** — never the refined set (§2.1), so
   refinement can never widen access.
3. **Enforce `intent`** (below), which may short-circuit with a cached result.
4. **Deserialize and validate** into `C::Input`. Failure →
   `precondition_failed` + `ErrorDetail::Validation`.
5. **Refine effects** on the validated input. This is also the whole
   implementation of `instance.catalog`'s `dry_run_effects` (§3.5.1).
6. **Call** the handler under `ctx.deadline`.
7. **Record** the audit entry: actor, device, `request_id`, `intent_id`, the
   refined `effects` and, when they differ, `effects_declared`.

The TUI calls this directly with `Actor::Local`. `omt-server` calls it after
authenticating and mapping the credential to a `Role`. The CLI calls it over the
local socket. Because authorization is step 2 of dispatch and not a transport
concern, no surface can bypass it.

**Where `intent` is enforced — step 3, and only there.** Dispatch holds the
bounded recent-results cache keyed by `RequestId`
([07 §3.5](07-remote-protocol.md#35-capability-call--result)), and `decl.intent`
decides what a repeated `RequestId` means:

| `intent` | Repeated `RequestId` in dispatch |
|---|---|
| `Query` (no intent) | re-execute; queries are safe by definition |
| `Cas` | serve the cached result; on a miss, re-execute — the handler's own CAS on `(identity, intent_id)` makes that safe |
| `Append { dedup }` | serve the cached result; on a miss, forward with `intent_id` and let the handler's dedup cache return the original entry |
| `RawStream` | **reject with `conflict`**; never served from cache and never re-executed. Resumption is `ack`, not repetition |
| `ExternallyConfirmed` | **reject with `conflict`**. D15's "never retry an injection, by any actor except a human who can see the screen" is enforced here, once, rather than in each transport |
| `Lww` | re-execute; the CAS on `version` produces a visible loser |

A `Command` whose `Decl.intent` is `None` cannot reach dispatch — the
`capability!` macro fails the build, and `seal()` asserts it again for
plugin-contributed declarations, which are not macro-checked (§3.6).

#### 3.5.1 `instance.catalog` and `dry_run_effects`

`instance.catalog` answers two questions, so it takes a mode:

```rust
struct CatalogInput {
    /// Client's cached hash; `{ unchanged: true }` when it matches.
    known_hash: Option<CatalogHash>,
    /// Ask "what would this call actually do?" without doing it.
    dry_run_effects: Vec<DryRunRequest>,
}
struct DryRunRequest { name: String, input: Value }

struct CatalogOutput {
    hash: CatalogHash,
    capabilities: Vec<CatalogEntry>,       // omitted when `unchanged`
    dry_run_effects: Vec<DryRunResult>,
}
struct DryRunResult {
    name: String,
    declared: Effects,                     // decl.effects
    refined: Effects,                      // ErasedHandler::refine_json
    /// Whether the surface must ask first, derived from `refined`, not `declared`.
    requires_confirm: bool,
}
```

It is a batch because the confirmation decision is usually made for a screenful
of affordances at once — a phone deciding which of eight visible actions get a
confirm gesture should not make eight round trips. `dry_run_effects` runs steps
1, 2, 4 and 5 of the dispatch path and stops: it authorizes (so it cannot be
used to probe capabilities the caller may not call), validates, refines, and
never reaches the handler.

### 3.6 Plugins: runtime registration, and what parity means for it

Plugin-contributed capabilities ([11](11-plugins.md)) register **at run time**,
after codegen has run and after the web bundle has been built. They therefore
cannot be in `DECLS`, cannot appear in the generated `CapabilityName` union, and
cannot be covered by §5's exhaustiveness check. Pretending otherwise would make
the check a lie, so the catalog has **two tiers**, and the difference is
declared rather than hidden:

```rust
pub enum Origin { Core, Plugin { plugin: PluginId, version: Version } }
```

Every `CatalogEntry` carries one. `instance.catalog` returns both tiers merged;
`catalog_hash` covers both, so enabling a plugin invalidates every client's
cached catalog exactly as a version bump does.

A plugin capability supplies the same `Decl` fields, from its manifest rather
than from the macro. Because no macro validated it, the plugin host validates it
at load, against the same rules, and **refuses to register on failure** rather
than degrading: name well-formed and not colliding with a core name or another
plugin's; `title` present unless `hidden`; `intent` present for every `Command`;
`effects` a subset of the plugin's granted permission set
([11 §4.2](11-plugins.md#4-permissions)); input/output JSON Schema present and
self-contained. A rejected capability surfaces as
`plugin.list`'s `status = "invalid"` with the diagnostic, which is the existing
mechanism.

**How it reaches the web client.** Not as generated TypeScript — there is none.
The web client ships one **generic, schema-driven renderer**: given a
`CatalogEntry` with `Origin::Plugin`, it renders a form from the input JSON
Schema, calls `dispatch` through the same typed transport, and renders the
output from the output schema. This is the same renderer
[10 §10](10-configuration.md#10-config-capabilities)'s `config.schema` already
requires for settings forms, so it is not new machinery.

**So parity for a plugin capability means something weaker, and the docs say
which.** Two grades, both enforced, neither confused with the other:

| | Core capability | Plugin capability |
|---|---|---|
| Reachable on TUI / CLI / API / web | yes | yes |
| Web surface | hand-written handler, exhaustiveness-checked at build | generic schema-driven form |
| Enforcement | CI parity test over `DECLS` | load-time conformance check in `omt-plugin-host`, refusing registration |
| Generated docs | `docs/reference/capabilities.md` | `plugin.info`, at run time |

The honest statement: a plugin capability gets **generic parity** — it is
reachable and usable from every surface — but not **bespoke parity**, a
purpose-built UI on each. That is the correct trade: the alternative is either
forbidding plugin capabilities on the web (which breaks
[P2](01-principles.md#p2--pluggable-extension-without-modification)) or shipping
a build step at plugin-install time (which breaks
[P3](01-principles.md#p3--parity-one-capability-three-surfaces)'s promise that
adding a capability is cheap). A plugin that wants a bespoke web surface
contributes a UI asset through the plugin host's own extension point; that is a
plugin feature, not a catalog obligation.

**Errors** are the closed `CapabilityError` of §2.3 — six stable codes, a human
message, and a typed `detail` — so every surface, including the generic plugin
renderer, can render a failure consistently.

## 4. Events are the read-side twin

Capabilities are request/response. Live state flows through the event bus
(`omt-events`), and subscription is itself a capability
(`events.subscribe`) so the same auth and the same schema generation apply.

Invariants:

- Every event has `(session_id, seq)` with `seq` monotonic per session, in the
  envelope defined by [`omt-events`](02-crate-map.md#omt-events). Terminal byte
  frames share that sequence space
  ([07 §5.1](07-remote-protocol.md#51-sequence-spaces)); workspace-scoped events
  carry `workspace` instead of `session` and use the workspace's own space
  ([15 §4.6](15-workspace-explorer.md#46-file-watching)).
- A client may resume with `since_seq`; the instance replays from its log or
  responds `Resync` ([07 §5.2](07-remote-protocol.md#52-replay-window)) with a
  snapshot.
- The TUI subscribes to the same bus with the same envelope. There is no
  internal-only event.
- Events are *derived from* state changes, never the mechanism of them — a
  handler mutates state and the state layer emits; handlers do not publish
  events by hand.

## 5. The parity contract

For each capability, five artifacts must exist. The parity test enumerates the
registry and asserts all five; a missing one fails CI with the capability name.

| # | Artifact | Where it lives | How it is checked |
|---|---|---|---|
| 1 | API route + schema | generated by `omt-catalog` codegen | generated file is committed; CI regenerates and diffs |
| 2 | TUI reachability | `omt-tui`'s action table and the palette | see below — this arm is weaker than it looks and the weakness is deliberate |
| 3 | Web handler | `web/src/capabilities/registry.ts` | generated TS declares the union; a type-level exhaustiveness check fails the web build if a case is unhandled |
| 4 | Docs entry | generated `docs/reference/capabilities.md` | regenerated and diffed in CI |
| 5 | Palette entry | generated from `title`/`aliases`/`hidden` (§2) | test asserts every non-`hidden` capability has a `title` |

### 5.1 What artifact 2 actually asserts, and what it does not

[16 §3.1](16-input-and-keymap.md) establishes that the command palette *is* the
universal TUI affordance: its contents are the catalog, so every capability is
reachable by searching for it in plain words, and **bindings are an optimization
for frequency rather than a requirement for reachability**. That argument is
correct, and it is the reason omt can keep a tiny un-prefixed key budget instead
of inventing a chord for a hundred and fifty operations.

It also means artifact 2 cannot assert what a naive reading suggests. "Every
non-`Admin` capability has a TUI action" is satisfied by palette membership,
which is universal — so read literally, that clause reduces to *the palette
exists*. Stating it as though a per-capability binding were verified would be a
guarantee this project does not have.

What artifact 2 therefore asserts, precisely:

1. **Reachability.** Every non-`Admin` capability is reachable in the TUI — via
   the palette, which requires a `title` (artifact 5), or via an explicit
   binding.
2. **The reverse direction, which is the half with teeth.** Every action bound
   in the keymap names a capability that exists. This is what catches drift: a
   binding pointing at a renamed or deleted capability fails CI, which is
   exactly the defect that would otherwise ship as a key that silently does
   nothing.
3. **`hidden` capabilities need a real binding or an exemption.** A `hidden`
   capability is omitted from the palette (§2), so palette membership cannot
   satisfy clause 1 for it. It must carry either an explicit binding or a
   `Parity::Exempt` naming the TUI surface. Without this rule a `hidden`
   capability would pass artifact 2 with no TUI affordance of any kind.

**What no automated check can assert is that an affordance is *good*.** A
capability reachable only by typing its name into a palette is reachable, not
discoverable, and for something a user reaches for under pressure — interrupting
a runaway agent, cancelling a transfer — that distinction is the whole product.
Frequency-of-use is a judgement, so it stays a design review responsibility:
when a change adds a capability a user will want in a hurry, it gets a binding
and a visible control, and the reviewer's job is to notice. The test guarantees
nothing is *unreachable*; it cannot guarantee anything is *at hand*.

Two mechanical checks worth naming, because both have already caught a real
contradiction in these documents:

- **Artifact 2 in both directions.** *Every action names a real capability* is
  the half that catches drift: a keybinding in [16 §11](16-input-and-keymap.md)
  that names an action no capability declares fails on the first CI run.
- **`refine_effects` is sound** (§2.1) and **every `Command` declares an
  `intent`** (§2.2). Both are enumerated over the registry by the same test.

The parity test enumerates `DECLS` (§3.3) — the same list codegen consumed — so
the test and the generated artifacts cannot disagree about what the catalog
contains. Plugin-contributed capabilities are **not** in that list and are held
to §3.6's load-time conformance check instead.

Escape hatch, deliberately narrow: a capability may declare
`parity = Parity::Exempt { surfaces, reason }` (§2.3). `surfaces` may be `All`
or a specific list, so a CLI-only exemption does not silently excuse the web.
Exemptions are enumerated in the generated
docs so they are visible, and the test asserts the list matches an explicit
allow-list in the repo — you cannot add one silently. Legitimate examples:
`instance.shutdown` (admin, no phone affordance needed), local-only debug dumps.

**The exhaustiveness trick for the web client** is what makes surface #3 real
rather than aspirational: codegen emits

```ts
export type CoreCapabilityName = "session.send_text" | "session.list" | ...;
export const handlers: { [K in CoreCapabilityName]: Handler<K> } = { ... };
```

A new capability breaks the web build until someone writes the handler. That is
the desired failure mode.

It is `CoreCapabilityName`, not `CapabilityName`, because the union is closed by
construction and a plugin's capabilities are discovered at run time (§3.6).
Those arrive as `CatalogEntry` values with `Origin::Plugin` and are rendered by
the generic schema-driven renderer, which is total by construction and so needs
no exhaustiveness check. Naming the type `Core…` is what stops a reader from
concluding the union is the whole catalog.

## 6. Capability groups (initial surface)

This is the shape, not the exhaustive list; each change adds its own.

| Group | Representative capabilities |
|---|---|
| `instance` | `info`, `health`, `catalog`, `shutdown`, `peers.list`, `peers.add` |
| `workspace` | `list`, `open`, `close`, `rename`, `vcs.*`, `files.*`, `worktree.*`, `history` |
| `session` | `list`, `create`, `close`, `attach`, `detach`, `resize`, `send_text`, `send_keys`, `write_bytes`, `scrollback.get`, `search`, `blocks.list`, `blocks.get`, `blocks.rerun`, `writer.acquire`, `writer.release` |
| `pane` | `split`, `close`, `focus`, `navigate`, `move`, `zoom`, `stack.*`, `float` |
| `layout` | `get`, `set`, `preset`, `balance`, `views.*`, `promote`, `save`, `apply_saved`, `import_tmux` |
| `agent` | `state`, `explain`, `bind`, `unbind`, `prompt`, `interrupt`, `queue.list`, `queue.enqueue`, `queue.remove`, `commands.list`, `commands.run` |
| `interaction` | `list`, `get`, `resolve`, `cancel` |
| `media` | `clipboard.read`, `clipboard.write`, `image.upload`, `image.paste`, `file.push`, `file.pull` |
| `stt` | `session.start`, `session.stop`, `providers.list` |
| `media` (cont.) | `blob.begin`, `blob.commit`, `transfer.progress` |
| `config` | `get`, `schema`, `set`, `unset`, `validate`, `reload`, `sources`, `default`, `export`, `import`, `pending`, `project.trust` |
| `theme` / `keys` | `theme.list/get/import`, `keys.list/conflicts` |
| `workflow` / `launch` | `list`, `get`, `run`, `save`, `delete` |
| `plugin` | `list`, `info`, `install`, `uninstall`, `enable`, `disable`, `upgrade`, `health`, `logs`, `call`, `permissions.get/set` |
| `presence` | `list` |
| `audit` | `query` (Admin) |
| `events` | `subscribe`, `resume` |

The full consolidated list, with kinds, roles and effects, is the hand-written
[`docs/reference/capabilities-draft.md`](../reference/capabilities-draft.md),
which §5 artifact #4 replaces with generated output.

There is deliberately **no `notification` group**:
[D12](decisions.md#d12--no-push-notifications-in-v1-open-and-replay-instead)
removes push from v1, so `notification.push.*` and `notification.test` are not
v1 capabilities and must not be declared
([07 §8](07-remote-protocol.md#8-notifications-to-a-closed-tab--none-in-v1)).

`layout.*` is a group of its own rather than a `pane.*` sub-group, because these
operations act on a **view** and not on a pane
([17 §9.3](17-panes-and-layout.md#93-how-this-surface-sits-in-the-catalog)).

Two of these deserve emphasis:

- **`interaction.resolve`** is the flagship path. It is how a phone answers an
  `AskUserQuestion` card. It is resolvable exactly once and idempotent by
  `(interaction_id, identity_or_device, intent_id)` — **not** by the actor, which
  changes on every reconnect and would make a device's own retry read as a
  stranger overriding it
  ([D15](decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism)
  consequence 6). A retry under the same key returns the original outcome; a
  different identity gets `conflict` with a discriminating `detail.state`. Full semantics in
  [12 §4](12-collaboration.md#4-interaction-ownership). Its result is broadcast
  to every surface — including the TUI, which then shows the card as answered by
  whoever answered it.
- **`agent.commands.list` / `agent.commands.run`** give the web client native
  slash-command semantics: the catalog exposes the agent's own resolved command
  list (from `system/init`, ACP `available_commands_update`, or disk
  enumeration), so a phone gets the same completion popup the TUI has.

## 7. Versioning

- Capability names are stable. Renames ship as an alias for two minor versions,
  with the old name marked deprecated in the generated docs.
- Input types may gain optional fields; required fields may not be added.
  Output types may gain fields; clients ignore unknown ones.
- The protocol carries a catalog version; on handshake, a client learns which
  capabilities the instance actually has. This is how a phone talks to several
  instances on different versions at once — it renders the intersection and
  greys out the rest, rather than failing.

## 8. Why this is worth the machinery

Without a catalog, parity is a promise that decays: someone adds a keybinding
that calls a session method directly, and six months later the web client is a
second-class citizen and mobile users are told to "use the TUI for that".

With the catalog, the cheapest way to add a feature is also the correct one —
declare it, implement one handler, and the API, the CLI, the schema, the docs
and the web type-checking come along. Doing it the wrong way is *more* work,
which is the only enforcement that survives contact with a deadline.
