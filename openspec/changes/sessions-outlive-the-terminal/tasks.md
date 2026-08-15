## 1. Rendering a terminal back into bytes

- [x] 1.1 `omt-term`: the screen as escape sequences — text, colours,
      attributes, cursor position, which screen is active
- [x] 1.2 A test that feeding a seed to a fresh emulator reproduces the
      original's text, colours and cursor
- [x] 1.3 Wide characters, empty rows and a screen full of blanks survive the
      round trip

## 2. Something to be behind of

- [x] 2.1 `SessionRuntime` keeps a bounded ring of recent output with a byte
      position
- [x] 2.2 The ring never grows without bound and never blocks the pump
- [x] 2.3 A reader can ask for bytes since a position, or be told it is too far
      behind

## 3. The push side of the socket

- [x] 3.1 `Subscribe` accepted on the unix socket
- [x] 3.2 Output pushed as binary frames, with the position in the header
- [x] 3.3 A reply is never interleaved inside another frame
- [x] 3.4 A subscriber that has fallen behind is seeded again
- [x] 3.5 A subscriber that goes away takes nothing else with it

## 4. The client

- [x] 4.1 Connect, subscribe, feed a local emulator per session
- [x] 4.2 Input over the socket, with the writer epoch
- [ ] 4.3 Agent state, rosters and cards from the event stream rather than a
      local instance
- [x] 4.4 Resize

## 5. The seam in the frame loop

- [ ] 5.1 One place a pane's terminal comes from, one place input goes
- [ ] 5.2 Every surface unchanged: splits, panel, copy mode, conversation,
      notifications, status bar
- [ ] 5.3 The local path kept until the attached one is at parity

## 6. Lifecycle

- [x] 6.1 Discover an instance; start one when there is none
- [ ] 6.2 Quitting detaches, and the sessions keep running
- [ ] 6.3 Starting again finds them and repaints
- [ ] 6.4 The key says `detach` again, and the confirmation keeps the case it
      still describes

## 7. Proof

- [ ] 7.1 The screen test that proved the defect, inverted: a foreground process
      in a pane survives quitting
- [ ] 7.2 Re-attaching shows what the session's screen shows now
- [ ] 7.3 An agent mid-turn is still mid-turn after the terminal is gone

## 8. Close-out

- [ ] 8.1 clippy, workspace tests, the screen suite, `npm run check`
- [ ] 8.2 layering, licensing, file length, codegen, test isolation
- [ ] 8.3 05 §4.1b rewritten: it describes a gap that no longer exists
