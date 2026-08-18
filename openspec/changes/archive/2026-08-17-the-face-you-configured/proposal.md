# The face you configured

## Why

The appearance group, minus the two keys whose reasons are the absence of a
feature behind them.

`theme.get` always served the built-in dark theme, so `appearance.theme_mode`
and `appearance.theme_light` changed nothing anywhere. The web client
rendered its built-in face regardless of every `appearance.*` and `font.*`
key: no font was applied, the cursor was always a block, the block cards wore
their first style forever, and the settings screen reported a face that was
not the one on screen. `appearance.density` and `appearance.block_style`
had consumers that never heard of them.

## What changes

- `theme.get` honours `appearance.theme_mode`: `light` serves the new
  built-in light theme, `dark` and `auto` the dark one, and
  `appearance.theme_light` names the light half of the pair.
- The web client asks `config.get` once beside `theme.get` and turns the
  appearance settings into CSS variables, consumed in `tokens.ts` with the
  old values as defaults — so an unconfigured client renders exactly what it
  always did, and a configured one renders what it asked for: the machine
  face and its size and line height, ligatures on or off, fallback families
  appended, the cursor's shape and blink, density, and the block cards'
  border style.
- The settings gate learns to read the web client: single-quoted keys and
  `values[...]` bracket access. It had reported eleven wired settings as
  unwired, which is a gate lying in exactly the direction a gate must not.

## Non-goals

- `appearance.theme` stays declared: no theme registry exists, the importer
  writes files nothing registers, and the two built-ins are named.
- `appearance.show_agent_badges` stays declared: the roster shows no agent
  badge today, so there is nothing for the switch to hide.

## Impact

- `omt-theme`: the light half of the built-in pair, tested for readability
  like the dark one.
- `omt`: `theme.get` reads the mode keys.
- `web`: one new module, seven unit tests, CSS variables with defaults.
