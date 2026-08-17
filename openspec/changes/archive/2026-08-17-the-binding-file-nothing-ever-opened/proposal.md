# The binding file nothing ever opened

## Why

`docs/architecture/10-configuration.md` §8.2 specifies `keybindings.toml`, and
`docs/guide/keybindings.md` tells users their own bindings appear in the
cheatsheet. **No Rust file in this repository mentions `keybindings.toml`.**
Nothing opens it, so:

- a rebind does nothing, and reports nothing;
- `keys.list` and `keys.cheatsheet` answer from `omt_input::keymap::defaults()`,
  so the cheatsheet's claim to include "your own bindings" is false;
- the leader is the constant `PREFIX_KEY` with a hard-coded `leader_action`
  match in `omt-tui`, so `leader = "ctrl-a"` is unreachable;
- `keymap = "vim" | "emacs"` — the one HANDOFF §2 lists as fixed — is read from
  the *config* stack by `crates/omt/src/run.rs`, where the key does not exist. A
  real merge answers `value = None` with `OMT-C101 unknown key `keymap``, so the
  copy-mode flavour was always the default and anybody who wrote the key into
  `config.toml` got an error for their trouble.

There is a second half to it. The types that exist disagree with the document
they cite: `omt_input::keymap::BindingFile` carries `Vec<(String, String)>` and
a `context:chord` trigger spelling that §8.2 does not have, while §8.2's
`when` predicates, `[[binding]]` tables, `args`, `force`, `platform`,
`requires`, `modes`, `repeatable` and `<leader>` expansion exist nowhere. A
checker that validates a grammar nobody writes is not a checker.

## What changes

- **The file is opened**, where 10 §8.2 and 10 §2.1 say it lives: the user
  config home, then the project directory, in config-layer order.
- **The whole of §8.2 is parsed**: both action forms, `[[binding]]` tables,
  `<leader>` expansion in any position, space-separated chord sequences, mouse
  triggers, `"none"` unbinding, and the four gate fields.
- **`when` is a real predicate** over 16 §4.1's `ContextSet` — `&&`, `||`, `!`,
  parentheses — with an unknown name reported rather than treated as false.
- **Resolution follows 16 §2.3**: pending chord, modal context, specificity,
  layer, passthrough. A binding carries its source, so `keys.list` can say where
  a binding came from.
- **Every refusal has a code and a sentence**: an unmet gate is `OMT-C408`, a
  reserved chord without `force` is `OMT-C410`, `modes` under a non-modal keymap
  is `OMT-C421`. A binding that is not installed says so; none is dropped in
  silence.
- **The runtime uses it**: the leader, the keymap flavour, and the leader
  actions come from the resolved map rather than from a constant and a hard-coded
  match, and `keys.list` / `keys.cheatsheet` report what is actually bound.

## Non-goals

- No new binding grammar. §8.2 is the specification; this change implements it
  rather than extending it.
- Not the modal keymaps of 16 §6 beyond the `keymap` field selecting one, nor
  the `keymaps/` inheritance of §6.5.
- No web-client keybinding UI; the parity surfaces already exist.

## Impact

- `omt-input`: the binding file — grammar, contexts, compilation, resolution.
- `omt-config`: finding the file in the layer order it already owns.
- `omt`, `omt-tui`: the leader and its actions come from the resolved map.
- `keys.check_file` gains the fields it was always documented to check.
- Depends on nothing; runs in parallel with the settings work.
