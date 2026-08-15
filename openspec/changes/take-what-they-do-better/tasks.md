## 1. Reading their source

- [x] 1.1 Every user-facing surface in both, checked against omt, with a
      verdict and a reason per row
- [x] 1.2 Written down where it can be argued with:
      `docs/reference/read-from-the-others.md`
- [x] 1.3 No competitor name in the repository, in this file or any other

## 2. Agent reports over the pty

- [x] 2.1 `OSC 777 ; notify` with omt's marker parsed, payload rejoined across
      the separator JSON is full of
- [x] 2.2 Normalized through the same per-agent mapping a hook uses
- [x] 2.3 `Marker` tier, and a test that it can never claim `Hook`
- [x] 2.4 The agent comes from observation, never from the message
- [x] 2.5 Unusable payloads dropped in silence
- [x] 2.6 The channel's version advertised in every spawned pty
- [x] 2.7 06 §3.6b

## 3. Notifications inward

- [x] 3.1 `OSC 9` and `OSC 777 ; notify` become a `Notified` terminal event
- [x] 3.2 Passed to the terminal omt is drawing on
- [x] 3.3 A screen test that a program in a pane reaches the outer terminal
- [x] 3.4 04 §5.4 — `9` is `Must` now, with `777` beside it

## 4. Quitting

- [x] 4.1 The defect proved first: a foreground process in a pane does not
      survive the key
- [x] 4.2 The key relabelled `quit`
- [x] 4.3 A confirmation listing what will end, ordered by what needs a person
- [x] 4.4 Answered in the overlay, because an overlay takes every key
- [x] 4.5 05 §4.1b, including what a real detach would take

## 5. Close-out

- [x] 5.1 clippy, layering, licensing, codegen, file length — including the
      2000-line hard limit, which the emulator had crossed
- [x] 5.2 The emulator's OSC surface split into its own module
- [x] 5.3 Workspace tests and the screen suite
