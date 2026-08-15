## Why

omt cannot see an agent that is plainly running. Start omt, start Claude Code
in it, ask omt which agents are running: *"no agent in any pane."* The agent's
own interface is on the screen at the time.

The cause is that omt looks in the one place the answer is hardest to get.
`identify()` reads the foreground process's argv and environment through
`KERN_PROCARGS2`, and the shipped Claude Code binary is a signed, hardened
Mach-O: the call is refused for a process the user owns, `identify()` returns
`None`, and every surface downstream reports nothing. Worse, every test omt has
for this passes, because the test suite runs inside an agent session whose
`CLAUDECODE` and `CLAUDE_CODE_ENTRYPOINT` are inherited by every process the
harness spawns — so the probe matched the *harness's* environment and called it
a detection. Three passing tests, one of them driving the real binary, and the
feature does not work.

Two mature terminals that solve this problem solve it somewhere else entirely,
and they agree with each other: **the agent already announces itself to the
terminal.** Claude Code writes `OSC 0 ; ✳ Claude Code` — its name and, in the
first glyph, its state: `✳` when idle and a braille spinner while working. It
writes `OSC 4` progress too. This needs no permissions, survives ssh, does not
care how the binary was launched or signed, and is the agent *deliberately*
telling its host what it is doing — which is what
[P4](../../docs/architecture/01-principles.md) asks omt to observe and what
`omt-term` already parses into `HostAction::SetTitle` and throws away.

Below that, both read the screen — but not by grepping it. They match
**structural regions** (the prompt box body, the text after the last horizontal
rule, the last N non-empty lines, the OSC title) against per-agent rule sets
with priorities and `any`/`all`/`not` combinators, versioned as data so an
agent's interface can change without a release.

## What Changes

- **A marker tier between the protocol and the floor.** OSC title and OSC
  progress become readings: identity from the name, state from the state glyph.
  `omt-term` already produces the events; nothing consumed them.
- **The process probe becomes a fallback and stops failing silently.** A
  hardened binary's argv is unreadable, and the name is not — `p_comm` needs no
  permissions. A probe that cannot read argv still knows what is running.
- **Screen regions, as data.** Per-agent rules with priorities and combinators,
  over named regions of the pane's own grid, so blocked/working/idle can be
  read from an agent that has no hooks and no protocol.
- **"Done" is not "idle".** An agent that finished while nobody was looking is
  a different thing from one sitting at a prompt, and it is the state the whole
  product exists to surface.
- **The word "unknown" never reaches a screen.** It describes omt's knowledge,
  not the agent, and it reads as "omt has no idea what this is".
- **One buffer, one diff, per frame.** Panes, chrome and overlays render into a
  single frame that is diffed once, with the cursor as frame state. Today each
  pane diffs itself and the chrome and overlays are written over the result,
  which is why an overlay looks pasted on, why closing one leaves the screen
  stale, and why an idle omt wrote 619 bytes a second.
- **A persistent agent panel**, not only a list behind a key. What is running,
  where, and which one needs a human — visible without asking.
- **Test isolation.** Every harness strips inherited agent environment. A test
  that can pass because of the environment it runs in is not a test.

## Capabilities

### New Capabilities

- `agent-observation`: how omt decides which agent is in a pane and what it is
  doing — the tier ladder, the OSC marker tier, the region rules, and what is
  shown when nothing is known.

### Modified Capabilities

_None yet._ The TUI's rendering architecture and the agent panel are the second
half of this work and get their own change once this one lands, so that the
thing that fixes the reported bug is not held behind a rewrite of the renderer.

## Impact

- `crates/omt-agent-adapters` — detection inputs gain OSC title and progress;
  rules become data
- `crates/omt/src/probe.rs` — the process probe stops being the only source and
  stops failing whole
- `crates/omt-pty` — read a process name without needing its argv
- `crates/omt-daemon` — feed `SetTitle` into the merge machine
- `crates/omt/tests/screen.rs` — a harness that cannot inherit an agent
  environment
- No capability declarations change.
