# Settings

Generated from `omt-config`'s schema. Every setting has a default, because a feature works before it is configured.

| Key | Holds | Default | Reload | What it does |
|---|---|---|---|---|
| `agents.detection` | all \| told-only \| off | `"all"` | immediately | Which agent-detection sources to run. |
| `appearance.block_style` | bordered \| gutter \| minimal | `"gutter"` | immediately | how command blocks are delimited. |
| `appearance.cursor.blink` | bool | `false` | immediately | The appearance cursor blink. |
| `appearance.cursor.style` | block \| bar \| underline | `"block"` | immediately | The appearance cursor style. |
| `appearance.density` | compact \| comfortable | `"comfortable"` | immediately | affects TUI chrome and web padding. |
| `appearance.font.fallbacks` | list | `[]` | immediately | The appearance font fallbacks. |
| `appearance.font.family` | text | `"monospace"` | immediately | The appearance font family. |
| `appearance.font.ligatures` | bool | `false` | immediately | The appearance font ligatures. |
| `appearance.font.line_height` | number | `1.2` | immediately | Line height. |
| `appearance.font.size` | number | `13.0` | immediately | The appearance font size. |
| `appearance.show_agent_badges` | bool | `true` | immediately | agent-state badges on panes/tabs. |
| `appearance.theme` | text | `"omt dark"` | immediately | The theme, by name. Built in, or one imported into the themes directory. |
| `appearance.theme_light` | text | `"omt-light"` | immediately | used when the OS reports light mode. |
| `appearance.theme_mode` | auto \| dark \| light | `"auto"` | immediately | Theme mode. |
| `font.family` | text | `"monospace"` | immediately | The font a client renders the terminal in. |
| `font.line_height` | number | `1.2` | immediately | Line height as a multiple of the font size. |
| `font.size` | number | `13.0` | immediately | Point size. |
| `hints.enabled` | bool | `true` | immediately | Show the hint line. It stops offering the introduction once the prefix has been used. |
| `log.file` | text | `""` | immediately | The log file. Default: $XDG_STATE_HOME/omt/omt.log. |
| `log.level` | error \| warn \| info \| debug \| trace | `"info"` | immediately | The log level. |
| `log.max_size` | text | `"32MiB"` | immediately | The log max size. |
| `log.redact_secrets` | bool | `false` | immediately | Redact secrets. |
| `media.blob_dir` | text | `""` | immediately | The media blob dir. Default: $XDG_RUNTIME_DIR/omt/<instance>/blobs. |
| `media.clipboard.osc52_max_bytes` | text | `"64KiB"` | immediately | Osc52 max bytes. |
| `media.clipboard.osc52_read` | bool | `false` | immediately | Osc52 read. |
| `media.clipboard.osc52_write` | bool | `true` | immediately | Osc52 write. |
| `media.images.enabled` | bool | `true` | immediately | The media images enabled. |
| `media.images.max_rows` | integer | `20` | immediately | The media images max rows. |
| `media.images.protocol` | auto \| kitty \| sixel \| iterm2 \| none | `"auto"` | immediately | The media images protocol. |
| `media.images.thumbnail_max_px` | integer | `512` | immediately | Thumbnail max px. |
| `media.osc_bridge.max_bytes` | text | `"8MiB"` | immediately | The media osc bridge max bytes. |
| `media.quota.max_blob_bytes` | text | `"32MiB"` | immediately | Max blob bytes. |
| `media.quota.max_blobs_per_session` | integer | `200` | immediately | Max blobs per session. |
| `media.quota.max_total_bytes` | text | `"512MiB"` | immediately | Max total bytes. |
| `media.quota.ttl` | integer | `24` | immediately | The media quota ttl. |
| `media.quota.ttl_referenced` | integer | `7` | immediately | Ttl referenced. |
| `media.reverse_socket.hosts` | list | `[]` | immediately | The media reverse socket hosts. |
| `notifications.when` | needs-you \| any-change \| off | `"needs-you"` | immediately | When to notify. `needs-you` is a card waiting on a human, which is the only one worth a buzz by default. |
| `server.bind` | text | `"127.0.0.1:7717"` | on listener restart | Where the web surface listens. Loopback by default, deliberately. |
| `shell.auto_install` | bool | `true` | immediately | Auto install. |
| `shell.command_metadata` | bool | `false` | immediately | Command metadata. |
| `shell.inject_env` | bool | `false` | immediately | Inject env. |
| `shell.osc133` | emit \| consume \| both | `"both"` | immediately | The shell osc133. |
| `shell.propagate_over_ssh` | bool | `false` | immediately | Propagate over ssh. |
| `shell.shells` | text | `""` | immediately | The shell shells. Default: auto-detected. |
| `terminal.bell` | off \| visual \| audible \| both | `"visual"` | immediately | The terminal bell. |
| `terminal.blocks` | bool | `true` | immediately | Track command blocks. On for every shell; a shell that emits no marks still gets blocks, with less known about each. |
| `terminal.blocks.enabled` | bool | `true` | immediately | The terminal blocks enabled. |
| `terminal.blocks.heuristic_fallback` | bool | `true` | immediately | Heuristic fallback. |
| `terminal.blocks.max_retained` | integer | `2000` | immediately | Max retained. |
| `terminal.bracketed_paste` | bool | `true` | immediately | Bracketed paste. |
| `terminal.default_cwd` | home \| last \| workspace | `"workspace"` | immediately | Default cwd. |
| `terminal.default_shell` | text | `""` | immediately | The shell new sessions start. Empty means omt uses $SHELL. |
| `terminal.env_passthrough` | list | `["TERM_PROGRAM","COLORTERM","LANG"]` | immediately | Env passthrough. |
| `terminal.highlight` | bool | `true` | immediately | Colour the command line by token. |
| `terminal.mouse_reporting` | bool | `true` | immediately | Mouse reporting. |
| `terminal.paste_confirm_lines` | integer | `10` | immediately | Paste confirm lines. |
| `terminal.paste_confirm_on_newline` | bool | `true` | immediately | Paste confirm on newline. |
| `terminal.reflow.on_resize` | active_block \| all \| none | `"active_block"` | immediately | The terminal reflow on resize. |
| `terminal.scrollback_lines` | integer | `10000` | immediately | How many lines of scrollback each session keeps. |
| `terminal.word_separators` | text | `" \t[]{}'\"<>,;:"` | immediately | Characters that end a word when double-clicking or moving by word. |
