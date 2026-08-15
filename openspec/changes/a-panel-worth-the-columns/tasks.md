## 1. The document

- [x] 1.1 19 §2.1 amended: one row of chrome stays absolute, a vertical panel
      is added with its own justification test
- [x] 1.2 The amendment says why a column is a different trade from a row

## 2. The entry model

- [x] 2.1 One computation from the instance to what a row says
- [x] 2.2 Ordered by attention across the whole list, grouped by workspace
- [x] 2.3 The mark, the agent's name, and where it is
- [x] 2.4 Nothing says "unknown"

## 3. The panel

- [x] 3.1 Drawn in a fixed column, to the left of the panes
- [x] 3.2 Refuses when the panes would be left too narrow
- [x] 3.3 Off by default, one setting
- [x] 3.4 A key toggles it, and the choice persists
- [x] 3.5 The status bar is untouched: one row, at the bottom, always

## 4. Both surfaces

- [x] 4.1 The web client renders the same entries
- [x] 4.2 A test that the two agree — a table generated from the Rust source
      by `cargo xtask vocabulary`, asserted by the client's own tests, the same
      shape as the capability parity gate. It found a real disagreement on its
      first run: the terminal ranked a starting session with the working ones
      and the client ranked it last, so a session somebody had just opened sat
      below every finished one on the phone.

## 5. Close-out

- [x] 5.1 clippy, workspace tests, layering, codegen, test-isolation
- [x] 5.2 `npm run check`
- [x] 5.3 Through omt's own emulator: it appears where it fits, refuses where it
      does not, and toggles
