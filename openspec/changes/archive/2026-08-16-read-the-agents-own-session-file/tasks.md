# Tasks

## 1. Being told where the file is
- [x] 1.1 `HookEvent.transcript_path`, optional, documented as told-not-derived
- [x] 1.2 `omt-hook` reports it from the payload it already reads
- [x] 1.3 Schema snapshot updated
- [x] 1.4 Instance keeps it per session and forgets it with the session

## 2. The reader
- [x] 2.1 `Reading { pos, partial }` with position, torn-line hold, shrink reset
- [x] 2.2 Tests: only-new-bytes, torn line across two reads, replaced file,
      unparsable line, missing file

## 3. The mapping
- [x] 3.1 `AgentAdapter::transcript_records`, defaulting to none
- [x] 3.2 Claude Code: user/assistant, text, thinking, tool_use, tool_result,
      usage; sidechains dropped
- [x] 3.3 Tests per record shape, including one that must produce nothing

## 4. Ingest
- [x] 4.1 Share the hook path's absorption; parameterize tier and source tag
- [x] 4.2 Per-session seen-set, written by both paths, bounded
- [x] 4.3 Poll beside the pump's other per-frame work
- [x] 4.4 Test: a hook-reported call is not repeated by the file
- [x] 4.5 Test: a file written before omt looked reaches the conversation

## 5. Gates
- [x] 5.1 `cargo fmt`, `clippy`, `layering`, `file-length`, `codegen-check`
- [x] 5.2 `cargo test --workspace` and the screen suite
- [x] 5.3 `openspec validate` and archive
