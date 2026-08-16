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
- [x] 4.3 The roster read from the instance: titles, agent state, thread and
      blocked counts — `session.list` already carries all four
- [x] 4.4 Resize

## 5. The seam in the frame loop

- [x] 5.1 One place a pane's terminal comes from, one place input goes
- [x] 5.2 Every surface unchanged: thirty-two screen tests through the seam
      say what they said before
- [x] 5.3 The local path kept until the attached one is at parity — it is the
      one the terminal still runs

## 6. Lifecycle

- [x] 6.1 Discover an instance; start one when there is none
- [x] 6.2 Proven at the level below the terminal: a client creates a session,
      goes away entirely, and the session and its process do not notice
- [x] 6.3 A returning client is seeded with what it missed while away
- [ ] 6.4 The key says `detach` again, and the confirmation keeps the case it
      still describes

## 7. Still to do: the terminal's own surfaces on a remote roster

The frame loop draws from the seam and both sides of it work. What is not
done is the half of the terminal that reads the *instance* rather than the
*terminal*: the agent panel, the agent list, the status bar's chips and the
quit confirmation all take a local `Instance` today.

- [x] 7a.1 One view model both modes produce — the roster the instance
      already reports, plus what only a local probe knows, as optional fields
      rather than absent ones
- [x] 7a.2 The row builders take that rather than an instance: the agent list,
      the sessions list, the panel and the quit confirmation
- [x] 7a.3 One table, two directions: `word_of` and `state_from_word` beside
      the words themselves, and the capability that puts a state on the wire
      asks it rather than keeping a second copy
- [x] 7a.4 The attached path built and proven behind `OMT_ATTACH`; making it
      the default is the next commit, once the whole suite has run against it

## 7. Proof

- [x] 7.1 A terminal killed outright, and the next one in the same directory
      showing the work that was left running
- [x] 7.2 Re-attaching shows what the session's screen shows now
- [x] 7.3 A split makes another session in the instance, which outlives the
      terminal exactly as the first one does
- [x] 7.4 Attached, quitting asks nothing — it ends nothing

## 8. Close-out

- [ ] 8.1 clippy, workspace tests, the screen suite, `npm run check`
- [ ] 8.2 layering, licensing, file length, codegen, test isolation
- [ ] 8.3 05 §4.1b rewritten: it describes a gap that no longer exists
