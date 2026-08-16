# Tasks

## 1. Motions
- [x] 1.1 Word forward, back, end; first non-blank; half-page
- [x] 1.2 A word motion stops at the end of its line
- [x] 1.3 Tests per motion, including the empty and end-of-line cases

## 2. Selection
- [x] 2.1 Line-wise selection, widening the span when the text is taken
- [x] 2.2 Tests: several rows, one row, and that character-wise is unchanged

## 3. Search
- [x] 3.1 Backward search; repeats follow the direction
- [x] 3.2 Smart case
- [x] 3.3 Tests for each, including a capital that must not match lower case

## 4. Prompts
- [x] 4.1 Previous and next prompt, from absolute rows handed in
- [x] 4.2 Unattributed blocks filtered out by the caller
- [x] 4.3 Tests: jumping, nothing before it, no prompts at all

## 5. The keymap that was never read
- [x] 5.1 A key table per flavour; `default` unchanged
- [x] 5.2 The emacs motion, search, mark and copy keys
- [x] 5.3 Tests for both tables

## 6. Gates
- [x] 6.1 fmt, clippy, layering, file-length, codegen-check, test-isolation
- [x] 6.2 workspace and screen suites
- [x] 6.3 validate and archive
