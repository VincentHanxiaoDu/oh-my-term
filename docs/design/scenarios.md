# Scenarios and Requirements Catalogue

This document is the **user-facing checklist** the architecture is validated
against. It enumerates who uses `omt`, what they do minute-to-minute, and — most
importantly — **what they will need that the current architecture does not
describe**.

It is deliberately not flattering. Part 3 is the point of the document; Parts 1
and 2 exist to make Part 3 credible. Where a need has no owning architecture
document, it is tagged **NO OWNER** and repeated in Part 4.

Reference points used throughout:
[00 overview](../architecture/00-overview.md),
[01 principles](../architecture/01-principles.md),
[decisions](../architecture/decisions.md),
[03 catalog](../architecture/03-capability-catalog.md),
[04 terminal](../architecture/04-terminal-core.md),
[05 session](../architecture/05-session-model.md),
[06 agent](../architecture/06-agent-layer.md),
[07 remote](../architecture/07-remote-protocol.md),
[08 web](../architecture/08-web-client.md),
[09 ssh/media](../architecture/09-ssh-and-media.md),
[10 config](../architecture/10-configuration.md),
[11 plugins](../architecture/11-plugins.md),
[12 collaboration](../architecture/12-collaboration.md),
[13 security](../architecture/13-security.md),
[15 explorer](../architecture/15-workspace-explorer.md),
[16 input](../architecture/16-input-and-keymap.md),
[18 semantic open](../architecture/18-semantic-open.md).

---

# Part 1 — Personas

Seven personas. They are chosen so that their *needs conflict*: satisfying P2
(the purist) and P5 (the beginner) with the same defaults is the hardest single
design problem in the product, and the conflicts are called out explicitly.

---

## P1 — Mara, the parallel agent driver

**Who.** Senior engineer on a 400k-line monorepo. Runs 3–5 Claude Code sessions
at once, one per git worktree, plus a dev server, plus a shell. Has been doing
this in tmux with a hand-rolled `~/bin/agents` script for six months.

**Today.** `tmux new -s work`, four windows, `git worktree add ../repo-featX`,
one `claude` per worktree. Tabs through windows every couple of minutes to check
whether anything is blocked. Misses a permission prompt for eight minutes,
regularly. Uses `tmux display-message` hacks and a terminal-bell-to-Slack script
that half works.

**Wants from omt.** One screen that says *which of my five agents needs me right
now*. Zero-latency switching between them. To answer a question without losing
her place. To queue "then run the migration" while an agent is mid-turn. To know
which worktree an agent is in without reading the prompt. To see that agent #3
has been "working" for 22 minutes and is probably stuck in a retry loop.

**Uninstalls if.** Switching panes has perceptible lag. omt's state display is
ever *wrong* — a pane shown idle that is actually blocked destroys the only
reason she installed it. A daemon restart loses her five running agents (see
[05 §8](../architecture/05-session-model.md#8-persistence-and-restore): it
does). omt's keybindings eat something her agent CLI needs.

**Conflicts with.** P2 (she wants a status bar, a dashboard, chrome; he wants
none of it).

---

## P2 — Kenji, the terminal purist

**Who.** 15 years of vim + tmux + zsh. Runs `nvim` full-screen most of the day.
Uses an agent CLI occasionally, and does not want it to become the center of his
tool. Has strong opinions about latency and about programs that redraw when he
did not ask them to.

**Today.** tmux with `set -g prefix C-a`, a 1-line status bar he wrote himself,
`vim-tmux-navigator` bound to `C-h/j/k/l` **without a prefix**. Copy mode is
vi-mode and he uses it constantly.

**Wants from omt.** Nothing, ideally. He wants it to be a strictly better tmux:
faster, correct reflow, real 24-bit color, `Ctrl+B` he can rebind to `Ctrl+A`,
detach/attach that works, and no visual noise. He will grudgingly enjoy the
agent stuff once a month.

**Uninstalls if.** omt intercepts a key `nvim` needs and it takes more than one
minute to find out why (16 §5.3 `omt keys explain` is the right answer and must
exist on day one). Any perceptible input latency versus a bare terminal. A
status bar he cannot turn off. A "welcome to omt!" screen on second launch. His
tmux config cannot be approximated. Session survival across `omt` upgrades is
worse than tmux's (**it currently is** — see gap G12).

**Conflicts with.** P5 (Sam needs discoverability, Kenji needs invisibility) and
P1 (dashboard).

---

## P3 — Ana, the away-from-desk operator

**Who.** Staff engineer, two young kids, works in 25-minute blocks. Starts a
long agent task, walks away, resumes from her phone on the couch or on a walk.

**Today.** Starts `claude`, leaves, comes back 40 minutes later to find it had
been blocked on `AskUserQuestion` for 38 of them. Has tried Blink + tmux on the
phone; the agent's TUI at 40 columns is unusable; arrow-keying a selection box
on a touchscreen is worse.

**Wants from omt.** To find out an agent is blocked the moment she opens her
phone — [D12](../architecture/decisions.md#d12--no-push-notifications-in-v1-open-and-replay-instead)
ships no notification backend, so discovery is open-and-replay, and the cost of
that is that she is not interrupted, only ready. A card on
the phone with real tappable options and readable descriptions. Free-text
answers with voice dictation while walking. To see, in one glance on waking, a
digest of *what the agent did in the last hour*, not just what it is doing now.
To be confident that tapping "Yes, edit the file" from a phone did the same
thing as pressing Enter at the desk.

**Uninstalls if.** She opens the tab and the card is gone/expired/already
answered without explanation. A tap resolves the wrong option (this is exactly
what D3 exists to prevent, and the fear is rational). She answers on the phone,
walks to the laptop, and the two disagree. Opening is slow, or the replay misses
an interaction that opened and went terminal while she was away — the two
mandatory refetches
([07 §8.2](../architecture/07-remote-protocol.md#82-what-replaces-it-open-and-replay))
exist precisely to prevent that.
Her terminal output — with an `.env` echoed in it — turns out to be sitting
unencrypted in a state directory synced to iCloud (gap G9).

---

## P4 — Dmitri, the SSH-native SRE

**Who.** Operates 30+ machines. Lives in `ssh box && tmux attach`. Runs agents
on the *remote* boxes, not locally, because that is where the data is. Frequently
on a hotel/aeroplane connection.

**Today.** `mosh` where he can, `tmux -CC` on iTerm2 where he can't. Copying a
stack trace out of a remote box to a local Jira ticket takes four steps. Opening
a remote file in his local editor means `scp` or `sshfs`.

**Wants from omt.** `omt --remote prod-3` and everything works: clipboard both
ways, `file:line` in a stack trace opens in his *local* editor
([18 §6](../architecture/18-semantic-open.md#6-the-omt-ssh-remote-flow)),
screenshots paste to an agent running remotely
([09 §5](../architecture/09-ssh-and-media.md#5-case-b-image-paste-over-ssh--the-core-mechanism)).
A single phone/laptop view federating all 30 boxes
([07 §1](../architecture/07-remote-protocol.md#1-topology-and-federation)).
Installation on a box that has no outbound internet (gap G16).

**Uninstalls if.** He has to install omt on all 30 boxes and keep the versions
in sync manually (07 §1.5 handles protocol skew but not *deployment*). It
breaks under 400 ms RTT and 2% packet loss. It cannot be run as a system service
that survives his logout (gap G13). Anything about it needs a browser to
administer.

---

## P5 — Sam, the beginner

**Who.** Two years out of a bootcamp. Uses the macOS Terminal and VS Code's
integrated terminal. Has never used tmux; tried once, could not figure out how
to get out. Uses Claude Code daily.

**Today.** One terminal tab per thing. Loses work when the tab closes. Does not
know panes exist.

**Wants from omt.** A nicer terminal that stops losing his work. Splitting panes
without learning a prefix key. To *discover* that omt can answer questions on
his phone — he will never read a docs site to find out. To not be punished for
mis-typing a config key.

**Uninstalls if.** First launch shows a black screen with no indication that
anything is different from his old terminal. He hits `Ctrl+B` by accident and
enters a mode he cannot escape. He edits `config.toml`, makes a typo, and omt
refuses to start (10 §6.4 must guarantee it does not). There is no in-app help.

---

## P6 — Priya, the tech lead

**Who.** Leads six engineers. Reviews a lot. Pairs remotely. Wants visibility
into what agents are doing across the team without becoming a bottleneck.

**Today.** Screen-shares over Zoom to debug with someone. Copy-pastes agent
transcripts into Slack threads.

**Wants from omt.** To send a colleague a read-only link to a running session
that expires ([13 §3.2](../architecture/13-security.md#32-invite-links-the-primary-onboarding-path)).
To revoke it instantly. To hand over the keyboard mid-session and take it back
([12 §3](../architecture/12-collaboration.md#3-the-writer-token)). To leave a
note attached to a session for the person taking over (gap G7). To answer a
question card on someone else's behalf when they're asleep, with attribution.

**Uninstalls if.** Sharing means her whole machine is exposed — a link scoped to
one session must be scoped to one session, not to the instance. Revocation is
not immediate. The audit log cannot tell her who approved a given tool call
(12 §8 does record this — good).

---

## P7 — the automation user (CI, scripts, cron)

**Who.** Not a person: a `Makefile`, a GitHub Action, a nightly cron, a Raycast
script. Increasingly, *another agent*.

**Today.** `tmux send-keys` and `tmux capture-pane` in shell scripts. Fragile,
but universally available and scriptable.

**Wants from omt.** `omt session create --workspace ~/repo --command claude
--json`, stable exit codes, `--json` on every read, a way to *wait* for a
session to go idle or for an interaction to open, and to resolve interactions
non-interactively. Auth from an environment variable, never a browser.

**Uninstalls if.** Output is only ever human-formatted. The CLI requires a TTY.
There is no way to script against a remote instance without an interactive
invite flow. Exit codes do not distinguish "agent failed" from "omt failed".

---

# Part 2 — Journeys

Format: **Trigger → Steps → What the system shows → Success → Failure modes.**
Keys are literal. `⟨leader⟩` is `Ctrl+B` by default
([16 §3.2](../architecture/16-input-and-keymap.md#32-default-leader-ctrlb)).

---

### J1 — First run, ever (Sam)

- **Trigger.** `brew install omt`, then `omt`.
- **Steps.** Type `omt` ⏎.
- **Shows.** A terminal. Bottom line: a one-line status bar showing
  `omt · ~/code/app · main · ⟨Ctrl+B⟩ ?` where `?` is literally the help hint.
  Above the prompt, a single dismissible line: *"Shell integration not
  installed — blocks, history and `file:line` clicking are limited. Press
  ⟨leader⟩ i to install."*
- **Success.** Sam types `ls` and it works exactly like his old terminal. He
  presses `Ctrl+B ?` within the first session and sees a searchable key list.
- **Failure.** No hint at all (he never learns anything). A modal wizard (Kenji
  uninstalls). The hint reappears every launch after dismissal.
- **Coverage.** **NO OWNER.** 10 covers config, 04 §7.1 covers what the shell
  integration *is*, 06 §7.1 covers the *agent* integration installer. Nothing
  owns first-run experience, the status bar, or `⟨leader⟩ ?`. See G1.

### J2 — Discovering a feature without reading docs

- **Trigger.** Sam wonders if omt can split panes.
- **Steps.** `Ctrl+B` then pause 500 ms.
- **Shows.** The pending-leader hint overlay
  ([16 §3.3](../architecture/16-input-and-keymap.md#33-pending-leader-ui)): a
  grouped list of the leader namespace with descriptions.
- **Success.** He presses `%`, gets a vertical split, and never opened a
  browser.
- **Failure.** The overlay lists 60 bindings unfiltered. The overlay appears
  instantly and flickers for expert users (must be delayed and must be
  suppressible).
- **Coverage.** 16 §3.3, 16 §6.10. Good.

### J3 — Command palette as the discovery path

- **Trigger.** "There must be a way to do X."
- **Steps.** `Ctrl+B` `p` (or `Ctrl+Shift+P` where deliverable).
- **Shows.** A fuzzy list of **every capability in the catalog** with its
  description, keybinding, and effects glyphs — generated, not hand-maintained
  ([03](../architecture/03-capability-catalog.md), 16 §3.5).
- **Success.** Every feature in the product is reachable by typing a word.
- **Failure.** The palette lists internal capabilities (`session.__replay`) with
  no human name. Capability declarations need a `hidden` flag and a mandatory
  human-readable summary; verify 03 has one.

### J4 — Migrating from tmux (Kenji) — the adoption barrier

- **Trigger.** Kenji tries omt on a Saturday.
- **Steps.** `omt`; immediately `Ctrl+A c` (his prefix) — nothing happens.
  He runs `omt migrate tmux`.
- **Shows.** A report: *"Read `~/.tmux.conf` (74 lines). Mapped 41 settings.
  Wrote `~/.config/omt/keybindings.toml` and `config.toml`. 9 settings have no
  omt equivalent, listed below with reasons. 4 need review."* Including:
  `prefix C-a` → `leader = "ctrl+a"`; `mode-keys vi` → `copy_mode.keymap =
  "vim"`; `set -sg escape-time 0`; `bind -n C-h select-pane -L` → an
  **unprefixed** binding, with a conflict warning that `nvim` also uses `C-h`
  and a pointer to 16 §5.
- **Success.** His muscle memory works within ten minutes. Unmappable settings
  are named, not silently dropped.
- **Failure.** No migration path at all — he closes it in five minutes. A
  migration that silently drops `bind -n` bindings, which are the ones he cares
  about most.
- **Coverage.** **NO OWNER.** 16 defines the keymap format and 10 the config;
  neither owns tmux/zellij import. See G6.

### J5 — Coexisting with a tmux he refuses to give up

- **Trigger.** Kenji's team's deploy runbook is `tmux attach -t deploy`.
- **Steps.** Inside an omt pane he runs `tmux attach`.
- **Shows.** The tmux session, working. omt's status bar shows the pane as
  `nested: tmux` and the agent binding for that pane goes `Unknown` with a
  tooltip *"observation unavailable inside a nested multiplexer"*
  (06 §9 states this; it must be *visible*, not only documented).
- **Success.** Terminal fidelity is perfect. omt's leader does not collide:
  omt's `Ctrl+B` is consumed by omt, so Kenji rebinds omt's leader to `Ctrl+A`
  or uses `⟨leader⟩ ⟨leader⟩` passthrough (16 §3.4 escape hatches).
- **Failure.** Double-prefix hell with no documented resolution. omt claims an
  agent state inside the tmux (it must not — 06 caps this correctly).
- **Coverage.** 16 §3.4, 06 §9, 04 §1.5. Adequate but the *visible* nested
  indicator is unowned.

### J6 — Starting an agent

- **Trigger.** Mara wants an agent on a new feature.
- **Steps.** `⟨leader⟩ w` → workspace picker → `../repo-featX` →
  `⟨leader⟩ a` → agent picker (`claude`, `codex`, `opencode` — from
  `[agents]` config) → ⏎.
- **Shows.** A new session, `claude` starting in a real PTY, session card in the
  dashboard flips to `starting → idle` within ~2 s. The session chip shows
  `claude · sonnet-4.5 · featX · permissionMode: default`.
- **Success.** It is the real CLI, with its real config and its real slash
  commands. The agent binding is established at tier ≥3 within 2 s.
- **Failure.** Hooks were never installed, so it silently drops to tier 0 and
  Mara does not know. The **degradation must be visible on the session card**,
  with a one-key fix (06 §7.1 installer). `agent.explain` (06 §4) covers the
  diagnosis; surfacing it prominently is the part at risk.

### J7 — Agent blocks while the user is looking at that pane

- **Trigger.** `AskUserQuestion` fires.
- **Steps.** Nothing — Mara is watching.
- **Shows.** The agent's own TUI card, unmodified (P4). omt additionally paints
  the pane border in the `needs-you` color and updates the status bar. omt does
  **not** overlay its own card on top of the agent's — that would double the UI.
- **Success.** She arrow-keys and hits ⏎ in the agent, exactly as before. omt
  observes the resolution via hook/transcript and closes its `Interaction`
  ledger entry as `ResolvedExternally`.
- **Failure.** omt renders a competing card and the two disagree.
  **Resolved by [D11](../architecture/decisions.md#d11--omt-mirrors-the-agents-own-card-it-does-not-intercept-or-replace-it):**
  omt does not park the tool call and never draws a card in the pane, so the
  agent's own card always renders at the desk exactly as it would without omt.
  The remaining hazard is not a competing card but a competing *writer* — her
  keystrokes racing an injected answer — which is
  [D13](../architecture/decisions.md#d13--synthetic-delivery-is-a-gated-transaction-never-a-bare-write)'s
  gated transaction and 12 §4.5's conflict case.

### J8 — Agent blocks while the user is in another pane

- **Trigger.** Same, Mara is in pane 3.
- **Steps.** She notices the dashboard indicator; `⟨leader⟩ n` = jump to next
  session needing attention.
- **Shows.** Status bar: `⚠ 1 needs you · claude/featX`. Optional bell/OSC 9.
- **Success.** `⟨leader⟩ n` is a single key away and cycles by urgency, oldest
  first.
- **Failure.** No cross-pane attention affordance except reading a bar. There
  must be a jump-to-next-blocked binding. Not currently in 16 §8.2's namespace —
  verify.

### J9 — Agent blocks while the user is away entirely

- **Trigger.** Ana is on a walk. No client attached.
- **Steps.** Nothing buzzes — no notification backend ships in v1
  ([07 §8](../architecture/07-remote-protocol.md#8-notifications-to-a-closed-tab--none-in-v1)).
  She learns of it when she next opens a client.
- **Shows.** Opening the PWA → authenticated WS → resync → replay → the ranked
  list puts the blocked session first, with the card
  ([07 §8.2](../architecture/07-remote-protocol.md#82-what-replaces-it-open-and-replay)).
- **Success.** Under 10 s from opening to a readable card with tappable options.
- **Failure.** The interaction timed out
  (12 §4.3) before she got there — the card must then show *"this expired and
  the agent proceeded with its default"* rather than vanishing. C7 in 12 covers
  "interaction while nobody attached"; the *user-facing expiry explanation* is
  weakly specified.

### J10 — Answering a question card from the TUI

- **Steps.** `⟨leader⟩ n` to the blocked session; the card owns the keyboard
  (16 §4.4); `j`/`k` or `1`/`2`/`3`; ⏎.
- **Shows.** Selection, then a resolution line: `resolved by you (this device) ·
  native hook`.
- **Success.** Fidelity is `Native`, not `Synthetic` (D3). The card closes for
  every attached client simultaneously.
- **Failure.** Card keyboard ownership conflicts with an inner vim mode
  (16 §4.4 handles). Two options with identical labels.

### J11 — Answering from desktop web

- **Steps.** Browser tab, dashboard, click the option button inline in the row
  ([08 §6](../architecture/08-web-client.md#6-agent-session-dashboard)).
- **Shows.** Optimistic selection, then confirmed by the event
  (12 §7). Row moves out of "Needs you".
- **Success.** Answering without opening the session — explicitly the design
  intent, and correct.
- **Failure.** Optimistic UI shows success and the server rejects because
  someone else won the race (12 §4.2). The correction must be *visible*
  (12 §7.3), not a silent revert.

### J12 — Answering from a phone

- **Steps.** Open the PWA → the ranked list surfaces the blocked session → card
  sheet → thumb-reachable option buttons at the bottom third (08 §8.1) → tap.
- **Shows.** Question header, full option descriptions (the thing the terminal
  card truncates), agent + workspace + branch chips, permission mode.
- **Success.** No horizontal scrolling, no pinch-zoom, no 40-column terminal.
- **Failure.** Multi-select questions rendered as single-select. Option
  descriptions truncated to one line — on a phone there is room, and this is
  where omt beats the native TUI; do not waste it.

### J13 — Answering with free text

- **Trigger.** None of the offered options is right.
- **Steps.** Tap **"Something else…"** → keyboard → type → Send.
- **Shows.** A text field with the original question above it.
- **Success.** The free-text answer is delivered through the same native
  channel. For Claude Code's `AskUserQuestion` this requires knowing whether the
  hook response schema accepts an arbitrary string or only a listed option.
- **Failure.** **This is an open question the design does not answer.** If free
  text is not natively expressible, the only path is synthetic PTY input, which
  D3 permits (free text is position-independent) but which must be labelled. 06
  §5.0 `InteractionResponse` needs an explicit `Freeform(String)` variant with a
  per-adapter declaration of whether it is supported. See G21.

### J14 — Adding a comment rather than picking an option

- **Trigger.** Ana wants to answer "option 2, but use the existing helper".
- **Steps.** Long-press option 2 → "Answer with note" → type → Send.
- **Shows.** Option 2 selected + a note field.
- **Success.** The note reaches the agent. For most agents this means: resolve
  with option 2 natively, **then** enqueue the note as the next user message
  (06 §8 message queue), presented to the user as one action.
- **Failure.** The note is silently dropped because the hook schema has no slot
  for it. The compose-then-queue behaviour is not described anywhere. See G21.

### J15 — Changing your mind after answering

- **Trigger.** Ana taps "Yes" and immediately realises she meant "No".
- **Steps.** She looks for undo.
- **Shows.** An **Undo** affordance is only honest while the answer has not been
  written — the `Resolving` window between the CAS and the injection, which is
  milliseconds under [D11](../architecture/decisions.md#d11--omt-mirrors-the-agents-own-card-it-does-not-intercept-or-replace-it)
  because nothing is parked. Keystrokes already typed cannot be retracted.
- **Success.** omt does not offer a fake undo. Once the state is `Submitted` it
  says plainly *"already sent to the agent — press Esc/interrupt to stop it"* and
  offers `agent.interrupt` as the honest next action. R18's retraction window is
  re-scoped accordingly.
- **Failure.** 12 §4.1 makes interactions resolve-once and irreversible with no
  grace window. That is correct for consistency and **wrong for a phone**, where
  fat-fingering is the norm. A short, explicitly-modelled *pending* state before
  commit is needed. See G8.

### J16 — Queueing follow-up work mid-turn

- **Steps.** Session is `working`. In the dashboard row, `+ add to queue…` →
  "run the migration after this" → ⏎.
- **Shows.** `Queue (1)` under the row (08 §6.1).
- **Success.** Works from phone, TUI and `omt agent queue enqueue`.
- **Failure.** For a non-Claude agent, omt falls back to an omt-managed queue
  flushed on idle. It must be *labelled* omt-managed and must state what happens
  if the agent never goes idle or the session dies with items pending. The
  latter is unspecified — pending items should be persisted and surfaced, never
  silently lost.

### J17 — Interrupting an agent

- **Steps.** `⟨leader⟩ Esc`, or swipe-left on the dashboard row, or
  `omt agent interrupt <session>`.
- **Shows.** The agent's own interrupt behaviour (Claude Code: Esc). omt sends
  the agent-native interrupt where one exists, else `Ctrl+C` to the PTY tagged
  `Synthetic`.
- **Success.** Identical result on all three surfaces.
- **Failure.** Double-Esc semantics differ per agent; a phone user cannot
  express "Esc twice". Per-adapter interrupt modelling is needed
  (06 §7 adapter trait) — verify it exists.

### J18 — Resuming after `/compact`

- **Trigger.** Context window fills; Claude Code compacts.
- **Shows.** A visible marker in omt's timeline: `context compacted · 187k → 42k
  tokens`, from the normalized `Usage`/compaction `AgentEvent` (06 §8).
- **Success.** The user understands why the agent "forgot". Scrollback is
  untouched — omt never rewrites output (P4).
- **Failure.** Compaction is invisible and the user thinks omt lost state.

### J19 — Running several agents across worktrees

- **Steps.** `omt launch review` — a launch configuration
  ([10 §9.1](../architecture/10-configuration.md#91-launch-configurations))
  creating 3 worktrees + 3 agent sessions + 1 shell in a saved layout.
- **Shows.** A 2×2 layout, each pane titled `branch · agent · state`.
- **Success.** Reproducible multi-agent setup in one command; the tmuxinator
  replacement.
- **Failure.** The launch config cannot *create* git worktrees, because 15 §1.1
  forbids VCS mutation in v1. So `omt launch` must be able to run arbitrary
  setup commands — confirm 10 §9.1 supports a pre-command hook. If it does not,
  this journey fails.

### J20 — Comparing what three agents produced

- **Trigger.** Three agents finished the same task differently.
- **Steps.** Dashboard → select 3 sessions → **Compare**.
- **Shows.** Ideally: side-by-side changed-file sets, diff stats per branch, and
  each agent's final summary.
- **Success.** Decide in two minutes which branch to keep.
- **Failure.** **Not designed.** 15 §8.3 gives "files the agent changed this
  session" per session; there is no multi-session comparison view. A minimum
  honest answer is a diff-stat table across selected sessions. See G18.

### J21 — Noticing an agent is stuck

- **Trigger.** Session has been `working` for 22 minutes.
- **Shows.** Dashboard "Working" section sorted by elapsed, with an amber chip
  past a configurable threshold, and the current tool call + repeat count
  (`Bash(npm test) ×7`).
- **Success.** Loop detection by *repetition of identical tool calls*, which is
  observed data, not a guess.
- **Failure.** No such affordance exists in 08 §6 beyond elapsed time. Repeat
  counting is cheap and high-value. See G19.

### J22 — Working over SSH with the thin client

- **Steps.** `omt --remote prod-3` from the laptop.
- **Shows.** A local omt TUI driving the remote instance over an SSH stdio
  transport (07 §2.4). Local clipboard, local editor, local screenshot paste.
- **Success.** Feels local. `⟨leader⟩` handled locally; everything else
  forwarded (16 §7.1).
- **Failure.** omt is not installed remotely. There must be a `omt --remote
  --bootstrap` that offers to scp a static binary, or a precise error naming the
  install command. Unspecified.

### J23 — Pasting a screenshot to a remote agent

- **Steps.** Screenshot on macOS → focus omt pane → `Cmd+V`.
- **Shows.** `📎 screenshot-2026-08-03.png (412 KB) → /tmp/omt-blob/…` inserted
  into the agent prompt as a path.
- **Success.** Tier 1 reverse socket (09 §5.2) when using `omt --remote`.
- **Failure.** Plain `ssh` in Ghostty → tier 4: QR code / `omt paste` with a
  named reason (D7). The failure message quality *is* the feature here.

### J24 — Opening a remote file locally from a stack trace

- **Steps.** Test fails; `src/api/handler.rs:214:9` in output. `⟨leader⟩ f` →
  hint labels → press `a`.
- **Shows.** The file opens in the local editor at line 214, fetched to a
  mirrored path (18 §6.3).
- **Success.** No `scp`, no path guessing; cwd resolved from OSC 133/7
  (18 §3.1).
- **Failure.** The snapshot problem — he edits the local copy and it is not the
  remote file (18 §6.5 recommends read-only-by-default). The read-only default
  must be loud, or he loses work.

### J25 — Copying remote output to the local clipboard

- **Steps.** `⟨leader⟩ [` copy mode → `v` select → `y`. Or: focus a block and
  press `y` (block-level copy, no selection needed).
- **Shows.** `copied 1.2 KB to local clipboard`.
- **Success.** Works over `omt --remote` (09 §3.1) and in a foreign terminal via
  OSC 52 where supported, with a size-limit fallback (09 §3.3).
- **Failure.** OSC 52 truncation at ~74 KB in some terminals, silently. Must be
  detected and reported.

### J26 — Reviewing a diff before approving a permission card

- **Trigger.** Agent asks to edit `Cargo.toml`.
- **Steps.** Card shows the tool + arguments; tap **View diff**.
- **Shows.** Inline diff inside the permission card (15 §8.5), word-level on
  desktop, unified with a `→` line-wrap on mobile (15 §7.4).
- **Success.** Approve with the actual change in front of you, on a phone.
- **Failure.** Very large diffs. Needs a cap with "open full diff" escape.

### J27 — Jumping from a test failure to the source line

- **Steps.** `cargo test` fails → `⟨leader⟩ f` → hint → ⏎.
- **Shows.** Opens in `$EDITOR` at the line, or in the omt file viewer.
- **Success.** 18 covers this thoroughly.
- **Failure.** Rust's `-->` prefix, Python tracebacks, Jest's paths and Go's
  `file.go:12` all need rules; 18 §2.7 has a default table — confirm it covers
  the top ~10 toolchains.

### J28 — Phone loses network mid-answer

- **Trigger.** Subway.
- **Shows.** A connection chip goes amber → red. The card stays rendered,
  greyed, with `will send when reconnected` or `cannot send offline` — one of
  the two, never ambiguous.
- **Success.** On reconnect, resume by sequence number (07 §5); if the
  interaction was resolved by someone else meanwhile, an explicit
  *"already answered by laptop"* message.
- **Failure.** Queued writes replayed blindly after 20 minutes offline — a
  stale approval sent into a changed world. **Queued mutations must expire.**
  08 §8.5 covers reconnect; mutation staleness is unspecified. See G8.

### J29 — Laptop sleeps and wakes

- **Trigger.** Lid closed 3 hours.
- **Shows.** On wake, the TUI reattaches; a banner if the replay window was
  exceeded (07 §5.2) saying *"3h of output was not replayed; scrollback is
  intact"*.
- **Success.** Sessions and agents kept running (the daemon never stopped).
- **Failure.** Silent full-screen redraw hiding the fact that output was
  dropped. Agents that themselves died on sleep (network-dependent CLIs) must be
  distinguishable from omt's own gap.

### J30 — The daemon restarts

- **Trigger.** `omt upgrade`, or a crash.
- **Shows.** Sessions return as `Orphaned`; scrollback and history intact; each
  offers **restart** re-spawning the same argv/cwd/env (05 §8.2).
- **Success.** Nothing is lost except the processes.
- **Failure.** **The processes are lost.** For P1 and P4 this is the difference
  between omt and tmux, and tmux wins. An `omt upgrade` that kills five running
  agents mid-turn is close to disqualifying. See G12.

### J31 — The machine reboots

- **Shows.** After login, `omt` shows the previous workspaces and their
  orphaned sessions with full history, and a **restore layout** action.
- **Success.** Layout + history + block log survive.
- **Failure.** The daemon does not autostart, so a phone attaching after a
  reboot finds nothing. There is no launchd/systemd unit story. See G13.

### J32 — Sharing a read-only view with a colleague

- **Steps.** `⟨leader⟩ s` on a session → **Share read-only** → expiry (1 h) →
  copy link, or `omt share create --session s_4b2f --role viewer --ttl 1h`.
- **Shows.** A signed invite URL scoped to that session.
- **Success.** The colleague sees live output and cards, cannot type, cannot see
  other sessions or other workspaces. Presence shows him watching (12 §2).
- **Failure.** **Credential scope is per-workspace in 13 §4.1, not
  per-session.** A single-session share is exactly the common case and must be
  expressible. Verify; if absent, it is a gap.

### J33 — Revoking the share

- **Steps.** `⟨leader⟩ s` → active shares list → **Revoke**, or
  `omt auth revoke <cred-id>`.
- **Shows.** The colleague's client disconnects within one second with a clear
  message.
- **Success.** Revocation is immediate on the live socket, not just at next
  auth.
- **Failure.** Revocation only takes effect on token expiry. 13 §5.2 covers
  rotation; live-socket termination on revoke must be explicit.

### J34 — Voice input while walking

- **Steps.** In the phone composer, press-and-hold the mic → speak → release.
- **Shows.** Live partial transcript; on release, an editable final transcript
  with **Send** and **Re-record** (08 §7.2). Never auto-send.
- **Success.** Hands-busy answering of a free-text question.
- **Failure.** Street noise → garbage. The always-editable transcript is the
  right mitigation. Code identifiers dictate badly (`snake_case`,
  `Vec<Option<T>>`) — a per-workspace vocabulary hint from the file index /
  recent identifiers would help materially and is not designed.

### J35 — Voice in a noisy office / dictation errors

- **Steps.** Same; the transcript reads "add a unit test for the reset
  handler" as "add a unit test for the recept handler".
- **Success.** He edits the one word and sends. Because omt never auto-sends,
  the error costs 3 seconds.
- **Failure.** An STT provider outage. Must degrade to the keyboard with a
  message, and the BYOK key must never be shipped to the browser (08 §7.3 —
  confirm the daemon proxies STT rather than exposing the key client-side).

### J36 — Configuring omt three ways

- **(a) File.** `$EDITOR ~/.config/omt/config.toml`; save; omt live-reloads
  (10 §6) and shows `config reloaded · 3 keys changed`.
- **(b) TUI.** `⟨leader⟩ ,` → generated settings editor from the schema
  (10 §4.1) with descriptions and validation as you type.
- **(c) Web.** Settings page, same schema, same validation, writing to the same
  file with the same layering (10 §2.3).
- **Success.** All three produce byte-identical results, and (b)/(c) preserve
  comments and formatting in the TOML they rewrite.
- **Failure.** Comment-destroying round-trips through a TOML writer. 10 §2.3
  must commit to a format-preserving edit (`toml_edit`); confirm it does.

### J37 — Making a config mistake and recovering

- **Steps.** He sets `terminal.scrollback_lines = "lots"`.
- **Shows.** On save: a diagnostic with file, line, column, the value, the
  expected type, and a suggestion (`OMT-C1xx`), and **omt keeps running on the
  last good config** (10 §6.4).
- **Success.** He can never brick omt from the config file. `omt config check`
  and `omt config reset <key>` exist.
- **Failure.** A config error at *startup* (not reload) with no prior good
  config — omt must start with defaults and a loud banner rather than refuse.
  10 §6.4 covers reload; the cold-start case must be equally forgiving.

### J38 — Adding a second machine

- **Steps.** On the laptop: `omt instance add` → shows a QR + `omt://` invite.
  On the phone: scan → instance appears in the switcher (07 §1.3).
- **Shows.** A federated session list across both (07 §1.6).
- **Success.** Under 60 seconds, no account, no cloud.
- **Failure.** The desktop machine is not reachable (NAT). The Tailscale path
  must be a first-class documented flow, not an afterthought (13 §10 checklist).

### J39 — The two machines run different omt versions

- **Steps.** Laptop `0.9`, server `0.7`.
- **Shows.** The instance chip carries a version badge; capabilities the older
  instance lacks are hidden, not broken, with a tooltip *"requires omt ≥ 0.8 on
  this instance"* (07 §1.5, 08 §3.4).
- **Success.** Graceful, explicit degradation.
- **Failure.** Handled well in the design. Remaining risk: no `omt upgrade
  --all-instances` for P4's 30 boxes.

### J40 — Long-running agent overnight

- **Trigger.** Mara starts a big refactor at 23:00 and goes to bed.
- **Steps.** Morning: opens the laptop.
- **Shows.** Ideally: *"claude · featX — ran 4 h 12 m, 3 questions answered by
  the timeout default, 47 files changed, 2 test runs failed then passed,
  $4.18 spent, finished at 03:24."*
- **Success.** She understands the night in 30 seconds.
- **Failure.** **Not designed.** 12 §8 claims the audit log answers "what
  happened while I was away", but the audit log excludes PTY output *and* agent
  tool calls that did not go through a capability, is Admin-role-gated, and is
  a forensic record, not a digest. Claude Code's own `away_summary` (06 §8) is
  displayed verbatim but only exists for one agent and only describes *state*.
  See G3.

### J41 — Reviewing what an agent actually did

- **Steps.** Session → **Timeline** tab.
- **Shows.** A chronological, collapsible list: turns, tool calls with
  arguments, file changes, command blocks with exit codes, interactions and how
  they were resolved, compaction events, errors.
- **Success.** Constructible entirely from data omt already has
  (`AgentEvent::FileChanged`, block log, interaction ledger).
- **Failure.** No document owns it. The pieces exist in 04/05/06/12 and are
  never assembled into a user-facing artifact. See G3.

### J42 — "Which session was I doing the auth refactor in?"

- **Steps.** `⟨leader⟩ /` global search → type `refactor auth middleware`.
- **Shows.** Ideally: hits across *all* sessions' scrollback, command history,
  and agent transcripts, on all connected instances, ranked, with a jump.
- **Success.** Finds it in one query.
- **Failure.** **Not designed.** 04 §8.1 is within-session terminal search.
  05 §9 is command-*text* history search (SQLite FTS on the command string
  only), explicitly per-instance, and explicitly not cross-instance in v1.
  Nothing indexes scrollback content or agent transcripts. See G2.

### J43 — A terminal purist doing ordinary non-agent work all day

- **Steps.** `nvim`, `git`, `cargo`, `rg`, `less`, `htop`, `psql`, `man`, all
  day. Splits, zooms `⟨leader⟩ z`, detaches `⟨leader⟩ d`, reattaches
  `omt attach`.
- **Shows.** A terminal. Nothing agent-related, because no agent is running.
- **Success.** Every agent affordance is *absent*, not greyed out, when no agent
  is bound. Latency indistinguishable from the host terminal. `TERM`,
  `COLORTERM`, `terminfo` correct. Mouse reporting passes through (18 §5.1).
  Sixel/kitty images work in `nvim` plugins.
- **Failure.** Any agent chrome in a shell-only session. A `TERM` value nothing
  recognizes — omt must ship or reuse a real terminfo entry and document the
  fallback. Not stated anywhere; 04 §5 defines the sequence surface but not the
  advertised `TERM`. Small but load-bearing gap.

### J44 — Automation: driving omt from a script

- **Steps.**
  ```sh
  export OMT_TOKEN=$(cat ~/.omt-ci-token)
  sid=$(omt session create --workspace "$PWD" --command claude --json | jq -r .id)
  omt session send-text --session "$sid" --text "fix the failing test" --submit
  omt agent wait --session "$sid" --for idle --timeout 30m --json
  omt session export --session "$sid" --format jsonl > run.jsonl
  omt session kill --session "$sid"
  ```
- **Shows.** JSON on stdout, diagnostics on stderr, documented exit codes.
- **Success.** The CLI tree is generated from the catalog (03), so this is
  mostly free — *if* `--json`, `wait`, `export` and an exit-code contract exist.
- **Failure.** `agent wait` and `session export` are not in the catalog groups.
  Non-interactive auth via env var is not specified. Without these, CI use is
  impossible. See G14.

### J45 — CI answering an interaction non-interactively

- **Trigger.** An unattended run blocks on a permission card.
- **Steps.** `omt interaction list --json`, then
  `omt interaction resolve --id int_88 --option 1`.
- **Success.** Scriptable, audited, attributed to the CI credential.
- **Failure.** This is a *policy engine* by the back door and sits close to D1's
  line. The honest position: omt exposes the capability, and any auto-answer
  logic lives in the user's script or a plugin (11), never in omt. That should
  be stated explicitly, because someone will ask for `auto_approve = true` in
  `config.toml` and the answer must be a documented no.

### J46 — Sensitive output on screen

- **Trigger.** `cat .env` or an agent printing an API key in a tool result.
- **Shows.** The terminal shows it (P4 — omt never rewrites output). But omt
  *persists* it: scrollback snapshots, block records, the blob store.
- **Success.** The user can mark a session `no-persist`, and a workspace can be
  configured to never write scrollback to disk.
- **Failure.** **Not designed.** 13 §8 redacts *logs and events*; 05 §9 redacts
  *command text* in history and explicitly says *"the block's output is not
  redacted — that is a different problem"*. That different problem is unowned.
  See G9.

### J47 — "Delete everything about this project"

- **Steps.** `omt workspace purge ~/code/client-x --yes`.
- **Shows.** A report of what was removed: N sessions, N blocks, N MB of
  scrollback, N history rows, N blobs, N audit entries.
- **Success.** A single, verifiable, complete deletion.
- **Failure.** **Not designed.** 05 §8.2 has size-based eviction and a 1-year
  block retention; 12 §8 has 90-day audit retention. Nothing offers targeted
  deletion or export. See G4.

### J48 — Multiple humans on one Linux box

- **Trigger.** Two developers ssh into a shared build server, both run `omt`.
- **Shows.** Two independent daemons, per-uid socket paths, per-uid state
  directories, no interference.
- **Success.** Obvious and correct — but must be *specified* (socket path,
  `XDG_RUNTIME_DIR`, port allocation when both want a web server).
- **Failure.** A fixed default port collides for the second user with a
  confusing error. See G13.

### J49 — Screen-reader user on the TUI

- **Trigger.** A blind engineer using another tool/NVDA + a terminal.
- **Success.** He can operate omt: bindings are announceable, the pending-leader
  overlay is a list, cards can be read linearly, and a `--plain` or
  low-chrome mode exists that avoids continuous redraws.
- **Failure.** **Not designed.** 08 §9.1 is a genuinely good *web* a11y section.
  16 §9 is about layouts and IME, not assistive tech. A ratatui full-screen app
  is close to unusable with a screen reader unless designed for it, and the
  honest answer may be "use the web client" — but that must be *stated*. See G10.

### J50 — CJK user

- **Trigger.** A developer typing Chinese in an agent prompt via an IME, with
  CJK filenames in the file tree.
- **Success.** Width handling per 04 §2.2; IME composition per 16 §9.3; the web
  client's composition events handled. File tree and diff alignment correct.
- **Failure.** Ambiguous-width characters (box drawing, ✓/✗) rendered at a
  different width than the user's terminal chose — the classic misalignment.
  04 §2.2 must expose an `ambiguous_width` setting. UI *strings* remain English
  by P10, which is a deliberate and acceptable no.

### J51 — Windows

- **Trigger.** A Windows developer tries omt.
- **Success/Failure.** Unclear. Windows appears incidentally across 02, 09, 10,
  16 (ConPTY, `%APPDATA%`, clipboard formats) but **no document states whether
  Windows is a supported target**, and the agent-integration story (shell
  integration for bash/zsh/fish, Unix-socket hooks, `/proc` inspection for the
  process tier) is Unix-shaped throughout. See G11.

### J52 — Air-gapped install

- **Trigger.** P4 installs on a network with no egress.
- **Success.** omt works fully: no telemetry (10 §7.11), no required egress
  (00 §8). There is no notification backend to configure at all (D12), and the
  daemon opens no outbound connection.
- **Failure.** Plugin distribution (11 §6), STT providers (08 §7.3), and the web
  client's asset delivery must all be verified offline-clean, and the web client
  must load no CDN fonts. Minor but worth an explicit statement. See G16.

### J53 — omt itself is slow

- **Trigger.** Typing feels laggy.
- **Steps.** `omt doctor` / `⟨leader⟩ ⇧D`.
- **Shows.** Daemon uptime, per-session PTY throughput, parser time, render FPS,
  event-bus backpressure state (07 §6), store write latency, memory per session,
  attached clients and their round-trip times, degraded observation sources.
- **Success.** A user (or a bug report) can answer "why is this slow" without a
  profiler.
- **Failure.** `omt doctor keys` (16 §7.5) and media doctoring (09) exist, but
  there is no general health/diagnostics surface, no crash-report path, and no
  documented log location beyond a config key. See G5.

### J54 — omt crashes

- **Trigger.** A panic in the VT parser on a hostile byte sequence.
- **Success.** The daemon survives (parser panics are caught per session and the
  session is marked degraded, not the instance killed); a crash record is
  written with the offending bytes quarantined; the user is told how to report
  it.
- **Failure.** One malformed sequence takes down every session on the machine.
  05 §8.2 covers *restore* after a crash but nothing states the blast radius of
  a per-session fault. Should be an explicit invariant.

### J55 — Handing a session to a colleague

- **Trigger.** Priya pairs with an engineer, then hands over.
- **Steps.** Share link (Operator role) → `⟨leader⟩ W` release writer token →
  he acquires it.
- **Shows.** Both see who is driving (12 §3.4, mandatory on every surface).
- **Success.** Clean, visible handoff with attribution in the audit log.
- **Failure.** No way to leave a note for the person taking over, no threaded
  comment on a session, no "@mention". Beyond D4's scope today; see G7.

---

# Part 3 — Needs the current design does not cover

Each gap: **the need**, **in scope?**, **the minimum honest answer**.

---

## G1 — First-run experience, onboarding, and in-product help — **NO OWNER**

**Need.** Sam launches `omt` and must learn, without docs, that (a) a leader key
exists, (b) shell integration is worth installing, (c) agent hooks are worth
installing, (d) there is a phone client. Kenji must be able to turn all of that
off permanently in one action. Journeys J1, J2, J6.

**In scope.** Yes, and it is the single highest-leverage unowned item: adoption
is decided in the first 120 seconds.

**Minimum honest answer.** A `19-onboarding.md` owning: the default status bar
(content and how to disable it); the one-line first-run hint and its dismissal
persistence; `⟨leader⟩ ?` as a generated key/capability reference; a single
`omt setup` command that installs shell integration + agent hooks with a
diff-preview before writing to the user's rc files; and the rule that omt shows
at most one unsolicited hint per install.

## G2 — Search and recall across sessions, transcripts and machines — **NO OWNER**

**Need.** J42. "Which session was I doing X in?", "find the command that
produced this error last Tuesday", "search everything the agent said about
`AuthMiddleware`".

**What exists.** 04 §8.1 — search within one session's grid+scrollback.
05 §9 — SQLite FTS over *command strings only*, per instance, explicitly not
federated in v1. Agent transcripts are read (tier 3) but never indexed.

**In scope.** Yes. This is the payoff of persisting everything, and without it
the persistence is cost without benefit.

**Minimum honest answer.** Extend `omt-store` with an FTS index over closed
block *output* (capped per block, e.g. first + last 8 KB, configurable, and
skippable per workspace for G9) and over normalized agent turn text. One
capability `search.query { scope, text, filters }` returning
`(instance, session, block, offset, snippet)`. Cross-instance = client-side
fan-out and merge, exactly as 07 §1.6 does for session lists. Ship
within-instance first; federation is a client concern.

## G3 — "What happened while I was away" — a session digest and timeline — **NO OWNER**

**Need.** J40, J41. The state machine tells you what an agent is doing *now*.
Nobody asks that question in the morning.

**What exists.** 12 §8 *claims* the audit log answers this, but the audit log
deliberately excludes PTY output, records only capability calls (an agent's tool
calls are not capability calls), and is Admin-gated. 06 §8 surfaces Claude
Code's own `away_summary` verbatim — good, but single-agent and state-only.

**In scope.** Yes, and it needs no new observation — only assembly. On
positioning: per [D9](../architecture/decisions.md#d9--positioning-what-omt-may-and-may-not-claim)
the strongest differentiated claim is the **block model** — real VT emulation,
panes and layouts, and a mobile client in one product, with OSC 133 segmentation
turning scrollback into a collapsible list on a phone. The timeline and digest
are the direct continuation of that claim: the same segmented record, read over
time instead of over a screen. Remote answering is **commoditized** and is not
the benchmark to rank against; its one differentiated form is answering a card
from a phone *while the user's real interactive TUI is still on screen*. That
ordering does not change the judgement here — this is high-value, in scope, and
cheap relative to what it returns, because every input already exists.

**Minimum honest answer.** A `Timeline` view built from data already collected:
turn boundaries, tool calls, `AgentEvent::FileChanged`, closed blocks with exit
codes, interaction opens/resolutions (including timeout-defaults), compaction
events, usage deltas, errors. Plus a deterministic, non-LLM header line —
"4 h 12 m · 47 files · 3 interactions (2 timed out) · 2 failed commands · $4.18"
— composed by counting, never generated (P4 forbids omt talking to a model).
Owner: a new `20-timeline-and-digest.md`, or a major section of 06.

## G4 — Data lifecycle: retention, export, deletion — **partial owner, weak**

**Need.** J47. After six months omt holds gigabytes about every project the user
has touched, including client work under NDA. They will want it out, and they
will want proof.

**What exists.** 05 §8.2 size-capped scrollback eviction, 1-year block
retention, 90-day audit retention (12 §8). No export. No targeted deletion.
No accounting of total footprint. Blob store lifetime (09 §2) is not
cross-referenced with any retention policy.

**In scope.** Yes — a legal/trust requirement, not a nicety.

**Minimum honest answer.** Three capabilities and one config block:
`store.usage` (bytes by workspace/session/kind), `store.export
{ scope, format: jsonl|zip }`, `store.purge { scope, dry_run }` covering
scrollback, blocks, history, transcript caches, blobs and audit entries for that
scope, with a printed manifest of what was deleted. Config: per-workspace
retention overrides. Owner: extend 05 §8 into a Data Lifecycle section, or a
new `21-data-lifecycle.md`.

## G5 — Observability of omt itself — **NO OWNER**

**Need.** J53, J54. "Why is this slow", "is the daemon healthy", "where are the
logs", "it crashed, what do I send the maintainer".

**What exists.** `[log]` config (10 §7.12). `omt doctor keys` (16 §7.5) and
media doctoring (09). A `health` capability referenced in passing (03).
Nothing systematic.

**In scope.** Yes. P5 (production-grade) makes this a principle-level
requirement, and a self-hosted daemon with no introspection generates
unanswerable bug reports.

**Minimum honest answer.** `omt doctor` as an umbrella command with subcommands
(`keys`, `agents`, `media`, `store`, `net`), a `system.health` query returning
the metrics listed in J53, a `⟨leader⟩ ⇧D` diagnostics panel with parity on
web, per-session fault isolation as a stated invariant (J54), and
`omt bug-report` producing a redacted bundle (versions, config with secrets
stripped, recent log tail, health snapshot) that the user reviews before
sending.

## G6 — tmux/zellij migration and coexistence — **NO OWNER**

**Need.** J4, J5. Every serious target user already has a multiplexer and a
`.tmux.conf` they have tuned for years. Muscle memory is the barrier, not
features.

**What exists.** 16 chose `Ctrl+B` (correct) and supports rebinding; 10 §8.2 has
the keybinding format; 00 §8 states the nesting position. No importer, no
command-equivalence table, no coexistence guidance.

**In scope.** Yes, cheaply. Most of the work is a mapping table.

**Minimum honest answer.** `omt migrate tmux [--from ~/.tmux.conf]` producing
omt config + a report of unmapped settings with reasons; a documented
`tmux command → omt command` table in the reference docs (`tmux new-session` →
`omt session create`, `capture-pane` → `session.capture`, `send-keys` →
`session.send-text`, etc. — which also serves G14); and an explicit "running
omt inside tmux / tmux inside omt" section stating what degrades. Owner: 16, or
a new `22-migration.md`.

## G7 — Team and sharing beyond read-only links — **deliberately deferred (D4), but state the boundary**

**Need.** J55. Handoff notes, per-session comments, "who owns this session",
async collaboration across time zones.

**In scope.** Mostly **no** for v1 — D4 is a correct decision. But the *absence*
should be a stated anti-requirement rather than an omission, and one small piece
is worth doing.

**Minimum honest answer.** Keep D4. Add exactly one primitive: a free-text
`session.note` (set/get, plain text, part of session metadata, shown on every
surface and in the audit log). It costs almost nothing, covers handoff, and does
not imply an identity system. Everything else — threads, mentions, orgs — is an
anti-requirement (Part 5).

## G8 — Retraction, staleness, and the honest limits of undo — **partial**

**Need.** J15, J28. Two distinct problems conflated: (a) I tapped the wrong
option two seconds ago; (b) my queued approval from 20 minutes offline should
not fire into a changed world.

**What exists.** 12 §4.1 resolve-once with no grace window; 12 §7 optimistic UI
with visible correction; 08 §8.5 offline reconnect without mutation expiry.

**In scope.** Yes — this is a correctness and trust issue on the surface omt
exists to serve.

**Minimum honest answer.** Add a `Pending` phase to interaction resolution: the
resolution is broadcast immediately (so nobody else can answer) but is not
delivered to the agent for `resolve_grace` (default 3 s, 0 on TUI where the
input was deliberate, configurable), during which any surface can retract it.
And: every client-queued mutation carries a `valid_until`; on reconnect,
expired mutations are shown to the user for re-confirmation rather than sent.

## G9 — Privacy of persisted terminal output — **NO OWNER**

**Need.** J46. Terminal output contains `.env` dumps, `kubectl get secret -o
yaml`, JWTs in curl output, database rows. omt writes scrollback, block records
and blobs to disk, unencrypted, by default, and keeps blocks for a year.

**What exists.** 13 §8 redacts logs and *events*. 05 §9 redacts *command text*
in history and explicitly punts on output. 15 §9.4 has a "sensitive files"
notion for the explorer. Nothing covers persisted PTY output.

**In scope.** Yes. This is the most likely source of a serious incident report,
and it is currently only addressed by 13 §1.3's "not a privilege boundary
against your own user" — which is true and does not cover backups, sync clients,
or a stolen laptop.

**Minimum honest answer.** Three things, all cheap:
(1) a per-session and per-workspace `persist_scrollback = false`, with a visible
indicator, so `omt session create --ephemeral` is available for a secrets-heavy
shell; (2) the same secret-pattern redactor used for logs applied to block
output *at index time* for G2's search index, so the searchable copy is never
the risky one; (3) a documented statement of exactly what is written where, with
file modes, so a user can exclude it from Time Machine/Dropbox. Optional
at-rest encryption is explicitly **not** promised — the key would have to live
next to the data.

## G10 — Accessibility of the TUI — **NO OWNER**

**Need.** J49. 08 §9.1 handles the web client properly. The TUI, the CLI and the
`omt --remote` thin client have no accessibility story at all, and a full-screen
ratatui app with a live status bar is actively hostile to a screen reader.

**In scope.** Partly. Making a TUI screen-reader-friendly is genuinely hard.

**Minimum honest answer.** State the position: **the web client is the
accessible surface**, and it has parity by P3, so nothing is inaccessible.
Then do the cheap TUI work: honour `NO_COLOR` and `prefers-reduced-motion`
equivalents; a `--no-chrome` mode with no status bar and no periodic redraw;
make sure every capability has a CLI form (it does, via 03) so the CLI is a
fully scriptable, screen-reader-friendly surface; ensure motor-impaired users
are not required to use chords (the palette covers this). Write it down in 08
§9 or a shared accessibility section.

## G11 — Windows: target or not — **NO OWNER**

**Need.** J51. Windows appears in 02 (ConPTY), 09 (clipboard formats), 10
(`%APPDATA%`), 16 (Windows Terminal keys) — implying support — while the
observation pipeline (shell integration for bash/zsh/fish, Unix-socket hooks,
`/proc` inspection) is Unix-only, implying it does not.

**In scope.** The *decision* is in scope and overdue; the implementation may
well not be.

**Minimum honest answer.** A decision record: **v1 targets macOS and Linux;
Windows is supported only via WSL2**, where it is Linux. Native Windows is
explicitly later. Then remove or qualify the incidental Windows references so
they do not read as commitments, and make the web client the Windows story for
users who need one.

## G12 — Session survival across daemon restart and upgrade — **owned but the answer is weak**

**Need.** J30, J31. tmux users' single most-relied-upon property. 05 §8 is
honest: metadata survives, **processes do not**, and reparenting is deferred to
an open question.

**In scope.** Yes, and this is the biggest *substantive* risk to adoption by P1,
P2 and P4 — the three personas most likely to evangelise the product.

**Minimum honest answer.** Two parts. Short term: make `omt upgrade` refuse to
restart the daemon while agent sessions are `working`, offer to wait, and
require an explicit `--force`; make the `Orphaned` → **restart** flow one key and
excellent. Medium term: take the PTY-supervisor question seriously as a planned
work item rather than an open question, because "restarting omt kills your
agents" will be the top complaint in every thread about it. Either way, the
limitation must be stated on the README, not discovered.

## G13 — Running omt as a service, and multi-user machines — **NO OWNER**

**Need.** J31, J48. Autostart at boot; survive logout; two uids on one box;
which port, which socket, which state dir; `omt` on a headless server.

**What exists.** 13 §1.3 correctly says the daemon is not a privilege boundary.
11 mentions systemd only in the context of plugins. Nothing else.

**In scope.** Yes, and small.

**Minimum honest answer.** Ship a `launchd` plist and a `systemd --user` unit
(`omt service install|status|uninstall`); specify socket path
(`$XDG_RUNTIME_DIR/omt/<instance>.sock`, mode 0600) and state dir per uid;
make the web server default to an ephemeral port with the actual port
discoverable via `omt instance info`, so two users never collide; state plainly
that **omt is per-user and there is no system-wide multi-user daemon** — a
system-wide daemon would be a privilege boundary omt does not implement.

## G14 — Automation and non-interactive use — **partial owner (03), incomplete**

**Need.** J44, J45, P7. Also relevant to G6, because `tmux send-keys` in
existing scripts is what people are migrating from.

**What exists.** The catalog generates an `omt <group> <verb>` CLI (03), which
is most of the work.

**Minimum honest answer.** Make the CLI contract explicit in 03: `--json` on
every capability (schema-derived), stdout is data / stderr is diagnostics, a
documented exit-code table distinguishing usage error, auth failure, omt error
and *remote operation failed*; `OMT_TOKEN`/`OMT_INSTANCE` env auth with no TTY
required; and three capabilities that are missing and that scripts need:
`session.capture` (scrollback → text/JSONL), `agent.wait { for: idle | blocked |
exit, timeout }`, and `interaction.list/resolve` (probably present — verify).
Plus a stated position that omt never auto-answers interactions itself (J45).

## G15 — Cost, usage and rate limits across sessions — **partial owner, thin**

**Need.** Five parallel agents on a paid plan burn real money and hit real rate
limits. Users want: current session cost, today's total across all sessions and
instances, which session is expensive, and a warning before hitting a limit.

**What exists.** 06 §8 normalizes usage and rate-limit payloads into
`AgentEvent`. 08 §6 shows "a token/cost readout" on a working row. That is the
whole design. There is no aggregation, no history, no per-workspace total, no
limit surfacing beyond raw events, and nothing persisted.

**In scope.** Yes for *display and aggregation of what the agent reports*.
Explicitly **not** in scope for billing, budget enforcement, or halting an agent
— that would be policy over the agent (D1) and omt cannot bill anything.

**Minimum honest answer.** Persist the `Usage` events alongside block records;
add `usage.query { scope: session|workspace|instance|all, since }` returning
tokens by type, cost where the agent reports it, and the last known rate-limit
state with its reset time; render a compact "today: $X · N sessions" chip on the
dashboard and a per-session line on the timeline (G3). Costs are *reported by
the agent*, never computed by omt from a price table omt would have to keep
current — say so, so a wrong number is never omt's invention.

## G16 — Offline / air-gapped operation — **mostly fine, needs a statement**

**Need.** J52. **In scope**: yes, and nearly satisfied by 00 §8 already.

**Minimum honest answer.** An explicit "omt works with zero egress" section
listing the only features that need the network (cloud STT, plugin registry
fetches, `omt update` checks), each off or optional,
and a CI check that the web client bundle references no external origin.

## G17 — Internationalization — **adequate, with one gap**

**Need.** J50. CJK width (04 §2.2), IME (16 §9.3), RTL (mentioned in 04/16),
non-US layouts (16 §9.1) are all addressed.

**Minimum honest answer.** Add an `ambiguous_width = single|double` setting
mirroring the user's terminal choice; ensure the web client handles
`compositionstart`/`compositionend` (08 does not mention IME); and record
**UI strings are English-only, per P10** as a deliberate anti-requirement rather
than a gap.

## G18 — Comparing parallel agent results — **NO OWNER**

**Need.** J20. The multi-worktree workflow is a headline use case, and the
workflow ends in a comparison that omt does not support.

**In scope.** Yes, at a modest level.

**Minimum honest answer.** A dashboard multi-select with a compare view showing,
per selected session: branch, agent+model, elapsed, files changed (from 15 §8.3),
diff stat vs. the base branch, last block exit codes, cost (G15), and the
agent's own summary. No merging, no VCS mutation (15 §1.1 holds).

## G19 — Detecting a stuck or looping agent — **NO OWNER**

**Need.** J21. `working` for 22 minutes is indistinguishable from `working` for
20 seconds in the current model except by elapsed time.

**In scope.** Yes, and it requires only counting.

**Minimum honest answer.** Track, per binding: elapsed in current state,
consecutive identical tool calls, and consecutive failing commands. Surface as
metadata on the session card (`Bash(npm test) ×7`) with a configurable amber
threshold. **No automatic action** — omt does not interrupt agents on its own
(D1's spirit). Purely attentional.

## G20 — Deferral versus the local user's native card — **CLOSED by D11**

**Need.** J7. If omt parked the tool call to enable remote answering, the user at
the terminal would never see the agent's own card — omt would have *degraded* the
local experience to enable the remote one, contradicting P4.

**Answer.** [D11](../architecture/decisions.md#d11--omt-mirrors-the-agents-own-card-it-does-not-intercept-or-replace-it):
**omt does not park the tool call.** The agent's own CLI draws its own card,
normally and unchanged; omt observes it through the `PreToolUse` hook — which
fires *before* the card is drawn and carries the verbatim `questions` array —
mirrors it to remote surfaces, and syncs the resolution in whichever direction it
happens. There is no competing card and no local degradation, so the question
this gap posed does not arise.

The risk did not vanish, it moved: a remote answer is now delivered *through* the
agent's own card, so the local user's keyboard and omt's injection are two
writers on one PTY. That is
[D13](../architecture/decisions.md#d13--synthetic-delivery-is-a-gated-transaction-never-a-bare-write)'s
gated transaction, 12 §3.5's `InputGate`, and 12 §4.5's conflict case. Deferral
survives only as an opt-in optimization
([06 §5.3](../architecture/06-agent-layer.md#53-the-deferral-mechanism--demoted-to-an-optional-optimization)).

## G21 — Free-text and annotated answers to structured questions — **specification gap in 06/08**

**Need.** J13, J14. Real users very often want "none of these" or "that one,
but…". A card that only offers the agent's enumerated options is a worse
interface than the terminal, where the user can always type.

**Minimum honest answer.** `InteractionResponse` gains `Freeform(String)` and
`Choice { option, note: Option<String> }`; each adapter declares which it can
deliver natively; where it cannot, the note is delivered as a queued follow-up
message (06 §8) and the UI says so before sending. Never silently drop a note.

## G22 — Notification quality and fatigue — **partial**

**Need.** Moot for v1: D12 ships no notification backend, so there is nothing to
be fatigued by. The gap is preserved because the policy is what a future backend
— a native app or a plugin — will immediately need: five agents finishing
overnight must not produce five 03:00 buzzes.

**Minimum honest answer.** The `NotifyPrefs` design is already written down and
deferred in [remote-continuity §5.6](remote-continuity.md#56-noise-control--deferred-and-why-the-design-is-kept):
quiet hours, per-workspace/per-session mute, and a tier split. Nothing to build
in v1.

## G23 — Session naming, organisation and scale — **thin**

**Need.** At 30 sessions across 6 workspaces and 4 instances, the flat unified
list (07 §1.6) stops working. Users need renaming, pinning, filtering, and
archiving.

**Minimum honest answer.** `session.rename`, a per-session pin flag, saved
filters on the dashboard, and an `archived` state that hides a session without
deleting its history. Mostly small additions to 05's capability surface.

## G24 — Attaching an agent to an already-running process

**Need.** A user starts `claude` in a plain terminal, then wishes it were in
omt. Today they must kill and restart, losing context.

**In scope.** Marginally. Honest answer: **no** — omt cannot adopt a PTY it did
not create, and the observation tiers depend on injected env (tier 2) and known
argv (tier 1). Say so, and offer the good alternative: the agent's own
`--resume`/session-id, re-launched inside omt, which preserves the conversation
if not the terminal. Worth documenting because users will try it.

---

# Part 4 — Prioritized requirements

`M` = Must (v1 is not credible without it), `S` = Should, `L` = Later.
"Owner" names the architecture document that should specify it.

## Terminal and multiplexer fundamentals

| # | Req | Pri | Journeys | Owner |
|---|---|---|---|---|
| R1 | Full VT emulation with correct reflow, 24-bit color, mouse passthrough, images | M | J43, J50 | 04 |
| R2 | Input latency indistinguishable from the host terminal under normal load | M | J43 | 04 §9 |
| R3 | Panes/splits/zoom/detach/attach with tmux-comparable semantics | M | J43 | 05 |
| R4 | Leader key defaulting to `Ctrl+B`, fully rebindable, unprefixed bindings supported | M | J4, J5 | 16 |
| R5 | `omt keys explain <chord>` and static conflict detection against inner-program keymaps | M | J2, J43 | 16 §5 |
| R6 | Declared `TERM`/terminfo story, including what is advertised and the fallback | M | J43 | [19 §7](../architecture/19-onboarding.md#7-term-and-terminfo--r6) — `pty` sessions only |
| R7 | Per-session fault isolation: one bad session never kills the instance | M | J54 | [22 §4.4](../architecture/22-operations.md#44-per-session-fault-isolation-r7) |
| R8 | Block model + shell integration for bash/zsh/fish, with heuristic fallback | M | J1, J25 | 04 §6–7 |
| R9 | `ambiguous_width` setting; IME composition on TUI and web | S | J50 | 04 §2.2, 16 §9.3, 08 |
| R10 | tmux/zellij config importer + command-equivalence table | S | J4 | [19 §5](../architecture/19-onboarding.md#5-tmux-and-zellij-migration-and-coexistence) |
| R11 | Documented nesting behaviour + visible `nested:` indicator | S | J5 | [06 §9](../architecture/06-agent-layer.md#9-failure-modes-and-their-handling) + [19 §5.4](../architecture/19-onboarding.md#54-coexistence-tmux-inside-an-omt-pane) (the badge) |

## Agent observation and interaction

| # | Req | Pri | Journeys | Owner |
|---|---|---|---|---|
| R12 | Merged multi-tier `AgentEvent` stream; tier 0 may never produce structured content | M | J6–J9 | 06 |
| R13 | `agent.explain` + **visible** degradation indicator on the session card | M | J6 | [06 §4](../architecture/06-agent-layer.md#4-merging-confidence-tiers-not-voting) + [22 §5.1](../architecture/22-operations.md#51-the-panel-p3--all-three-surfaces) (the `tier` and `faults` columns) |
| R14 | Structured `Interaction` renderable and resolvable from TUI, API and web | M | J10–J12 | 06 §5, 08 §5 |
| R15 | Remote answering must not degrade the local user's card: the agent's own CLI draws it, omt mirrors and syncs | M | J7 | **Decided — [D11](../architecture/decisions.md#d11--omt-mirrors-the-agents-own-card-it-does-not-intercept-or-replace-it), [06 §5.2](../architecture/06-agent-layer.md#52-responders--how-the-answer-gets-back); deferral demoted to an opt-in optimization ([06 §5.3](../architecture/06-agent-layer.md#53-the-deferral-mechanism--demoted-to-an-optional-optimization))** |
| R16 | Exactly-once interaction resolution, broadcast, attributed | M | J11, J55 | 12 §4 |
| R17 | `Freeform` and `Choice + note` responses, with per-adapter capability declaration | M | J13, J14 | [06 §5](../architecture/06-agent-layer.md#5-interactions--the-flagship-path) — `ChoiceAnswer.other` / `.comment` and the `Text` variant in §5.0 |
| R18 | Short retraction window (`Pending` phase) before a resolution reaches the agent | S | J15 | **NO OWNER (G8)** |
| R19 | Queued client mutations expire and require re-confirmation after long offline | S | J28 | **NO OWNER (G8)** |
| R20 | Message queue mirrored and writable from every surface; omt-managed fallback labelled and persisted | M | J16 | 06 §8 |
| R21 | Per-adapter interrupt, surfaced identically on all surfaces | M | J17 | 06 §7 |
| R22 | Agent's own permission mode displayed read-only on every surface | M | J6, J45 | 06 §10 Q4, 13 §7.2 |
| R23 | Stuck/loop attention signal: elapsed + repeated tool call + repeated failures | S | J21 | [20 §10](../architecture/20-recall-and-usage.md#10-detecting-a-stuck-or-looping-agent-g19) (`attention.*`) |
| R24 | Compaction, subagents and usage normalized and displayed | S | J18 | 06 §8 |
| R25 | Explicit statement that omt never auto-answers interactions | M | J45 | decisions (D1 extension) |

## Away-from-desk

| # | Req | Pri | Journeys | Owner |
|---|---|---|---|---|
| R26 | ~~Push notification on `blocked`~~ — **dropped by [D12](../architecture/decisions.md#d12--no-push-notifications-in-v1-open-and-replay-instead)**; replaced by open-and-replay discovery | M | J9 | [07 §8.2](../architecture/07-remote-protocol.md#82-what-replaces-it-open-and-replay) |
| R27 | ~~Self-hosted notification path (ntfy/webhook)~~ — **dropped by D12**; a plugin's business, not core's | M | J9, J52 | [07 §8.3](../architecture/07-remote-protocol.md#83-the-reserved-extension-point) |
| R28 | Notification policy: quiet hours, per-workspace mute, severity tiers — **deferred with D12**, design kept for a future backend | S | J40 | [remote-continuity §5.6](remote-continuity.md#56-noise-control--deferred-and-why-the-design-is-kept) |
| R29 | Mobile-first card rendering: full descriptions, thumb-reach, multi-select correct | M | J12 | 08 §5, §8 |
| R30 | Voice input with always-editable transcript, never auto-send; keys never in the browser | S | J34, J35 | 08 §7 |
| R31 | **Session timeline + morning digest, composed by counting, never generated** | M | J40, J41 | [20 §8](../architecture/20-recall-and-usage.md#8-timeline-and-digest) (`timeline.*`, `digest.*`) |
| R32 | Reconnect with exact sequence resume; explicit banner when the replay window was exceeded | M | J28, J29 | 07 §5 |

## Remote and multi-machine

| # | Req | Pri | Journeys | Owner |
|---|---|---|---|---|
| R33 | Federation across instances; per-instance authority; unified list | M | J38 | 07 §1 |
| R34 | Version-skew degradation with explicit "requires omt ≥ x on this instance" | M | J39 | 07 §1.5, 08 §3.4 |
| R35 | `omt --remote <target>` thin client with local clipboard/editor/media | M | J22–J25 | 09, 16 §7, 18 §6 |
| R36 | Diagnosed media fallbacks naming the terminal and the reason (D7) | M | J23 | 09 §5.6 |
| R37 | Remote-file open defaults to read-only with a loud indicator | M | J24 | 18 §6.5 |
| R38 | Bootstrap path when omt is absent on the remote host | S | J22 | [22 §7](../architecture/22-operations.md#7-bootstrap-onto-a-host-with-no-omt-r38) (`remote.bootstrap`, `remote.probe`) |
| R39 | Multi-host upgrade/version management for many boxes | L | J39 | [22 §6](../architecture/22-operations.md#6-upgrade) |

## Persistence, search, lifecycle

| # | Req | Pri | Journeys | Owner |
|---|---|---|---|---|
| R40 | Metadata/layout/blocks/scrollback survive daemon restart; sessions return `Orphaned` with one-key restart | M | J30, J31 | 05 §8 |
| R41 | `omt upgrade` refuses to restart while agents are `working` without `--force`; limitation documented in README | M | J30 | [22 §1.3](../architecture/22-operations.md#13-graceful-shutdown-with-agents-running) + [22 §6.1](../architecture/22-operations.md#61-omt-upgrade) |
| R42 | PTY supervisor so processes survive daemon restart | L | J30 | **[05 §13](../architecture/05-session-model.md#13-open-questions) open question (G12)** |
| R43 | Daemon autostart via launchd/systemd; `omt service install` | S | J31 | [22 §1.2](../architecture/22-operations.md#12-omt-service-install) (`service.*`) |
| R44 | Per-uid socket/state/port isolation on shared machines | M | J48 | [22 §2](../architecture/22-operations.md#2-multi-user-machines) |
| R45 | **Cross-session search over blocks, output and agent transcripts** | M | J42 | [20 §3](../architecture/20-recall-and-usage.md#3-index-design)–[§4](../architecture/20-recall-and-usage.md#4-ranking) (`search.*`) |
| R46 | Cross-instance search via client-side fan-out | S | J42 | [20 §7](../architecture/20-recall-and-usage.md#7-cross-instance-search) |
| R47 | `store.usage`, `store.export`, `store.purge` with printed manifests | M | J47 | [21 §4](../architecture/21-data-lifecycle.md#4-export-and-purge), [21 §8](../architecture/21-data-lifecycle.md#8-capabilities) |
| R48 | Per-workspace retention overrides | S | J47 | 05 §8.2 (extend) |
| R49 | Session rename, pin, filter, archive | S | J19, J38 | `session.rename` in [05 §10.2](../architecture/05-session-model.md#102-session); **pin, saved filters and `archived` remain unowned (G23)** |

## Privacy and security

| # | Req | Pri | Journeys | Owner |
|---|---|---|---|---|
| R50 | Loopback default; refuse public bind without an auth backend | M | J38 | 13 §2 |
| R51 | Invite links, bearer tokens, tailnet identity; per-device credentials | M | J32, J38 | 13 §3 |
| R52 | **Session-scoped** share credentials, not only workspace-scoped | M | J32 | 13 §4.1 (verify/extend) |
| R53 | Revocation terminates live sockets within ~1 s | M | J33 | 13 §5.2 (make explicit) |
| R54 | Secret redaction in logs, events, audit and command history | M | J46 | 13 §8, 05 §9 |
| R55 | **`persist_scrollback = false` per session/workspace, with a visible indicator** | M | J46 | [21 §2.5](../architecture/21-data-lifecycle.md#25-per-workspace-and-per-session-control) + `store.persist.*` in [21 §8](../architecture/21-data-lifecycle.md#8-capabilities) |
| R56 | Redaction applied to any output that enters a search index | M | J42, J46 | [21 §2](../architecture/21-data-lifecycle.md#2-redaction-before-write) (the detector) + [20 §5](../architecture/20-recall-and-usage.md#5-redaction-and-the-index) (the ordering guarantee) |
| R57 | Documented inventory of what is written where, with file modes | M | J46, J47 | [21 §1](../architecture/21-data-lifecycle.md#1-the-inventory) |
| R58 | Audit log with actor, device, interaction responses; queryable | M | J55 | 12 §8 |
| R59 | No telemetry, no required egress, verified in CI | M | J52 | 10 §7.11, 13 §9 |

## Configuration, onboarding, operability

| # | Req | Pri | Journeys | Owner |
|---|---|---|---|---|
| R60 | One schema; file + TUI + web editing with identical validation | M | J36 | 10 |
| R61 | Format-preserving TOML writes from TUI/web edits | M | J36 | 10 §2.3 (confirm) |
| R62 | A config error never prevents startup; last-good or defaults + loud banner | M | J37 | 10 §6.4 (extend to cold start) |
| R63 | Precise diagnostics with file/line/column/value/suggestion and stable codes | M | J37 | 10 §5 |
| R64 | **First-run experience: status bar, one hint, `⟨leader⟩ ?`, `omt setup`** | M | J1, J2, J6 | [19 §1](../architecture/19-onboarding.md#1-first-run-second-run-tenth-run)–[§3](../architecture/19-onboarding.md#3-omt-setup) |
| R65 | Command palette listing every non-hidden capability with descriptions | M | J3 | 03 + 16 §3.5 |
| R66 | Launch configurations able to run setup commands (e.g. `git worktree add`) | M | J19 | 10 §9.1 (confirm) |
| R67 | **`omt doctor` umbrella + `system.health` + diagnostics panel + `omt bug-report`** | M | J53, J54 | [22 §3](../architecture/22-operations.md#3-omt-doctor)–[§5](../architecture/22-operations.md#5-diagnostics-panel-and-omt-bug-report) |
| R68 | Log location, rotation and redaction documented and discoverable from the UI | S | J53 | 10 §7.12 (surface it) |

## Automation

| # | Req | Pri | Journeys | Owner |
|---|---|---|---|---|
| R69 | `--json` on every capability; stdout data / stderr diagnostics; documented exit codes | M | J44 | [22 §8.1](../architecture/22-operations.md#81-the-contract) (the automation contract), over [03](../architecture/03-capability-catalog.md)'s codegen |
| R70 | Non-interactive auth via `OMT_TOKEN`/`OMT_INSTANCE`, no TTY required | M | J44 | [22 §8.2](../architecture/22-operations.md#82-non-interactive-auth-r70) |
| R71 | `session.capture`, `agent.wait`, `interaction.list/resolve` capabilities | M | J44, J45 | [22 §8.3](../architecture/22-operations.md#83-the-primitives-scripts-need-r71), [22 §10](../architecture/22-operations.md#10-capabilities) |
| R72 | Attribution of scripted actions in the audit log | S | J45 | 12 §8 |

## Cost, comparison, accessibility, platform

| # | Req | Pri | Journeys | Owner |
|---|---|---|---|---|
| R73 | **Persist usage events; `usage.query` by session/workspace/instance/day** | M | J40 | [20 §11](../architecture/20-recall-and-usage.md#11-usage-and-cost-g15) (`usage.*`) |
| R74 | Rate-limit state and reset time surfaced per session and instance | S | J40 | [20 §11](../architecture/20-recall-and-usage.md#11-usage-and-cost-g15) (`usage.limits`) |
| R75 | Costs are reported by the agent, never computed by omt — stated | M | — | [20 §11.1](../architecture/20-recall-and-usage.md#111-the-rule-stated-where-nobody-can-miss-it) |
| R76 | Multi-session compare view (diff stat, files, cost, summary) | S | J20 | [20 §9](../architecture/20-recall-and-usage.md#9-comparing-parallel-agent-results-g18) (`compare.sessions`) |
| R77 | Web client WCAG 2.2 AA, verified with axe-core | M | J49 | [08 §9.1](../architecture/08-web-client.md#91-accessibility) |
| R78 | **Stated position: the web client is the accessible surface; TUI `--no-chrome`, `NO_COLOR`, no chord-only paths** | M | J49 | **NO OWNER (G10)** — [08 §9.1](../architecture/08-web-client.md#91-accessibility) covers the web client and [19 §2.1](../architecture/19-onboarding.md#21-the-status-bar-contract) gives `--no-chrome`/`NO_COLOR`; **no document states the position** |
| R79 | **Decision record: macOS + Linux in v1; Windows via WSL2 only** | M | J51 | **NO OWNER (G11)** — asserted in passing by [19 §5.6](../architecture/19-onboarding.md#56-why-switch-and-when-not-to) and Part 5 §11; 22 does not carry it and there is no decision record |
| R80 | `session.note` free-text handoff note on every surface | S | J55 | **NO OWNER (G7)** — [20 §8.1](../architecture/20-recall-and-usage.md#81-the-timeline) *consumes* a `Note` entry, but no document declares the capability |
| R81 | Documented answer for "adopt an already-running agent": no, and the `--resume` alternative | S | J24 | **NO OWNER (G24)** — refused in Part 5 §16 and in G24; [06 §9](../architecture/06-agent-layer.md#9-failure-modes-and-their-handling) does not state it |

**Requirements with NO OWNER, counted: 6 of 81.**

The original count was 31, clustering in six areas — onboarding (G1), search
(G2), digest/timeline (G3), data lifecycle + privacy (G4, G9),
self-observability + service (G5, G13), and automation (G14). Documents
[19 — Onboarding](../architecture/19-onboarding.md),
[20 — Recall and usage](../architecture/20-recall-and-usage.md),
[21 — Data lifecycle](../architecture/21-data-lifecycle.md) and
[22 — Operations](../architecture/22-operations.md) were written to close them
and did; the owner cells above now name the section that owns each one.

The six that remain, and why each is genuinely still open:

| # | Why it is still unowned |
|---|---|
| R18 | Retraction window before a resolution reaches the agent. G8 is a partial gap and nobody has specified the `Pending` phase; it interacts with exactly-once resolution ([12 §4](../architecture/12-collaboration.md)) and needs a decision there first |
| R19 | Expiry and re-confirmation of queued client mutations after a long offline. Same gap (G8), same missing decision |
| R78 | The accessibility *position statement*. [08 §9.1](../architecture/08-web-client.md#91-accessibility) specifies web accessibility and [19 §2.1](../architecture/19-onboarding.md#21-the-status-bar-contract) gives `--no-chrome`/`NO_COLOR`, but no document says out loud which surface is the accessible one |
| R79 | The platform decision record. macOS + Linux v1 / Windows-via-WSL2 is asserted in prose in several places and recorded in no decision entry; it belongs in `decisions.md`, not in a document |
| R80 | `session.note`. [20 §8.1](../architecture/20-recall-and-usage.md#81-the-timeline) reads a `Note` timeline entry, but no document declares the capability that writes one |
| R81 | The documented "no" for adopting an already-running agent. Refused in Part 5 §16 and analysed in G24; [06 §9](../architecture/06-agent-layer.md#9-failure-modes-and-their-handling) does not carry it, so it exists only in this file |

Three further entries are tagged as owned-but-open rather than unowned, and
should not be counted either way: **R15** (settled by D11 and no longer live —
[06 §5.2](../architecture/06-agent-layer.md#52-responders--how-the-answer-gets-back)),
**R42** (PTY supervisor —
[05 §13](../architecture/05-session-model.md#13-open-questions)), and **R49**,
where `session.rename` exists and pin/filter/archive do not.

---

# Part 5 — Anti-requirements

Stated as refusals, with reasons, so that scope creep has to argue against a
written position.

1. **No policy layer over agent permissions.** No allow-list, deny-list, danger
   classifier, or `auto_approve`. D1. A second gate means two mental models and
   false confidence about cases omt cannot see.
2. **omt never talks to a model.** No summarisation, no "explain this error",
   no naming sessions with an LLM. Every digest number in G3 is counted, not
   generated. P4. The moment omt calls an API it inherits keys, cost, latency
   and a trust problem.
3. **No hosted service, no cloud sync, no telemetry, no account.** 00 §8.
   Federation is client-side; there is no omt backend to compromise.
4. **No re-implementation of an agent's prompt loop or UI.** omt renders the
   agent's own structured data; it does not build a better Claude Code.
5. **No arrow-key-counting synthetic input by default.** D3. Selecting the
   wrong option on a phone, invisibly, is worse than not offering the feature.
6. **No VCS mutation in v1** — no commit, no stage, no branch, no worktree
   creation from the explorer. 15 §1.1. Read-only git is a bounded, safe,
   testable surface; mutation is where a terminal tool starts destroying work.
7. **No organizations, teams, roles beyond share-scoping, or SSO.** D4. One
   `session.note` is the entire concession (G7).
8. **No editor.** omt opens `$EDITOR` and views files; it does not become one.
   The workspace explorer is navigation and review, not authoring.
9. **No shell.** 00 §8. omt integrates with zsh/bash/fish; it does not
   implement completion, aliases or a prompt.
10. **No translated UI.** English only, P10. Input i18n (IME, width, layouts)
    is fully supported; string localisation is not.
11. **No native Windows in v1.** WSL2 only (G11). Half-working PTY, hook and
    shell-integration paths on Windows would cost more than they return.
12. **No claim to sandbox or contain an agent.** 13 §1.3. omt observes; if your
    agent is malicious, omt faithfully shows you what it does.
13. **No undo of an agent's file edits.** omt does not own the filesystem and
    will not pretend to. The honest offering is: surface *what changed* (15
    §8.3), surface the agent's own checkpoint/rewind feature where it has one,
    and point at git. Anything more would be a backup product wearing a
    terminal's clothes.
14. **No observation through a nested multiplexer.** 00 §8. The terminal works;
    the agent state is `Unknown` and says so.
15. **No at-rest encryption promise for scrollback.** The key would live beside
    the data on the same machine, under the same uid. `persist_scrollback =
    false` (R55) is the honest control instead.
16. **No adoption of processes omt did not spawn.** G24. The observation tiers
    depend on injected env and known argv.
17. **No plugin permitted to bypass the capability catalog.** 11 §4. A plugin
    that needs a private door means the catalog is wrong.
18. **No feature that exists on only one surface.** P3. If it cannot be
    expressed in the catalog and rendered on a phone, it does not ship.
