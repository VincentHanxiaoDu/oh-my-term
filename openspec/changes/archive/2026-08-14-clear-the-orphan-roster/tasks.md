## 1. The roster

- [x] 1.1 Orphaned sessions group below live ones
- [x] 1.2 The subtitle counts running and orphaned separately
- [x] 1.3 A roster of nothing but orphans says so plainly

## 2. Per-session controls

- [x] 2.1 Restart, through `session.restart`, on the roster row and the
      session screen
- [x] 2.2 Close, through `session.close`
- [x] 2.3 The old output survives a restart, above a separator

## 3. Clearing them

- [x] 3.1 One action that closes every orphan
- [x] 3.2 It states the count before it closes anything
- [x] 3.3 It is absent at zero rather than present and inert
- [x] 3.4 A running session is never closed by it, at any count

## 4. The TUI

- [x] 4.1 **Wrong as written, and corrected rather than built.** The status
      bar's cell B lists this process's *panes*, not the instance's sessions,
      so an orphan can never appear there — an orphan is by definition a
      session no pane holds. The session chooser (`^B s`) does list every
      session and already prints its state and "not on screen". `omt session
      close` and `omt session restart` both work from the CLI. The gap was
      web-only.

## 5. Tests

- [x] 5.1 Unit: grouping and the two counts
- [x] 5.2 Unit: the clear-all set never contains a running session
- [x] 5.3 Playwright on both profiles: restart, close, clear-all. They skip on
      a first-run instance and say why: an orphan is born exactly one way, by
      omt restoring a snapshot whose pty is gone, so the cases need an instance
      that has been restarted once. A skip that names its reason is honest; a
      green tick over an assertion that never ran is what this whole change
      came out of.
- [x] 5.4 The e2e suite runs against a clean state file, so the empty-roster
      path is exercised every run — done in `complete-the-web-surfaces`, where
      it immediately found six call sites tapping a button that no longer
      existed
- [x] 5.5 The e2e helper opens a live session rather than the first row

## 6. Close-out

- [x] 6.1 `npm run check` clean
- [x] 6.2 clippy, workspace tests, codegen-check, layering
- [x] 6.3 A screenshot pass over the roster in both themes and both shapes,
      against an instance restarted once so the orphans are real. Two defects
      only the render showed:
      - `nothing running · 3 sessions ended with omt` truncated to `3 sessions
        end…` in the app bar — a headline that stops mid-word
      - the clear-all row sat in the session panel with the session row shape,
        so it read as a fourth session called "Close 3 ended sessions": a
        destructive control disguised as a thing to open
