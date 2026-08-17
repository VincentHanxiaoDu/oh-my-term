# Settings, and what reads them

A setting is a promise. A settings screen that shows a key, validates it,
records which file it came from and then changes nothing is worse than no
setting at all: it fails in the one way nobody checks for, because it looks
exactly like a setting that worked.

This is the audit `HANDOFF.md` §2 asks for, applied to configuration: for every
key in `omt-config`'s `SETTINGS`, **who reads it**. Not who documents it, not
who validates it — who acts on it.

How it was produced: every `key:` in `crates/omt-config/src/schema.rs`, looked
for as a literal string in `crates/`, `web/src/` and `xtask/`, excluding
`omt-config` itself, which defines and validates keys but acts on none of them.

## Read today

| Key | Reader |
|---|---|
| `notifications.when` | `crates/omt/src/run.rs`, `crates/omt-notify/src/policy.rs` |
| `notifications.sound` | `crates/omt/src/run.rs` |
| `notifications.sound.needs_you` | `crates/omt/src/run.rs` (via `sound_file`) |
| `notifications.sound.finished` | `crates/omt/src/run.rs` (via `sound_file`) |

Four, not seven. The first pass counted three more and was wrong about each,
which is worth recording because the mistake is the same one twice over:

- `terminal.blocks` matched `terminal.blocks()` — a method call on a terminal,
  not a setting being read.
- `terminal.foreground` and `terminal.background` matched `omt-theme`'s
  importer, which maps *VS Code* keys that happen to be spelled the same.
  `host_colours` reads `$OMT_TERM_FOREGROUND` and `$COLORFGBG`, and no setting.

A string match is not a reader. The gate below looks for the key inside a
lookup, in the crates that actually hold the resolved configuration.

## Not read by anything

Fifty-nine keys. They parse, they validate, they appear in `config.get` with
their provenance, and nothing downstream asks for them:

```
agents.detection
appearance.block_style        appearance.cursor.blink       appearance.cursor.style
appearance.density            appearance.font.fallbacks     appearance.font.family
appearance.font.ligatures     appearance.font.line_height   appearance.font.size
appearance.show_agent_badges  appearance.theme              appearance.theme_light
appearance.theme_mode
font.family                   font.line_height              font.size
hints.enabled
log.file                      log.level                     log.max_size
log.redact_secrets
media.blob_dir
media.clipboard.osc52_max_bytes  media.clipboard.osc52_read  media.clipboard.osc52_write
media.images.enabled          media.images.max_rows         media.images.protocol
media.images.thumbnail_max_px media.osc_bridge.max_bytes
media.quota.max_blob_bytes    media.quota.max_blobs_per_session
media.quota.max_total_bytes   media.quota.ttl               media.quota.ttl_referenced
media.reverse_socket.hosts
server.bind
shell.auto_install            shell.command_metadata        shell.inject_env
shell.osc133                  shell.propagate_over_ssh      shell.shells
terminal.bell                 terminal.blocks.enabled       terminal.blocks.heuristic_fallback
terminal.blocks.max_retained  terminal.bracketed_paste      terminal.default_cwd
terminal.default_shell        terminal.env_passthrough      terminal.highlight
terminal.mouse_reporting      terminal.paste_confirm_lines  terminal.paste_confirm_on_newline
terminal.reflow.on_resize     terminal.scrollback_lines     terminal.word_separators
```

Three of them checked by hand, because a list is easier to believe than to
verify:

- `terminal.scrollback_lines` — `omt-term`'s `Terminal` is built with
  `ScrollbackLimits::default()` at `crates/omt-term/src/terminal.rs:200`, whose
  `max_lines` is the built-in 10 000. The configured number reaches nothing.
- `terminal.default_shell` — appears only in `docs/reference/settings.md`.
  Panes take the shell from the environment.
- `media.images.*` — the image path is unconditional; the enable switch, the
  protocol choice and the row cap are read nowhere.

> **Since this audit was written**, the binding file half of it is fixed: the
> two sections below record what was found, and
> `openspec/changes/archive/2026-08-17-the-binding-file-nothing-ever-opened`
> records what was done. `keymap` now comes from `keybindings.toml`, which
> `omt-config::keys` opens and `omt-input::resolve` compiles. The fifty-nine
> settings below are still unread.

## The key nothing could set

`crates/omt/src/run.rs` reads `keymap` through `terminal_bits::setting`, and
`keymap` is not in `SETTINGS`. A merge of a layer containing it answers:

```
value = None
diagnostic OMT-C101 keymap: unknown key `keymap`
```

So the copy-mode flavour was always the default, and anybody who wrote the key
into `config.toml` got an error rather than emacs motions.
`docs/architecture/10-configuration.md` §8.2 puts `keymap` in
`keybindings.toml` — a file nothing in the repository opens; see below.

## The file nothing opens

`keybindings.toml` is specified in `10-configuration.md` §8.2 and described to
users in `docs/guide/keybindings.md`. No Rust file in this repository mentions
it. `omt_input::keymap::BindingFile` is only ever built from the *input* of
`keys.check_file`, which checks a file somebody hands it; `keys.list` and
`keys.cheatsheet` answer from `omt_input::keymap::defaults()`, and the leader is
the constant `PREFIX_KEY` with a hard-coded `leader_action` match.

So the cheatsheet's claim to include "your own bindings" is not true, and a
rebind does nothing.

## Keeping this honest

The list above goes stale the moment a setting is wired up. What does not is
`cargo xtask settings-readers`, and the declaration it checks — `UNWIRED` in
`crates/omt-config/src/schema.rs`, which names every unread setting *and why*.

The gate fails in both directions:

- a setting nothing reads and nothing declares is a silent promise;
- an entry in `UNWIRED` for a setting that has since been wired up is a stale
  excuse, and stale excuses are how a list like this stops meaning anything.

Both were seen failing before the table was filled in:

```
`terminal.blocks` is a setting nothing reads. Wire it up, or add it to
`UNWIRED` in omt-config's schema with the reason.
`appearance.theme` has a reader and is still listed in `UNWIRED`. Remove the
entry: a stale excuse is how the list stops meaning anything.
```
