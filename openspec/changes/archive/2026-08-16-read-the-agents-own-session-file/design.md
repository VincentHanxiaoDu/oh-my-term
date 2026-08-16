# Design

## Where the path comes from

`HookEvent` gains `transcript_path: Option<String>`. The hook already receives
it in the payload it is handed on stdin; it was being dropped. Optional, so an
older hook binary against a newer daemon is unchanged — the field is absent and
nothing is read, which is exactly the behaviour before this change.

Rejected: deriving `~/.claude/projects/<encoded-cwd>/<agent-session>.jsonl`.
The encoding is undocumented and agent-specific, the directory holds every
conversation ever held in that directory, and the failure mode is silent and
wrong rather than loud and absent. The agent knows; ask the agent.

The path is stored on the instance beside the agent's own session id, by the
same `observe_*` shape, and cleared with the session.

## The reader

```rust
pub struct Reading { pos: u64, partial: String }
impl Reading { fn read(&mut self, path: &Path) -> Vec<serde_json::Value> }
```

One struct per file, held per session. `read` opens, compares length to `pos`,
seeks, reads to end, splits on `\n`, keeps a trailing fragment in `partial`,
advances `pos` by the bytes *consumed as whole lines* — not by the bytes read,
which is the bug that eats a torn record.

A shorter file than `pos` resets both fields. Rotation is not otherwise
detected: inode tracking would add a platform-specific path for a case the
length check already covers for the only writer that matters, an agent that
appends.

Unparsable lines are skipped. A transcript holds bookkeeping records omt has no
opinion on (mode changes, file-history snapshots, queue operations), so "not a
payload" is the common case, not an error.

## The mapping

`AgentAdapter` gains:

```rust
fn transcript_records(&self, record: &serde_json::Value) -> Vec<AgentPayload>
```

defaulting to empty, so no adapter is forced to have one. Claude Code's maps
`user`/`assistant` records: content blocks become `UserMessage`,
`AssistantText`, `Reasoning`, `ToolCall`, `ToolResult`; `message.usage` becomes
`Usage`. `partial` is always false — a file is written a message at a time,
which is a true observation of a coarser source, not a defect.

Everything else is dropped. Sidechain records (`isSidechain`) belong to
subagents and are dropped for now rather than attributed to the main thread.

## Not reporting the same thing twice

Per session, a bounded set of what has been seen:

- tool calls by the agent's own call id,
- records by their uuid.

The hook path writes into the same set. So the ordering that matters — hook
first, file later, which is the normal one — suppresses the file's copy, and
the reverse ordering suppresses nothing, which is correct: the hook is the
stronger source and its version should win.

Bounded, because a long session's set would otherwise grow without limit; the
oldest ids are dropped, and re-showing a very old duplicate is a better failure
than unbounded memory.

## Where it is polled

The daemon pump, beside `remembered.absorb`. Reading is a length check and a
seek; there is no watcher and no thread. A watcher would buy latency omt cannot
use — the pump is already the tick at which every surface updates.
