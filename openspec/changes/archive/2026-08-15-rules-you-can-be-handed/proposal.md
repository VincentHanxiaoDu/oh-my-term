## Why

An agent CLI moves a line and omt starts reporting the wrong state. The rules
are a file you can edit, which is most of the answer — but writing one means
knowing what the agent draws, and the only way to find that out today is to
squint at a terminal and retype what you see. omt has the screen already.

The other half is distribution. One person works out that an agent renamed its
confirmation prompt; everybody else finds out by their omt being wrong. The
terminal this was read from solves that by fetching rule sets. That is the
right idea and the wrong mechanism for omt to copy directly: a rule set is a
matcher that runs against your screen, and fetching one automatically means
running a stranger's matcher against your work without being asked.

So: the same distribution, made an act rather than a background task.

## What Changes

- **`omt agent-rules capture`** writes what a pane's agent is drawing, as a
  file you can edit into rules — the screen, and each named region of it marked
  out, so you can see what `after_last_rule` actually contains before writing a
  rule against it.
- **`omt agent-rules import <path|url>`** takes a rule set, shows what it would
  change, and installs it only when told to. Never on a timer, never at
  startup, never as a side effect of anything else.
- **`omt agent-rules check`** applies the installed set to a pane and says
  which rule matched and why, so a wrong answer can be traced to a rule rather
  than argued about.
- **An imported set is checked before it is kept.** It must name the agent, and
  parse, and every rule must have a test — the failure that cannot be noticed
  from the output because the output is always confident.
- **Nothing is fetched without a person asking.** No auto-update, no version
  check on start, no telemetry about which sets are installed.

## Capabilities

### New Capabilities

- `agent-rules-distribution`: how a rule set is captured, imported, inspected
  and refused.

### Modified Capabilities

_None._

## Impact

- `crates/omt/src/capabilities/` — three capabilities, all `Operator`
- `crates/omt-agent-adapters/src/manifest.rs` — the checks an import must pass
- `docs/reference/agent-rules.md` — capture and import, for the person whose
  agent just changed
- The CLI gets `omt agent-rules …` through the existing group/verb dispatch
