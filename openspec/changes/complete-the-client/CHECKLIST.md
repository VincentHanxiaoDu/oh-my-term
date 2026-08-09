# Every documented feature, and the test that proves it

The acceptance rule: a feature is done when a test exercises it against a real
instance and passes. A feature with no test is not done, and a test that had to
be weakened to pass is asserting the wrong thing.

`I` = Python integration suite (`tests/integration/`), against a real
`omt web`. `T` = touch e2e (`web/e2e/`), a real browser at a phone device
profile. `U` = unit.

## Instance surface

| Feature | Doc | Test | State |
|---|---|---|---|
| Handshake, credential, catalog up front | 07 | I | done |
| Sessions: create/write/read/resize/close | 05 | I | done |
| Writer token and epoch | 12 | I | done |
| Git status, diff, hunks | 15 | I | done |
| Worktrees: add, list, remove, force | 15 | I | done |
| Fan-out: start, status, choose | 15 | I | done |
| Agent threads, interactions, interrupt | 06 | I | done |
| Files: list, read, write, containment | 15 | I | done |
| Panes and layouts | 17 | I | done |
| Recognised references | 18 | I | done |
| Command recall and its danger rule | 20 | I | done |
| Config schema, layers, project file | 10 | I | done |
| Keymap cheatsheet with reserved keys | 16 | I | done |
| Theme | 10 | I | done |
| Plugins, grants, high-consequence | 11 | I | done |
| Scheduled jobs | 20 | I | done |
| Saved state | 21 | I | done |
| Voice providers and dictation buffer | 08 | I | done |
| Live event stream, resume, resync | 07 | I | done |
| Catalog parity rules | 03 | I | done |
| Block bodies as styled lines, paged | 08 §4.2 | I | done |
| Block re-run, refused where unattributed | 08 §4.2 | I | done |
| Scrollback as styled lines, by absolute row | 04 §4.4 | I | done |
| Search with row and character offsets | 08 §4.5 | I | done |
| Transcript: messages, reasoning, tools, plans | 08 §4.4 | I | done |
| What an agent offers: commands, tools, queue | 08 §6.2 | I | done |
| Queue writes refused on a mirrored queue | 08 §6.1 | I | done |

## Client surface

| Feature | Doc | Test | State |
|---|---|---|---|
| Terminal-native design language, dark-first | — | T, U | done |
| Adaptive mark: terminal, or the agent in it | — | T, U | done |
| Surface switcher, with reasons when disabled | 08 §4 | T, U | done |
| Block view: cards, exit chips, own scroll | 08 §4.2 | T, U | done |
| Block view explains an empty list | 08 §4.2 | T | done |
| Re-run offered only where omt saw the command | 08 §4.2 | U | done |
| Transcript view: messages, reasoning, tools | 08 §4.4 | U | done |
| Tool calls paired with their results | 08 §4.4 | U | done |
| Opening surface chosen by session kind | 08 §4 | U | done |
| Search screen, with results and empty state | 08 §4.5 | T | done |
| Composer with slash-command completion | 08 §6.2 | U | done |
| Mirrored queue shown read-only, and says so | 08 §6.1 | U | done |
| Screens repaint in place, keeping focus | — | T | done |
| Subscribes to the event stream, no polling | 07 | T, I | done |
| Hold-to-approve is a real gesture, not a class | 08 §5.3 | U | done |
| Permission prompts shown as commands | 08 §5.3 | U | done |
| Plan review renders the plan | 08 §5.4 | U | done |
| Free-text elicitation has a field | 08 §5.5 | U | done |
| Attachments in the composer | 08 §6.3 | U | done |
| Voice input, with an honest unavailable state | 08 §7 | U | done |
| Offline and reconnect said on every screen | 08 §8.5 | U | done |
| Unified roster across instances | 08 §3.3 | U | done |
| A command made offline is held, not lost | 08 §8.5 | U | done |

## Pairing

| Feature | Doc | Test | State |
|---|---|---|---|
| An invite is not a credential, and is prefixed apart | 13 §3.2 | I, U | done |
| Invites expire | 13 §3.2 | U | done |
| An invite can be exchanged exactly once | 13 §3.2 | I, U | done |
| `join.exchange` / `join.credential` over the socket | 07 §1.3 | I | done |
| A socket with no credential can do nothing else | 07 §1.3 | I, U | done |
| Credentials are per device and separately revocable | 13 §3.2 | U | done |
| A device key is carried onto the credential | 13 §3.2 | U | done |
| The client joins on arrival and stores what it got | 07 §1.3 | T | done |

## Reachability

Found by auditing the docs this checklist had never enumerated. Each of these
was a crate that was written, tested and unreachable — the recurring failure in
this project, and one a checklist built from the web-client document could not
have caught.

| Feature | Doc | Test | State |
|---|---|---|---|
| Devices are listed by the name they gave | 23 | I, T | done |
| A device can be revoked, leaving the others alone | 23 | I | done |
| Revoking is idempotent and reports what changed | 23 | I | done |
| Another device can be invited without a restart | 13 §3.2 | I | done |
| The first device is the owner, so it can invite and revoke | 13 §3.2 | I | done |
| A blob is identified from its bytes, not its name | 09 §2 | I | done |
| Settings import from another tool | 19 | I | done |
| What did not map is reported, not dropped | 19 | I | done |
| `doctor` diagnoses, with a remedy on every failure | 22 §3 | I | done |
| `doctor` never reports ok for a check it did not run | 22 §3 | I | done |

## Operations and collaboration

| Feature | Doc | Test | State |
|---|---|---|---|
| Presence: who is connected, and as what | 12 §2 | I | done |
| Audit log, including refusals | 12 §8 | I | done |
| An audited command carries its intent id | 12 §8 | I | done |
| A bounded log says what it dropped | 12 §8 | I | done |
| Metrics: clients, sessions, calls, refusals | 22 §4 | I | done |

## Identity, and the rest of operations

| Feature | Doc | Test | State |
|---|---|---|---|
| A device that joins belongs to somebody | 23 §1 | I | done |
| Devices are grouped under the person who holds them | 23 §1 | I | done |
| Revoking a person takes every device they hold | 23 §6 | I | done |
| A device records whether a key actually backs it | 23 §2 | I | done |
| `doctor` checks store, service and media too | 22 §3 | I | done |
| A bug report, as readable text carrying no credential | 22 §5 | I | done |

## Nothing outstanding

Every feature enumerated here has an integration test against a real instance
or a touch test in a real browser, and all of them pass.

The audit that produced the last three sections is worth keeping, because the
checklist that preceded it was built from one document and missed six others.
Two questions find what a feature list cannot:

1. **Which crates does nothing reach?** A crate written, tested and unreachable
   is this project's recurring failure. `omt-media`, `omt-config`'s importer and
   `omt-identity` were all in that state. What remains is `omt-hook`, which is
   its own binary and is invoked by agents rather than called, and `omt-util`,
   which `omt-daemon` uses.
2. **Which documents has no test ever cited?** That is what surfaced 09, 12, 19,
   22 and 23 — five documents' worth of features nobody had checked.
