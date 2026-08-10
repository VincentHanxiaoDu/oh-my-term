# Settings

Generated from `omt-config`'s schema. Every setting has a default, because a feature works before it is configured.

| Key | Holds | Default | Reload | What it does |
|---|---|---|---|---|
| `agents.detection` | all \| told-only \| off | `"all"` | immediately | Which agent-detection sources to run. |
| `appearance.theme` | text | `"omt dark"` | immediately | The theme, by name. Built in, or one imported into the themes directory. |
| `font.family` | text | `"monospace"` | immediately | The font a client renders the terminal in. |
| `font.line_height` | number | `1.2` | immediately | Line height as a multiple of the font size. |
| `font.size` | number | `13.0` | immediately | Point size. |
| `hints.enabled` | bool | `true` | immediately | Show the hint line. It stops offering the introduction once the prefix has been used. |
| `notifications.when` | needs-you \| any-change \| off | `"needs-you"` | immediately | When to notify. `needs-you` is a card waiting on a human, which is the only one worth a buzz by default. |
| `server.bind` | text | `"127.0.0.1:7717"` | on listener restart | Where the web surface listens. Loopback by default, deliberately. |
| `terminal.blocks` | bool | `true` | immediately | Track command blocks. On for every shell; a shell that emits no marks still gets blocks, with less known about each. |
| `terminal.highlight` | bool | `true` | immediately | Colour the command line by token. |
| `terminal.scrollback_lines` | integer | `10000` | immediately | How many lines of scrollback each session keeps. |
