# What ends a word, and how many blocks

## Why

Two more out of `UNWIRED`, both in the terminal group.

`terminal.word_separators` is in the settings reference *and* in the importer
that maps other terminals' configurations onto omt's — so somebody importing
their old setup got the key carried across and then ignored. A word motion in
copy mode always stopped at whitespace, which makes `/usr/local/bin` one word
whatever anybody configured.

`terminal.blocks.max_retained` capped nothing: the block tracker was built with
its own default at every session, so a user who wanted fewer blocks kept two
thousand and one who wanted more got two thousand.

## What changes

- Copy mode's word motion takes the configured separators. Empty keeps the
  whitespace-only rule, which is what a caller with no configuration wants.
- A session's terminal is built with the configured block cap.
- Both caps travel as one `SessionLimits` value into `omt-daemon`, which sits
  below the configuration layer and cannot read a setting itself. Adding the
  second cap as another unlabelled argument at six call sites is how the first
  one gets swapped for it.
- Copy mode's key handler takes a `Viewport` rather than five loose arguments,
  for the same reason.

## Non-goals

- `terminal.blocks.enabled`, `bell`, `mouse_reporting`, `bracketed_paste`,
  `paste_confirm_*`, `reflow.on_resize`, `env_passthrough`, `default_cwd` and
  `highlight` stay declared unwired.

## Impact

- `omt-term`: one optional cap on `TermConfig`.
- `omt-daemon`: `SessionLimits`, replacing a bare `ScrollbackLimits` argument.
- `omt-tui`: `word_between`, with `word` delegating to it.
- `omt`: the readers, and the copy-mode viewport.
