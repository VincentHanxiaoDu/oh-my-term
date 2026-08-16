# Read the agent's own session file

## Why

Every source omt has observes the present. A hook fires as the agent acts, a
marker arrives as the pane writes it, a screen rule reads what is drawn now.
Attach to an agent that has already been working for an hour and omt's
conversation view is empty — not because nothing happened, but because omt was
not there when it did.

The agent's own session file was. It is written by the agent, it holds the
whole conversation, and omt has had a tier for it (`Tier::Transcript`), a
source tag for it (`EventSourceTag::Transcript`) and a row for it in the
sources table since the agent layer was designed. Nothing has ever produced
either. This is the same shape of defect as `recall.suggest`: a vocabulary that
exists everywhere and a producer that exists nowhere, which from the outside is
indistinguishable from "there was nothing to show".

Reading the two comparable products found that **neither of them tails a
transcript**. One reads the whole file at save points to upload it and write it
back so a resume works on another machine, keeping every record opaque; the
other never opens the file and uses the path its hook reports purely as a
resume reference. What is worth taking from them is the *discovery* — the agent
says where its file is — and what is worth adding is the reading.

## What changes

- The hook carries `transcript_path`, so omt is **told** where the file is and
  never derives a path from an encoded working directory.
- A reader that holds a position, holds a partial trailing line, and restarts
  when the file shrinks.
- A per-adapter mapping from the agent's own records to `AgentPayload`, reusing
  the vocabulary the hook path already normalizes into.
- Ingest at `Tier::Transcript` / `EventSourceTag::Transcript`, sharing the hook
  path's absorption so the transcript, the ledger, the roster and the event
  stream all see it.
- Per-session suppression of anything a stronger source already reported, so a
  tool call known from a hook is not shown twice.

## Impact

- `omt-proto`: one added optional field on `HookEvent` (wire-compatible).
- `omt-hook`: reports it.
- `omt-agent-adapters`: record → payload mapping, Claude Code first.
- `omt`: the reader, the poll site, and the shared absorption.
- No capability is added; this produces events on paths that already exist.
