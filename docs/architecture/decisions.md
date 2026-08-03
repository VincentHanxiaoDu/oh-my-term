# Decision Log

Decisions that constrain the whole system, with the reasoning that produced
them. Anything here overrides a contradicting statement elsewhere in `docs/`;
if you find a contradiction, fix the other document.

---

## D1 — omt adds no policy layer over an agent's permission semantics

**Decision.** omt mirrors each agent CLI's own permission gate exactly. If the
CLI asks, omt surfaces the question on every surface. If the CLI does not ask —
because the user ran `--dangerously-skip-permissions`, or configured
`bypassPermissions`, or the tool is on its allow-list — omt does not invent a
prompt. omt never adds an approval step the CLI would not have shown, and never
suppresses one it would.

**Reasoning.** This is principle [P4](01-principles.md#p4--native-semantics-observe-never-re-implement)
applied to its most consequential case. Every agent CLI already has a designed,
documented, user-configured permission model; a second, different gate in omt
would mean two mental models, two configurations, and a class of bugs where the
user's `--dangerously-skip-permissions` silently does not apply. Worse, an omt
policy layer would create false confidence: users would believe omt is
protecting them in cases it cannot see.

**Consequences.**
- The `Interaction` ledger has no notion of "too dangerous to show remotely".
- There is no omt-side allow-list, deny-list, or auto-approve. Those features
  belong to the agent CLI, and omt surfaces the agent's own configuration
  read-only so the user can see which mode a session is in.
- `effects` on capabilities remains, but it drives UI affordances and the audit
  log, not permission decisions.

---

## D2 — Remote is exactly equivalent to local

**Decision.** Once a client is authenticated, acting through the web client or
the API is equivalent to sitting at the TUI. There is no reduced remote role, no
remote-only confirmation step, no capability that works locally and not
remotely.

**Reasoning.** The user's framing: *"web remote 进来就和操作 tui 一样的"* — access
control answers **who can connect**, not **what they may do once connected**.
Splitting those two would break parity ([P3](01-principles.md#p3--parity-one-capability-three-surfaces))
and produce exactly the second-class mobile experience the product exists to
avoid.

**Consequences.**
- Roles (`Viewer`/`Operator`/`Admin`) exist for *sharing* — minting a read-only
  link for someone else to watch — not for degrading the owner's own devices.
- All the security effort goes into the connection boundary: bind policy,
  credential strength, expiry, revocation, transport. See
  [13 — Security](13-security.md).
- The audit log records actor and device for every capability call, because
  attribution is still valuable even when authority is uniform.

---

## D3 — Synthetic input is bounded by state dependence, not by tool danger

**Decision.** When an agent exposes no native response channel, omt may write to
the PTY **only when the answer is position-independent** — that is, when
producing it does not require omt to have guessed the agent's internal UI state.

Allowed:
- Line-oriented CLIs where stdin *is* the documented input channel (Aider's
  confirm prompts are `input()` reads; writing `y\n` is not a hack).
- Full-screen TUIs that accept a direct, position-independent selection —
  typing `1`/`2`/`3`, `y`/`n`, or free text.
- Submitting typed text to a prompt box.

Forbidden by default:
- Any answer that requires counting arrow keys relative to a cursor position omt
  inferred from the screen.

**Reasoning.** The original framing ("don't synthesize for dangerous tools") was
wrong, because it measured the *consequence* rather than the *failure mode*.
Writing to stdin is not itself fragile — it is how these programs are driven.
What is fragile is needing to know where a highlight bar currently sits, which
is screen-derived state that a version bump, a locale change, or a resize can
invalidate. When that inference is wrong, omt selects the *wrong option*, and on
a phone the user cannot see that it went wrong. Position-independent answers
have no such inference step, so their correctness is a testable property rather
than a gamble.

**Consequences.**
- The `Responder` trait reports `fidelity: Native | Synthetic`, and synthetic
  responders additionally declare `state_dependence: Independent | Inferred`.
  Only `Independent` is enabled by default; `Inferred` requires explicit opt-in
  per agent and is labelled as experimental wherever it appears.
- Adapters are responsible for discovering whether an agent's prompt offers a
  position-independent form, and preferring it.
- Every synthetic response is tagged in the event stream and visibly attributed
  in the UI as omt-typed, on every surface.

---

## D4 — Single user, many devices — with the interfaces left open for many users

**Decision.** The primary scenario is one person across a laptop, a phone, and
several machines. Build presence, the writer token, exactly-once interaction
resolution, and per-device attribution. Do **not** build organizations, teams,
or fine-grained ACLs now — but do not design them out either.

**Reasoning.** The concurrency primitives (arbitration, causality, exactly-once)
are needed even for one user with two devices, so they are not deferrable. The
identity and authorization machinery of true multi-tenancy is a large, separate
body of work with no user for it yet.

**Consequences.**
- `Actor` carries an identity from the start, even though every identity is the
  same person today. Nothing in the protocol assumes a single actor.
- `AuthBackend` is a trait, so an organizational backend is an addition, not a
  refactor. See [11 — Plugins](11-plugins.md) and [13 — Security](13-security.md).

---

## D5 — Initial agent coverage

**Decision.** Ship with four tracks, in this order:

1. **Claude Code — full depth.** Hooks (30 events), `AskUserQuestion` cards,
   the message queue, transcript tailing, slash commands. The only agent where
   every flagship feature is achievable, so it defines the ceiling.
2. **A generic ACP adapter.** One implementation covering opencode, Gemini CLI,
   Goose and Qwen Code, delivering native permission interactions and slash
   command discovery across all four. The best coverage-per-unit-work available.
3. **Codex CLI.** Hooks plus an `app-server` client.
4. **The heuristic floor.** Every other agent gets `busy | idle | needs you`,
   so nothing is completely invisible.

**Reasoning.** Depth on one agent proves the architecture and delivers the
differentiator; ACP buys breadth cheaply; the heuristic floor guarantees the
product degrades rather than excludes.

**Consequences.** The adapter trait must be validated against all four shapes
before it is frozen, or it will encode Claude Code's assumptions. The ACP
adapter is therefore built *early*, not last.

---

## D6 — Terminal emulation is built on a third-party byte-level parser

See [04 — Terminal core](04-terminal-core.md) for the full evaluation. Recorded
here because it is a foundational, hard-to-reverse choice.

---

## D7 — Image paste over SSH is only promised where omt controls both ends

**Decision.** Full-fidelity clipboard and image paste is a documented feature of
`omt ssh` / `omt --remote` and of the web client. In a foreign terminal over a
plain `ssh`, omt attempts the paths that exist (realistically: kitty, with a
non-default config flag) and otherwise falls back to a *diagnosed* out-of-band
path — QR to the phone, or `omt paste --to <instance>:<session>` from the
laptop — naming the terminal and the reason.

**Reasoning.** See [09 — SSH and media](09-ssh-and-media.md) §5. Universal
foreign-terminal clipboard access is a bet against software omt does not
control, and it cannot be won. A precise failure message with a working
alternative is a better product than an unreliable feature.
