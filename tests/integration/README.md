# The integration suite

Python, against a **real `omt web`** over the **real wire protocol**. Every test
starts an instance, connects a WebSocket, completes the handshake and calls
capabilities exactly as a client does.

Nothing here is mocked. That is the point: the failures worth catching have all
been ones where the pieces worked and the connection between them did not — a
capability nothing exposed, an event stream nothing pushed, a config layer
nothing loaded. A mocked instance agrees with omt about every one of those.

## Running it

```sh
cargo build -p omt                      # the binary under test
cd tests/integration
python3 -m venv .venv
.venv/bin/pip install -r requirements.txt
.venv/bin/python -m pytest -q
```

Or in Docker, which is what CI runs and what avoids "works on my machine":

```sh
docker compose -f tests/integration/compose.yml run --rm integration
```

`OMT_BINARY` points the suite at a different build.

## What is covered

One module per documented surface, named after the document it checks.

| Module | Covers | Document |
|---|---|---|
| `test_connection.py` | handshake, credentials, what is served without one | 07 |
| `test_sessions.py` | create, write, read, resize, writer token, close | 05 |
| `test_git_worktree.py` | status, diff, hunks, worktrees, fan-out | 15 |
| `test_agents.py` | threads, interactions, interrupt, the supported agents | 06 |
| `test_workspace_surfaces.py` | files, panes, layouts, blocks, recall, recognised references | 04, 17, 18 |
| `test_configuration.py` | schema, layers, keymap, theme, instance identity | 10, 16 |
| `test_extensibility.py` | plugins and grants, scheduled jobs, saved state, dictation | 11, 13 |
| `test_events_and_parity.py` | the live event stream, and every rule the catalog must obey | 03, 07 |

## What these tests are for

They check that omt does what the **documentation** says, not what the code
says. Where the two disagreed, the answer was not always "fix the code":

- `recall.suggest` was suggesting `rm -rf ./build`, and the first version of
  that test called it a bug. It is not: the judgement is about the *target*, so
  `rm -rf /` is suppressed and an ordinary `rm -rf` is not. Suppressing every
  `rm -rf` would make the feature useless for the thing people actually run and
  teach them to ignore it. The test now asserts the real rule.
- `session.restart` refuses a live session. That is its contract — it respawns a
  session restored from a snapshot — so the test asserts the refusal.
- `state.restore` reports `found: false` for a snapshot that is not there. First
  run has none, and that is an ordinary state rather than a failure.

A test that had to be weakened to pass is a test that was asserting the wrong
thing. A test that fails is a bug **or** a documentation error, and which one it
is has to be decided by reading the document, not by making the red go away.
