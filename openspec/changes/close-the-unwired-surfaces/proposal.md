## Why

The previous change counted keys. This one counts **wiring**, and the count is
worse: the surfaces omt had already specified, declared and bound were, in four
places, connected to nothing.

- `ProtoMessage::HookEvent` had no handler anywhere in the tree, and the process
  that draws a terminal bound no socket, and a pane omt spawned carried no
  `$OMT_SOCK`. Three breaks in one path, so every agent running inside omt
  reported nothing: no transcript, no question cards, no thread roster.
- `notifications.when` was a setting nothing read. The product's premise is that
  agents work while you do something else, and nothing omt did reached anybody
  who was doing something else.
- Copy mode had bindings in the keymap, a section in the keys document and a
  line in `⌃b ?`, and no implementation. A key that is advertised and does
  nothing reads as a broken program rather than an unbuilt one.
- Nothing stored what an agent calls its own conversation, so a session that
  outlived its terminal could only be restarted as a *different* agent in the
  same directory.

Each of these had tests passing on both sides of the gap.

## What Changes

**The hook ingress, end to end.** A handler for `HookEvent` answered before
dispatch and without a handshake; a socket bound beside the terminal that owns
the sessions, serving connections and owning neither the pump nor the snapshot;
`$OMT_SOCK` in every pane omt spawns. An unattributed report is dropped rather
than attributed, and so is one naming a session this instance has never seen.
A question raises a card on the ledger and reports `Blocked` at hook tier.

**The conversation, readable from the keyboard.** `⟨leader⟩ t` shows what an
agent said and did rather than what it painted — which the web client has
rendered since it shipped and no terminal could reach. Open cards are read from
the ledger and shown with it, never copied into the record.

**Notifications.** A new `omt-notify` crate: a terminal-raised notification
(OSC 9, or the structured OSC 99 where the emulator has it), wrapped for a
multiplexer, with the agent's own words sanitized because they travel inside an
escape sequence. A sound played by whatever the machine already has — omt ships
no audio. And the part that decides *when*, which is the feature: a report is
not a change, being waited for always counts, finishing counts only when nobody
was watching.

**Copy mode.** `⟨leader⟩ [`, absolute-row coordinates, selection, and a copy
that goes out over OSC 52 — the only clipboard reachable from the far end of an
ssh link.

**Resuming an agent.** `AgentAdapter::resume_argv` per agent, because each CLI
spells it differently; the agent's own session id remembered from its hooks; and
`agent.resume_plan`, a query that says what would be run or why it cannot.

## Capabilities

### New Capabilities

- `hook-ingress`: reading an agent's own hook reports into an instance.
- `attention`: telling somebody an agent needs them when they are elsewhere.
- `scrollback-reading`: reading and copying what has scrolled away.
- `agent-resume`: continuing an agent's conversation rather than starting one.

### Modified Capabilities

- `agent-observation`: a hook is now a source, at hook tier, and raises cards.
- `tui-surfaces`: the terminal gains the conversation view.

## Impact

- New crate `omt-notify` at L2; the workspace is 29 crates.
- One new capability, `agent.resume_plan`, with its schema snapshot and its web
  handler — the parity gate refused the build until both existed.
- `cargo xtask install` installs `omt-hook` as well as `omt`. The settings file
  on the developing machine named an absolute path to a binary nobody had
  installed, which from every surface looks exactly like a hook that was never
  set up.
