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
 confirm prompts are `input` reads; writing `y\n` is not a hack).
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
`omt ssh` / `omt ssh` and of the web client. In a foreign terminal over a
plain `ssh`, omt attempts the paths that exist (realistically: kitty, with a
non-default config flag) and otherwise falls back to a *diagnosed* out-of-band
path — QR to the phone, or `omt paste --to <instance>:<session>` from the
laptop — naming the terminal and the reason.

**Reasoning.** See [09 — SSH and media](09-ssh-and-media.md) §5. Universal
foreign-terminal clipboard access is a bet against software omt does not
control, and it cannot be won. A precise failure message with a working
alternative is a better product than an unreliable feature.

---

## D8 — Two session modes: `pty` (default) and `native` (ACP)

**Decision.** omt supports two ways to run an agent, chosen per session:

- **`pty` — the default.** omt spawns the user's real CLI in a real PTY. The
 agent draws its own TUI; omt observes it through the tiered source model of
 [06 §4](06-agent-layer.md#4-merging-confidence-tiers-not-voting). This is the
 product premise and stays the default everywhere.
- **`native` — opt-in.** omt spawns the agent in ACP mode
 (`opencode acp`, `gemini --acp`, `cursor-agent acp`, the official Claude and
 Codex adapters, …) and speaks JSON-RPC to it. The agent has **no TUI**; omt
 renders the whole session itself from typed events.

Invoked as `omt claude` versus `omt claude --native`, and settable per workspace
in config.

**Reasoning.** Research ([`acp-and-elicitation.md` §10](../research/acp-and-elicitation.md))
established that these are mutually exclusive: **ACP is not an observability
sidecar, it is a replacement front end.** You cannot run a CLI's TUI and speak
ACP to the same process. That research also corrected three earlier beliefs:
the ACP ecosystem is ~40 agents rather than four and includes first-party
Claude, Codex, Cursor and Copilot adapters; `session/request_permission` is
verified live and is a blocking request with **no timeout** — a materially
better shape for a phone round-trip than Claude Code's still-unverified
`permissionDecision: "defer"`; and ACP has its own `elicitation/create`, so
`Choice` is not permanently Claude-only.

Refusing `native` would forfeit a verified mechanism that is better than the one
the flagship feature currently depends on. Making it the default would turn omt
into "another ACP client" competing with Zed and JetBrains, and would silently
take away the user's actual CLI — its keybindings, its `/voice`, its permission
UX, its on-disk sessions. Note especially that the Claude ACP adapter wraps the
**Agent SDK, not the Claude Code CLI**: a user in `native` mode is not running
Claude Code at all. That must be a deliberate, informed choice, never a default.

**Consequences.**
- `SessionMode` is part of the session model, visible on every surface. A
 `native` session is always labelled as such — the user must never be confused
 about which product they are talking to.
- The `AgentAdapter` trait must express both modes; adapters declare which they
 support. The tiered `EventSource` model applies to `pty` only.
- In `native` mode omt owns the rendering, which makes the block view, the
 interaction cards and the mobile experience strictly better. That is the
 honest selling point, and the honest cost is stated next to it.
- ACP v1 is the build target; v2 (Draft, with a `state_update` notification that
 maps directly onto `AgentState`) is negotiated where offered.
- `native` mode does not weaken [D1](#d1--omt-adds-no-policy-layer-over-an-agents-permission-semantics):
 omt still surfaces exactly the agent's own permission requests.

---

## D9 — Positioning: what omt may and may not claim

**Decision.** The competitive survey
([`remote-agent-products.md` §12`](../research/remote-agent-products.md)) evaluated
omt's five intended differentiators against shipping products. The results
change what omt says about itself and what it builds first.

| Claim | Verdict | What omt may say |
|---|---|---|
| Runs your real CLI | **Partial** | Not "runs your real CLI" — competitors say that too, while monkey-patching `fetch`, injecting flags, or patching the binary on disk. omt's defensible claim is narrower: *your real CLI, in a real PTY, with its own TUI, keybindings and slash commands, **observed from outside rather than instrumented from within**.* |
| Remote `AskUserQuestion` cards | **Commoditized** | Do not claim this as a differentiator. happy ships it with 11-language i18n, a dedicated notification path, and *argument editing before approval* — which omt does not yet design for. The differentiated claim is: **answer a card from a phone while the user's real interactive TUI is on screen**, which is a consequence of the hook-defer path and which nobody does. |
| A real terminal multiplexer, not a chat UI | **Genuinely differentiated — the strongest** | No product combines a real VT parser, panes/layouts, and a mobile client. The **block model** (OSC 133 segmentation making scrollback a collapsible list on a phone, full terminal one tap away) is the specific unclaimed idea and resolves the raw-PTY-vs-cards split the whole category is stuck on. |
| Multi-instance federation across your own machines | see survey | Real but narrow; state it factually. |
| TUI/API/web parity enforced in CI | see survey | Real; it is a process claim, so demonstrate it rather than assert it. |

**Consequences.**

1. **The defer spike is promoted to the single highest-priority risk.** It is
 not one risk among many: it is the load-bearing assumption for the only
 differentiated part of the question-card feature. If `permissionDecision:
 "defer"` does not park a tool call long enough for a phone round-trip, that
 feature degrades to a worse copy of an existing free product. It runs before
 anything is built on it, and [D8](#d8--two-session-modes-pty-default-and-native-acp)'s
 `native` mode is the designed fallback (ACP's `session/request_permission` is
 verified, blocking, and has no timeout).
2. **The block model is promoted from a terminal-core feature to a product
 differentiator**, and is scheduled accordingly — with eyes open that a
 correct VT parser with grid, scrollback and reflow is the most expensive item
 on the roadmap and competes with iTerm2 and other terminals on their own ground.
3. **Add argument editing before approval** to the interaction model. A
 competitor has it, users value it, and omt's `Interaction` shape already
 carries the tool input — it is a small addition that closes a real gap.
 (It stays inside [D1](#d1--omt-adds-no-policy-layer-over-an-agents-permission-semantics):
 editing an argument is answering the agent's own prompt with a modified
 input, exactly as the agent's own UI allows, not omt adding a policy.)
4. Marketing copy and the README are held to this table.

---

## D10 — Platform targets: macOS and Linux; Windows via WSL2

**Decision.** v1 targets macOS and Linux natively. Windows users are supported
**through WSL2**, which is Linux and therefore costs nothing extra. Native
Windows (ConPTY, named-pipe hook transport, Windows process inspection) is
explicitly **not** a v1 target.

**Reasoning.** The observation pipeline — the thing that makes omt omt — is
Unix-shaped end to end: Unix-socket hooks, `/proc` and `sysctl` process
inspection, foreground process groups, `SO_PEERCRED`, and bash/zsh/fish
integration. ConPTY is a different lifecycle model with no `SIGWINCH`, no
process group and no `TIOCSWINSZ`, so a native port is not a porting layer but
a second implementation of the core. Meanwhile several documents had acquired
incidental Windows commitments (`omt-pty` "Unix and Windows",
`ReadDirectoryChangesW`, `%APPDATA%`, Windows Terminal key encodings) that read
as contracts nobody had agreed to.

**Consequences.**
- Those incidental commitments are qualified or removed. Where a design leaves
 a Windows seam open at no cost (a `WatchDriver` trait, a `PtyHandle`
 abstraction), the seam stays — it is cheap and honest.
- `omt-pty` is scoped to Unix for v1. Native Windows becomes a future change
 with its own decision, not an assumed obligation.
- The README states the support matrix plainly, so no one discovers it by
 failing to build.

---

## D11 — omt mirrors the agent's own card; it does not intercept or replace it

**Decision.** When an agent raises a structured interaction, the agent's **own
CLI draws its own card, normally, locally.** omt does not park the call and does
not render a replacement in the pane. omt observes the interaction (via the
`PreToolUse` hook, which fires before the card is drawn and carries the verbatim
payload), mirrors it to every remote surface, and synchronizes the resolution in
whichever direction it happens:

- answered **locally** in the CLI → omt observes the resolution and every remote
 surface updates to "already answered";
- answered **remotely** → omt delivers the answer to the agent, and the CLI's
 own card resolves.

**Reasoning.** This is [P4](01-principles.md#p4--native-semantics-observe-never-re-implement)
taken literally, and it corrects a design that had drifted. The earlier plan
leaned on `permissionDecision: "defer"` to park the tool call for a phone
round-trip — but parking means the CLI never draws its card, so the local user
loses the native experience omt exists to preserve, and omt inherits the job of
drawing an overlay on a live TUI and reconciling it with the agent's redraws.
the instinct here is right: never render a replacement for the agent's own
UI. omt's addition is not a better card, it is *reach*.

**Consequences.**
- **The defer spike is demoted.** It was [D9](#d9--positioning-what-omt-may-and-may-not-claim)'s
 load-bearing unverified assumption; it is now an optional optimization. The
 hook payload alone supplies everything needed to render remotely.
- **The risk moves, it does not vanish.** The remote-answer path now delivers
 the answer through the agent's own card, which for agents with no native
 response channel means synthetic input, governed by
 [D3](#d3--synthetic-input-is-bounded-by-state-dependence-not-by-tool-danger).
 The spike's question changes to: **does Claude Code's `AskUserQuestion` card
 accept a position-independent selection (typing a number or letter), or does
 it require counting arrow keys?** That experiment is cheaper than the defer
 one and its answer is needed either way.
- Where a *native* response channel exists (ACP `session/request_permission`,
 the opencode plugin, Codex app-server), it is used and no synthetic input is
 involved. `native` sessions ([D8](#d8--two-session-modes-pty-default-and-native-acp))
 are unaffected — omt owns the rendering there by construction.
- D9's differentiated claim survives and gets sharper: *answer the agent's own
 card from your phone while the real TUI is on screen, with both sides in
 sync.*
- The local user is never shown an omt-drawn card in `pty` mode. If omt cannot
 mirror an interaction, the remote surface says "needs you" and offers the
 terminal view — the honest degradation already specified in
 [06 §4](06-agent-layer.md).

---

## D12 — No push notifications in v1; open-and-replay instead

**Decision.** omt ships **no push notification backend**. When a client is not
connected, it is not notified. When the user opens a client, it reconnects,
replays what it missed, and presents what needs attention first. The `Notifier`
trait and its call sites are specified and reserved; **zero backends ship in
v1**.

**Reasoning.** A browser cannot be pushed to directly — with the tab closed, the
only path is the browser vendor's relay (FCM for Chrome/Android, APNs for
Safari/iOS). That means the daemon making an outbound connection to a third
party, and it leaks the metadata *"this machine needs its owner, now"* even
though the payload is encrypted. For a tool whose stated position is no cloud,
no telemetry and no required egress
([00 §8](00-overview.md)), making that the default is a contradiction, and
making it optional still requires building and maintaining it. The alternative
users would actually be told to use (self-hosted ntfy on a tailnet) adds an app
and a deployment step to onboarding.

**Consequences.**
- **omt makes no outbound network connections at all.** This becomes a plain,
 checkable property rather than a caveated one.
- **A real capability is given up**, and the docs say so: the "agent blocks, the
 phone buzzes in your pocket" journey is not in v1. Users learn an agent needs
 them when they next open a client.
- **Open-and-replay becomes the load-bearing path and must be excellent.** Cold
 start to a useful screen, the continuity ranking that decides what to show
 first, and complete recovery of everything missed while disconnected are now
 primary features, not conveniences. See
 [`../design/remote-continuity.md`](../design/remote-continuity.md).
- The iOS Web Push reliability experiment is cancelled; that risk is retired.
- **The extension point stays open on purpose.** A future native iOS/Android
 app has a first-party push channel that does not route through a browser
 vendor, and a user or third party can ship a `Notifier` plugin (ntfy,
 Telegram, Bark, webhook) without touching core — per
 [P2](01-principles.md#p2--pluggable-extension-without-modification). Nothing
 in the design may assume notifications never exist.

---

## D13 — Synthetic delivery is a gated transaction, never a bare write

**Decision.** Amends [D11](#d11--omt-mirrors-the-agents-own-card-it-does-not-intercept-or-replace-it).
When a remote resolution is delivered by synthetic input, it is an **atomic,
gated PTY transaction**, never a bare write:

1. acquire the session's writer token;
2. verify **input quiescence** — no human bytes for a quiet period;
3. **re-verify against the freshest source** that the agent is still in the same
 interaction;
4. write the answer as one unit;
5. release.

Any check failing fails the resolve with `conflict` or `precondition_failed`,
visibly, on the surface that attempted it. A partial write is never permitted.

**Reasoning.** D11 moved the delivery channel from the hook's response slot to
the PTY, and in doing so gave up a safety property that was doing real work:
[12 §4.2](12-collaboration.md) noted that `omt-hook` holds exactly one in-flight
response slot per interaction, so *even a bug in the ledger cannot produce two
hook decisions*. A PTY has no such property — it accepts arbitrarily many
writers and has no compare-and-swap. The interaction ledger still serializes
omt's own resolvers against each other, but it **cannot** serialize omt against
the human at the keyboard, because the human is not a capability caller.

The concrete failure: a phone resolves and types `1\r` while the local user
types `2\r`. The streams interleave into `12\r\r` and the agent reads option
"12". Neither side observes a conflict — the ledger records resolved-by-phone,
the agent did something else, and the audit log is false. Silent divergence with
a false record is a worse failure than the two-phase-commit problem D11
removed.

The rule underneath is simple and was previously an unstated assumption:
**synthetic input is only safe into an idle input path.** D13 makes it a
precondition that is checked rather than hoped for.

**Consequences.**
- The local TUI's passthrough bytes into a session with an open interaction must
 pass through the same serialization point; that is the only place the two
 writers can be ordered.
- The writer token is a **lease**, and auto-acquisition must key on "no other
 client has written recently" rather than "exactly one client is attached" —
 the latter never fires in [D4](#d4--single-user-many-devices--with-the-interfaces-left-open-for-many-users)'s
 own primary two-device scenario.
- Interactions are modelled as **observed, not owned**:
 `Open { deliverable: Native | Synthetic { requires_token } | None }`. Remote
 answerability renders from `deliverable`, never from mere openness — so a card
 omt cannot safely answer is never shown as answerable.
- Per-agent coverage for observing a **resolution** (not just for raising one)
 belongs in [06 §7.3](06-agent-layer.md)'s matrix. Where it is absent — a
 denied permission may emit no `PostToolUse` at all — the card must expire
 rather than linger, or a late remote answer lands in whatever the agent is
 doing *now*.
- **`native` mode has no such race**, because ACP supplies a real response
 channel and no synthetic input is involved. This partially inverts
 [D5](#d5--initial-agent-coverage)'s ordering rationale: Claude Code is the
 depth ceiling but also the *least safe* delivery path, so the ACP adapter's
 priority rises.

---

## D14 — Agent sessions get a transcript surface; blocks are for shell work

**Decision.** A `pty` session has **three** surfaces, not two:

| Surface | For | Source |
|---|---|---|
| **Block view** | ordinary shell work | OSC 133 segmentation ([04 §6](04-terminal-core.md)) |
| **Transcript view** | agent sessions | the merged agent event stream ([06](06-agent-layer.md)), available whenever the binding has a tier ≥ Transcript source |
| **Terminal view** | always available | the grid |

The mobile default is chosen by session kind: block view for a shell, transcript
view for an agent, terminal view one tap away from either.

**Reasoning.** The block model does not work for agent sessions, and cannot be
made to. Two independent reasons:

- An alt-screen TUI produces no command boundaries at all — [04 §6.3](04-terminal-core.md)
 correctly suspends segmentation there.
- **Worse, and less obvious: there is no shell in the loop.** An agent CLI is
 the foreground process for the whole session, so no OSC 133 ever arrives —
 whether or not it uses the alternate screen. (Claude Code does use the
 alternate screen for some full-screen views; verified in
 [`spike-card-answering.md` §5](../research/spike-card-answering.md). That only
 adds intervals where segmentation is suspended outright.) So the
 heuristic segmenter runs — and its close conditions are "PTY quiet **and the
 foreground pgid returned to the shell**" or "a real OSC 133 `A`". In a
 long-lived agent session the foreground pgid is the agent for the whole
 session and no `A` ever arrives, so **neither close condition can ever fire**.
 The result is one unbounded block, open forever, whose contents are a
 cursor-addressed redraw stream flattened to lines — incoherent at 40 columns.

So on a phone, the flagship use case would have shown an empty list or one card
of garbage, falling back to a letterboxed 200-column grid — precisely what the
block model existed to fix.

The remedy was already built and merely unexposed: the tier 3/4/5 sources carry
full assistant messages, tool calls and results, and `native` mode's renderer
already displays exactly this. It was reserved for `native` sessions only.

**Consequences.**
- **[D9](#d9--positioning-what-omt-may-and-may-not-claim) is corrected.** The
 honest claim is two sentences, both true: *the block model makes ordinary
 shell work first-class on a phone — genuinely unclaimed; agent sessions are
 made first-class by the observed-transcript view plus interaction cards,
 derived from the agent's own structured sources.* The previous single claim
 was not true for the primary use case.
- The heuristic segmenter must never open a block it cannot close: block view is
 suppressed for a session with an agent binding and no OSC 133, and the UI says
 why. [04 §6.4](04-terminal-core.md) must state that its first close condition
 is unreachable when the foreground process never returns to the shell.
- **State the floor honestly.** A heuristic-tier agent (Aider, Amp, Crush in TUI
 mode — [D5](#d5--initial-agent-coverage) track 4) gets neither blocks nor a
 transcript: only busy/idle/needs-you and a letterboxed grid.
 [06 §7.3](06-agent-layer.md)'s coverage matrix gains a "mobile surface"
 column, so this is visible rather than discovered.

---

## D15 — Five classes of pending intent, each with its own delivery mechanism

**Decision.** Every mutation a client can originate belongs to exactly one of
five classes, and each class has one prescribed mechanism. A capability
declaration names its class; the class determines whether a retry is safe.

| Class | Examples | Mechanism | Retry safe? |
|---|---|---|---|
| **CAS + intent identity** | `interaction.resolve` (omt-side), `config.set` | CAS on version-or-state, plus `(identity, intent_id)` | **Yes** — returns the original result |
| **Append with dedup key** | `agent.queue.enqueue` | client `intent_id` + bounded server dedup cache | **Yes** |
| **Raw byte stream** | `session.write_bytes` | writer `epoch` + consumed-offset `ack`; **never replayed** | **No** — reject loudly |
| **Externally-confirmed intent** | an injected answer, an injected enqueue | at-most-once write, confirm by **observation**, `Undelivered` on timeout | **No — never retry** |
| **LWW free text** | drafts | CAS on `version`, visible loser | Yes |

**Reasoning.** The design specified exactly one intent to a high standard —
interaction resolution — and left the rest without identity, durability, expiry
or a failure state. Worse, [D11](#d11--omt-mirrors-the-agents-own-card-it-does-not-intercept-or-replace-it)
moved the flagship path into a class that **did not previously exist in the
design**.

The fourth class is the discovery. An answer delivered by writing at a UI omt
does not own fits none of the other three: there is no CAS target representing
the agent's card; an `intent_id` cannot dedup it, because the sink is a UI, not
a log — a duplicate is not a duplicate row but a keystroke landing *somewhere
else entirely*; and a consumed-offset ack proves nothing, because the PTY
consumes bytes whether or not the card did. Before D11 this was a corner case
for the synthetic responder. After D11 it is the entire remote-answer path.

**Consequences.**

1. **Split the interaction state machine.** `Submitted { by, response, at }` when
 the bytes are written; `Resolved` **only when omt observes the agent record
 the answer** (hook `PostToolUse`, a transcript entry, a tool result) inside a
 bounded window; `Undelivered { reason, response }` otherwise, with the text
 preserved and surfaced everywhere as *"your answer may not have reached the
 agent — check the terminal"*. Today the ledger `fsync`s `Resolved` and
 asserts a success it cannot verify.
2. **Never retry an injection** — not on reconnect, not on restart, not by any
 actor except a human who can see the screen. A crash between CAS and
 injection goes to `Undelivered`, never to a replay. This also requires
 `Resolving` to carry the **response**, which it currently does not, so today
 a crash cannot even report what was lost.
3. **`agent.queue.enqueue` carries a `BindingId` and requires
 `AgentState::Working`.** Without it, a replayed enqueue against a session
 whose agent has exited lands **in the shell prompt and is executed as a
 command**. [D3](#d3--synthetic-input-is-bounded-by-state-dependence-not-by-tool-danger)
 does not protect here — it governs answers, and "submitting typed text to a
 prompt box" is on its allowed list. The failure is target identity, not state
 inference. Queued mutations also carry `valid_until` and require
 re-confirmation rather than silent replay after a long offline period.
4. **Confirmation signals already exist and must be used.** Claude Code's
 `queue-operation` line *is* an enqueue receipt; a transcript entry *is* an
 answer receipt. The design read them as mirror data and never as delivery
 confirmation.
5. **Stable request identity.** `RequestId` becomes `(DeviceId, monotonic u64)`
 persisted client-side, with a bounded recent-results cache in dispatch that
 replays the stored result on a repeat. Today `RequestId` is *"unique per
 connection"*, so a client whose socket dies mid-call can never learn whether
 the call applied. This one mechanism makes the first two classes work.
6. **Fix the idempotency key.** `(interaction_id, actor, response)` breaks on
 exactly the retry it was written for: a reconnecting device gets a new
 `ActorId`, so its own retry reads as a stranger overriding it. Key on
 `(interaction_id, identity_or_device, intent_id)`.
7. **Reserve `ack: u64` in the terminal frame header before the wire freezes.**
 It was already required for mosh-style predictive echo; it is *also* the only
 safe resumption mechanism for the byte-stream class. Recording both rationales
 so it cannot be value-engineered out as "a v2 feature".
8. **Durable intent log for the omt-managed queue** (`Ordered` class), and a row
 in [21 §6.2](21-data-lifecycle.md)'s loss table. It is memory-only today, so
 every non-Claude agent's queued text dies unrecorded on `kill -9`. Add a CI
 check that every persisted type has a loss-table row — the same trick
 [03 §5](03-capability-catalog.md)'s parity test plays.
9. **A durable attention log**, required by [D12](#d12--no-push-notifications-in-v1-open-and-replay-instead).
 Open-and-replay discovers only *live* state plus a 4 MiB window, so an
 interaction that opened **and went terminal** inside an offline gap appears
 nowhere: the user opens the app to an idle session, never learning their agent
 asked and gave up. Per `(identity, session)`, every interaction reaching a
 terminal state since the actor's read mark, queryable as
 `interaction.list { since_read_mark, include_terminal }` and rendered first on
 reconnect.
10. **Distinguish the failure modes.** `LedgerError`'s `AlreadyResolved`,
 `Cancelled` and `Abandoned` currently collapse onto one `conflict` code, so a
 phone cannot tell "someone else answered" from "the agent gave up". Add a
 discriminating `detail.state` and three renderings.

**The spike this creates.** Whether Claude Code's `AskUserQuestion` card accepts
a **position-independent** selection decides whether the flagship path is on by
default at all: if the card is arrow-key-only, D3 disables it, and D9's headline
claim is off by default in its headline case. This replaces the defer spike as
the highest-priority experiment.

---

## D16 — Remote answering is per-card-type, and the preconditions are empirical

**Decision.** Remote answering of a `pty`-mode agent card is offered **per card
type**, according to what has been verified answerable position-independently.
For Claude Code v2.1.x, verified live
([`spike-card-answering.md`](../research/spike-card-answering.md)):

| Card | Remote answering | Mechanism |
|---|---|---|
| `AskUserQuestion`, single-select | **Yes, fully** | one ASCII digit resolves the option at that absolute index and submits in the same keystroke |
| Tool permission (Bash/Write/Edit/MCP) | **Allow yes; specific deny no** | option 1 is always `Yes`; the list is 2–4 options depending on state the hook payload cannot see, so `No`'s index is not derivable. `Esc` is the only position-independent negative |
| `AskUserQuestion`, multiSelect | **No** | digits toggle, but submitting requires navigating to a Submit row |
| Plan review (`ExitPlanMode`) | **No** | 2–5 conditional options; the last is a text input |

**Reasoning.** The spike confirmed the mechanism is genuinely position-independent
— `r.onChange?.(A.value)` resolves out of the **full** option array by absolute
index, never reading `focusedValue`, proven by sending `↓ ↓ 1` and getting option
1. So [D3](#d3--synthetic-input-is-bounded-by-state-dependence-not-by-tool-danger)
is satisfied and [D9](#d9--positioning-what-omt-may-and-may-not-claim)'s headline
claim is on by default.

But it also showed that answerability is **not uniform across cards**, and that
the constraints are empirical rather than derivable. Offering a blanket "answer
anything remotely" would silently mis-answer three of the four card types.

**Consequences — additional preconditions on [D13](#d13--synthetic-delivery-is-a-gated-transaction-never-a-bare-write)'s gated transaction.**

1. **Never bracket the write.** Claude Code enables `ESC[?2004h`; a digit wrapped
 in `ESC[200~ … ESC[201~` does **nothing** while a bare digit resolves. omt's
 remote-input path bracket-wraps client text — correct for pasted prose — so
 synthetic answers must bypass that path entirely. This is a silent failure
 mode, not a loud one.
2. **One key per write.** Coalesced bytes arriving in a single read are not
 decoded as separate key events (`b"13"` toggled nothing; `b"1"` then `b"3"`
 toggled both). Multi-byte answers are separate ordered writes, still inside
 one token-held transaction.
3. **Check the row numbers.** Numeric selection is disabled exactly when the
 printed `1.` `2.` prefixes are suppressed — both are the same `hideIndexes`
 flag. *"Is a number rendered on the row?"* is therefore a cheap, reliable
 runtime precondition, and omt should use it.
4. **`Esc` is the universal safe negative**; `y`/`n` do nothing on any of these
 cards.

**Also settled:** digits are not in Claude Code's keybinding registry, so a
user's `keybindings.json` cannot rebind them away — the accelerator is stable
against user configuration, though not against a version bump. Version
fragility is handled by [D15](#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism)'s
confirm-by-observation rule, which turns a changed keymap into a visible
`Undelivered` rather than a silent wrong answer.

**Where a card is not remotely answerable**, the surface shows it read-only with
the reason and offers the terminal view — the honest degradation
[06 §4](06-agent-layer.md) already specifies. omt never presents an
unanswerable card as answerable.

---

## D17 — Parity is a floor against unreachability, not a promise of good affordances

**Decision.** The parity test guarantees that **no capability is unreachable on
any surface**. It does not, and cannot, guarantee that a capability is *at hand*
on any surface. Where a user needs something in a hurry, that is a design-review
obligation, not a CI obligation, and the documents say which is which.

**Reasoning.** An audit of the capability surface found that the test's TUI arm
had quietly become vacuous. [16 §3.1](16-input-and-keymap.md) established —
correctly — that the command palette *is* the universal TUI affordance, since
its contents are the catalog; that is what lets omt keep a tiny un-prefixed key
budget instead of inventing a chord for a hundred and fifty operations. But
[03 §5](03-capability-catalog.md) went on claiming a per-capability *binding* was
verified. Read together, "every non-`Admin` capability has a TUI action" reduced
to "the palette exists" for roughly 140 of 150 entries.

The mechanism was fine. The claim was wrong, and a wrong claim about a
correctness gate is worse than a weaker gate honestly described, because it stops
people looking. The audit found exactly what that produces:
`agent.interrupt` — the flagship stop action — had only a swipe gesture on the
web, in a document that had itself written the rule *"no gesture is the only way
to reach a capability"*; and `media.image.upload` and `session.search` were
declared, type-checked into requiring a web handler, and had no interface
designed anywhere.

**Consequences.**
- [03 §5.1](03-capability-catalog.md#51-what-artifact-2-actually-asserts-and-what-it-does-not)
 states what artifact 2 asserts: reachability, the reverse direction (every
 bound action names a real capability — the half that catches drift), and the
 rule that a `hidden` capability has no palette entry and therefore needs a real
 binding or a surface exemption.
- **Surface-local UI verbs do not belong in the catalog.** A capability is a
 cross-surface operation. `tui.zoom_font` was already demoted to a client
 preference on this reasoning; the rest of the `tui.*` family follows, so that
 no surface is obliged to implement another surface's metaphor.
- **Two things the test cannot see are called out where they occur**: per-agent
 degradation ([06 §7.3](06-agent-layer.md)'s matrix, which is per-agent while
 the test is per-capability) and per-card answerability
 ([D16](#d16--remote-answering-is-per-card-type-and-the-preconditions-are-empirical),
 where one `Parity::Full` capability succeeds for one card type and not another).
 Both are honest designs; neither is checkable by enumerating the registry.
- Review asks a question CI cannot: *would a user reach for this under pressure,
 and can they?*
