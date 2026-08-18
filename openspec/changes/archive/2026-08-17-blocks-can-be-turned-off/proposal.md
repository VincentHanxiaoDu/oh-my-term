# Blocks can be turned off

## Why

`terminal.blocks.enabled = false` changed nothing: the block tracker was always
on, however it was configured. The setting was in the reference and the
settings screen, and a user who turned blocks off still had every command
marked, attributed and counted.

The subtlety is the fallback. Output with no marks opens an unattributed block,
which is what makes an SSH session to a host with no integration legible — and
that fallback is a *block*, so "blocks disabled" has to disable it too.
"Disabled" means no prompts, no commands, no unattributed guesses.

## What changes

- `BlockTracker.marks`: when off, no transition opens or closes a block, and
  `output_at` races nothing.
- The flag travels on `TermConfig` and then on `SessionLimits`, down to
  `omt-term` which is below the configuration layer and cannot read a setting
  itself.
- The entry leaves `UNWIRED`.

## Non-goals

- `blocks.max_retained` and `blocks.heuristic_fallback` were handled in their
  own changes. `bell`, `env_passthrough`, `default_cwd`, `highlight` and the
  paste group stay declared unwired.
