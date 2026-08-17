# A setting either does something, or says it does not

## Why

Sixty-six settings are declared, validated, merged with provenance and shown by
`config.schema`. **Four of them are read by anything.** The other sixty-two
parse, type-check, report which file they came from — and change nothing.

`terminal.scrollback_lines` never reaches `ScrollbackLimits`: every terminal
keeps the built-in ten thousand lines. `media.images.enabled` cannot turn
images off. `terminal.default_shell` is documented in the settings reference
and appears nowhere else in the repository.

From outside, a setting that does nothing is indistinguishable from one that
works: it is accepted, it is listed, it has a source, and the behaviour it
names is whatever the default was. This is `HANDOFF.md` §2's defect exactly —
a vocabulary that exists everywhere and a consumer that exists nowhere — and it
is the largest instance of it left in the repository.

Counting them was itself instructive. The first pass reported seven readers;
three were wrong. `terminal.blocks` matched a method call, and
`terminal.foreground`/`terminal.background` matched `omt-theme`'s importer,
which maps VS Code keys spelled the same way. A string match is not a reader.

## What changes

- `UNWIRED` in `omt-config`'s schema: every setting nothing reads, **and why**.
  Beside the settings themselves, so it is read by whoever adds one.
- `cargo xtask settings-readers`, which fails in both directions:
  - a setting with no reader and no entry in `UNWIRED`;
  - an entry in `UNWIRED` for a setting that now has one.
- A reader means the key inside a lookup — `get(...)`, `setting(...)` — in the
  crates that hold the resolved configuration, not a string anywhere in the
  tree. Tests do not count: a fixture proves somebody typed the key.
- `HANDOFF.md` §4, `README.md` and the pull-request template run the new gate
  with the others.
- `docs/reference/settings-and-their-readers.md`: the audit, and the three it
  first got wrong.

## Non-goals

- **This change wires nothing up.** It makes the emptiness declared instead of
  silent, so that wiring can be done a group at a time against a gate that
  notices. Each group is its own change.
- No change to what any setting means, or to the merge.

## Impact

- `omt-config`: one table, no behaviour.
- `xtask`: one gate, with tests for the two ways it was nearly wrong.
- Depends on nothing.
