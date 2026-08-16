# Tasks

## 1. Keeping the bytes
- [x] 1.1 The scanner keeps the payload as it arrived
- [x] 1.2 A per-terminal bound, oldest evicted first
- [x] 1.3 Only what can be re-sent is kept; the rest keeps its card
- [x] 1.4 Tests: kept, evicted, refused

## 2. The handshake
- [x] 2.1 The query, and the reply read from the raw descriptor
- [x] 2.2 A deadline, after which there is no support
- [x] 2.3 Run once, after raw mode, before the first key is read
- [x] 2.4 Tests for the reply parser, including the wrong reply

## 3. Drawing
- [x] 3.1 Placement and clipping to the pane rectangle
- [x] 3.2 The overlay pass, after the pane is rendered
- [x] 3.3 Nothing drawn without a cell size
- [x] 3.4 Tests for the clip arithmetic, including fully off-screen

## 4. Re-sending
- [x] 4.1 A signature per placement; unchanged means nothing sent
- [x] 4.2 Deletion when a placement goes away
- [x] 4.3 Tests for both

## 5. Gates
- [x] 5.1 fmt, clippy, layering, file-length, codegen-check, test-isolation
- [x] 5.2 workspace and screen suites, including that a terminal which did not
      answer receives no graphics bytes at all
- [x] 5.3 validate and archive
