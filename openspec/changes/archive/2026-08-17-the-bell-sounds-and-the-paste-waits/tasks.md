# Tasks

## 1. The bell
- [x] 1.1 The Bell event is consumed — audible, visual flash, off
- [x] 1.2 Real-pty tests for audible and off

## 2. The paste
- [x] 2.1 `Input::Paste`, thresholds, the y/n answer in the hint row
- [x] 2.2 Real-pty tests: asked, approved, dropped, and the one-line control
- [x] 2.3 The `hint_dirty` flag that survives an iteration boundary — the
      confirmation first never appeared because a flag set while processing
      input was lost when the next frame began

## 3. The blocks and the reflow
- [x] 3.1 `block_fallback`; the marks-off test extended
- [x] 3.2 `Reflow`; `None` cuts via `Grid::cut`; tests moved to an
      integration file

## 4. The cwd
- [x] 4.1 `home` | `last` | `workspace`

## 5. Declared, not wired
- [x] 5.1 `env_passthrough`: reason is the architecture (no filtering point)
- [x] 5.2 `highlight`: reason is the absent feature

## 6. The gates
- [x] 6.1 `reads()` reads windows, not lines — the fix for a gate that failed
      on formatting, with a regression test
- [x] 6.2 The isolation gate follows `#[path]` harnesses — the screen suite
      split into a module directory to stay one binary under the line limit
- [x] 6.3 The claude bar test waits for its condition instead of a fixed
      six-second sleep — the fixed sleep failed under load, exactly as
      HANDOFF §5 warned
- [x] 6.4 fmt, clippy, six gates, workspace and screen suites, web check
