## What this changes

<!-- One or two sentences. What is different afterwards, and why. -->

## Licensing

`docs/architecture/14-licensing.md` §8. **Interfaces are facts; code is
expression.** Reproducing an escape sequence, a wire protocol or a file format
is how interoperability works. Copying, adapting or translating source is not,
and it is what would stop omt being Apache-2.0.

- [ ] This change contains no code copied, adapted or translated from any
      GPL or AGPL source.
- [ ] Any upstream behaviour it reproduces is documented under `docs/research/`
      and cited by section.
- [ ] New dependencies are covered by the allow-list in `deny.toml`.

A description saying "ported from `app/src/terminal/foo.rs`" is rejected on
sight, and the branch is **not** merged after rewording — the concern is the
code, not the sentence. "Implements the OSC 133 handling described in
`docs/research/…` §2.4" is exactly right.

Reviewers: for any suspiciously well-shaped constant table or state machine,
ask where it came from. An answer of "I had the file open" means the change is
rewritten from the notes.

## Checks

- [ ] `cargo test --workspace`
- [ ] `cargo clippy --workspace --all-targets`
- [ ] `cargo run -p xtask -- codegen-check` (if a capability changed)
- [ ] `cargo run -p xtask -- layering`
- [ ] `cargo run -p xtask -- settings-readers` (if a setting was added or wired up)
- [ ] `cargo run -p xtask -- licensing`
