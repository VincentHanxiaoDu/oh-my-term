# The line omt said it would add

## Why

`shell.auto_install` is `true` by default in
`docs/architecture/10-configuration.md` §7.4, and
[18 §513](../../../docs/architecture/18-semantic-open.md) leans on it. Nothing
installs anything: the setting is read by no code at all, and the
`shell.integration` capability says the opposite in its own documentation —
"omt prints it and never edits a shell's configuration itself".

So a user with the default configuration is promised working command blocks and
gets a shell that marks nothing, while two documents disagree about whose job
it was. Blocks without marks fall back to the heuristic, which is why sessions
look *nearly* right: the failure is a slightly worse experience rather than an
error, which is how it survived this long.

## What changes

- omt installs the snippet, once, into the rc file of the shell it is about to
  start, when `shell.auto_install` is on and `shell.shells` does not exclude
  that shell.
- **Idempotent by construction.** The line is written inside a marked block:
  a file that already has the marker is left exactly as it was, and the marker
  is what a later version edits rather than appending a second copy.
- **It says what it did.** A line added to somebody's shell configuration
  without a word is indistinguishable from a shell that behaved differently
  today; the first-run summary names the file.
- **It never touches a file it did not recognise.** A shell omt has no snippet
  for, an unreadable rc file, or a `$HOME` that is not a directory: nothing is
  written and the reason is available rather than swallowed.
- `shell.integration`'s documentation stops claiming omt never edits shell
  configuration, and says what it actually does.

## Non-goals

- No rewriting or removal of a snippet somebody edited: omt owns the marked
  block and nothing else in the file.
- Not the remaining `shell.*` settings; `osc133`, `inject_env`,
  `propagate_over_ssh` and `command_metadata` stay declared unwired.

## Impact

- `omt`: a small installer, called once at startup.
- The screen harness gains a `$HOME` of its own — **required**: without it the
  suite would append to the developer's own `~/.zshrc`, which is exactly the
  class of test-isolation defect this repository already has a gate for.
- Documentation: 10 §7.4's note, the capability's doc line, the guide.
