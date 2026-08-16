# Tasks

## 1. Seeing it
- [x] 1.1 An APC scanner ahead of the parser, resumable across reads
- [x] 1.2 Kitty keys: id, action, more, format, pixel size, cell size
- [x] 1.3 The iTerm2 inline sequence, single and multipart
- [x] 1.4 Tests: whole, split across reads, split mid-key, a non-graphics APC

## 2. Holding it
- [x] 2.1 Chunk accumulation with a bound on the whole transmission
- [x] 2.2 Over the bound drops all of it; the pane keeps working
- [x] 2.3 An unfinished transmission does not become an image
- [x] 2.4 Tests for each

## 3. Room for it
- [x] 3.1 Rows reserved, cursor advanced, `C=1` honoured
- [x] 3.2 Tests: following output lands below it; it scrolls like content

## 4. Showing it
- [x] 4.1 The card in the reserved rows
- [x] 4.2 A screen test: `imgcat`-shaped bytes into a real pane

## 5. Gates
- [x] 5.1 fmt, clippy, layering, file-length, codegen-check, test-isolation
- [x] 5.2 workspace and screen suites
- [x] 5.3 validate and archive
