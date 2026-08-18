# The bell sounds, and the paste waits

## Why

Six more out of `UNWIRED`, plus the two that turned out to be something else.

- `terminal.bell` reached nothing: the daemon emitted a `Bell` event and no
  one consumed it — the vocabulary-without-a-consumer shape again. A bell in a
  pane did nothing whatever the setting said.
- `paste_confirm_lines` and `paste_confirm_on_newline`: a paste of two hundred
  lines went straight into the shell, where its first line ran.
- `terminal.blocks.heuristic_fallback`: the fallback that opens an unattributed
  block ran whatever it said.
- `terminal.reflow.on_resize`: a resize always re-wrapped everything.
- `terminal.default_cwd`: panes started wherever omt had, whatever it said.

And two that are not wires: `terminal.env_passthrough` wants a filtering point
that does not exist — a child inherits omt's whole environment, so a passthrough
list has nothing to act on — and `terminal.highlight` names a feature whose
consumer (`omt-input::highlight`) is called nowhere at all. Both are declared
unwired with those reasons, which is the honest state, and both are said out
loud rather than marked `yet` forever.

## What changes

- The Bell event is consumed: `audible` writes `\x07` to the outer terminal,
  `visual` flashes a `␇` in the status bar for 350 ms, `off` does nothing.
- A paste that exceeds the thresholds waits for a y/n answer, shown in the
  hint row; y hands it over, anything else drops it, and a second paste cannot
  stack on the first. Verified through a real pty both ways.
- `heuristic_fallback = false` means marks are the only thing that opens a
  block. `reflow.on_resize = "none"` cuts the screen instead of re-wrapping;
  `all` and `active_block` coincide, because the scrollback is
  width-independent by construction and the choice costs nothing — said in the
  type's documentation rather than silently.
- `default_cwd` is `home`, `last` (the earliest session's directory) or
  `workspace`, which is what panes always did.
- The reading gate's `reads()` survived rustfmt splitting a call across lines
  — it had failed twice on formatting, which is a gate failing for the wrong
  reason.

## Non-goals

- `env_passthrough` and `highlight` are deliberately not wired; see above.
- The remaining terminal keys (`bell`'s visual flash is the only chrome
  change) and the media, shell, log and appearance groups are their own
  changes.

## Impact

- `omt-tui`: `Input::Paste`, `Status.bell`, the paste hint.
- `omt-term`: `Reflow`, `Grid::cut`, `block_fallback`, reflow integration
  tests moved out to keep the file under the limit.
- `omt`, `omt-daemon`, `omt-config`: readers and plumbing.
