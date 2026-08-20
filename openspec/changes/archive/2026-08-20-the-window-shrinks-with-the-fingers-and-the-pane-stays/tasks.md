# Tasks

## 1. The poll
- [x] 1.1 A size read each round; the layout follows the window
- [x] 1.2 The signal remains, but is no longer the only channel

## 2. The floor
- [x] 2.1 `MIN_PANE_COLS` 40 → 12, with the reasoning written down
- [x] 2.2 Layout tests pinned to the new floor, shrink as the asserted case
- [x] 2.3 A zero rect is not painted — the overwrite that hid both prompts

## 3. The tests
- [x] 3.1 The harness can resize, and exposes the child pid
- [x] 3.2 Grow local, grow attached, shrink single, shrink a split
- [x] 3.3 Two load-flaky ceilings raised, assertions unchanged

## 4. Gates
- [x] 4.1 fmt, clippy, six gates, workspace, screen 60, web 347
