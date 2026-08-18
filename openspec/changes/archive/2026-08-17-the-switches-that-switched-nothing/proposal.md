# The switches that switched nothing

## Why

Four more out of `UNWIRED`, and every one of them is a switch somebody would
reach for precisely when omt was doing something they did not want.

- `terminal.mouse_reporting = false` did not stop omt asking the terminal for
  mouse reports, so every click still went to omt rather than to the terminal's
  own selection — which is the one thing that setting exists to allow.
- `terminal.bracketed_paste = false` likewise.
- `hints.enabled = false` could not turn off the surface whose whole purpose is
  to stop nagging.
- `server.bind` has a deliberately narrow scope — a repository you cloned must
  not be able to move your daemon off loopback — and nothing read it, so the
  scope was protecting a value that could not be set. `omt web` used the
  compiled-in address.

## What changes

- `RawGuard::enter_with(Modes)`: omt asks for the modes the configuration asked
  for. `enter()` keeps today's behaviour, which is both on.
- The hint line is suppressed when hints are off. **The leader strip is not**:
  it is a menu somebody opened by pressing the leader, and hiding what they
  just asked for is a different thing from not nagging them.
- `omt web` with no `--bind` takes `server.bind`.

## Non-goals

- `terminal.bell`, `blocks.enabled`, `paste_confirm_*`, `reflow.on_resize`,
  `default_cwd`, `highlight`, `env_passthrough`, the `log.*` group, the
  appearance group and the rest of `shell.*` stay declared unwired.

## Impact

- `omt-tui`: `Modes`, and a guard that takes them.
- `omt`: three readers and one default.
