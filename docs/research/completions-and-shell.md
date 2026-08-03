# Completions, Shell Integration, History and Input Highlighting — Research for `oh-my-term`

Companion to [`another terminal.md`](another terminal.md) (§2 shell integration, §7 completions engine, §8 input/history)
and [`../architecture/04-terminal-core.md`](../architecture/04-terminal-core.md) §6–7. Those two
cover another terminal's approach; this document covers the rest of the landscape and ends with a build/buy
recommendation.

Evidence markers follow [`14-licensing.md`](../architecture/14-licensing.md) §4:
`[SRC]` read from upstream source, `[DOC]` from public documentation, `[INF]` labelled inference.

**Headline conclusions**, expanded in §6:

1. **omt should not own the input line.** Confirms the existing
   [P4](../architecture/01-principles.md) decision. §5 shows the constraint is not a
   preference — it is arithmetic about who owns the bytes.
2. **Completion data: adopt the Fig/Amazon-Q spec corpus. It is MIT, and there is now an
   Apache-2.0 successor.** `withfig/autocomplete` is MIT `[SRC]`; the live successor
   `aws/amazon-q-developer-cli-autocomplete` is **Apache-2.0** `[SRC]`. Both are clean for an
   Apache-2.0 project. Everything a naive reading would fear here — GPL contamination — comes not
   from the specs but from *native shell completion scripts* (§1.5, §6.1).
3. **Shell out to `carapace` as an optional dependency; do not reimplement it.** MIT `[SRC]`,
   Go binary, JSON output, and it already solves the hardest sub-problem (invoking bash/zsh/fish
   completion from outside those shells) in a way we can neither improve on nor safely copy.

---

## 0. Verified licence table

Everything in this document, resolved up front. Checked via the GitHub API on 2026-08-03 `[SRC]`.

| Project | Licence | Archived | Last push | Apache-2.0 compatible? |
|---|---|---|---|---|
| `withfig/autocomplete` (Fig specs) | **MIT** (© 2021 Hercules Labs Inc.) | no | **2025-05-05** | **yes** |
| `aws/amazon-q-developer-cli-autocomplete` | **Apache-2.0** | no | 2026-02-03 | **yes** |
| `aws/amazon-q-developer-cli` | Apache-2.0 | no | 2026-06-22 | yes |
| `microsoft/inshellisense` | MIT | no | 2026-08-03 | yes |
| `carapace-sh/carapace` (library) | Apache-2.0 | no | 2026-08-01 | yes |
| `carapace-sh/carapace-bin` (binary + completers) | MIT (© 2020 rsteube) | no | 2026-08-03 | yes |
| `carapace-sh/carapace-bridge` | MIT | no | 2026-08-01 | yes |
| `Valodim/zsh-capture-completion` (vendored in bridge) | MIT (© 2015 Vincent Breitmoser) | — | — | yes |
| `clap-rs/clap` (`clap_complete`) | Apache-2.0 / MIT | no | 2026-08-01 | yes |
| `sigoden/argc` | **Apache-2.0** | no | 2026-06-29 | yes |
| `atuinsh/atuin` | MIT (© 2021 Ellie Huxtable) | no | 2026-08-03 | yes |
| `cantino/mcfly` | MIT | no | 2026-04-14 | yes |
| `larkery/zsh-histdb` | MIT | no | 2024-04-27 | yes |
| `junegunn/fzf` | MIT | no | 2026-08-01 | yes |
| `zsh-users/zsh-syntax-highlighting` | BSD-3-Clause | no | 2026-02-08 | yes |
| **`fish-shell/fish-shell`** | **GPL-2.0** (GitHub reports `NOASSERTION` — mixed tree) | no | 2026-07-31 | **no — do not vendor** |
| **`scop/bash-completion`** | **GPL-2.0** | no | 2026-08-03 | **no — do not vendor** |
| zsh's own `Completion/` tree | zsh licence (MIT-like, but with the zsh advertising-free clause) | — | — | yes in principle; **not worth the audit** |

The two red rows are the whole licence story: **fish's and bash-completion's completion corpora are
GPL-2.0 and cannot be vendored into omt.** They can be *executed* as separate processes at runtime
— running a GPL program and reading its stdout is use, not derivation `[INF, standard reading]` —
which is exactly what carapace does and exactly what omt should do.

---

## 1. Completion spec ecosystems

The data is the expensive part. Nobody has ever failed to write a completion *engine*; people fail
to accumulate 600 CLI specs.

### 1.1 Fig / Amazon Q autocomplete specs

**Licence: MIT.** `[SRC] withfig/autocomplete/LICENSE` — verbatim first two lines:

```
MIT License

Copyright (c) 2021 Hercules Labs Inc. (Fig)
```

**Status.** The repo is *not* archived, but its last push was **2025-05-05** `[SRC]` — fifteen
months stale as of this writing. The README states "Amazon Q Developer CLI, formerly known as Fig,
is open source" `[DOC]`. The living fork is **`aws/amazon-q-developer-cli-autocomplete`**, licensed
**Apache-2.0**, last pushed 2026-02-03 `[SRC]`. Note the low star count (54) — it is a working
mirror inside the AWS org, not a community hub. The AWS-side feature set has moved toward agentic
chat; autocomplete is maintained but is no longer the product's centre of gravity `[INF]`.

**Practical read for omt** `[INF]`: the corpus is a *frozen asset with a maintained fork*, not a
living upstream. Treat it the way you would treat a terminfo database — vendor a snapshot, pin the
version, expect to patch it yourself, do not expect upstream to fix `kubectl` for you. The npm
package `@withfig/autocomplete` is the practical distribution vehicle; inshellisense pins
`2.675.0` `[SRC] inshellisense/package.json`. There is also `@withfig/autocomplete-types`, the
TypeScript schema alone.

**Schema.** another terminal's v2 signature schema (`another terminal.md` §7.2) is the Fig schema with another terminal-specific
renames; the shapes are the same. The load-bearing structures:

- `Spec` = default-exported `Fig.Subcommand`. Fields: `name` (string or string[]), `description`,
  `subcommands: Subcommand[]`, `options: Option[]`, `args: Arg | Arg[]`, `additionalSuggestions`,
  `loadSpec` (lazy sub-spec reference), `generateSpec` (runtime spec construction).
- `Option`: `name`, `description`, `args`, `isRepeatable`, `isRequired`, `exclusiveOn`,
  `dependsOn`, `requiresSeparator`, `isDangerous`, `hidden`, `priority`.
- `Arg`: `name`, `description`, `isOptional`, `isVariadic`, `template`
  (`"filepaths" | "folders" | "history" | "help"`), `suggestions`, `generators`, `default`,
  `isCommand`, `isScript`, `loadSpec`, `parserDirectives`.
- `Generator`: `script` (string or `(tokens) => string`), `postProcess`,
  `custom: (tokens, executeShellCommand, context) => Suggestion[]`, `trigger`,
  `getQueryTerm`, `cache: { ttl, strategy: "stale-while-revalidate" | "max-age" }`.

The dangerous bit for a Rust host: **`generators` are arbitrary JavaScript.** `script` alone is
declarative and cheap; `custom` and `postProcess` are functions. another terminal solved this by embedding a JS
engine (`crates/a JS engine crate`) and shipping compiled specs as an embedded asset `[SRC] another terminal.md §7.1`.
inshellisense is already JS so it pays nothing. **This is the single biggest hidden cost of
adopting Fig specs in Rust** `[INF]` — see §6.1 for the way out.

`az`, `gcloud` and `aws` specs are excluded by inshellisense "due to their large size"
`[DOC] inshellisense README` — they are megabytes of generated TS. Anyone adopting the corpus
inherits that problem.

### 1.2 inshellisense (Microsoft) — the closest prior art to omt

MIT, 10.6k stars, actively maintained (last push 2026-08-03) `[SRC]`. Self-described as
"a terminal native runtime for autocomplete" `[DOC]`.

**Its architecture is omt's architecture.** From `package.json` `[SRC]`:

```json
"dependencies": {
  "@withfig/autocomplete": "2.675.0",
  "@xterm/addon-unicode11": "^0.9.0",
  "@xterm/headless": "^6.0.0",
  "node-pty": "1.2.0-beta.8",
  ...
}
```

`node-pty` + `@xterm/headless` means: **it spawns the user's real shell in a PTY, parses the output
with a headless terminal emulator, and draws an overlay.** It is not a shell, not a shell plugin,
and not a line editor. That is precisely omt's position, which makes every constraint it hit a
constraint omt will hit.

**How it hooks the shell.** `shell/` contains `shellIntegration.{bash,fish,nu,ps1,xsh}`,
`shellIntegration-{env,login,profile,rc}.zsh`, and a vendored `bash-preexec.sh` `[SRC]`. The zsh
path is a ZDOTDIR redirect lifted from VS Code's design — `shellIntegration-env.zsh` verbatim
`[SRC]`:

```zsh
if [[ -f $USER_ZDOTDIR/.zshenv ]]; then
	IS_ZDOTDIR=$ZDOTDIR
	ZDOTDIR=$USER_ZDOTDIR
	# prevent recursion
	if [[ $USER_ZDOTDIR != $IS_ZDOTDIR ]]; then
		. $USER_ZDOTDIR/.zshenv
	fi
	USER_ZDOTDIR=$ZDOTDIR
	ZDOTDIR=$IS_ZDOTDIR
fi
```

It emits a **private OSC namespace, `6973`** ("ISE" on a dialpad, presumably) — from
`shellIntegration-rc.zsh` `[SRC]`:

```zsh
__is_prompt_start() { builtin printf '\e]6973;PS\a' > /dev/tty }
__is_prompt_end()   { builtin printf '\e]6973;PE\a' > /dev/tty }
__is_update_cwd()   { builtin printf '\e]6973;CWD;%s\a' "$(__is_escape_value "${PWD}")" }
```

Two details worth stealing outright:

- **`> /dev/tty`, not stdout.** Prompt marks written to stdout get captured by
  `$(...)` command substitution and by pipes. Writing to `/dev/tty` bypasses that.
  omt's snippet should do the same `[INF — omt's 04 §7.2 does not currently say this]`.
- **The prompt-start hook is bound to `zle-line-init`, not `precmd`**, with the original widget
  preserved and delegated to `[SRC]`:

  ```zsh
  if (( ${+widgets[zle-line-init]} )); then
      zle -A zle-line-init __is_orig_zle_line_init
      __is_zle_line_init() { __is_prompt_start; zle __is_orig_zle_line_init; __is_prompt_end }
  else
      __is_zle_line_init() { __is_prompt_start; __is_prompt_end }
  fi
  zle -N zle-line-init __is_zle_line_init
  ```

  This fires on *every* line-editor init including after `Ctrl-C` and after a `zle reset-prompt`,
  which `precmd` does not. It is a strictly better prompt-boundary signal than precmd-only marking.

**How it knows what the user typed.** `src/isterm/commandManager.ts` `[SRC]` — this is the file to
read if you read one thing. On `handlePromptEnd()` it registers an xterm marker and records
`promptEndX = buffer.active.cursorX`; the command text is then the buffer content between
`(promptEndMarker.line, promptEndX)` and the cursor. There is real-world grime in it:

- Right-prompt detection by looking at the last 5 cells of the prompt line
  (`rightPromptLookbackWidth = 5`) and, if non-blank, truncating the prompt at the first run of 4
  spaces (`commandWhitespaceTerminationWidth = 4`) `[SRC]`.
- A `#promptRewrites` flag per shell, because nushell *re-renders the accepted prompt line*, so
  accepted command lines are tracked in `#acceptedCommandLines` to avoid double-counting `[SRC]`.
- `onBufferChange` sets `#suspended = buffer.type === "alternate"` — **all command tracking is
  disabled on the alt screen**, matching another terminal's rule and omt's 04 §6.3.
- A `registerCsiHandler({final: "J"})` on `ED 2`/`ED 3` that resets prompt state, because `clear`
  invalidates every marker `[SRC]`.

And the piece that answers §4 of this brief:

```ts
private _isSuggestion(cell: IBufferCell | undefined): boolean {
    const color = this._getFgPaletteColor(cell);
    const dim = (cell?.isDim() ?? 0) > 0;
    const italic = (cell?.isItalic() ?? 0) > 0;
    const dullColor = color == 8 || color == 7 || (color ?? 0) > 235 || (color == 15 && dim);
    ...
}
```

inshellisense **detects the shell's own autosuggestion by its colour** (grey/dim/italic) so it can
exclude it from the command text. That is the state of the art for a terminal that does not own the
input line: you can *read* the line and *classify* its rendered attributes, but you did not produce
them and you cannot change them.

**Its rendering** is a popup drawn by `src/ui/suggestionRenderer.ts` over the shell's output, with
`batchScheduler.ts` coalescing PTY writes and `stdioProxy.ts` forwarding keys. It also exposes
`inshellisense complete "<line>"` as a plain subprocess API returning JSON — see §1.3, where
carapace consumes exactly that.

### 1.3 carapace — the multi-shell engine, and the bridge

Three repos `[SRC]`: `carapace-sh/carapace` (library, **Apache-2.0**),
`carapace-sh/carapace-bin` (binary + completer corpus, **MIT**), `carapace-sh/carapace-bridge`
(**MIT**). Ten shells: bash, cmd, elvish, fish, ion, nushell, oil, powershell, tcsh, zsh (several
experimental) `[DOC]`.

**Architecture.** Completers are Go code (cobra commands) plus a growing YAML **spec** format with
a published JSON Schema at `https://carapace.sh/schemas/command.json` `[SRC]`. An `Action` is the
core abstraction — a lazily-invoked value producer with combinators: `Cache`, `Chdir`, `Filter`,
`MultiParts`, `NoSpace`, `Prefix`, `Split`, `Style`, `Suffix`, `Tag`, `Timeout`, `Unique`,
`Invoke`, `Batch` `[SRC] carapace/docs/src/carapace/action/*`. Compare this to Fig's
`Generator` — carapace's is strictly more composable and, crucially, **declarative in YAML with no
embedded JavaScript**.

**The bridge is the interesting part** `[SRC] carapace-bin/docs/src/spec/bridge.md`. carapace can
import completions from other *frameworks* and other *shells*, addressed as macros in a YAML spec:

| Target | Macro | Mechanism |
|---|---|---|
| argcomplete (Python) | `$carapace.bridge.Argcomplete([az])` | env-var protocol |
| carapace | `$carapace.bridge.Carapace([freckles])` | native |
| carapace-bin | `$carapace.bridge.CarapaceBin([gh])` | native |
| clap (Rust) | `$carapace.bridge.Clap([dynamic])` | `<cmd> complete --index N --type 9 …` |
| click (Python) | `$carapace.bridge.Click([watson])` | env-var protocol |
| cobra (Go) | `$carapace.bridge.Cobra([kubectl])` | `__complete` hidden subcommand |
| posener/complete | `$carapace.bridge.Complete([vault])` | env-var protocol |
| **inshellisense** | `$carapace.bridge.Inshellisense([node])` | `inshellisense complete "<line>"` → JSON |
| kingpin | `$carapace.bridge.Kingpin([tsh])` | — |
| urfave/cli | `$carapace.bridge.Urfavecli([tea])` | — |
| yargs (JS) | `$carapace.bridge.Yargs([ng])` | — |
| **bash** | `$carapace.bridge.Bash([tail])` | §1.5 |
| **fish** | `$carapace.bridge.Fish([git])` | §1.5 |
| **zsh** | `$carapace.bridge.Zsh([git])` | §1.5 |
| powershell | `$carapace.bridge.Powershell([ConvertTo-Json])` | — |

Note that **`bridge.Inshellisense` exists**: carapace can consume the entire Fig corpus through
inshellisense's JSON API without ever parsing a TypeScript spec `[SRC]`. The consumer code is
short and the JSON shape is stable — `{ Suggestions: [{ Name, allNames, Description, Icon }] }`,
where `Icon` is an emoji discriminator (`📀` files, `🔗` flags) `[SRC]
carapace-bridge/pkg/actions/bridge/inshellisense.go`.

The bridge doc's own caveat is worth quoting `[SRC]`:

> Even when the command supports your current shell it is still beneficial to bridge it as this
> enables embedding like `sudo [spec.name] <TAB>`. It also avoids the issue of shell startup delay
> when sourcing the completion in init scripts […] However, bridging is limited to supported
> commands/frameworks and how well it actually works.

And, on the shell bridges specifically:

> Invoking completion in shells is quite tricky though and edge cases are likely to fail.

That sentence is the honest summary of §1.5.

**Invocation from an external program.** carapace's shell adapters all funnel into one call shape:
`carapace <completer> <shell> <args...>`, with `carapace --export <cmd> <args...>` (documented at
`docs/src/carapace/export.md`) producing a stable JSON envelope of
`{version, messages, nospace, usage, values: [{value, display, description, style, tag}]}`
`[DOC/INF — schema shape inferred from `internal/export/export.go` and the shell adapters; treat
the exact key list as unverified]`. For omt the practical contract is: *spawn `carapace export …`,
read JSON on stdout, done.* No FFI, no cgo, no linking, no licence entanglement.

### 1.4 Rust-native options

- **`clap_complete`** (Apache-2.0/MIT). Two distinct things: static script generation (`bash`,
  `zsh`, `fish`, `elvish`, `powershell`, and `clap_complete_nushell` as a separate crate), and
  **dynamic completion** — `CompleteEnv` / the `complete` subcommand, which is what
  `bridge.Clap` drives: `<cmd> complete --index N --type 9 --no-space --ifs=\n -- <args...>`
  `[SRC] carapace-bridge/pkg/actions/bridge/clap.go`. Value for omt: any clap-based CLI (a large
  and growing fraction of modern Rust tooling, including omt itself) can be completed with zero
  spec authoring. Cost: only covers clap.
- **`argc`** — **Apache-2.0** `[SRC]`, a comment-driven bash CLI framework that also ships
  `argc --argc-compgen <shell> <script> <args...>`, and separately maintains an *`argc-completions`*
  corpus generated by scraping `--help` output. Interesting as a *generation* strategy: scrape
  `--help`, emit a declarative spec. carapace has the same idea as `carapace-parse` /
  `carapace-scrape` / `carapace-generate` tools `[SRC] carapace-bin docs SUMMARY`.
- **`clap_complete_nushell`, `complete`, `shell-completion` crates**: none carry a spec corpus.
  There is no Rust-native equivalent of the Fig corpus and there will not be one. `[INF]`

**Conclusion of §1.1–1.4**: the two corpora that exist are Fig's (MIT/Apache-2.0, ~600 CLIs,
JS-flavoured, frozen) and carapace's (MIT, several hundred completers, Go+YAML, alive). Both are
licence-clean. Neither is written in Rust.

### 1.5 Invoking native shell completions from an external process

This is what another terminal does over OSC 9280 (`another terminal.md` §7.4) with the shell it already owns. An external
process without a live shell has to do it the hard way. carapace's three implementations are the
best documented reference `[SRC] carapace-bridge/pkg/actions/bridge/{bash,zsh,fish}.go`.

#### bash — `complete -p` + `COMP_*` + the completion function

Two steps. First, **discovery**: enumerate completion scripts by scanning the eight-tier
`bash-completion` search path, faithfully mirroring `_comp_load`
`[SRC] carapace-bridge/pkg/bridges/bash.go`:

```
$BASH_COMPLETION_USER_DIR/completions
$XDG_DATA_HOME/bash-completion/completions  |  ~/.local/share/bash-completion/completions
$BASH_SOURCE/completions
<each $PATH entry>/share/bash-completion/completions
$XDG_DATA_DIRS/*/bash-completion/completions
  fallback: /usr/local/share/bash-completion/completions, /usr/share/bash-completion/completions,
            /data/data/com.termux/files/usr/share/bash-completion/completions
/etc/bash_completion.d, /opt/homebrew/etc/bash_completion.d, /usr/local/etc/bash_completion.d
$HOMEBREW_PREFIX/{etc/bash_completion.d,share/bash-completion/completions}
```

Second, **invocation**: run `bash --rcfile <isolated .bashrc> -i <script>` with `COMP_LINE` in the
environment, where the script is `[SRC] carapace-bridge/pkg/actions/bridge/bash.sh`:

```bash
set +o history                                    # do not pollute the user's history
COMP_WORDS=($COMP_LINE)
[ "${COMP_LINE: -1}" = " " ] && COMP_WORDS+=("")
COMP_CWORD=$((${#COMP_WORDS[@]} - 1))
COMP_POINT=${#COMP_LINE}
source .../bash_completion                        # the bash-completion framework
__load_completion "${COMP_WORDS[0]}"              # lazy-load the per-command script
$"$(complete -p "${COMP_WORDS[0]}" | sed -r 's/.* -F ([^ ]+).*/\1/')"   # call the -F function
for i in "${COMPREPLY[@]}"; do ... echo "$i"; done
```

Pitfalls, every one of which is visible in that fifteen-line script:

- `COMP_WORDS=($COMP_LINE)` is **word splitting, not shell lexing** — quoted arguments with spaces
  break. This is a known-wrong approximation that carapace ships anyway.
- The regex only handles `complete -F <fn>`; `-C <command>`, `-W <wordlist>`, `-G <glob>`,
  `-o default` are all silently unsupported.
- `compopt -o nospace` is not read back; carapace hardcodes
  `NoSpace('/','=','@',':','.',',')` with a `// TODO check compopt` `[SRC]`.
- Sourcing bash-completion at all costs 50–200 ms `[INF]`; you must cache.
- `-i` (interactive) is required for some frameworks but means the isolated rc file must be
  genuinely isolated, or you run the user's whole `.bashrc` on every keystroke.

#### zsh — `zsh/zpty` + a `compadd` hook

zsh's `compsys` cannot be called as a function; it only runs inside ZLE. The canonical workaround
is **spawn zsh on a pseudo-terminal from inside zsh, override `compadd`, and press Tab**. carapace
vendors `Valodim/zsh-capture-completion` (MIT) for exactly this `[SRC]`. The shape:

```zsh
zmodload zsh/zpty
zpty z zsh -f -i                    # inner shell on a pty
zpty -w z source <init-script>      # init: PROMPT=; compinit; source isolated .zshrc
zpty -w z "$*"$'\t'                 # write the buffer plus a literal Tab
while zpty -r z; do :; done | ...   # read back
```

The init script `[SRC]`:

```zsh
PROMPT=
autoload -U compinit && compinit -d "$CONFIG/.zcompdump_capture"
source "$CONFIG/.zshrc"
[[ "$oldfpath" != "$fpath" ]] && compinit     # second compinit if the rc changed fpath
bindkey '^M' undefined                        # never run a command
bindkey '^J' undefined
bindkey '^I' complete-word
null-line () { echo -E - $'\0' }
compprefuncs=( null-line ); comppostfuncs=( null-line exit )   # NUL-delimit the output
zstyle ':completion:*' list-grouped false
zstyle ':completion:*' insert-tab false
zstyle ':completion:*' list-separator ''
compadd () { ... builtin compadd -A __hits -D __dscr "$@" ; ... echo -E - $IPREFIX$apre$hpre$hit$dsuf$hsuf$asuf" -- "$dscr }
```

The `compadd` override is the trick: it injects `-A __hits -D __dscr` so zsh's own matcher does the
matching and writes results into arrays instead of the display, then prints them one per line as
`value -- description`. It delegates untouched when `-O`/`-A`/`-D` are already present.

Pitfalls, each one load-bearing:

- **`bindkey '^M' undefined`** — without it, a completion that inserts a unique match followed by
  the shell accepting the line *runs the command*. This is a remote-code-execution-grade footgun in
  a completion path.
- Prefix/suffix reconstruction (`-P/-p/-S/-s` via `zparseopts`) is approximate; the upstream comment
  admits it cannot do zsh's `-r` remove-func magic.
- Directory-suffix `/` is emulated "in a half-assed way" by pattern-matching `-f` in the compadd
  args `[SRC, upstream's own wording]`.
- Output is `\r\n`-delimited (it is a pty) and **carries ANSI colour**, so it must be
  `stripansi`'d, and zsh-quoted (`\ `, `\$`, `\(` …) so it must be unquoted through a 22-entry
  replacer `[SRC] carapace-bridge/pkg/actions/bridge/zsh.go`.
- Two `compinit` calls are needed because sourcing the user's `.zshrc` may change `fpath`.
- Cost: a whole zsh process, a pty, and two `compinit`s per invocation. Hundreds of milliseconds
  cold `[INF]`. Cacheable only at the "which commands have completers" level, not per-keystroke.

#### fish — `complete --do-complete`, the only civilised one

fish exposes a first-class API `[SRC] carapace-bridge/pkg/actions/bridge/fish.go`:

```
fish --no-config --command 'set __fish_config_dir <dir>; source "$__fish_data_dir/config.fish"; source <dir>/config.fish; complete --do-complete="git chec"'
```

Output is `value\tdescription`, one per line. No pty, no hooks, no quoting circus. Discovery is
equally clean: `echo $fish_complete_path $__fish_data_dir/completions` then list `*.fish`
`[SRC] carapace-bridge/pkg/bridges/fish.go`.

**But**: fish's completions are GPL-2.0 (§0). Executing `fish` as a subprocess is fine. Vendoring
`share/completions/*.fish` into an Apache-2.0 binary is not.

#### Cross-cutting pitfalls of all three

1. **You must source the user's rc to be correct, and you must not source the user's rc to be
   safe.** carapace's answer is an *isolated, user-editable* rc under
   `~/.config/carapace/bridge/{bash,zsh,fish}/` that defaults to empty and that the user opts into
   populating `[SRC]`. That is the right shape: correctness is opt-in, and the default is fast and
   side-effect-free.
2. **Side effects are real.** A completion function may `cd`, may set variables, may hit the
   network (`aws`, `kubectl` against a cluster), may prompt. Running it per keystroke is not
   acceptable; running it on explicit Tab is.
3. **Performance.** bash ≈ 50–200 ms, zsh ≈ 200–800 ms cold, fish ≈ 50–150 ms `[INF, order of
   magnitude]`. None is viable synchronously on a keystroke. All must be async with a generation
   counter and a stale-result discard.
4. **another terminal's OSC 9280 approach avoids all of this** by asking the *already-running, already-warm,
   already-configured* shell — at the cost of needing a private protocol and a shell that is
   sitting at a prompt. `another terminal.md` §7.4's `IFS= read -d $'\4' -s "$(echo -e "TEMP?\e]9280;P\a")"
   < /dev/tty` trick — using an OSC as a `read` prompt so the query is not echoed — is the elegant
   part, and it is an interface fact omt may reimplement.

---

## 2. Shell integration protocols — standard vs proprietary

### 2.1 OSC 133 (FinalTerm / "semantic prompts")

The de-facto standard. The normative document is Per Bothner's `semantic-prompts.md` in the
freedesktop `terminal-wg` proposals; it is currently behind an anti-bot wall, so the sequences below
are corroborated from *emitters and consumers* rather than the spec text `[SRC/DOC]`.

| Sequence | FinalTerm name | Meaning |
|---|---|---|
| `OSC 133 ; A ST` | `FTCS_PROMPT` | start of the (primary) prompt |
| `OSC 133 ; B ST` | `FTCS_COMMAND_START` | end of prompt / start of user-editable input |
| `OSC 133 ; C ST` | `FTCS_COMMAND_EXECUTED` | command dispatched; output begins |
| `OSC 133 ; D [; <exit>] ST` | `FTCS_COMMAND_FINISHED` | command finished; `<exit>` optional |
| `OSC 133 ; P ; k=<kind> ST` | — | prompt mark **without** implicit fresh-line |
| `OSC 133 ; L ST` | — | ensure fresh line (rarely emitted) |

Parameters seen in the wild, read from real integration scripts `[SRC]`:

- `aid=<id>` — *application id*, correlating marks to a process. Ghostty's bash integration uses
  `$BASHPID`: `printf "\e]133;D;%s;aid=%s\a" "$ret" "$BASHPID"` and
  `printf "\e]133;A;redraw=last;cl=line;aid=%s\a" "$BASHPID"` `[SRC] ghostty.bash:177,188`.
  This is what lets a terminal ignore marks forged by a nested/remote process — and is exactly the
  role omt's `SetUserVar=omt_session` nonce plays in `04-terminal-core.md` §7.2.
- `cl=line` — click behaviour: translate a click inside the input zone into left/right arrow keys,
  bounded to one line. `cl=m` (multiline) also exists.
- `redraw=last` — tells the terminal the prompt will be redrawn, so it should not treat the region
  as final.
- `k=<kind>` — prompt kind for `133;P`: `i` = initial/primary prompt, `s` = secondary (PS2),
  `r` = right prompt, `c` = continuation. Ghostty: `PS1='\[\e]133;P;k=i\a\]'$PS1'\[\e]133;B\a\]'`
  and `PS2='\[\e]133;P;k=s\a\]'$PS2'\[\e]133;B\a\]'` `[SRC] ghostty.bash:145-146`.
  another terminal uses `\e]133;P;k=r\a` for the right prompt `[SRC] another terminal.md §2.4`.
- `cmdline=<quoted>` — kitty's zsh integration attaches the command text to `C`:
  `print -nu $fd -f '\e]133;C;cmdline=%q\a' -- "$1"` `[SRC] kitty-integration:227`.

**`A` vs `P` is a real distinction and easy to get wrong.** Ghostty's comment, verbatim `[SRC]`:

> Use 133;P (not 133;A) inside PS1 to avoid fresh-line behavior on […] 133;A with fresh-line is
> emitted once via printf below.

`133;A` implies "ensure we are at the start of a fresh line", which is destructive if emitted from
inside `PS1` on a redraw. So: emit `A` once, imperatively, before the prompt; use `P;k=…` for the
in-`PS1` markers. Ghostty also handles `ble.sh` by emitting `133;P;k=i` instead of `133;A` because
ble.sh does its own cursor tracking `[SRC] ghostty.bash:180-188`.

**Emitters**: kitty, Ghostty, WezTerm, iTerm2, VS Code, another terminal, inshellisense (as a secondary),
starship (opt-in), oh-my-posh, fish 3.6+, nushell, Windows Terminal's samples.
**Consumers**: kitty (prompt jump `ctrl+shift+z/x`, output browsing, click-to-move-cursor),
WezTerm (`ScrollToPrompt`, semantic zones), iTerm2 (marks, "select output of command"),
VS Code (command decorations, sticky scroll), Ghostty (jump-to-prompt, resize behaviour),
tmux 3.4+ (partial), another terminal (prompt-region routing only — it uses its own hooks for lifecycle).

**Known desync hazards** `[INF, corroborated by kitty's source comments]`:
- `Ctrl-C` at an empty prompt produces `A` with no intervening `C`/`D`.
- A `zle reset-prompt` (used by every "clock in the right prompt" plugin) re-emits `A`.
- kitty explicitly guards a state variable: *"0: no OSC 133 [AC] marks have been written yet.
  1: the last written OSC 133 C has not been closed with D yet"* `[SRC] kitty-integration:30-31`,
  and emits a bare `\e]133;D\a` to close a dangling `C` `[SRC] :158`.
- kitty strips its own marks back out of `PS1`/`PS2` before re-adding them
  (`PS1=${PS1//$'%{\e]133;A\a%}'}` `[SRC] :219-220`) so repeated sourcing does not stack marks.
  omt's installer must do the same.

### 2.2 The neighbouring OSCs

| Sequence | Origin | Payload |
|---|---|---|
| `OSC 7 ; file://<host><path> ST` | de-facto standard | cwd. kitty deviates: `kitty-shell-cwd://<host><path>` so it can distinguish its own reports `[DOC]` |
| `OSC 9 ; 9 ; <path> ST` | ConEmu / Windows Terminal | cwd, Windows convention. Cheap to also consume `[DOC]` |
| `OSC 1337 ; CurrentDir=<path> ST` | iTerm2 | cwd `[DOC]` |
| `OSC 1337 ; RemoteHost=<user>@<fqdn> ST` | iTerm2 | user + host `[DOC]` |
| `OSC 1337 ; SetUserVar=<key>=<base64> ST` | iTerm2 | arbitrary key/value; **the extensibility escape hatch**. WezTerm implements it and fires a `user-var-changed` event; sets `WEZTERM_PROG`, `WEZTERM_USER`, `WEZTERM_HOST`, `WEZTERM_IN_TMUX` `[DOC]` |
| `OSC 1337 ; SetMark ST` | iTerm2 | a navigable mark; VS Code implements it too `[DOC]` |
| `OSC 1337 ; ShellIntegrationVersion=<n> ; <shell> ST` | iTerm2 | version stamp — the mechanism omt's `OMT_SHELL_INTEGRATION` stamp mirrors `[DOC]` |
| `OSC 633 ; A/B/C/D/E/P ST` | VS Code | private superset of 133. `E ; <commandline> [; <nonce>]` sets the command line explicitly; `P ; <Prop>=<Val>` sets `Cwd`, `IsWindows`, `HasRichCommandDetection` `[DOC]` |
| `OSC 6973 ; PS / PE / CWD;<v> ST` | inshellisense | private prompt-start/end/cwd `[SRC]` |
| `OSC 9278`, `9277`, `9279`, `9280` | another terminal | JSON hooks, in-band generators, grid reset, completions `[SRC] another terminal.md §2.1` |
| `OSC 51 ; A ...` | Emacs / `EmacsClient` | Emacs-specific in-band messaging; effectively unused outside Emacs `[DOC]` |
| `OSC 2 ; <title> ST` | universal | kitty's integration sets it to cwd or the running command `[DOC]` |

Two observations for omt:

- **`OSC 633 ; E` is the feature omt's protocol lacks.** VS Code's `E ; <commandline> ; <nonce>` is
  the shell *telling* the terminal what the command text is, rather than the terminal reading it
  off the screen between `B` and `C`. That is strictly more reliable — it survives multi-line
  commands, right prompts, syntax-highlight redraws, and history-substring redraws. omt's
  `04 §7.2` gets `command_text` from "the OSC 133 B..C region or the `omt` hook"; if the `omt` hook
  is `SetUserVar=omt_cmd=<b64>` emitted from `preexec` immediately before `133;C`, omt gets VS
  Code's reliability without a new OSC number. **Recommended addition.** `[INF]`
- **`SetUserVar` really is enough.** WezTerm and iTerm2 both implement it, so an omt snippet that
  carries git/venv/nonce/command through `SetUserVar` degrades to *partially useful* in two other
  terminals rather than *worthless*, which was the stated rationale in `04 §7.2`.

### 2.3 Installers — the auto-injection techniques

The single most-copied idea in this space. Ranked by invasiveness, least first.

**kitty — zero files touched** `[DOC]`:

- **zsh**: set `ZDOTDIR` to kitty's own directory containing a `.zshenv` that restores the original
  `ZDOTDIR`, sources the user's `.zshenv`, then loads kitty's code. Normal zsh startup continues.
- **fish**: prepend kitty's directory to `XDG_DATA_DIRS`; fish autoloads
  `<dir>/fish/vendor_conf.d/*.fish`; the loaded script *removes its own path from `XDG_DATA_DIRS`*
  so children do not inherit it.
- **bash**: start bash in **POSIX mode** with `ENV=<kitty's script>`. POSIX mode suppresses bash's
  own startup files; kitty's script then disables POSIX mode and sources the user's files itself,
  in the right order. "User scripts see no behavioral difference versus vanilla bash."
- `KITTY_SHELL_INTEGRATION` carries a space-separated feature list; the integration code reads it,
  acts, and **unsets it** so it does not leak into the environment.
- Granularity: `enabled`, `disabled`, `no-rc`, `no-cursor`, `no-title`, `no-prompt-mark`,
  `no-complete`, `no-cwd`, `no-sudo`.
- The zsh code defers to `precmd_functions` so a user's `.zshrc` can opt out by *appending
  keywords to `KITTY_SHELL_INTEGRATION`* — the rc file gets a vote after the fact.

**Ghostty — same family, documented per shell** `[SRC] src/shell-integration/README.md`:
bash via POSIX-mode + `ENV`; zsh via temporary `ZDOTDIR` (restored after); fish and nushell via
`XDG_DATA_DIRS` + `vendor_conf.d` / vendor autoload; elvish via `XDG_DATA_DIRS` + `use`. Shell
detection is "a simple string match on the basename of the command to execute". Two honest failure
modes are documented: macOS's `/bin/bash` (3.2) does not support the automatic path, and a
system-wide `/etc/zshenv` that sets `ZDOTDIR` **overrides Ghostty's**, silently disabling
integration. Manual fallback is a two-line guarded `source "$GHOSTTY_RESOURCES_DIR"/…`.

**VS Code** — injects args and/or env at launch and gates on `$TERM_PROGRAM == "vscode"`; the
`terminal.integrated.shellIntegration.enabled` setting disables it. Its zsh implementation is the
`USER_ZDOTDIR` dance that inshellisense copied verbatim (§1.2) `[DOC]`.

**iTerm2** — the opposite pole: `curl -L https://iterm2.com/shell_integration/install_shell_integration.sh | bash`
appends a `source ~/.iterm2_shell_integration.$SHELL` line to the user's rc `[DOC]`. Explicit,
inspectable, permanent, and it works over ssh (because the *file* is on the remote host). It also
emits `ShellIntegrationVersion` so the terminal knows what it is talking to.

**starship** — not an integration installer but the relevant precedent for *not clobbering*: it
takes over the prompt entirely via `eval "$(starship init zsh)"`, and its OSC 133 support is opt-in
(`add_newline`, `format` containing the marks) rather than automatic, because a prompt framework
that unilaterally emitted marks would collide with the terminal's own injection. **omt's snippet
will collide with starship, kitty, and vscode-shell-integration in the same shell.** The mitigation
is the one another terminal uses: *detect existing `\e]133;A` in `PS1`/`PROMPT` and do not add a second*
`[SRC] another terminal.md §2.4` — plus kitty's strip-then-re-add idempotence `[SRC]`.

**bash-preexec** (MIT) — the universal answer to "bash has no `preexec`". It hooks `DEBUG` trap +
`PROMPT_COMMAND`. Vendored by another terminal `[SRC]` and by inshellisense `[SRC] shell/bash-preexec.sh`,
and named in omt's `04 §7.1`. Its documented limitation is in another terminal's own comment `[SRC] another terminal.md
§2.2`: for bash the reported command "will only include up to the first job control indicator, e.g.
`|`, `&&`". If omt sends command text via `SetUserVar` from `preexec`, bash will send a truncated
command. **Use `history 1` instead of the preexec argument in bash** to recover the full line
`[INF]`.

**zsh native hooks** — `precmd_functions`, `preexec_functions`, `chpwd_functions`, plus
`add-zsh-hook` for safe appending, plus `zle-line-init` / `zle-line-finish` widgets which fire on
every line-editor cycle (§1.2). Correct practice: always `add-zsh-hook`, never assign; always
`zle -A` the existing widget and delegate, never `zle -N` over it.

### 2.4 The five hard propagation problems

| Case | kitty | Ghostty | VS Code | iTerm2 | another terminal | **omt (04 §7.3)** |
|---|---|---|---|---|---|---|
| **(a) user's existing rc** | never touched; env-var injection only | never touched | never touched | appends one `source` line | appends a marked block | appends a marked, idempotent block + version stamp |
| **(b) subshells** | env vars inherit; `KITTY_SHELL_INTEGRATION` re-read | same | same | the `source` line is in the rc, so any new shell re-runs it | env inherit + `*_init_subshell.sh` assets + a synthetic "integrated" block | env inherit (`OMT_SESSION`, `OMT_SHELL_INTEGRATION`, nonce) |
| **(c) `su` / `sudo -i`** | **breaks** — env is scrubbed, `$HOME` changes, kitty's dir may be unreadable | breaks; has a `no-sudo` feature that only fixes *terminfo* | breaks | works if the *target user's* rc also sources a readable script | breaks; the rc-snippet path is the fallback | breaks unless `sudo -E`; must be documented, not papered over |
| **(d) ssh to a host without the terminal installed** | `ssh` kitten copies terminfo + integration to the remote and cleans up | documents `ssh-env`/`ssh-terminfo` features | remote-extension path, or nothing | the install script runs *on the remote* — best-in-class | detects `ssh …`, waits for a prompt heuristic, **appends a self-contained emitter to the remote rc**, confirms via a `SourcedRcFileForanother terminal` DCS callback. No binary installed. | same as another terminal, with explicit user confirmation |
| **(e) non-interactive shells** | `KITTY_SHELL_INTEGRATION` present but the rc is not sourced | same | same | rc not sourced | guards on interactivity | guards: refuse when non-interactive, when `TERM=dumb`, when already installed |

Two things stand out. **kitty's `ssh` kitten is the only project that solves (d) without writing to
the remote's rc** — it ships the integration over the connection into a temp dir for the duration of
the session `[DOC]`. That is a genuinely better answer than another terminal's rc-append, and it is worth
considering for omt: `omt ssh` (which `16-input-and-keymap.md` §7 already contemplates as a thin
client) could push the snippet into `$TMPDIR` on the remote and set `ENV`/`ZDOTDIR` through
`SendEnv`/a wrapper command, leaving nothing behind `[INF]`. The cost is that it only works for
omt-initiated ssh, not for `ssh` typed into a shell.

**Nobody solves (c).** `su` and `sudo -i` scrub the environment by design and switch `$HOME`. The
honest answers are: document it, detect it (the shell's `$USER` changes and the nonce is absent →
mark the block `origin: Heuristic`), and offer the same one-keystroke rc-append that ssh gets.

---

## 3. Command history

### 3.1 The field

| System | Store | Ranking | Sync | Licence |
|---|---|---|---|---|
| **fish** | `~/.local/share/fish/fish_history` (YAML-ish, `- cmd:` / `when:` / `paths:`) | recency + prefix for autosuggestion; `history merge` for cross-session | none | GPL-2.0 |
| **atuin** | SQLite, one row per command, plus an encrypted record log | filter-mode + search-mode, recency-ordered | yes, E2E encrypted | MIT |
| **zsh-histdb** | SQLite (`commands`, `places`, `history` — normalised) | frecency-ish via SQL views, cwd- and host-weighted | none | MIT |
| **mcfly** | SQLite + a small neural net | learned ranking over recency/frequency/cwd/exit/context | none | MIT |
| **fzf history** | the shell's own history file | fzf fuzzy score only | none | MIT |

### 3.2 atuin — the schema, verbatim

`[SRC] crates/atuin-client/migrations/20210422143411_create_history.sql`:

```sql
create table if not exists history (
	id text primary key,
	timestamp integer not null,
	duration integer not null,
	exit integer not null,
	command text not null,
	cwd text not null,
	session text not null,
	hostname text not null,
	unique(timestamp, cwd, command)
);
create index if not exists idx_history_timestamp on history(timestamp);
create index if not exists idx_history_command on history(command);
```

Later migrations add, in order `[SRC]`: `deleted_at integer` (2023 — tombstones, because sync needs
deletes), `author text` + `intent text` (2026-02 — "who ran this, and why": the human/agent
attribution problem omt calls `Attribution` in `04 §6.2`), `shell text` (2026-07). Plus indexes
added in 2026-07 for `active_history`, `filtered_history`, `hostname`, and a *dropped*
`command` index — i.e. they learned that indexing the command text was not paying for itself.

Note what is **not** in the schema: no git branch, no exit-signal distinction, no parent process, no
tty. another terminal's `commands` table `[SRC] another terminal.md §8.2` has `git_branch`, `start_ts`/`completed_ts` as
separate columns, `username`, `shell`, `workflow_command`, `is_agent_executed`. another terminal's is closer to
what omt wants.

### 3.3 atuin's ranking signals

Not a score — a **filter dimension plus a search mode** `[SRC] crates/atuin-client/src/database.rs`:

```
FilterMode::Global          → no constraint
FilterMode::Host            → hostname = ?
FilterMode::Session         → session = ?
FilterMode::SessionPreload  → session-scoped with a preloaded window
FilterMode::Directory       → cwd = ?
FilterMode::Workspace       → cwd LIKE '<git_root>%'
```

`SearchMode` is `Prefix | FullText | Fuzzy | Skim`. Ordering is `ORDER BY f.timestamp` — **pure
recency within a filter**, not frecency `[SRC]`. Dedup is a window function, and the comment on it
is instructive `[SRC] database.rs:785,1018`:

```sql
ROW_NUMBER() OVER (PARTITION BY command, cwd, hostname ORDER BY timestamp DESC)
```
> …than GROUP BY so that the timestamp-ordered scan can stop as soon as [enough rows are found]

That is the performance lesson: **dedup with a window function inside a subquery, not `GROUP BY`**,
so the scan is early-terminating. another terminal does the equivalent in Rust with a reverse walk and two
independent `HashSet`s — one for commands, one for AI prompts `[SRC] another terminal.md §8.2`.

`FilterMode::Workspace` is worth calling out: cwd-prefix-matched against the **git root**, gated on
a `workspaces` setting (default `false`) `[SRC] settings.rs:1488`. This is the right cwd signal —
"same repo" beats "same directory" for a developer.

**mcfly** is the only one that learns. It trains a small network on features including recency,
frequency, exit status, cwd match, and whether the previous command matched, and re-ranks
accordingly. It is the most *satisfying* design and the least *predictable* one; the recurring user
complaint is non-reproducible ordering `[INF]`. For omt, a **transparent weighted score** —
"recency × frequency, ×2 if same cwd, ×1.5 if same git root, ×0.25 if exit≠0" — captures most of
mcfly's value while remaining explainable in `omt doctor`.

### 3.4 Privacy and secret redaction

atuin's is the only serious implementation, and it is a two-layer design `[SRC]`.

**Layer 1 — regex denylist at write time.** `crates/atuin-client/src/secrets.rs` opens with
`// This file will probably trigger a lot of scanners. Sorry.` and defines
`SECRET_PATTERNS: &[(&str, &str, TestValue)]` — a `(name, regex, self-test)` triple per pattern,
compiled into a single `RegexSet`. Covered: AWS access key IDs (`A[KS]IA[0-9A-Z]{16}`),
`AWS_SECRET_ACCESS_KEY` / `AWS_SESSION_TOKEN` / `AZURE_.*_KEY` / `GOOGLE_SERVICE_ACCOUNT_KEY` env
assignments, `atuin\s+login`, GitHub PATs old and new (`ghp_`, `gh1_`, `github_pat_`, `gho_`,
`ghu_`, `ghs_`, `ghr_`, `v1\.[0-9A-Fa-f]{40}`), GitLab `glpat-`, Slack `xoxb-`/`xoxp-`/webhook
paths, Stripe `sk_test_`/`sk_live_`, Netlify `nf[pcoub]_`, npm `npm_`, Pulumi `pul-`. Enabled by
default: `secrets_filter` defaults to `true` `[SRC] settings.rs:1490`.

The **self-test-per-pattern** design is the part to copy: every entry carries a string that must
match, and a single `#[rstest]` asserts all of them, both bare and embedded in surrounding text
`[SRC]`. A secret-redaction denylist without per-pattern tests rots silently.

**Layer 2 — user-configurable exclusion.** `history_filter: RegexSet` (drop matching commands
entirely) and `cwd_filter: RegexSet` (drop everything from matching directories, e.g. a customer's
repo) `[SRC] settings.rs:1063-1066`. Also `store_failed: bool` (default `true`) — whether to record
commands that exited non-zero `[SRC] :1513`.

**Layer 3 — encryption in transit and at rest on the server.** `PASETO v4.local` with **envelope
encryption**: a random content-encryption key per record, wrapped with the user's master key via
PASERK `PieWrappedKey` `[SRC] record/encryption.rs`. The rationale comment is explicit that this
was chosen for HSM/KMS-compatibility and cheap key rotation ("Rotating a key is as simple as
re-encrypting the CEK"). The record store itself `[SRC] record-migrations/*`:

```sql
create table if not exists store (
  id text primary key,   -- globally unique ID
  idx integer,           -- incrementing integer ID unique per (host, tag)
  host text not null, tag text not null,
  timestamp integer not null, version text not null,
  data blob not null, cek blob not null
);
create unique index record_uniq ON store(host, tag, idx);
```

An append-only, per-`(host, tag)` monotonically indexed log of opaque encrypted blobs. Sync is
"tell me your highest idx per (host, tag), send me the gap" — no conflict resolution needed because
each host only ever appends to its own chain. The earlier `records` table had a `parent text unique`
hash-chain; the `store` design replaced it with an integer index. **This is the correct design for
omt's own cross-device sync** if it ever wants one (`12-collaboration.md` territory), and it is MIT.

### 3.5 What omt should store

`[INF]` — a merge of another terminal's columns, atuin's discipline, and omt's own `Attribution`:

```sql
CREATE TABLE commands (
  id            INTEGER PRIMARY KEY,
  command       TEXT NOT NULL,
  cwd           TEXT,
  git_root      TEXT,           -- for FilterMode::Workspace-equivalent
  git_branch    TEXT,
  exit_code     INTEGER,        -- NULL while running
  exit_signal   INTEGER,        -- distinct from code; 130/141 are not failures
  started_at    INTEGER NOT NULL,
  finished_at   INTEGER,
  duration_ms   INTEGER,
  session_id    TEXT NOT NULL,  -- omt session nonce
  host          TEXT, user TEXT, shell TEXT,
  attribution   TEXT NOT NULL,  -- 'human' | 'agent:<kind>' | 'unknown'
  agent_run_id  TEXT,
  block_id      INTEGER,        -- back-link into the block list
  redacted      INTEGER NOT NULL DEFAULT 0,
  deleted_at    INTEGER
);
```

Ranking signals, in the order they earn their keep: **same git root > same cwd > recency >
frequency > same session > exit status**. Dedup at query time with a window function, never at
write time (you lose the frequency signal). Redact at write time with atuin's pattern list plus
per-pattern tests. Default `store_failed = true` but rank failures down — a failed command is often
exactly what you want to recall and edit.

---

## 4. Syntax highlighting of the input line — the honest analysis

### 4.1 How the two existing implementations work, and why neither transfers

**fish** highlights natively: the line editor owns the buffer, so on every keystroke it re-lexes
the buffer, classifies tokens (command found on `$PATH`, valid path, unclosed quote, …), and
re-emits the line with SGR attributes. It is *the shell writing coloured bytes to the terminal*.

**zsh-syntax-highlighting** (BSD-3-Clause) is a ZLE plugin: it hooks the `zle-line-pre-redraw`
widget and sets the `region_highlight` array, which ZLE consults when it redraws. Again: *the shell
writing coloured bytes*.

**another terminal** highlights the input line because another terminal *is* the line editor — `InputBufferModel { buffer_value, cursor_point }` `[SRC] another terminal.md §8.1`, with colour derived from the completer's own token
classification (`Command→Green, Subcommand→Blue, Variable→Magenta, Argument→Cyan, Option→Yellow`).
another terminal's shell runs with its line editor effectively bypassed.

In all three cases the entity that colours the text is the entity that owns the buffer.

### 4.2 What omt can and cannot do

omt runs a real shell in a real PTY. The shell's line editor owns the input line. Therefore:

**Impossible without owning the input line:**

1. **Recolouring the user's input as they type.** The shell has already written those bytes with its
   own SGR. omt would have to overwrite the same cells with different attributes on every keystroke,
   racing the shell's own redraw. Any redraw the shell performs — history search, autosuggestion
   update, `zle reset-prompt`, a right prompt clock tick — clobbers omt's paint. This is not "hard";
   it is a lost race, every time, with visible flicker.
2. **Inline "ghost text" autosuggestion.** Same reason. Both fish and `zsh-autosuggestions` do this
   by inserting dim text *into the buffer region* the shell owns.
3. **Cursor-position-aware inline editing affordances** — click-to-position beyond what
   `OSC 133;A;cl=line` already delegates to the shell via synthetic arrow keys.
4. **Error underlining on an unknown command as you type**, in place.

**Possible, and cheap:**

1. **Reading the input line.** Between the `133;B` mark and the cursor, exactly as inshellisense
   does (§1.2). Reliable enough to power everything below. Made *fully* reliable by having the
   shell report the line itself via `SetUserVar=omt_cmd` at `preexec` (§2.2) — though that arrives
   only at submit time, not per keystroke.
2. **A completion popup overlaid on the shell's output.** inshellisense proves this works across
   bash/zsh/fish/pwsh/nu/xonsh with a headless emulator and a PTY. Keys are intercepted before
   forwarding; on accept, omt writes the completion text into the PTY as if typed.
3. **Post-hoc highlighting of the *command* in a closed block.** Once `133;C` fires and the command
   text is known, omt renders the block header itself — that is omt's own surface, and it can be
   coloured however omt likes. **This delivers most of another terminal's visual signature for none of the
   risk**, and it is the recommendation.
4. **Highlighting in omt's own input surfaces**: the command palette, the agent prompt box, a
   "compose a command" overlay, the web/mobile composer (`08-web-client.md` §6.2 already builds a
   real completion popup there). These are omt-owned buffers with no shell contention.
5. **Detecting the shell's autosuggestion** by cell attributes (dim/grey/italic), so omt does not
   mistake it for typed input `[SRC] inshellisense commandManager.ts`.

**The middle path, and its cost**: omt could ship an optional *shell plugin* — a zsh ZLE widget /
fish binding that does the highlighting in-shell, driven by an omt-side completer over a private
channel. This is exactly another terminal's OSC 9280 pattern inverted. It works, it is the only way to get real
inline highlighting, and it costs: a per-shell plugin to maintain, a keystroke-latency round trip to
omt and back, guaranteed conflict with `zsh-syntax-highlighting` and `zsh-autosuggestions` (which
most zsh users already run), and a hard dependency on the user installing something. `[INF]`

**Verdict:** ship (3) and (4) now; keep (2) as a Tab-triggered popup; treat the shell plugin as a
never-unless-users-demand-it. Recolouring someone else's line editor is a category error.

---

## 5. Should omt own the input line? Both sides.

### 5.1 The case for owning it

- Every another terminal-class polish item becomes available: inline highlighting, ghost-text autosuggestion,
  a real completion menu with descriptions, vim/emacs editing that is *ours* and therefore uniform
  across TUI, web and mobile, multi-line editing with a real editor, and a single place to
  implement `Attribution`.
- **Parity across surfaces** is the strongest argument, and it is an omt principle
  ([P3](../architecture/01-principles.md)). The web and mobile clients *must* have an omt-owned
  composer — there is no shell line editor on a phone. If the TUI defers to the shell and the phone
  does not, omt maintains two completion UXs forever.
- Typeahead capture stops being a problem to solve (`another terminal.md` §2.4's `ESC-i` `InputBuffer` hook
  exists only because another terminal owns the line).
- Blocks become cleaner: omt knows the command text exactly, always, with no screen-scraping.

### 5.2 The case against

- **It breaks the user's shell.** Every zsh user with `zsh-autosuggestions`,
  `zsh-syntax-highlighting`, `fzf-tab`, `zsh-vi-mode`, `atuin`, `starship`, or a custom ZLE widget
  loses all of it. That is not a minority; on zsh it is close to the majority. another terminal can absorb this
  because another terminal is a product with a marketing budget; an open-source terminal that silently disables
  a user's dotfiles gets uninstalled.
- **It requires bypassing the shell's line editor**, which means either running the shell
  non-interactively (losing job control, `$?`, aliases, functions, and every rc-defined behaviour)
  or fighting ZLE for the buffer. another terminal does the former for its own generators via a private channel
  and still keeps a real interactive shell for the user; the complexity in `another terminal.md` §2.5 and §8.1
  is the price.
- It contradicts [P4](../architecture/01-principles.md) — *observe, never re-implement* — which
  `04 §6.2` already invoked to justify the shared-scrollback block design. Reversing it re-opens
  the three-grid architecture omt explicitly rejected.
- **Correctness cost is unbounded.** Shell quoting, history expansion (`!!`, `!$`), alias
  expansion, glob expansion, `$(...)` nesting, here-docs, multi-line `for` loops, vi-mode, bracketed
  paste, IME composition — the line editor is a shell feature with thirty years of accreted
  behaviour, and every gap is a bug report.
- inshellisense — the project that tried hardest to get another terminal-class autocomplete without being a
  shell — **did not own the input line**, and its architecture is the one that shipped for six
  shells on three platforms.

### 5.3 Recommendation

**omt should not own the input line in the terminal pane. It should own every other input surface
it already has, and it should make those surfaces good enough that the asymmetry reads as
intentional.**

Concretely `[INF]`:

1. **Terminal pane**: the shell owns the line. omt reads it (§4.2 mechanism 1), overlays a
   Tab-triggered completion popup (mechanism 2), and highlights the command *in the block header
   after submission* (mechanism 3). No inline recolouring, no ghost text, ever.
2. **omt-owned surfaces** — command palette, agent prompt box, web/mobile composer, `omt run`
   quick-command overlay — get the full treatment: completer-driven highlighting, descriptions,
   history ranking, the works. This is one `omt-input` crate shared by all of them and by the web
   client via `03-capability-catalog.md`'s enumeration.
3. **Parity is preserved by promoting the composer, not the terminal.** On mobile, the composer
   *is* the input. On desktop, the composer is one keystroke away and is where the good UX lives.
   A user who wants another terminal's experience opens the composer; a user who wants their dotfiles types at
   the prompt. Both work, neither is degraded.
4. **Revisit only if** telemetry or user reports show the composer is unused and the demand for
   inline highlighting is loud. It is a reversible decision in that direction; owning the line first
   and backing out is not.

---

## 6. Build/buy recommendation

### 6.1 Completion data: buy, in three tiers

**Tier 0 — omt's own commands.** `clap_complete`'s dynamic mode, for free, because omt is a clap
CLI. Apache-2.0/MIT. Zero work.

**Tier 1 — `carapace` as an optional external dependency. Recommended primary.** `[INF]`

Detect `carapace` on `$PATH`; if present, get completions by spawning
`carapace <cmd> export <args…>` and parsing JSON. If absent, omt still works with tier 2 and tier 3
and shows a one-line "install carapace for richer completions" hint in `omt doctor`.

Why this and not a reimplementation:

- **Licence**: MIT / Apache-2.0 across all three carapace repos. Spawning a subprocess creates no
  derivation question at all.
- **It already solves the bridges.** §1.5 showed that invoking bash/zsh/fish completion externally
  is a swamp of pty tricks, `compadd` overrides, ANSI stripping and 22-entry unquote tables. That
  code exists, is MIT, is maintained, and is *not* something omt should carry in Rust.
- **It already bridges the Fig corpus** via `bridge.Inshellisense`, so tier-1 and tier-2 compose.
- It is a single static Go binary, installable from every package manager, with no runtime deps.
- The cost is one process spawn per completion request — which is *already* what any correct
  solution costs (§1.5 pitfall 3), so it is not a regression.

**Tier 2 — a vendored snapshot of the Fig corpus, parsed as data.** `[INF]`

The corpus is MIT (or Apache-2.0 from the AWS fork) and it is the only source of *descriptions* for
600 CLIs, which is what makes the popup feel like an IDE rather than a word list. But:

- **Do not embed a JavaScript engine.** another terminal did (`a JS engine crate` + `RustEmbed` of compiled specs) and it
  is a large, permanent cost. Instead, run an **offline transform**: a build-time script that
  evaluates each spec once in Node, walks it, and emits a *declarative subset* — names, aliases,
  descriptions, subcommand/option/arg trees, `template` values, and `generator.script` strings —
  as a compact binary blob (postcard/rkyv) embedded with `include_bytes!`. Specs whose behaviour is
  genuinely dynamic (`custom`, `postProcess`, `generateSpec`) either degrade to their static parts
  or are dropped, and the transform reports coverage.
- Skip `aws`, `az`, `gcloud` (multi-megabyte, generated) exactly as inshellisense does; delegate
  those to tier 1 or to the tools' own completion.
- Pin the version, record it in `NOTICE`, and treat updates as a manual, reviewed operation.
- Prefer the **Apache-2.0** `aws/amazon-q-developer-cli-autocomplete` tree as the snapshot source
  over MIT `withfig/autocomplete`: same data, licence identical to omt's, and it is the one still
  receiving commits.

**Tier 3 — omt-native declarative specs** in the config directory, for the long tail and for user
overrides. Use a schema that is a *subset of carapace's YAML spec* so users can move specs between
the two, and so omt can consume carapace specs directly. Do not invent a third format.

**What omt must never do**: vendor `bash-completion`'s scripts, fish's `share/completions/*.fish`,
or a translated-to-Rust version of either. Both are GPL-2.0. Executing them via carapace or via
omt's own subprocess call is fine; shipping them, or shipping code derived from them, is not.

### 6.2 Input line

Recommendation as stated in §5.3: **defer to the shell in the terminal pane; own the composer,
palette and agent box.** The completion machinery from §6.1 is shared: the same
`omt-complete` crate serves the Tab popup over the terminal pane *and* the composer, differing only
in who renders and who inserts.

### 6.3 What omt ships for shell integration, and how it installs safely

Confirming and extending `04-terminal-core.md` §7 `[INF]`:

**Emit** — per prompt cycle, unchanged from `04 §7.2`, with three additions:

```
precmd:   OSC 133 ; D ; <exit> ; aid=<nonce>            ST     # cheap, first, closes the block
          OSC 7 ; file://<host><pwd>                    ST
          OSC 1337 ; SetUserVar=omt_git=<b64>           ST
          OSC 1337 ; SetUserVar=omt_env=<b64>           ST
PS1:      OSC 133 ; P ; k=i ; aid=<nonce> ST  …PS1…  OSC 133 ; B ST      # ← P, not A (§2.1)
PS2:      OSC 133 ; P ; k=s ST  …PS2…  OSC 133 ; B ST
RPROMPT:  OSC 133 ; P ; k=r ST  …
once, imperatively, before the prompt:   OSC 133 ; A ; redraw=last ; cl=line ; aid=<nonce> ST
preexec:  OSC 1337 ; SetUserVar=omt_cmd=<b64 full command line>  ST      # ← new (§2.2)
          OSC 133 ; C                                    ST
```

Changes from the current draft, each justified above:
- **`133;P;k=…` inside `PS1`/`PS2`/`RPROMPT`, `133;A` once imperatively** — `A` implies fresh-line
  and is destructive on redraw (§2.1, Ghostty's comment).
- **`SetUserVar=omt_cmd`** from preexec — VS Code's `OSC 633;E` reliability without a new OSC
  number, and it makes multi-line and right-prompt command extraction exact (§2.2). In bash, take
  the text from `history 1`, not from bash-preexec's argument, which truncates at `|`/`&&`
  (§2.3).
- **All marks written to `/dev/tty`, not stdout** — otherwise `$(...)` capture eats them (§1.2).

**Consume** — OSC 133 A/B/C/D/P (all parameters, ignoring unknown ones), OSC 7, OSC 9;9,
OSC 1337 `CurrentDir`/`RemoteHost`/`SetUserVar`/`SetMark`, and OSC 633 A/B/C/D/E/P as an alias
family so VS Code-integrated shells work unmodified. Ignore metadata whose `aid=`/nonce does not
match the session.

**Install** — a hybrid of kitty's non-invasiveness and iTerm2's explicitness:

1. **Default (omt-spawned shells): inject, do not write.** kitty's technique, per shell:
   zsh → `ZDOTDIR` shim that restores `USER_ZDOTDIR` and sources the user's `.zshenv`/`.zshrc`;
   fish → prepend to `XDG_DATA_DIRS`, autoload from `vendor_conf.d`, then remove the path entry;
   bash → POSIX mode + `ENV`, then un-POSIX and source the user's files in order;
   pwsh → wrap `prompt`. Nothing is written to the user's disk in the common case. Carry
   `OMT_SHELL_INTEGRATION=<version>` and `OMT_SESSION=<nonce>`, and **unset the feature-list
   variable after reading it**, kitty-style.
2. **`omt integrations install`** remains, for users whose shell is not omt-spawned (tmux started
   elsewhere, `su`, a system `/etc/zshenv` that overrides `ZDOTDIR` — Ghostty documents this exact
   failure). It writes a version-stamped file under `~/.config/omt/shell/` and appends one marked,
   idempotent `source` block to the rc; `uninstall` removes exactly that block.
3. **Idempotence and coexistence, non-negotiable.** Before adding marks, strip omt's own marks from
   `PS1`/`PS2`/`RPROMPT` (kitty's `PS1=${PS1//…}`), and **scan for a pre-existing `\e]133;` in the
   prompt variables; if present, do not add a second set** (another terminal's check). Detect
   `$KITTY_SHELL_INTEGRATION`, `$TERM_PROGRAM=vscode`, `$ITERM_SHELL_INTEGRATION_INSTALLED`,
   `$GHOSTTY_RESOURCES_DIR` and stand down rather than double-mark.
4. **Refuse** when non-interactive, when `TERM=dumb`, when `$OMT_SHELL_INTEGRATION` matches the
   current version, or when the rc file is a symlink into a dotfiles repo without `--force` (the
   user's dotfiles are under version control and an unexpected diff is a real annoyance).
5. **ssh**: prefer kitty's approach — `omt ssh` pushes a self-contained emitter into the remote's
   `$TMPDIR` for the session's lifetime and sets `ENV`/`ZDOTDIR` through the wrapper, leaving
   nothing behind. Fall back to another terminal's rc-append **only with explicit per-host confirmation**, as
   `04 §7.3` already requires. Never install a binary on the remote.
6. **`omt doctor shell`** must exist and must say, per shell: whether integration is loaded, which
   version, whether marks were found in `PS1`, which other terminal's integration is also present,
   and — for completions — whether `carapace` is on `$PATH` and which bridges it reports.

### 6.4 Effort ordering

1. OSC 133 consume + emit with the §6.3 corrections. (Already planned; the corrections are cheap.)
2. `SetUserVar=omt_cmd` for exact command text. Small, high fidelity gain.
3. History store (§3.5) + redaction with atuin's tested-pattern discipline. Self-contained.
4. Block-header command highlighting driven by tier-0/tier-1 completion data. Highest
   polish-per-line-of-code in this document.
5. carapace integration (tier 1) behind a capability probe.
6. The composer's completion popup, shared with web/mobile.
7. Fig-corpus offline transform (tier 2). Largest single chunk; do it only after 5 proves the
   popup UX is worth feeding.
8. kitty-style zero-touch injection for omt-spawned shells. Fiddly per shell; do it once the
   emitted protocol has stopped changing.

---

## 7. Open questions

1. Does `carapace --export`'s JSON envelope have a stability guarantee? The schema was inferred,
   not read from a versioned document. Worth pinning a version and adding a contract test.
2. The Fig-corpus offline transform's coverage is unknown until measured: what fraction of the ~600
   specs are purely declarative, and what fraction lose meaningful behaviour when `custom` /
   `postProcess` generators are dropped? Measure before committing to tier 2.
3. `04 §7.2` justified `SetUserVar` partly on interoperability. Is `SetUserVar` actually implemented
   by enough terminals to matter, or is the real audience just omt + iTerm2 + WezTerm? If the
   latter, the argument for it over a private OSC weakens — though not enough to reverse the
   decision.
4. Does omt want atuin's record-log sync design for its own history sync, or is history sync out of
   scope entirely? The design is MIT and directly applicable, but it implies a server.
5. `su`/`sudo -i` (§2.4 case c) has no good answer anywhere. Is "detect and degrade to
   `origin: Heuristic`" acceptable, or does omt want the rc-append offer there too?
