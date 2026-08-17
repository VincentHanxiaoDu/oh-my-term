# Tasks

## 1. The plan
- [x] 1.1 `plan(args)` — task names in order, `--root`, errors for the rest
- [x] 1.2 Task names validated in `plan`, before any gate runs
- [x] 1.3 Tests: five names in order, `--root` both spellings, unknown task,
      unknown option, empty line prints usage

## 2. Running them
- [x] 2.1 Walk every task in order, naming each before it runs
- [x] 2.2 A failure does not stop the walk; the exit code names every failure
- [x] 2.3 `licensing` takes its fixture from `--root`

## 3. The documents
- [x] 3.1 `HANDOFF.md` §4 and `README.md`: the multi-gate line, now true
- [x] 3.2 `docs/status.md`: the layering row's crate count, which the gate
      itself reports

## 4. Gates
- [x] 4.1 fmt, clippy, and the five gates — in one command, which is the point
- [x] 4.2 workspace suite
- [x] 4.3 validate and archive
