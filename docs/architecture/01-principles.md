# Principles and Invariants

These are not aspirations. Each one is stated so that a reviewer can point at a
diff and say "this violates P3", and most are backed by a mechanical check.

---

## P1 — Clean: small crates, explicit seams

Every crate has one responsibility, a documented public API, and no knowledge of
its consumers. A module over ~1500 lines is a design smell and must be split
before it grows further; there is no file in omt that plays the role another tool's
10 600-line `server/headless.rs` plays.

**Enforced by:** a CI check on file length (warn at 1200 lines, fail at 2000),
`#![deny(missing_docs)]` on every public crate, and a dependency-direction test
(`cargo-deny`-style) asserting the layering in [02 — Crate map](02-crate-map.md)
has no cycles and no upward edges.

**Consequences.** Core types are defined once, in the lowest crate that needs
them, and everything above depends on the trait, not the implementation. No
crate may depend on `omt-tui` or `omt-server`; those are leaves.

---

## P2 — Pluggable: extension without modification

Six extension points are first-class, each a trait plus a registry:

| Extension point | Trait | Examples |
|---|---|---|
| Agent adapter | `AgentAdapter` | Claude Code, Codex, opencode, Gemini, ACP-generic |
| Observation source | `EventSource` | hooks, ACP, transcript tail, process probe, PTY heuristics |
| Transport | `Transport` | WebSocket, Unix socket, SSH stdio, (future) WebTransport |
| Auth backend | `AuthBackend` | invite link, bearer token, password, tailnet identity |
| Storage backend | `Store` | on-disk JSON+WAL, SQLite, in-memory |
| Speech-to-text | `SttProvider` | Deepgram, OpenAI, local whisper.cpp |

**The invariant:** adding an implementation must not require editing any file
outside its own crate and one registration line. If a new agent adapter forces a
change in `omt-core`, the trait is wrong.

**Enforced by:** each registry ships a `tests/third_party_impl.rs` that
implements the trait from *outside* the crate using only its public API. If that
test needs a `pub(crate)` item to be opened up, the abstraction has leaked.

Plugins come in two flavours — in-process Rust (compiled in, for first-party
adapters) and out-of-process (a subprocess speaking the omt plugin protocol, or
a WASM component), so third parties can extend omt without forking it. See
[11 — Plugin system](11-plugins.md).

---

## P3 — Parity: one capability, three surfaces

**Anything the native TUI can do, the API exposes, and the web client can do.**
There are no TUI-only capabilities and no web-only capabilities.

This is achieved structurally rather than by discipline. Capabilities are
declared once in the capability catalog; the TUI, the API server and the web
client are all *renderers* over that catalog. The TUI does not call into the
core directly — it dispatches catalog commands, exactly as a remote client does,
just without a network hop.

**Enforced by:** a parity test that walks the catalog and asserts, for every
capability: (a) a generated API route and JSON Schema exist, (b) a TUI action is
bound to it, (c) the web client's handler registry has an entry, and (d) the
capability appears in the generated reference docs. Adding a capability without
all four fails CI. See [03 — Capability catalog](03-capability-catalog.md).

**Corollary — one event stream.** State is broadcast once. The TUI and every
remote client subscribe to the same stream with the same schema. There is no
"internal" event that the API cannot see.

**Corollary — mobile is a target, not a fallback.** The web client is designed
for a phone first: touch-sized targets, one-handed reach, a block-based session
view that does not require a 100-column terminal, and native rendering of
question and permission cards. Desktop is the same UI with more room.

---

## P4 — Native semantics: observe, never re-implement

The agent's own CLI is the source of truth for its own UX. omt runs it in a real
PTY and shows the real thing. omt adds observation and remote surfaces; it never
re-implements an agent's prompt loop, never proxies the model API, and never
rewrites the agent's output.

Two rules fall out of this:

- **Structured content only from structured sources.** Cards rendered remotely
  must originate from hooks, a native protocol, or a transcript file — never
  from parsing ANSI. Screen heuristics may only produce a coarse activity
  guess. (another tool's screen-scraping manifests are a legitimate design under a
  different goal; they are not sufficient for remote rendering.)
- **Answers go back the way they came.** An answer to a deferred `PreToolUse`
  is returned as a hook decision; an ACP permission is answered over ACP.
  Synthesizing keystrokes into a PTY is the last resort, is always labelled as
  such in the event stream, and is never used for anything destructive.

---

## P5 — Production-grade from the first commit

No MVP tier, no stub layers, no `todo!()` in merged code. A change is done when
it has tests, docs, error types with actionable messages, and clean
`cargo clippy -D warnings`.

Concretely:
- Errors are typed per crate (`thiserror`), never `anyhow` in library code.
  `anyhow` is allowed only in binaries, at the top level.
- No `unwrap()`/`expect()` outside tests and documented invariants (enforced by
  clippy lints).
- Any protocol or on-disk format is versioned from v1 and has a compatibility
  test with a checked-in fixture.
- Every crate that parses untrusted input (VT sequences, config, wire messages)
  ships a fuzz target.

---

## P6 — Collaboration is a runtime feature, not just a workflow

Multiple humans and multiple agents act on the same instance concurrently. The
system must therefore have real answers to:

- **Input arbitration** — who is currently driving a session's PTY, and what
  happens when two clients type at once. (Answer: sessions have a *writer*
  token with explicit takeover and an observers list; every client always sees
  who is driving.)
- **Interaction ownership** — an `Interaction` can only be resolved once,
  by exactly one actor, with the resolution broadcast to everyone.
- **Causality** — every event carries a monotonic per-session sequence number,
  so a reconnecting client can resume exactly, and clients never render state
  out of order.
- **Presence** — clients, their capabilities, and what they are viewing are
  part of the state, so the TUI can show that a phone is attached to a pane.

See [12 — Concurrency and collaboration](12-collaboration.md).

---

## P7 — Configuration is data, and errors are precise

One layered configuration model (defaults → file → per-instance overrides →
runtime), editable three ways (config file, TUI editor, web UI) with the same
schema and the same validation. Unknown keys are reported, not ignored. Errors
carry the file, line, column, the offending value, and a suggestion.

Configuration is declared once as a Rust type; the JSON Schema, the TUI editor,
the web form and the documentation are all generated from it. A setting that
exists in the file and not in the TUI is a bug.

---

## P8 — Security by default, no ambient trust

- The daemon binds to loopback unless explicitly configured otherwise, and
  refuses to bind to a public interface without an auth backend configured.
- All remote capabilities are permissioned per credential, with read-only,
  operator, and admin roles; a shared invite link can be minted read-only.
- Nothing leaves the machine unless the user configured it to. No telemetry.
- Secrets (tokens, API keys) live outside the main config file, with strict
  file permissions, and are redacted in every log and every event.

See [13 — Security model](13-security.md).

---

## P9 — Clean-room with respect to studied code

omt is Apache-2.0. The another terminal source is AGPL-3.0 and iTerm2 is GPL-2.0. Their
*interfaces* — escape-sequence numbers, file formats, YAML schemas, protocol
shapes — are facts and may be reimplemented. Their *code* may not be copied,
adapted, or translated. See [14 — Licensing and provenance](14-licensing.md).

---

## P10 — Everything in English

Code, comments, docs, specs, commit messages, issues and pull requests. No
exceptions, so that any contributor can read any part of the project.
