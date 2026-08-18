# The markers can be declined

## Why

The shell group out of `UNWIRED`, plus agent detection.

- `shell.inject_env` was decoration with a wrong default: panes always carried
  `OMT_SESSION`/`OMT_SOCK`/`OMT_AGENT_EVENTS`, and the schema said the switch
  was off. A user who did not want agent markers in their panes had no way to
  say so, and the settings screen reported a value nothing consulted.
- `shell.osc133` and the already-wired switches described the same decision
  twice: `emit` (install the emitter), `consume` (read the marks), `both`.
- `shell.command_metadata`: the status bar's branch cell was unconditional.
- `agents.detection`: omt probed and advertised however it was configured.
- `shell.propagate_over_ssh` names a propagation path that does not exist:
  there is no ssh child-environment path to propagate through, and the marker
  env belongs to the host the pane is on. Declared with that reason rather
  than wired to nothing.

## What changes

- `inject_env` gates the marker injection in both pane-spawning paths (the
  local terminal and sessions created from the web). Its schema default is
  corrected to `true`, which is what has always happened and what 10 §7.4
  says — the settings screen had been reporting the opposite of reality.
- `shell.osc133` is the umbrella: `consume` half feeds the block tracker's
  marks switch, `emit` half feeds the auto-installer. `both` is exactly what
  happened before.
- `command_metadata` hides the branch cell when off; its schema default
  becomes `true` for the same reason as above.
- `agents.detection = false` stops the observation loop and the first-run
  advertised list. The web's explicit install flow keeps working: that is a
  user asking, not detection.

## Non-goals

- `propagate_over_ssh` stays declared, with the reason above.
- The log and appearance groups are their own changes.

## Impact

- `omt`: four readers, one corrected default each on the two that lied, and
  the probe loop's gate.
- `omt-config`: two default corrections.
