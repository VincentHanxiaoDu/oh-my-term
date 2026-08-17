# Tasks

## 1. Isolation first
- [x] 1.1 The screen harness gives each screen its own `$HOME`
- [x] 1.2 A test proves nothing outside that `$HOME` is written

## 2. The installer
- [x] 2.1 The marked block, written once
- [x] 2.2 `shell.auto_install` and `shell.shells` decide whether and which
- [x] 2.3 Every refusal has a reason a caller can report
- [x] 2.4 Tests: idempotency, an off switch, an unknown shell, a marked file

## 3. Saying so
- [x] 3.1 The first-run summary names the file that was changed
- [x] 3.2 A screen test through a real pty

## 4. The documents
- [x] 4.1 `shell.integration`'s doc line, 10 §7.4, the keybindings-style guide
- [x] 4.2 The entries leave `UNWIRED`

## 5. Gates
- [x] 5.1 fmt, clippy, six gates
- [x] 5.2 workspace and screen suites; schema snapshot if the wire changed
- [x] 5.3 validate and archive
