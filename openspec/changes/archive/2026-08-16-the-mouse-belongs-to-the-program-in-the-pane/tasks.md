# Tasks

## 1. What the program asked for
- [x] 1.1 Split `MouseTracking` into 1000/1002/1003; keep an "any" query
- [x] 1.2 Track 1007
- [x] 1.3 Tests: each mode sets only itself; the any-query still answers

## 2. Encoding
- [x] 2.1 `omt_input::mouse` — event, buttons, modifiers, SGR and legacy
- [x] 2.2 Legacy drops an out-of-range coordinate rather than wrapping it
- [x] 2.3 Tests per encoding, per button, with modifiers, past column 223

## 3. Routing
- [x] 3.1 Enable reporting on omt's terminal, and disable it on the way out
- [x] 3.2 The order in the design, with the gesture pinned to its pane
- [x] 3.3 Tests: reporting on forwards, Shift takes it back, motion is not sent
      to a click-only program, a release always follows its press

## 4. Selection and wheel
- [x] 4.1 Drag selects through copy mode; release copies; a bare click does not
- [x] 4.2 The three-way wheel decision, 1007 consulted
- [x] 4.3 A screen test: drag over real output, and the clipboard sequence omt
      writes

## 5. Gates
- [x] 5.1 fmt, clippy, layering, file-length, codegen-check, test-isolation
- [x] 5.2 workspace and screen suites
- [x] 5.3 validate and archive
