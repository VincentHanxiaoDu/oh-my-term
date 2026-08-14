## 1. The roster

- [ ] 1.1 Orphaned sessions group below live ones
- [ ] 1.2 The subtitle counts running and orphaned separately
- [ ] 1.3 A roster of nothing but orphans says so plainly

## 2. Per-session controls

- [ ] 2.1 Restart, through `session.restart`, on the roster row and the
      session screen
- [ ] 2.2 Close, through `session.close`
- [ ] 2.3 The old output survives a restart, above a separator

## 3. Clearing them

- [ ] 3.1 One action that closes every orphan
- [ ] 3.2 It states the count before it closes anything
- [ ] 3.3 It is absent at zero rather than present and inert
- [ ] 3.4 A running session is never closed by it, at any count

## 4. The TUI

- [ ] 4.1 The status bar counts orphans separately, at every width it degrades
      through

## 5. Tests

- [ ] 5.1 Unit: grouping and the two counts
- [ ] 5.2 Unit: the clear-all set never contains a running session
- [ ] 5.3 Playwright on both profiles: restart, close, clear-all
- [ ] 5.4 The e2e suite runs against a clean state file, so the empty-roster
      path is exercised every run
- [ ] 5.5 The e2e helper opens a live session rather than the first row

## 6. Close-out

- [ ] 6.1 `npm run check` clean
- [ ] 6.2 clippy, workspace tests, codegen-check, layering
- [ ] 6.3 A screenshot pass over the roster in both themes and both shapes
