# The rest of copy mode, and the keymap setting nothing read

## Why

Copy mode ships with vim's basic motions and a search. Three things are
missing that somebody reading their own scrollback reaches for immediately —
word motions, whole-line selection, and searching backwards — and one thing is
missing that no other terminal multiplexer can offer at all.

Separately, `keymap = "default" | "vim" | "emacs"` is documented in the
configuration reference, parsed into the binding file model, and **read by
nothing**. A user can set it, and watch it do nothing. That is the same defect
as the last several: a vocabulary that exists everywhere and a consumer that
exists nowhere, indistinguishable from the outside from a feature that has
nothing to say.

Reading the two comparable products changed two verdicts here:

- **Named registers are not a gap.** Neither has them in a scrollback copy
  mode — one has no copy mode at all, and the other's registers belong to its
  editor's vim mode. omt's copy goes to the system clipboard over OSC 52,
  which is what survives ssh; a second clipboard only omt can paste from is a
  place to lose text in. The row becomes a stated decision, not a backlog item.
- **Neither has a vim or emacs preset keymap.** One hardcodes a vim-ish copy
  mode that is not remappable; the other writes three bindings from a banner.
  omt documents the setting, so omt should honour it.

And one thing neither can do: **jumping to the previous prompt.** It requires
knowing where prompts are, which requires the shell having said so. omt models
exactly that already — every block carries the absolute row its prompt began
on, in the same coordinates copy mode moves in. One of the two never parses
the shell integration at all.

## What changes

- Word motions, first-non-blank, and half-page.
- Whole-line selection, because a copied command is a line far more often than
  it is a span.
- Backward search, with `n` and `N` following the direction the search was
  made in.
- Smart case: a lower-case query matches either, a query with a capital means
  it.
- `[` and `]` jump between prompts, skipping any block omt did not see a
  command for.
- `keymap = "emacs"` gives copy mode the emacs motion set, which is the first
  time anything reads that setting.

## Impact

- `omt-tui`: the motions, the selection mode, the search direction.
- `omt`: the key tables, one per flavour, and the prompt rows handed in.
- No capability, no wire change, no new configuration key — the key that
  exists starts working.
