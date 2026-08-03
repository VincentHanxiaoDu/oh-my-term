# Semantic Open — recognizing, resolving and acting on terminal text

Terminal output is full of pointers. `src/parser.rs:412:9` is a place in a file.
`https://github.com/x/y/pull/31` is a page. `a1b2c3d` is a commit.
`File "app/models.py", line 88` is a stack frame. A human reads these as links;
omt should too, and should let the user *act* on them — open the file in their
editor, jump to it in the workspace explorer, hand the path to the agent that is
already running in the next pane, or just copy it.

This document specifies that subsystem: how matches are recognized, how they are
resolved to concrete targets, what actions exist, and — the part that occupies
half the document — how any of this can work at all given that the program
inside the pane usually owns the mouse, and that the terminal emulator *outside*
omt frequently thinks it owns the click too.

The one-paragraph summary, stated up front so nothing below reads as overselling:

> **Keyboard hint mode is the primary mechanism, not the fallback.** Modifier
> click is offered, and works, but it is contingent on two pieces of software omt
> does not control (the inner TUI's mouse mode and the outer emulator's own
> handlers). A chord that labels every match on screen and waits for a two-letter
> label depends on neither, is identical on the TUI and the web client, and is
> the only design that satisfies [P3](01-principles.md#p3--parity-one-capability-three-surfaces)
> without qualification. Everything else is an accelerator on top of it.

Related documents:

- [01 — Principles](01-principles.md) — P2 pluggable, P3 parity, P4 native
  semantics, P8 security
- [03 — Capability catalog](03-capability-catalog.md) — the declaration style §8 uses
- [04 — Terminal core](04-terminal-core.md) — `Target`, `Position`, OSC 8, OSC 133
  blocks, mouse modes, selection. **This document is the layer above `omt-term` §8.**
- [09 — SSH and media](09-ssh-and-media.md) — the blob store and file transfer
  protocol §5 reuses verbatim
- [10 — Configuration](10-configuration.md) — `[open]` keys, keybinding format
- [15 — Workspace explorer](15-workspace-explorer.md) — `confine()`, sensitivity,
  `workspace.files.reveal`
- [16 — Input and keymap](16-input-and-keymap.md) — chord semantics, modifier
  forwarding, `omt doctor keys`. 16 owns input semantics and the trigger grammar;
  this document owns semantic open and the mouse *activation policy* that
  [16 §6.2](16-input-and-keymap.md#62-the-hard-case--omt-vim-mode-vs-a-real-vim-in-a-pane)
  now records as the one sanctioned exception to its mouse-suppression rule.
- Research: [iTerm2 §8.4](../research/iterm2.md#84-smart-selection--semantic-history)
  (semantic history, smart selection, `iTermPathFinder`, `iTermCachingFileManager`)

---

## 1. Scope and placement

### 1.1 What this subsystem is

Three stages, cleanly separated because they have different purity, different
trust and different homes:

```
  bytes → grid            recognition          resolution           action
  ───────────────    ─────────────────────  ─────────────────  ──────────────────
      omt-term         omt-term §8.3           omt-open           omt-open
      (existing)       Match { rule, span,     ResolvedTarget     OpenHandler
                               captures }      { kind, exists,    → editor / explorer
                                                 remote, … }        / browser / agent
      pure                  pure               syscalls, cached   spawns, network
```

- **Recognition is pure and lives in `omt-term`.** It is a function of the
  logical line's text plus its OSC 8 runs. It issues no syscalls, so it can run
  in the parser's own crate under [P1](01-principles.md#p1--clean-small-crates-explicit-seams)
  and is trivially fuzzable. `04 §8.3` already declares the `Target` enum and
  `Terminal::target_at`; this document specifies the rule set behind them and
  widens `Target` (§2.6).
- **Resolution and action live in a new L2 crate, `omt-open`.** Resolution
  `stat`s things; action spawns processes and opens URLs. Neither belongs in a
  pure state machine.
- **Everything user-facing is a capability**, so the phone can do all of it
  ([P3](01-principles.md#p3--parity-one-capability-three-surfaces)).

### 1.2 The new crate

```
L2 subsystems   omt-term · omt-pty · omt-agent-adapters · omt-transport
                omt-auth · omt-stt · omt-media · omt-open        ← new
```

`omt-open` depends on `omt-types`, `omt-catalog`, `omt-events`, `regex`, and
nothing else at L2 — it receives a `Match` (a plain data struct defined in
`omt-types`, *not* in `omt-term`) and a `ResolutionContext`, so crate rule 3 in
[02 §Dependency rules](02-crate-map.md#dependency-rules-mechanically-checked)
holds with no new exception. `omt-daemon` wires `omt-term`'s matcher output into
`omt-open`'s resolver.

Moving `Match`/`Target` down into `omt-types` is a small change to
[02](02-crate-map.md) and [04 §8.3](04-terminal-core.md#83-hyperlinks-and-detection);
noted in [§12](#12-open-questions) as an index/cross-reference edit for whoever
owns those files.

### 1.3 What this is not

- **Not a linkifier for the outer terminal.** omt renders its own grid; it does
  not emit OSC 8 to make the *host* emulator underline things. Doing so would
  hand activation to the host, which is precisely the conflict §4.2 exists to
  solve.
- **Not an editor.** Same boundary as [15 §1.1](15-workspace-explorer.md) — omt
  hands off to the real editor.
- **Not automatic.** No target is ever activated without an explicit user
  action. See [§8](#8-security).
- **Not available in `native` sessions in v1.** A `native` session
  ([D8](decisions.md#d8--two-session-modes-pty-default-and-native-acp)) has no
  PTY, no grid and therefore no `Position`. This document's entire model is
  `Match { span: Range<Position> }` over `omt-term` coordinates — every part of
  it, from anchoring across reflow (§2.5) to hint-label placement (§5.2) to
  mouse hit-testing (§5.1), is expressed in grid coordinates. **Semantic open
  is out of scope for `native` sessions in v1, and the UI offers no hints, no
  click targets and no `open.*` actions there.**

  **This is a real loss and it is stated rather than hidden**: a native agent's
  transcript is full of `src/lib.rs:42`, and those will not be clickable.

  **Why it is deferred rather than solved now.** The fix is to generalise the
  anchor from a grid span to a `ContentRef` that a transcript offset can also
  satisfy — sketched in §2.6 — and that is an `omt-types` change touching
  `Match`, `MatchId`, anchoring, and every handler signature. Doing it
  speculatively, before a native transcript renderer exists to validate it
  against, would freeze a second coordinate system on a guess about what a
  transcript offset looks like. Doing it later is an additive enum variant
  behind one type, which is exactly the shape of change that is cheap.

  **Recorded as the known future edit**, not as an open question: §2.6 carries
  the target shape, and the `omt-types` contracts change must land `Match` and
  `Target` with a *documented intent* to gain a `ContentRef` anchor — so the
  change is planned rather than rediscovered.

---

## 2. Recognition

### 2.1 Two sources, and their precedence

**Explicit (OSC 8).** A program that emits `ESC ] 8 ; params ; URI ST … ESC ] 8 ; ; ST`
has *told* omt what a span means. `omt-term` already stores these as
`ExtraAttrs::hyperlinks` runs with an interned `HyperlinkId`, and runs sharing an
`id=` parameter are one logical link across wrapped lines
([04 §8.3](04-terminal-core.md#83-hyperlinks-and-detection)). Emitters that
matter: `ls --hyperlink=auto`, `delta`, `cargo` (rustc diagnostics carry OSC 8
error-code links), `gcc` ≥ 9 with `-fdiagnostics-urls`, `rg --hyperlink-format`,
`gh`, `jj`, `eza`, `bat`.

**Heuristic (rules).** An ordered rule set of anchored regexes, run over
completed logical lines.

**Precedence: explicit beats heuristic, always, over any overlapping span.** The
reasons, in order of weight:

1. **The emitter knows things the text does not.** `rg --hyperlink-format=file://{host}{path}:{line}:{column}`
   emits an *absolute* URI even when the displayed text is relative to a
   directory omt cannot see. Preferring the visible text discards ground truth.
2. **P4.** [P4](01-principles.md#p4--native-semantics-observe-never-re-implement)
   says structured claims come from structured sources. An OSC 8 URI is a
   structured source; a regex over rendered output is not. Letting a regex
   override an explicit declaration is exactly the inversion P4 forbids.
3. **It is the emitter's escape hatch.** A program that dislikes omt's guess can
   fix it by emitting OSC 8. If heuristics could win, there would be no such
   hatch.

The one refinement: **explicit wins the *target*, heuristics may still enrich
it.** If OSC 8 says `file:///home/v/p/src/main.rs` and the surrounding text is
`src/main.rs:412:9`, omt takes the path from OSC 8 and the `line`/`col` from the
rule, because a `file:` URI has no standard line fragment and half the emitters
omit one. This is recorded on the match as `origin: Osc8WithHeuristicLineCol` so
it is visible in `open.resolve` output and in tests.

### 2.2 The matcher

```rust
/// One recognition rule. User-extensible per P2; the built-ins are ordinary
/// entries in the same table, with no privileged status.
pub struct Rule {
    pub id: RuleId,                 // stable string, e.g. "builtin.rustc_arrow"
    pub kind: MatchKind,            // what the match *is* (§2.3)
    pub regex: Regex,               // must contain at least the named group the kind requires
    pub precedence: i16,            // higher wins overlaps; built-ins occupy 0..=1000
    pub anchor: Anchor,             // Anywhere | LineStart | AfterWhitespace
    pub scope: RuleScope,           // Any | InBlockKind(..) | WhenCommandMatches(Regex)
    pub captures: CaptureMap,       // named group → semantic slot
    pub enabled: bool,
}

pub enum Anchor { Anywhere, LineStart, AfterWhitespace }

/// Semantic slots. A rule declares which of its named groups fill which slot;
/// this is what lets a user-written rule participate in resolution without
/// omt knowing anything about the language it parses.
pub enum Slot { Path, Line, Col, Url, Sha, Issue, Repo, Owner, Host, User(u8) }
```

The matcher is a **compiled `RegexSet` prefilter plus a per-rule confirm pass**,
the same shape `omt-term` already uses for multi-pattern search
([04 §8.1](04-terminal-core.md#81-search)):

1. One `RegexSet` over all enabled rules answers *which rules could match this
   line* in a single pass. On the overwhelmingly common line where nothing
   matches, that pass is the entire cost.
2. For each rule the set flagged, run its `Regex` with `find_iter`, honoring
   `Anchor`.
3. Collect `RawMatch { rule, byte_span, captures }`.
4. **Overlap resolution** (§2.4).
5. Convert byte spans to `Range<Position>` (§2.5).

`scope` exists because context is free accuracy: a `RuleScope::WhenCommandMatches`
rule only runs inside blocks whose `command` matches, using the block metadata
`omt-term` already captures from OSC 133 B/C. `^(?<path>[^:]+):(?<line>\d+):`
is a dangerous rule in general (it matches `Error: 42: something`) and a
*correct* rule inside a `rg`/`grep -n`/`ack` block. Scoping is how omt gets
grep-style matching without the false positives everywhere else.

### 2.3 Match kinds

```rust
pub enum MatchKind {
    Url,                    // any scheme; the allow-list is applied at action time (§7.1)
    Path,                   // with optional line/col
    GitRef,                 // sha, tag, branch
    Issue,                  // owner/repo#123, #123, JIRA-456
    Custom(RuleId),         // user rule with no built-in semantics; actions come from config
}
```

`Path` deliberately does not distinguish "file" from "directory" — that is a
*resolution* result (§3), not a recognition one, because the text is identical.

### 2.4 Ambiguity: how overlaps are resolved

Real output produces overlapping candidates constantly. `at /app/src/x.js:12:5`
matches the node-stack rule (span covering `/app/src/x.js:12:5`), the generic
`gnu_colon` rule (same span), and the bare-path rule (span covering only
`/app/src/x.js`). The rule, in order, applied to the set of `RawMatch`es on one
logical line:

1. **OSC 8 runs are laid down first and are immovable.** Any heuristic match
   overlapping an OSC 8 run is dropped, except for the line/col enrichment in
   §2.1.
2. **Highest `precedence` wins** any pair of overlapping spans.
3. **Equal precedence: the longer span wins.** `/app/src/x.js:12:5` beats
   `/app/src/x.js`. This is the rule that makes "path plus line" reliable, and
   it is why line/col rules and bare-path rules can coexist at the same
   precedence without a hand-tuned ordering between them.
4. **Equal precedence and equal length: earlier in the table wins.** Total
   ordering, so the result is deterministic and diffable in tests.
5. Non-overlapping matches all survive; a line can carry many.

Explicitly *not* done: iTerm2's "try prefixes and suffixes around the click
point, `stat`ing each" (`iTermPathFinder`). That approach is excellent for a
mouse-driven, click-anywhere UX and it is why iTerm2 can find a path with a
space in it. omt rejects it because it makes recognition **impure and
click-position-dependent** — the same line yields different matches depending on
where the pointer is, which cannot be pre-computed for hint mode, cannot be
rendered as a stable set of underlines, and cannot be tested as a function.
omt's answer for paths with spaces is §3.5's *disambiguation prompt*, which is
worse for one rare case and better for everything else.

### 2.5 Anchoring, and surviving reflow

A match is stored as `Range<Position>`, where `Position` is `omt-term`'s durable
`(LineId, col)` coordinate ([04 §3.4](04-terminal-core.md)). Because `Position`
is a logical-line coordinate and matching runs over **logical** (unwrapped)
lines, three properties fall out for free:

- **A match spanning a soft wrap is found normally.** `/very/long/path/to/f.rs:412`
  broken across a 80-column boundary is one match, not two fragments — the same
  win [04 §8.1](04-terminal-core.md#81-search) gets for search.
- **Matches survive reflow by construction**, exactly as block boundaries,
  selections and search results do. A resize does not re-run the matcher.
- **A rendered match may occupy several disjoint screen rectangles.** The
  renderer turns one `Range<Position>` into N row-segments at draw time; hint
  labels attach to the *first* rectangle in reading order.

Cache invalidation is by `Generation` (04 §4.1): a line's match list is valid
while its generation is unchanged, and generations are already the equality test
`omt-term` uses everywhere. Lines still being written (the line under the cursor)
are **never** matched, per 04 §8.3 — half a URL is not a URL, and flickering
underlines during output are worse than no underlines.

### 2.6 The `Match` and `Target` types

**`Target` is defined by [04 §8.3](04-terminal-core.md#83-hyperlinks-and-detection),
not here.** Only the type's *home crate* moves to `omt-types` (so `omt-open` can
consume it without an L2 dependency exception); the definition text stays in 04,
because 04 owns the scanner that produces it and this document owns only what is
done with it afterwards. The enum is reproduced below **verbatim from 04** as a
reading convenience — if the two ever differ, 04 wins and this copy is the bug.
`Match` is new here and this document owns it.

```rust
pub struct Match {
    pub id: MatchId,                    // (LineId, index) — stable while the line lives
    pub span: Range<Position>,
    pub rule: RuleId,
    pub kind: MatchKind,
    pub origin: MatchOrigin,            // Osc8 | Osc8WithHeuristicLineCol | Heuristic
    pub slots: SmallVec<[(Slot, Range<u16>); 4]>,   // byte ranges *within the matched text*
    pub text: CompactString,            // the matched text, for display and for handlers
    pub block: Option<BlockId>,         // owning block; supplies cwd/host at resolve time
}

// Reproduced from 04 §8.3; that document is authoritative.
pub enum Target {
    Url(Url),
    Path { raw: String, line: Option<u32>, col: Option<u32> },
    GitRef { raw: String },
    Issue { raw: String, repo: Option<String> },
    Custom { rule: RuleId, slots: SmallVec<[(Slot, Range<u16>); 4]> },
}
```

Three earlier divergences in this document are resolved *against* 04, and the
reasoning is recorded because each was a deliberate widening that is now paid for
elsewhere:

| Variant | 18 used to say | 04 says, and now so does 18 | Where the lost information comes from |
|---|---|---|---|
| `Url` | `Url { raw: String }` | `Url(Url)` | A parsed `Url` is strictly stronger: the scheme allow-list of §3.2 needs a parse anyway, and `Url::as_str()` recovers `raw`. |
| `Issue` | `{ owner, repo, number, key }` | `{ raw, repo }` | Recognition produces the raw reference; **decomposition into owner/number/key is a resolution concern**, performed by the issue handler (§4) from `raw` against the block's forge config. Recognition stays pure and forge-agnostic, which is the §2.4 rule. |
| `Custom.slots` | `BTreeMap<String, String>` | `SmallVec<[(Slot, Range<u16>); 4]>` | The map form both allocated per match and *copied* the slot text out of the line. Ranges into the matched text are what `Match::slots` already carries two fields above — the two were inconsistent inside this one document, and the range form is the one that survives reflow, because it does not snapshot the text. Handlers index `Match::text` with the range; §4's action templates are unaffected, since `Slot` is still a name. |

`Slot`, `RuleId` and `MatchId` are `omt-types` newtypes.

#### The `ContentRef` generalisation — recorded, not built

`Match::span` is a `Range<Position>`, which only a grid can produce. The
recorded future shape replaces it with:

```rust
pub enum ContentRef {
    /// A span in a terminal grid — the only variant in v1.
    Grid { span: Range<Position>, block: Option<BlockId> },
    /// A byte range within one entry of a rendered transcript. The variant a
    /// `native` session (and, later, the `pty` transcript view of
    /// [08 §4.4](08-web-client.md#44-transcript-view)) would use.
    Transcript { entry: EventSeq, offset: Range<u32> },
}
```

Everything downstream of recognition — resolution (§3), the handler registry
(§4), the `open.*` capabilities (§9) — already operates on `Target` and
`ResolutionContext` rather than on positions, and is unaffected. What *is*
affected is anchoring (§2.5), hint placement (§5.2), mouse hit-testing (§5.1)
and `SelectionMode::Semantic`, all of which would need a transcript-shaped
implementation alongside the grid one.

**This is an `omt-types` decision and it must land in the contracts change
either way** — either `Match` freezes with `span: Range<Position>` and a
documented intent to gain this variant, or it lands with `ContentRef` from the
start and only `Grid` is constructed in v1. This document takes the first
option; whoever owns the contracts change makes the final call, and the second
option is defensible if the enum is cheap to introduce now.

### 2.7 The default rule table

Precedence in the left column; higher runs first. Regexes are Rust `regex`
syntax (no backreferences, no lookaround except the `(?<!…)`/`(?=…)` forms the
crate supports via its `regex-lite`-compatible subset — where a lookaround is
shown below it is implemented as an explicit boundary check in the confirm pass,
noted per rule). Named groups map to `Slot`s of the same name.

| Prec | `id` | Kind | Regex | Captures / notes |
|---|---|---|---|---|
| 900 | `builtin.python_traceback` | Path | `File "(?<path>[^"\n]+)", line (?<line>\d+)` | Python, and Ruby's `from x.rb:12:in` variant below. Very high precedence: the quotes make it unambiguous and paths inside may contain spaces. |
| 890 | `builtin.python_pytest_in` | Path | `^(?<path>[^\s:][^:\n]*\.py):(?<line>\d+): in \S` | pytest's failure frames. `LineStart`. |
| 880 | `builtin.node_stack` | Path | `\bat (?:[^\s()]+ \()?(?<path>(?:file://)?(?:/|[A-Za-z]:[\\/])[^\s():]+):(?<line>\d+):(?<col>\d+)\)?` | Node/V8, jest, vitest, tsx. Handles both `at fn (p:l:c)` and `at p:l:c`. `file://` prefix stripped at resolve. |
| 875 | `builtin.node_stack_rel` | Path | `\bat (?:[^\s()]+ \()?(?<path>\.{0,2}/?[\w.@+-]+(?:/[\w.@+-]+)*\.[cm]?[jt]sx?):(?<line>\d+):(?<col>\d+)\)?` | Bundled/relative frames (webpack, esbuild). |
| 870 | `builtin.java_stack` | Path | `\bat [\w$.]+\((?<path>[\w$]+\.(?:java|kt|scala|groovy)):(?<line>\d+)\)` | Only a *basename* — resolution needs the workspace index (§3.4). |
| 860 | `builtin.rustc_arrow` | Path | `-->\s+(?<path>[^\s:][^:\n]*?):(?<line>\d+):(?<col>\d+)` | rustc, and every tool that copies its diagnostic format (miri, clippy, cargo-nextest). |
| 855 | `builtin.rust_panic` | Path | `panicked at (?<path>[^\s:][^:\n]*?):(?<line>\d+):(?<col>\d+)` | `thread '…' panicked at src/x.rs:9:5:` (Rust ≥ 1.73 format) |
| 850 | `builtin.go_frame` | Path | `^\t(?<path>(?:/|[A-Za-z]:[\\/])?[^\s:]+\.go):(?<line>\d+)(?: \+0x[0-9a-f]+)?$` | Goroutine dumps. `LineStart`, tab-anchored. |
| 845 | `builtin.go_vet` | Path | `\b(?<path>[^\s:]+\.go):(?<line>\d+):(?<col>\d+)\b` | `go build` / `go vet` / `staticcheck`. |
| 840 | `builtin.msvc_paren` | Path | `\b(?<path>[A-Za-z]:[\\/][^\s(]+|[^\s(:]+\.(?:cs\|cpp\|c\|h\|hpp\|vb)):\((?<line>\d+)(?:,(?<col>\d+))?\)` | MSVC, Roslyn, TypeScript's `tsc --pretty false` uses the same shape. |
| 830 | `builtin.gnu_colon` | Path | `(?<path>(?:[A-Za-z]:[\\/])?[~.]?[\w.@+/\\-]*[\w.@+-]\.[A-Za-z0-9_+-]{1,12}):(?<line>\d+)(?::(?<col>\d+))?(?=[:\s,)\]]\|$)` | The workhorse: gcc, clang, eslint, ruff, mypy, shellcheck, ripgrep with `--vimgrep`. Requires a **file extension**, which is what keeps `Error: 42: oops` out. |
| 820 | `builtin.grep_n` | Path | `^(?<path>[^\s:][^:\n]*):(?<line>\d+):` | `LineStart`, and `scope = WhenCommandMatches("^\\s*(rg\|grep\|ag\|ack\|git grep)\\b")`. Extension-free, so it is scoped — see §2.2. |
| 810 | `builtin.git_diff_header` | Path | `^(?:diff --git \|--- \|\+\+\+ )(?:[abciwo]/)?(?<path>[^\s\n]+)$` | `LineStart`. The `a/`/`b/` prefix is stripped as a capture-time transform, and `/dev/null` is dropped at resolve. |
| 805 | `builtin.git_hunk` | Path | `^@@ -\d+(?:,\d+)? \+(?<line>\d+)` | Carries only a line; pairs with the most recent `git_diff_header` in the same block to form a full target. The only rule with cross-line state; implemented as a per-block "last diff path" slot, reset at block open. |
| 800 | `builtin.url` | Url | `\b(?<url>[A-Za-z][A-Za-z0-9+.-]{1,31}://[^\s<>"'\x00-\x1f\x7f]+)` | Trailing `.,;:!?` and unbalanced `)`/`]`/`}` are trimmed by the confirm pass — an unpaired closer almost always belongs to the prose. |
| 795 | `builtin.url_www` | Url | `\bwww\.[A-Za-z0-9-]{1,63}(?:\.[A-Za-z0-9-]{1,63})+(?:/[^\s<>"']*)?` | Resolved with an `https://` prefix. |
| 790 | `builtin.mailto` | Url | `\b(?<url>[\w.+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+)\b` | Resolved as `mailto:`. Disabled by default — email addresses in logs are noise more often than they are targets. |
| 700 | `builtin.abs_path` | Path | `(?<path>(?:[A-Za-z]:[\\/]\|~/\|/)[\w.@+%-]+(?:[/\\][\w.@+%-]+)*/?)` | Bare absolute or `~`-rooted path with no line. |
| 690 | `builtin.rel_path` | Path | `(?<!\S)(?<path>\.{1,2}/[\w.@+%-]+(?:/[\w.@+%-]+)*/?)` | Explicitly relative (`./`, `../`). |
| 680 | `builtin.bare_rel_path` | Path | `(?<!\S)(?<path>[\w.@+-]+(?:/[\w.@+-]+)+\.[A-Za-z0-9]{1,12})(?!\S)` | `src/main.rs`. Requires **both** a `/` and an extension; without both the false-positive rate is intolerable. |
| 600 | `builtin.jira` | Issue | `\b(?<key>[A-Z][A-Z0-9]{1,9}-\d{1,7})\b` | Off by default; enabled by setting `open.issue.jira_base`. |
| 590 | `builtin.gh_issue_qualified` | Issue | `\b(?<owner>[\w.-]{1,39})/(?<repo>[\w.-]{1,100})#(?<number>\d{1,7})\b` | |
| 580 | `builtin.gh_issue_bare` | Issue | `(?<!\w)#(?<number>\d{1,7})\b` | Resolved against the block's git remote. Off in blocks with no git identity. |
| 500 | `builtin.git_sha` | GitRef | `\b(?<sha>[0-9a-f]{7,40})\b` | Guarded in the confirm pass: rejected unless it has ≥ 1 digit **and** ≥ 1 `a-f`, and rejected at length 8 or 32 when the surrounding text looks like a hash dump. Resolution additionally verifies with `git cat-file -e` (§3.6), so a false positive is silent, not wrong. |

**Extending it** ([P2](01-principles.md#p2--pluggable-extension-without-modification)):
rules are config data, validated at load like everything else in
[10 §5](10-configuration.md#5-validation-and-diagnostics).

```toml
[[open.rule]]
id         = "buildkite.job"
kind       = "custom"
regex      = 'https://buildkite\.com/(?<org>[\w-]+)/(?<pipe>[\w-]+)/builds/(?<num>\d+)'
precedence = 950
actions    = ["open_url", "copy"]

[[open.rule]]
id         = "monorepo.bazel_label"
kind       = "path"
regex      = '//(?<path>[\w/-]+):(?<target>[\w.-]+)'
precedence = 720
# Transform the captured label into a real path before resolution.
path_template = "{path}/BUILD.bazel"

# Turn a built-in off without editing it.
[[open.rule]]
id = "builtin.git_sha"
enabled = false
```

`omt open rules test --file fixture.txt` prints every match with its rule,
span, precedence and what it lost or won an overlap against — the debugging tool
that makes a user-extensible matcher actually usable.

---

## 3. Resolution

Resolution turns a `Match` into a `ResolvedTarget`: a concrete, absolute,
existence-checked thing, or an honest failure.

```rust
pub struct ResolutionContext<'a> {
    pub host: TargetHost,                 // Local | Remote { instance, host }
    pub block_cwd: Option<&'a Path>,      // OSC 7 / OSC 1337 CurrentDir at block start
    pub block_host: Option<&'a RemoteHost>,
    pub pane_cwd: Option<&'a Path>,       // fallback only
    pub workspace_roots: &'a [CanonicalPath],
    pub git: Option<&'a GitIdentity>,     // for GitRef / Issue resolution
    pub home: &'a Path,
}

/// **The canonical definition.** Other documents contribute fields; none
/// redeclare the struct.
pub struct ResolvedTarget {
    pub target: Target,
    pub resolution: Resolution,
    pub existence: Existence,
    pub sensitivity: Sensitivity,         // from 15 §9.4, so handlers can gate
    pub actions: Vec<ActionOffer>,        // ordered, best first (§4.3)
    /// Which machine owns the file. Filled by the *resolving* instance; §6.1.
    pub host: TargetHost,
    /// Set when the path is inside an open workspace root, so the explorer can
    /// jump straight to the node. `ExplorerRef` is defined in
    /// [15 §8.1](15-workspace-explorer.md#81-fileline-from-terminal-output).
    pub explorer: Option<ExplorerRef>,
}

pub enum Resolution {
    File { path: PathBuf, workspace: Option<WorkspaceId>, line: Option<u32>, col: Option<u32> },
    Dir  { path: PathBuf, workspace: Option<WorkspaceId> },
    Url  { url: Url, display: UrlDisplay },      // §7.1
    Commit { sha: String, workspace: WorkspaceId, subject: String },
    IssueUrl { url: Url },
    Ambiguous { candidates: Vec<PathBuf> },      // §3.5
    Unresolved { reason: UnresolvedReason },
}

pub enum Existence { Exists { kind: NodeKind, len: u64 }, Missing, Unknown /* remote, not yet probed */ }
```

### 3.1 The cwd question, which is the whole thing

**A relative path must be resolved against the cwd of the block that printed it,
not the pane's current cwd.** This is the single most important detail in the
document and it is the reason [04 §7](04-terminal-core.md) puts `cwd` on
`Block` rather than only on the session.

Why it matters, concretely:

```
~/proj $ cd crates/parser
~/proj/crates/parser $ cargo test
   … error[E0308]: mismatched types
     --> src/lexer.rs:88:17
~/proj/crates/parser $ cd ../..
~/proj $                                  ← user is here now, and clicks the error
```

The pane's cwd is `~/proj`. `~/proj/src/lexer.rs` may not exist (open nothing —
a confusing failure) or, far worse, **may exist and be a different file** — a
monorepo with a top-level `src/` and a hundred crates that each have one is not
exotic, it is the norm. Opening the wrong file at line 88 is the failure mode
this design most needs to avoid, because it is silent: the user reads a
plausible-looking file and debugs the wrong thing.

So resolution walks a strict ladder and **stops at the first success**:

1. **The match's `block.cwd`** (OSC 7 / `OSC 1337;CurrentDir=` captured at
   OSC 133 A/B time). Correct by construction.
2. **A path-only prefix search within the owning block's workspace root**, if
   the block has no cwd but the match's path is *unique* under that root
   (§3.4). One candidate → use it. Several → `Ambiguous`.
3. **The pane's current cwd**, and only if the resulting file exists **and** the
   block had no cwd. Marked `confidence: Low` on the result and shown with a
   "resolved against the pane's current directory" note in the UI.
4. Otherwise `Unresolved { reason: NoCwd }`.

Rung 3 is deliberately last and deliberately labelled. It exists because the
common case for a user *without* shell integration installed is a pane that
never changed directory, where the pane cwd is right; and it is labelled because
when the user did `cd`, it is wrong.

**This makes shell integration a load-bearing feature, not a nicety.** omt's
first-run flow already installs OSC 133 + OSC 7 emission
([04 §7.2](04-terminal-core.md#72-what-they-emit--standard-osc-133-not-a-private-namespace),
`shell.auto_install` in [10 §7.4](10-configuration.md)). This document adds one
line to the `omt doctor` output: *"path clicking will resolve against the pane's
current directory, not the command's — install shell integration to fix"*.

### 3.2 Path normalization

Applied in order, before any syscall:

1. Strip a `file://` or `file://<host>/` prefix; if `<host>` is non-empty and
   not `localhost` and not the session's host, the target is remote (§5).
2. Percent-decode **only** when the raw text came from a URL context. A literal
   `%20` in a shell path is a real character; a `%20` in a `file:` URI is a
   space. `MatchOrigin` tells them apart.
3. Expand a leading `~/` or `~user/` against `ResolutionContext::home` /
   the passwd database. Never expand `~` inside a path.
4. On Windows, normalize separators; recognize `\\?\`, drive-relative and UNC
   forms and reject the exotic ones rather than guessing.
5. Trim trailing punctuation that survived the confirm pass (`.`, `,`, `:`,
   `'`, `"`, `)` when unbalanced) — only for `builtin.*_path` rules, never for
   quoted forms like `python_traceback`.
6. Lexically normalize `.`/`..` **without** touching the filesystem, then join
   against the resolved cwd.

### 3.3 Existence, confinement and sensitivity

- **`stat` with a cache.** A `StatCache` keyed by absolute path, TTL 2 s,
  capacity 4096, negative entries cached at TTL 500 ms. Naive existence checking
  **once per match, on every re-match of a line**, is the classic performance
  killer here — a screen of a stack trace is dozens of `stat`s per frame, and
  scrolling re-runs them. (omt never checks on *hover*: §5.1 renders decoration
  from the matcher, not the pointer.) iTerm2 needed
  `iTermCachingFileManager` for exactly this reason
  ([iTerm2 §8.4](../research/iterm2.md#84-smart-selection--semantic-history)).
  The cache is invalidated wholesale on a workspace FS event from
  [15 §4.6](15-workspace-explorer.md) for paths under a watched root.
- **Confinement is applied at *action* time, not resolution time.** Resolution
  may report that `/etc/shadow` exists — a `Viewer` on a phone learning that a
  path printed on their own screen exists is not a disclosure. *Opening* it is
  gated: a path inside a workspace root goes through
  [15 §9.1](15-workspace-explorer.md#91-path-confinement)'s `confine()`; a path
  outside every root is offered only `reveal_in_editor` (Operator,
  `SPAWNS_PROCESS`) and `copy`, never `workspace.explorer.reveal` or `read`. This mirrors
  [15 §8.4](15-workspace-explorer.md#84-open-in-editor)'s existing split exactly.
- **Sensitivity** is computed with [15 §9.4](15-workspace-explorer.md#94-sensitive-files)'s
  matcher. A `Sensitive` target is badged 🔒 in the hint overlay, its *preview*
  is suppressed for `Viewer`, and "insert into agent prompt" requires an extra
  confirm on every surface — handing `.env` to an agent is a real exfiltration
  path and the one-tap version of it should not exist.

### 3.4 Basename-only targets and the workspace index

Java stack frames (`Foo.java:88`), Ruby's `in 'block in run'` frames, and many
test runners print a basename with no directory. These resolve through
[15 §4.3](15-workspace-explorer.md)'s `workspace.files.find` index:

- exactly one match under the block's workspace root → resolve to it;
- several → `Ambiguous` with the candidate list, ranked by (a) inside the block's
  cwd subtree, (b) shortest path, (c) most recently modified;
- none → `Unresolved { reason: NotFoundInWorkspace }`.

The index is the explorer's, not a second one. If the explorer is disabled
(`workspace.explorer.enabled = false`), basename resolution is simply
unavailable and says so.

### 3.5 When the path does not exist

**omt never opens a substitute.** The failure UX, in full:

- The hint label / underline for a non-existent path renders **dimmed with a
  strikethrough** rather than as an active link, so the user learns before
  acting. This costs one `stat` per visible match, from the cache above.
- Activating it anyway (it is not forbidden — the file may have just been
  deleted, and the user may want to copy the path) opens the **action menu**
  with `copy`, `search workspace for this basename`, and `insert into agent
  prompt` enabled, and `open in editor` disabled with an inline reason.
- The reason is specific, never "not found":

```
┌ omt · open ──────────────────────────────────────────────┐
│  src/lexer.rs:88:17                                       │
│                                                           │
│  Resolved to  ~/proj/src/lexer.rs   — does not exist       │
│  Resolved against the pane's current directory, because    │
│  the block that printed this has no recorded cwd.          │
│    ▸ Install shell integration    (omt shell install)      │
│    ▸ Search the workspace for "lexer.rs"          [enter]  │
│    ▸ Copy the text                                    [y]  │
└───────────────────────────────────────────────────────────┘
```

`UnresolvedReason` is a closed enum — `NoCwd`, `NotFound`, `NotFoundInWorkspace`,
`AmbiguousBasename`, `OutsideWorkspaceAndNoEditor`, `RemoteUnreadable`,
`TooLarge`, `Binary`, `Directory`, `SchemeNotAllowed`, `HostUnreachable` — and
each has a rendered explanation plus at least one offered remedy on all three
surfaces. Failure with a next step is the whole product difference here.

### 3.6 Non-path resolution

- **`GitRef`.** Resolved against the block's `GitIdentity` with
  `git cat-file -t <sha>`; a miss demotes the match to plain text (no menu, no
  underline) rather than offering a broken action. On a hit, `git show --stat`'s
  first line becomes the display subject, and actions are *show the commit in
  the explorer's diff view*, *copy*, and *open on the forge* when a remote is a
  recognizable GitHub/GitLab/Gitea/Bitbucket URL.
- **`Issue`.** `owner/repo#n` resolves directly. Bare `#n` resolves against the
  block's git remote; with no remote, the match is dropped. JIRA keys resolve
  against `open.issue.jira_base`. All produce `IssueUrl` and are then subject to
  §7.1's URL rules like any other URL — a forge URL is not privileged.

---

## 4. Actions and the handler registry

### 4.1 The trait

```rust
/// P2 extension point #7. Registered in `Registry<Box<dyn OpenHandler>>`.
#[async_trait]
pub trait OpenHandler: Send + Sync {
    fn id(&self) -> HandlerId;                     // "editor", "explorer", "browser", …
    fn label(&self) -> &str;                       // shown in menus, i18n-free per P10

    /// Cheap, synchronous, no syscalls: can this handler act on this target?
    /// Returns a rank; the highest-ranked applicable handler is the default.
    fn applicability(&self, t: &ResolvedTarget, ctx: &HandlerCtx) -> Option<Applicability>;

    /// Declared statically so the capability's audit record and the mobile
    /// confirm-sheet policy are correct without running anything.
    fn effects(&self) -> Effects;

    async fn activate(&self, t: &ResolvedTarget, ctx: &HandlerCtx) -> Result<Activation, OpenError>;
}

pub struct Applicability { pub rank: i16, pub reason: Option<&'static str> }

/// What actually happened, or what the *client* must do. See §5.3 — the split
/// between "the instance did it" and "the client must do it" is what makes the
/// thin client and the browser work through the same capability.
pub enum Activation {
    Done { detail: String },
    ClientMust(ClientAction),
}

pub enum ClientAction {
    OpenUrl { url: Url },
    OpenLocalFile { blob: BlobId, suggested_name: String, line: Option<u32>, col: Option<u32>,
                    provenance: RemoteProvenance, read_only: bool },
    CopyText { text: String },
    ShowInline { blob: BlobId, mime: Mime, line: Option<u32> },
}
```

### 4.2 The built-ins

| `id` | Applies to | What it does | Effects | Default rank |
|---|---|---|---|---|
| `editor` | `File` | Runs the configured editor **on the machine that owns the file** (§4.4). For a remote file with a local client, this becomes `ClientMust(OpenLocalFile)` after §5's fetch. | `READS_FS`, `SPAWNS_PROCESS` | 100 (local file inside a workspace: 90) |
| `explorer` | `File`, `Dir` | Reveals the path in omt's own workspace explorer at the right line ([15 §5](15-workspace-explorer.md)). No process spawn, works on a phone, works for remote files with zero transfer. | `READS_FS` | 95 for `File` in a workspace, 100 for `Dir` |
| `browser` | `Url`, `IssueUrl` | Hands the URL to the *client's* browser via `ClientMust(OpenUrl)` — never the daemon's. Opening a URL on a headless box is useless; opening it on the phone that tapped it is what was meant. | — | 100 |
| `copy` | everything | Copies the raw text, or the resolved absolute path, or `path:line` — three separate menu entries, because all three are wanted. Uses `media.clipboard.write` ([09 §3](09-ssh-and-media.md)). | — | 40 |
| `agent_insert` | `File`, `Dir` | Inserts a reference into the focused agent's prompt (§4.5). | `WRITES_PTY` | 80 when a bound agent has focus, else not applicable |
| `read_inline` | `File` | Renders the file inline in the block view / web client, at the line, syntax-highlighted, via `workspace.files.read`. The only action that is *strictly better* remote than local. | `READS_FS` | 85 on web/mobile, 30 on the TUI |
| `commit_show` | `Commit` | Opens the commit's diff in the explorer diff view. | `READS_FS`, `SPAWNS_PROCESS` | 100 |
| `search` | any | Searches the workspace for the match text. The always-available "I don't know what this is" action. | `READS_FS` | 10 |
| `command` | any, by config | Runs a user-defined argv template (§4.6). | `SPAWNS_PROCESS` | as configured |

### 4.3 Choosing a handler, and the menu

`open.resolve` returns `actions: Vec<ActionOffer>` sorted by effective rank.
Effective rank = handler `rank` + config bias:

```toml
[open]
# Per match-kind default: the handler id that wins a plain activation.
default = { path = "editor", dir = "explorer", url = "browser",
            commit = "commit_show", issue = "browser", custom = "menu" }

# Bias an individual handler up or down globally.
[open.rank]
explorer = +20        # "I live in omt; don't launch VS Code for every click"

# Per rule override, highest specificity.
[[open.binding]]
rule    = "builtin.git_diff_header"
handler = "explorer"
```

`default = "menu"` for a kind means a plain activation always shows the menu.

**The menu is the fallback and the always-available escape hatch.** Whenever
(a) the config says `menu`, (b) two offers tie within 5 rank points, (c) the
default handler is inapplicable (no editor configured, target missing), or (d)
the user activates with the "menu" chord instead of the plain one, omt shows the
full ordered list of applicable actions with one-key accelerators. On mobile it
is a bottom sheet, on the TUI a small popup at the match, in the web client a
context menu — one `ActionOffer` list, three renderers, per P3.

### 4.4 Which machine opens the editor

Three cases, and they are genuinely different:

| Client | File lives on | Editor runs on | Mechanism |
|---|---|---|---|
| TUI, local instance | local | local | `Activation::Done`, daemon spawns |
| Web client / phone, any instance | instance | **instance** | `Activation::Done`; this is exactly [15 §8.4](15-workspace-explorer.md#84-open-in-editor)'s "open that file on my laptop from my phone" and it is correct as is |
| `omt ssh` thin client | remote instance | **local** | §5 — fetch, then `ClientMust(OpenLocalFile)` |

The thin-client row is the interesting one and it is the reason `Activation` has
two variants at all. The config knob is `open.editor.side = "auto" | "instance" | "client"`,
defaulting to `auto`, which resolves to `client` when the calling client is a
full omt (the thin client and only the thin client) and `instance` otherwise.

Editor invocation is **argv, never a shell string** — same rule and same reason
as [15 §8.4](15-workspace-explorer.md#84-open-in-editor):

| Editor (detected from `$EDITOR`/`$VISUAL` basename, or `open.editor.program`) | argv |
|---|---|
| `code`, `code-insiders`, `codium`, `cursor`, `windsurf` | `["--goto", "{path}:{line}:{col}"]` |
| `zed` | `["{path}:{line}:{col}"]` |
| `subl`, `smerge` | `["{path}:{line}:{col}"]` |
| `vim`, `nvim`, `vi`, `view` | `["+call cursor({line},{col})", "{path}"]` (falls back to `["+{line}", "{path}"]` when col is absent) |
| `hx` (helix) | `["{path}:{line}:{col}"]` |
| `emacsclient` | `["-n", "+{line}:{col}", "{path}"]` |
| `emacs` | `["+{line}:{col}", "{path}"]` |
| `idea`, `pycharm`, `webstorm`, `goland`, `rustrover`, `clion` | `["--line", "{line}", "--column", "{col}", "{path}"]` |
| `nano` | `["+{line},{col}", "{path}"]` |
| `micro` | `["{path}:{line}:{col}"]` |
| `kak` | `["+{line}:{col}", "{path}"]` |
| `open` (macOS), `xdg-open`, `start` | `["{path}"]` — no line support, and the UI says so |
| anything else | `open.editor.args` if set, else `["{path}"]`, with a one-time note that line positioning is unavailable for this editor |

Substitution is **per-argv-element and typed**: `{line}`/`{col}` are integers
formatted by omt (never user text), `{path}` is the resolved `PathBuf` passed as
one element. An element that would become empty because `{col}` is `None` is
dropped whole, not left as a stray flag. There is no `sh -c` anywhere in this
path, so a file named `; rm -rf ~` is inert. See [§8.3](#83-editor-and-command-templates).

Terminal editors (`vim`, `hx`, `emacs -nw`, `nano`, `kak`, `micro`) are not
spawned into the void: when the client is a TUI, omt opens them in a **new omt
pane** (a `pane.split` + `session.create` with that argv), which is both the
only way they are usable and a nicer result than a GUI editor. `open.editor.in_pane`
(default `auto`, detected from a known-terminal-editor list plus `open.editor.terminal = true`).

### 4.5 Insert into the agent's prompt

The genuinely novel action, and the one that justifies this subsystem existing
inside an agent multiplexer rather than a terminal.

The workflow: an agent in pane 1 has just printed a failing test with a
`file:line`. The user wants to say *"fix that"*. Today they select the path with
the mouse, copy it, click into the agent, paste, and type. With
`agent_insert` they hit the hint chord, type two letters, and the path is in the
agent's prompt in the agent's own reference syntax, cursor after it.

Reuses [09 §7.1](09-ssh-and-media.md#71-handing-the-image-to-the-agent)'s
per-adapter reference machinery rather than inventing a second one — that
section already solved "how does *this* agent want to be handed a path", for
images. The trait gains a sibling method:

```rust
// Added to `AgentAdapter`, owned by [06 §7](06-agent-layer.md#7-adapters).
// One method; `attachment_reference` (09 §4.3.7) already covers the disk-blob case.

    /// How this agent wants to be handed a *source file* the user is pointing at.
    /// Returns `AttachmentReference`, the single reference type
    /// ([09 §7.1](09-ssh-and-media.md#71-handing-the-image-to-the-agent)).
    fn file_reference(&self, path: &Path, line: Option<u32>, col: Option<u32>)
        -> AttachmentReference;
```

| Agent | `file_reference` |
|---|---|
| Claude Code | `@<workspace-relative path>` when inside the workspace (its documented mention syntax), else an absolute path; line appended as `:{line}` in prose, since `@file:line` is not a documented form |
| ACP-generic | a `resource_link` content block in `session/prompt` — `Structured`, and preferred per P4 |
| opencode | a file part over its HTTP API — `Structured` |
| Aider | `Command { command: "/add <path>", then: "" }` |
| Codex CLI / Gemini / Qwen | `@<path>` |
| unknown | absolute path, space-suffixed |

Insertion goes through `agent.prompt` with `submit: false` — omt puts text in the
box and stops. It never presses enter. That is [D3](decisions.md#d3--synthetic-input-is-bounded-by-state-dependence-not-by-tool-danger)'s
"submitting typed text to a prompt box" (allowed, position-independent) and
nothing more.

When several agent panes are open, the target agent is the **focused** one; the
menu offers "insert into…" with a pane list when it is ambiguous.

**`agent_insert` is a paste, and a card suspends it.** Two consequences of
[D16](decisions.md#d16--remote-answering-is-per-card-type-and-the-preconditions-are-empirical),
whose transport rules are specified in
[16 §4.5](16-input-and-keymap.md#45-synthetic-answers-are-written-as-keys-never-as-text):

- For a `pty`-mode agent with no structured prompt channel, this insertion is
  `PtyWrite::Paste` — bracket-wrapped when the agent has enabled mode 2004, which
  is the *correct* framing for text and the opposite of what a synthetic card
  answer requires. The two paths must not be merged.
- While an interaction card is open in the target pane, `agent_insert` is
  **disabled**, not queued. A digit or a bracket sequence arriving at a card
  resolves or corrupts it; a card is not a prompt box, and D3's "submitting typed
  text to a prompt box" allowance does not reach it. The handler is greyed out
  with that reason, and the user is offered the card instead. This is the same
  target-identity failure [D15](decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism)
  item 3 names for a replayed enqueue.

### 4.6 User-defined command handlers

```toml
[[open.handler]]
id      = "gh-pr"
label   = "Open PR in gh"
matches = { kind = "issue" }
program = "gh"
args    = ["pr", "view", "{number}", "--repo", "{owner}/{repo}", "--web"]
effects = ["spawns_process", "network"]

[[open.handler]]
id      = "blame"
label   = "git blame here"
matches = { kind = "path", exists = true }
program = "git"
args    = ["-C", "{workspace_root}", "blame", "-L", "{line},+20", "--", "{path}"]
in_pane = true
```

`program` is resolved from `PATH` once at config load and the **resolved absolute
path** is what is executed and what the confirm sheet displays. Templates
substitute into argv elements only. `effects` is declared by the user and
validated to be a superset of what omt can infer (`SPAWNS_PROCESS` is forced);
under-declaring is a config error with an `OMT-C4xx` diagnostic, because
`effects` drives the mobile confirmation ([03 §2](03-capability-catalog.md)).

---

## 5. The conflicts

This is the part that decides whether the feature is real.

### 5.1 Mouse reporting — the inner program owns the click

When the program in the pane has enabled DECSET 1000/1002/1003 (+1006 SGR
encoding), **every button press is that program's input**. vim, helix, tmux,
htop, lazygit, and — critically for omt — essentially every agent TUI, do this.
[P4](01-principles.md#p4--native-semantics-observe-never-re-implement) is
unambiguous: omt must not steal input the inner program needs. A multiplexer
that swallows clicks so its own link handler can run has broken vim, and no link
feature is worth that.

This is **the common case**, not an edge case, and the design starts from it.

**What omt claims, precisely:**

| Situation | Plain click | Shift+click | Cmd/Super+click | Alt+click |
|---|---|---|---|---|
| Mouse reporting **off** | omt: selection | omt: extend selection | **omt: activate** | omt: block-select |
| Mouse reporting **on** (1000/1002/1003) | **forwarded to app** | **omt: activate** | omt: activate *if the outer terminal delivers it* (§5.2) | **forwarded** (many apps use it) |

The load-bearing row is Shift+click while reporting is on. The convention that
**Shift bypasses application mouse reporting** is long-standing xterm behavior
and is implemented by every terminal omt cares about (xterm's own
`allowMouseOps`/shift override, iTerm2, kitty, WezTerm, Ghostty, Alacritty,
Terminal.app, Windows Terminal all let Shift+drag select through an app that has
grabbed the mouse). Applications overwhelmingly do not expect to receive
shifted clicks, precisely *because* that convention exists. So omt does the same
thing one level down: **when mouse reporting is on, omt consumes Shift+click
itself and forwards everything else untouched.**

Two honest caveats:

- Shift+click is also the conventional "extend selection". omt therefore binds
  **Shift+click = activate** only when reporting is on (where selection is
  already unusual and the user reaching for Shift means "talk to the
  multiplexer, not the app"), and **Shift+click = extend selection** when
  reporting is off. This is context-dependent behavior, which is a smell — but
  the alternative is either stealing plain clicks from vim or having no mouse
  activation at all inside a TUI, and both are worse. It is configurable
  (`open.mouse.activate_modifier`) and it is documented in `omt keys list`.
- A tiny number of applications *do* handle shifted clicks. omt keeps a
  `terminal.mouse.shift_passthrough_programs` list (empty by default) matched
  against the pane's foreground process name, for the user who hits one.

**Modes 1002/1003 (drag and any-motion reporting) additionally mean omt must not
render hover underlines**, because computing them requires knowing where the
pointer is, and under 1003 every motion event is the app's. omt therefore
renders match decoration from the *matcher*, not from the pointer: matches are
underlined statically (or not at all, per `open.decorate`), never on hover, when
reporting is on. This is also a performance win — no work per mouse move.

**The mouse is never the only way.** Which brings us to:

### 5.2 Hint mode — the primary mechanism

Press a chord; every match in the viewport gets a short label; type the label;
the action runs. vimium, tmux-fingers, kitty's `hints` kitten and another terminal's
keyboard link navigation are all prior art for the interaction, and it is the
right one here for reasons specific to omt:

1. **It is independent of mouse reporting entirely.** The chord is a key
   sequence omt already reserves as its prefix; no application sees it.
2. **It is independent of the outer terminal.** No modifier negotiation, no
   emulator handler to fight (§5.3). It works over ssh, inside tmux, in
   Terminal.app.
3. **It is identical on every surface.** The web client renders the same labels
   and accepts the same keystrokes from a Bluetooth keyboard; the phone taps the
   label instead. That is P3 satisfied structurally rather than by effort.
4. **It composes with actions.** Selecting a label with a different key runs a
   different handler, which no click gesture can do without more modifiers.

```
<leader> f        enter hint mode, default action per §4.3
<leader> F        enter hint mode, always show the action menu on select
<leader> g        enter hint mode restricted to `kind = url`
```

(`<leader>` is `ctrl-b` by default; the token and the spelling are
[16 §3](16-input-and-keymap.md#3-the-leader-key)'s.)

Behavior:

- Labels come from a **homerow alphabet** (`open.hints.alphabet`, default
  `"asdfghjkl;qwertyuiopzxcvbnm"`), assigned so that the first N matches get
  one-character labels and the rest get two. Assignment is in **reading order**
  (top-left to bottom-right), not in match-discovery order, so the label of a
  given match is stable while the screen is.
- Labels render as a reverse-video overlay at the match start, **overlaying**
  the cell contents rather than shifting them, so nothing reflows and the
  underlying line stays readable.
- Typing a prefix filters; the overlay dims non-candidates. `esc` cancels.
  `space` while a label is selected opens the action menu for it.
- Scrolling is allowed while in hint mode; labels re-assign on the new viewport.
- **Scope is the viewport by default.** `ctrl-b f` in scrollback view hints the
  scrolled viewport. `open.hints.scope = "viewport" | "block"` — `block` hints
  every match in the block under the cursor, including its off-screen lines,
  which is how you act on the 40th frame of a stack trace without scrolling.

Hint mode is a **capability trio** (§8) rather than TUI-local state, so the web
client, the phone and a scripted client all drive the identical state machine
and see the identical label assignment. The instance owns hint state per client
(it is per-viewport, and two clients have different viewports).

### 5.3 The outer terminal already does this — and gets it wrong

iTerm2's semantic history is bound to **Cmd+click** by default
([iTerm2 §8.4](../research/iterm2.md#84-smart-selection--semantic-history)), and
iTerm2 consumes that event in the emulator; omt never sees a byte. WezTerm,
kitty (`open_url_with`, `mouse_map`), Ghostty and Windows Terminal have
comparable Ctrl/Cmd-click URL and path handling. So the conflict is not
theoretical.

It is also *actively harmful* in exactly omt's headline scenario:

> The user is on their laptop in iTerm2, attached via `omt ssh` to a remote omt.
> A remote compiler prints `/srv/app/src/lexer.rs:88:17`. The user Cmd+clicks.
> **iTerm2 resolves that path on the laptop.** Best case, nothing exists and it
> silently does nothing. Worst case — and this is not unlikely, because people
> check out the same repo on both machines — `/srv/app/src/lexer.rs` exists
> locally at a different commit, and iTerm2 opens the *wrong file*, at a line
> number that means something else, with no indication anything is wrong.

iTerm2 has a partial defence (it knows the remote host from OSC 1337
`RemoteHost=` and can be configured to run a command instead), but the default
behavior is local resolution and most users have not configured otherwise.

**omt's response, in three parts:**

**(a) Detect.** At session start omt identifies the outer emulator from
`TERM_PROGRAM`/`TERM_PROGRAM_VERSION`/`XTVERSION` (`CSI > 0 q`) — machinery
[09 §7.2](09-ssh-and-media.md#72-inline-display-in-the-tui) already builds for
graphics detection — and consults a table of known default click bindings:

| Emulator | Default handler | Modifier | omt's advice |
|---|---|---|---|
| iTerm2 | Semantic History | `Cmd` | Rebind or disable; use `Shift` for omt |
| kitty | `mouse_map` URL open | `Ctrl+Shift` (and plain click on URLs) | Conflicts with omt's Shift+click; remap |
| WezTerm | `OpenLinkAtMouseCursor` | `Ctrl` (macOS: `Cmd`) | Usually fine; verify |
| Ghostty | link open | `Cmd`/`Ctrl` | Usually fine |
| Terminal.app | URL open | `Cmd` | URLs only, no path handler — low conflict |
| Windows Terminal | URL open | `Ctrl` | URLs only |
| Alacritty | none by default | — | No conflict |

omt cannot *observe* the outer emulator's bindings, so this table is knowledge,
not detection, and it can be wrong for a user who reconfigured. That is why part
(b) exists.

**(b) Tell the user, once, at the moment it matters.** The first time a
`Cmd/Ctrl+click` produces *no* event omt can see — detected as "the user
activated hint mode within 3 seconds of an unexplained gap, or pressed the
modifier-click chord and omt received a plain click" — omt shows a one-shot card.
It never shows it twice, and it is suppressible forever from the card:

```
┌ omt · your terminal is intercepting that click ──────────────┐
│ iTerm2 handles Cmd+click itself (Semantic History) and omt    │
│ never sees it. On this session the file lives on `box`, so    │
│ iTerm2 would look for it on *this* machine.                   │
│                                                               │
│   ▸ Use  ⇧ Shift+click  instead                    (works now)│
│   ▸ Use  ctrl-b f       hint mode                  (works now)│
│   ▸ Turn iTerm2's handler off:                                │
│       Settings → Profiles → Advanced → Semantic History        │
│         → "Never" ;  or set the modifier to ⌥ Option           │
│   ▸ Copy an iTerm2 dynamic-profile snippet          [c]       │
│   ▸ Don't show this again                           [d]       │
└───────────────────────────────────────────────────────────────┘
```

**(c) Offer the config, don't apply it.** `omt doctor keys` (the flow owned by
[16](16-input-and-keymap.md)) gains an `open` section that prints, for the
detected emulator, the exact snippet — a kitty `mouse_map` line, a WezTerm
`keys`/`mouse_bindings` fragment, an iTerm2 dynamic-profile JSON key, a
Windows Terminal `actions` entry — and the exact GUI path where one exists. omt
**never edits another program's configuration file.** Writing to `~/.config/kitty/kitty.conf`
behind the user's back is exactly the ambient-authority behavior
[P8](01-principles.md#p8--security-by-default-no-ambient-trust) rules out, and
it is unrecoverable when omt guesses wrong about their config layout.

**(d) The structural answer.** All of the above is mitigation. The actual answer
is that omt's *documented, default, first-taught* activation is a chord, not a
click (§5.2). A feature whose primary path is a keystroke cannot be stolen by an
emulator, and the modifier-click support degrades from "the feature" to "a
convenience that works on most setups" — which is the correct risk posture for
something contingent on four other programs.

### 5.4 Selection versus click

Click-to-open must not break click-drag-to-select, and the failure mode
(activating on mouse-*down*, so a drag that starts on a link opens it) is the
classic bug.

Rules:

1. **Activation fires on mouse-*up*, never mouse-down**, and only if the up is
   within `open.mouse.slop` cells (default 1) of the down, within
   `open.mouse.max_ms` (default 500 ms), with no intervening motion beyond slop.
   A drag is a selection, unconditionally.
2. **A double-click is never an activation.** The second down cancels any
   pending activation from the first up. Double-click remains word/semantic
   select ([04 §8.2](04-terminal-core.md#82-selection)).
3. **An existing non-empty selection suppresses activation** on the click that
   clears it — the first click after a selection dismisses it and does nothing
   else, matching every text UI.
4. **Selection wins on hit-test ties.** If a modifier is held but a selection
   drag is already in progress, the drag continues.
5. `SelectionMode::Semantic` ([04 §8.2](04-terminal-core.md#82-selection))
   is redefined in terms of this document: semantic select selects the
   *innermost match span* under the cursor, falling back to `word_chars`. One
   definition of "the thing under the cursor", used by both selection and
   activation, so double-click-then-copy and hint-then-copy give byte-identical
   results. This is a small change to 04 §8.2's phrasing; see [§12](#12-open-questions).

### 5.5 Blocks — the third, mouse-free path

[04 §7](04-terminal-core.md) gives omt something a plain terminal does not: the
output of one command is an addressable object. So a fourth activation route
exists that needs neither mouse nor hint labels:

- `session.blocks.list` → a block → **"targets in this block"** → a list of
  every match with its resolved state, navigable with arrows, actionable with
  enter. On a phone this is a scrollable list of every file a failing test
  mentioned, which is a better UI than the terminal it came from.
- The block action menu gains **"open all files in this block"** (capped at
  `open.max_batch`, default 10, with a confirm naming the count) — the "cargo
  test printed six failures, open all of them" motion.
- A block's targets are computed on demand for the whole block, not just the
  visible part, and cached with the block.

### 5.6 Deferring to 16 — Input and keymap

Chord grammar, modifier normalization, the `when` predicate vocabulary, platform
overrides, conflict diagnostics (`OMT-C4xx`) and the `omt doctor keys` flow are
[16](16-input-and-keymap.md)'s, not this document's. This document contributes:

- two new `when` contexts, `hint_mode` and `open_menu_focused`, now carried in
  [16 §4.1](16-input-and-keymap.md#41-the-context-set)'s `ContextSet` alongside
  the `mouse_reporting` it already had;
- default bindings `<leader> f` / `<leader> F` / `<leader> g` (§5.2), now
  registered in [16 §8.2](16-input-and-keymap.md#82-the-leader-namespace).
  `<leader> g` coexists with the explorer's `g` prefix: they differ by `when`,
  which 16 §2.3 rule 3 resolves;
- one mouse-binding requirement, **now supported**: [16 §2.3](16-input-and-keymap.md#23-resolution)'s
  `Chord` admits a mouse event with modifiers (`"shift-mouse1"`) carrying a
  `when` predicate, and [16 §2.2](16-input-and-keymap.md#22-types) gives
  `MouseEvent` a shape (`kind`/`button`/`mods`/`pos`, decoded from SGR 1006).

---

## 6. The `omt ssh` remote flow

The scenario: laptop runs `omt ssh box` (a thin `omt-tui` over the ssh-stdio
transport, [09 §6](09-ssh-and-media.md#6-omt---remote-ssh-target--the-thin-client));
the session, the files and the agent are on `box`. The user hits `ctrl-b f`,
types `sd`, and the target is `/srv/app/src/lexer.rs:88:17` — on `box`.

### 6.1 How the client knows the file is remote

It doesn't have to guess. `ResolvedTarget` carries `host: TargetHost`, filled by
the *resolving* instance:

```rust
pub enum TargetHost {
    /// The resolving instance is the client's own machine.
    Local,
    /// The file lives on the instance; the calling client is elsewhere.
    Instance { instance: InstanceId, host: RemoteHost },
}
```

The client compares `instance` against its own instance id — a fact it learns at
handshake ([07](07-remote-protocol.md)), not something inferred. A block's
`host` field (from `OSC 1337;RemoteHost=`, [04 §7](04-terminal-core.md)) further
distinguishes "the instance's own filesystem" from "a host the *instance* ssh'd
into", which is a third machine and is handled honestly in §6.7.

### 6.2 The flow, end to end

```
 local omt-tui (laptop)                          remote omt instance (box)
 ──────────────────────                          ─────────────────────────
 ctrl-b f, "sd"
   → open.hints.select { hint_session, "sd" }  ──►
                                                  resolve (block cwd, stat, sensitivity)
                                                  handler = "editor",
                                                  editor.side resolves to `client`
                                              ◄── Activation::ClientMust(OpenLocalFile{
                                                    blob, name, line, col, provenance,
                                                    read_only: true })
   ── media.blob.begin/pull over the SAME       ►
      multiplexed control channel  [09 §4.2/§6.1]
   ◄─ binary frames, separate logical stream, lower priority than input
   → land in local blob store, materialize into
     the managed remote-mirror dir  (§6.3)
   → spawn local editor with §4.4 argv
   → register the file with the provenance
     sidecar + read-only marker  (§6.5)
```

Three points about the transfer:

- **It reuses [09](09-ssh-and-media.md)'s protocol exactly.** `media.file.pull`
  reads the path on the instance into a blob; `media.blob.*` streams it. The
  content-hash dedup means re-opening the same file twice costs one round trip
  ([09 §2](09-ssh-and-media.md#2-the-blob-store)). Nothing new is defined here,
  and that is deliberate: a second file-transfer protocol in the same product
  would be a maintenance and security liability for no gain.
- **It rides the multiplexed control channel**, so a 2 MB file does not stall
  keystrokes ([09 §6.1](09-ssh-and-media.md#61-the-media-path-in-thin-client-mode)).
- **It is a `media.file.pull` in the *pull* direction**, which is the direction
  [09 §7.4](09-ssh-and-media.md#74-file-push-and-pull) already specifies and
  quota-checks.

### 6.3 Where the file lands, and why the layout matters

**This is not a second file store.** The mirror tree is a named region *of the
blob store* — [09 §2](09-ssh-and-media.md#2-the-blob-store)'s `BlobClass::Mirror`
— and 09 owns it. Everything below describes the layout and the policy values
this document supplies; the enforcement, the sweeper, the refcount and the only
write path (`resolve_in_root`) all stay inside `omt-media`, per 09's rule that
rules live in the store and not in its callers.

```
$XDG_STATE_HOME/omt/remote/<host>/<h8>/<tail…>/<basename>
                            │       │    └── up to 3 trailing components of the remote dir
                            │       └────── first 8 hex of BLAKE3(full remote path)
                            └────────────── the remote host name

e.g.  ~/.local/state/omt/remote/box/3f9a1c02/app/src/lexer.rs
      sidecar:  ~/.local/state/omt/remote/box/3f9a1c02/app/src/.lexer.rs.omt-remote.json
```

Every element earns its place:

- **`<basename>` is preserved verbatim.** The editor tab says `lexer.rs`, syntax
  highlighting works, language servers pick the right grammar, and the file
  looks like what it is. A hash-named file is unusable.
- **The trailing path components** give the editor's breadcrumb and any
  "recently opened" list enough context to distinguish two `mod.rs` files.
- **`<h8>`** guarantees uniqueness without extending the human-readable part, so
  `/a/b/src/lexer.rs` and `/c/d/src/lexer.rs` never collide.
- **`<host>` first** means the path *reads* as remote. A user glancing at the
  title bar sees `omt/remote/box/…` and knows.
- The tree lives under `XDG_STATE_HOME`, not `XDG_RUNTIME_DIR`, because editors
  keep files open across reboots and a vanished path mid-session is worse than a
  stale one. That root is the `Mirror` class's root, chosen by `omt-media`
  ([09 §2](09-ssh-and-media.md#2-the-blob-store)) rather than by this crate. It
  is swept by the store's own sweeper, and it is pinned while an editor is
  believed to have it open — which `omt-open` expresses by calling the store's
  `pin`/`unpin`, never by touching a refcount itself.

The **sidecar** is the machine-readable provenance record: remote instance id,
host, absolute remote path, BLAKE3 of the fetched content, fetch time, the
session and block it came from, and `read_only`. `omt open remote list` prints
it; §6.5's write-back consumes it.

### 6.4 Limits, and the shapes that are not files

Checked on the **instance**, before a byte moves, so the failure is instant and
explained:

| Case | Behavior |
|---|---|
| Size > `open.remote.max_bytes` (default **4 MiB**). This is not a limit of this document's own: it *configures* the `Mirror` class's `max_blob_bytes` in [09 §2](09-ssh-and-media.md#2-the-blob-store), and the store enforces it. It sits well under 09's 32 MiB absolute ceiling because this is a *source file*, and a 30 MB one is a generated blob nobody wants in an editor | Refuse. Offer: `read_inline` with a line window (§6.6), `explorer` (streams a range), or "fetch anyway" as an explicit second action |
| Binary (sniffed per [15 §9.2](15-workspace-explorer.md#92-size-limits-and-binary-detection): NUL in first 8 KiB, or >5 % replacement chars) | Do not open in a text editor. Offer `read_inline` (which renders an image inline if it is one, per [09 §7.3](09-ssh-and-media.md#73-display-in-the-web-client)), `download to ~/Downloads`, `copy path` |
| Directory | Never fetched. Offer `explorer` (which is remote-native and needs no transfer), `insert path into agent prompt`, `copy` |
| Not readable by the remote user (`EACCES`) | `Unresolved { RemoteUnreadable }` with the actual errno string and the file's mode/owner from `stat`, so the user knows whether to `sudo` or give up |
| fifo / socket / device | Refused outright, same rule as [09 §8](09-ssh-and-media.md#8-security) |
| Sensitive per [15 §9.4](15-workspace-explorer.md#94-sensitive-files) | Fetch requires `Operator` and an explicit confirm naming the file; the fetched copy is `0600` and the mirror is created with `ttl_override = 1h` ([09 §2](09-ssh-and-media.md#2-the-blob-store)) instead of the class default |
| Symlink | Resolved **on the remote** through `confine()`; a link escaping every workspace root is fetchable only under the same "outside workspace ⇒ editor-only" rule as §3.3 |

### 6.5 The snapshot problem — and the recommendation

**The trap, stated plainly:** the file on the laptop is a *copy*. If the user
edits it and saves, the bytes go to `~/.local/state/omt/remote/box/…`. The remote
repo is unchanged. The agent on `box` — which is the entire reason the user is
attached to `box` — will never see the edit, will re-read the old file, and will
"fix" a bug the user already fixed, or overwrite the change. The user's work is
not lost from disk, but it is lost from the process, and they find out much
later. This is a data-loss-shaped surprise and the design must not paper over it.

Three mitigations ship, and they are not alternatives — all three are on:

**1. Read-only by default, enforced and stated.** The materialized file is
`chmod 0444`. Every editor in §4.4's table shows a "read-only" indication for a
non-writable file, and refuses to save without an explicit override
(`:w!`, VS Code's "Overwrite"). That is the mechanism doing the work: the user
hits the wall *before* they have made changes they care about, not after.
`open.remote.read_only = true` by default; setting it false requires
acknowledging the write-back mode below.

**2. Provenance is visible everywhere.** The path contains `remote/box`. The omt
status line shows `viewing remote:box:/srv/app/src/lexer.rs (snapshot, read-only)`
while an editor is believed open. `read_inline` and the explorer render a
persistent banner. The sidecar makes it recoverable by tooling.

**3. A designed write-back path — but not the default.** `open.remote.mode`:

| Mode | Behavior | Default? |
|---|---|---|
| `handoff` | Do not transfer. Hand the *remote* location to an editor that speaks remote natively (§6.5.1). | **preferred when available** |
| `snapshot` | Fetch, `0444`, read-only. | **the default fallback** |
| `writeback` | Fetch `0644`, watch the file, push changes back on save with conflict detection (§6.5.2). | opt-in, per host |
| `inline` | Never transfer; view in omt's own explorer / web client. | the mobile default |

**6.5.1 `handoff` — the recommendation.** When the local editor can open a
remote path itself, this is strictly better than any copy: there is one file, on
the machine that matters, and the editor's own remote machinery (which is
mature, handles saves, watches, and LSP) does the work.

| Editor | Handoff invocation | Preconditions omt checks |
|---|---|---|
| VS Code family (`code`, `cursor`, `windsurf`, `codium`) | `code --folder-uri vscode-remote://ssh-remote+<host>/<dir>` then `--file-uri vscode-remote://ssh-remote+<host>/<path>:<line>` | Remote-SSH extension present (`code --list-extensions`), and `<host>` resolvable as an ssh alias — omt uses the **same alias the user typed to `omt ssh`**, which is the one their `~/.ssh/config` already works with |
| JetBrains Gateway | `gateway ssh://<host>/<path>` (project-level; line positioning unavailable) | `gateway` on `PATH` |
| Neovim | `nvim` with `scp://<host>//<path>` (netrw) or an existing `--listen` server + `--remote` | netrw enabled; slow for large files, so offered but not preferred |
| Emacs | `emacsclient -n "/ssh:<host>:<path>"` (TRAMP) | `emacsclient` reaches a running server |
| Zed | remote projects exist but the CLI surface for opening one path is **unverified** — see [§12](#12-open-questions) | — |
| everything else | not available | — |

Detection is done **once per (editor, version)** and cached, not on every open.
When handoff is available, it is ranked above `snapshot` automatically and the
UI says which one it used.

**The recommendation, unambiguously: default `open.remote.mode = "auto"`, which
resolves to `handoff` when the detected editor supports it, `snapshot` otherwise,
and `inline` on the web/mobile client.** Rationale: handoff has no snapshot
semantics to explain because there is no snapshot; snapshot-read-only is a
correct, honest, *useful* fallback (reading a stack frame is the majority of
these activations and needs no write path at all); and `writeback` is a
synchronization feature, which is a category omt should be very reluctant to
enter — it has conflicts, partial writes, and permission preservation, and
getting it subtly wrong loses user data in a system whose whole selling point is
that an agent is concurrently editing the same tree.

**6.5.2 `writeback`, specified because deferring it silently would be worse.**
When explicitly enabled per host:

- The fetched file is `0644`. A `notify` watcher on the materialized path
  debounces 300 ms after the last write. The watcher lives in `omt-daemon` and is
  a **caller** of `omt-media`: it reads the mirror through the store's API and
  holds the mirror alive with a `pin` handle. It does not write into the blob
  tree and it does not mutate a refcount directly — [09 §2](09-ssh-and-media.md#2-the-blob-store)'s
  `resolve_in_root` remains the store's only writer.
- On change: recompute BLAKE3. If unchanged from the fetched hash (editors
  rewrite files on save without changes), do nothing.
- **Pre-flight conflict check.** Call `workspace.files.stat` on the remote and
  compare mtime+size against the sidecar's record, then fetch and hash if they
  differ. If the remote changed since the fetch — *which is exactly what happens
  when the agent edited it* — **do not push.** Show a conflict card with a diff
  of local-vs-remote and three choices: keep remote (discard local), overwrite
  remote (with the diff shown and a confirm), or write local changes to
  `<path>.omt-local` on the remote and open both.
- No conflict: `media.file.push` ([09 §7.4](09-ssh-and-media.md#74-file-push-and-pull))
  with `overwrite: true`, preserving the original mode, through the remote's
  `confine()`. Every push is an audit event and a visible toast naming the
  remote path.
- The watcher stops when the mirror's TTL expires or the session detaches,
  dropping its `pin`, and says so — a silently-stopped sync is the worst possible
  state. `open.remote.discard` is the explicit form of the same call.

`writeback` is documented as **experimental** and is off by default. That is a
deliberate, stated choice, not an omission.

### 6.6 The escape hatch that is always right

For a remote target, `read_inline` — render the file in omt's own viewer at the
line, streaming only a window around it via `workspace.files.read` — has no
transfer, no snapshot, no staleness beyond one request, works for a 2 GB log,
works on a phone, and is available for every case in §6.4's refusal table. It is
ranked first on mobile and offered on every failure. "You cannot open this, but
here is the content" is the difference between a limit and a dead end.

### 6.7 The third machine

If the *instance* is itself ssh'd somewhere (the block's `RemoteHost` differs
from the instance's host), omt does not chain transfers. The target is marked
`host: <that host>`, all filesystem actions are disabled with the reason "this
path is on `db-01`, which omt is not attached to", and the offered actions are
`copy`, `insert into agent prompt`, and — the useful one — **"attach omt to
`db-01`"**, which is `instance.peers.add`. Pretending to reach a machine omt has
no connection to is exactly the class of quiet wrongness §5.3 exists to
criticize in others.

---

## 7. Web and mobile parity

In a browser the mouse conflict evaporates: the web client renders omt's grid
itself and no inner program has grabbed anything. So the web client gets the
*best* version of this feature, and that is worth being explicit about
([P3](01-principles.md#p3--parity-one-capability-three-surfaces)'s "mobile is a
target, not a fallback").

**Desktop browser.** Matches render as real underlined spans. Plain click
activates the default handler; right-click opens the action menu; `ctrl-b f`
enters the identical hint mode driven by the identical capabilities. No modifier
is required and none is stolen, because the browser is not multiplexing anyone
else's mouse.

**Phone.** The design constraint is a 44 px minimum touch target against a
terminal cell that is ~8 px wide, so tapping a match in a rendered grid is not
viable and is not attempted:

- **Match spans get an expanded hit area** (a transparent overlay padded to 44 px
  tall, horizontally clamped to the span) in the grid view, and adjacent
  overlapping hit areas are resolved by a **disambiguation popover** listing the
  candidate matches by text — a tap that is between two links shows both rather
  than guessing.
- **Long-press (400 ms) opens the action sheet**; a plain tap runs the default
  handler. This is the platform convention and it is what users will try.
- **Block view is the primary surface**, per [08](08-web-client.md). Each block
  carries a **"Targets (7)"** affordance expanding to a proper list — full-width
  rows, 56 px tall, each showing the match text, its resolved path, its
  existence state, and a trailing action button. This is §5.5's block path, and
  on a phone it is better than the terminal it came from: no zooming, no
  precision tapping, and it works for matches scrolled off screen.
- **The hint-mode overlay is tappable**, so a phone user with the chord bound to
  a toolbar button gets the same labelled overlay and taps a label.
- **Every action is reachable.** The parity test (§9) asserts it, and the
  handler ranks in §4.2 are surface-aware precisely so the phone's defaults are
  right: `read_inline` ranks 85 on web and 30 on the TUI, so tapping a
  `file:line` on a phone **shows you the code**, syntax-highlighted, at that
  line, rather than trying to launch an editor on a machine you are not sitting
  at. That is the single best interaction in this document and it exists only
  because the file never needed to move.

**URL opening on the web** is `window.open(url, "_blank", "noopener,noreferrer")`
after §8.1's checks, from a real user gesture so popup blockers do not eat it.
`file:` URLs are **never** opened by the browser (browsers refuse anyway); they
route to `read_inline`.

---

## 8. Security

Terminal output is attacker-controlled. `cat` a hostile file, `curl` a hostile
server, or — the case that matters most here — let an agent print a model's
output, and arbitrary bytes are on screen carrying whatever text an adversary
chose. Everything in this document must hold under that assumption.

### 8.1 URLs

- **Never auto-open. Ever.** No hover-open, no "activate on render", no
  `open.autoopen` config key. Every activation is a user gesture on a specific
  match. This is not negotiable and there is no knob to relax it.
- **Scheme allow-list**, checked after normalization, default
  `["http", "https", "mailto"]`. `file` is **not** in the default list: a `file:`
  URL from terminal output opened by the OS handler executes whatever the OS
  associates with that extension, and on a thin client it would silently target
  the wrong machine. `file:` URLs are recognized (so §2.1's OSC 8 enrichment
  works — `rg --hyperlink-format` emits them) but resolve to a `Path` target and
  go through §3, never to the browser. Adding a scheme is explicit config, and
  the notoriously dangerous ones (`javascript`, `data`, `vbscript`, `blob`,
  `about`, `chrome`, `ms-msdt`, `search-ms`, `vscode`, `jetbrains`, `ssh`,
  `smb`, `\\`-UNC) are on a **deny-list that config cannot override** — a
  user-added scheme is intersected with the allow-list, not unioned past the
  deny-list.
- **Homograph and punycode display.** The host is decoded and re-rendered with
  an explicit policy borrowed from browser practice: a host whose labels mix
  scripts, or contain confusables outside the user's configured locale scripts,
  is displayed **as punycode** with a warning glyph. The action sheet always
  shows the **full, decoded-and-re-encoded origin** on its own line, in a
  distinct style, above the path — because the thing being confirmed is the
  origin, not the pretty text. `UrlDisplay { rendered, punycode_forced, mixed_script, truncated }`
  carries this to every surface so all three render the same warning.
- **Text/target mismatch is surfaced.** An OSC 8 span whose visible text is
  itself a URL with a different host than the link target is the phishing shape.
  The action sheet shows both, labelled `text:` and `target:`, and the default
  action becomes `menu` rather than `browser`.
- **Length and control characters.** A URL over 2048 chars is truncated for
  display with the full value available on demand. Any C0/C1, bidi-override
  (U+202A–U+202E, U+2066–U+2069) or zero-width character in the *displayed* text
  is rendered as an escaped glyph — the bidi trick that makes `evil.com` look
  like `moc.live` must not survive into a confirmation dialog.
- **No referrer, no window opener**; the daemon never fetches a URL to "preview"
  it, because that would turn a rendered line into an SSRF from the user's
  machine.

### 8.2 Paths

- **Confinement at action time** per §3.3, using [15 §9.1](15-workspace-explorer.md#91-path-confinement)'s
  `confine()` verbatim — the same `realpath`-then-`openat`-`O_NOFOLLOW` sequence,
  not a reimplementation. A symlink inside a workspace pointing at `/etc` is a
  `SymlinkTarget::Outside` dead end, not a fetch.
- **Sensitive files** per [15 §9.4](15-workspace-explorer.md#94-sensitive-files):
  badged, content refused to `Viewer`, extra confirm before `agent_insert`, and
  a shorter TTL on any fetched copy (§6.4). The redactor from
  [13 §8](13-security.md#8-secret-redaction) runs over `read_inline` output on
  the way out, as it already does for diffs.
- **`Unknown`/`Missing` never falls back to a different path.** §3.5.
- **No path is ever executed.** There is no "run this file" handler and there
  will not be one.

### 8.3 Editor and command templates

The highest-value injection target in this document, because the input is
attacker-controlled text and the output is a process.

1. **argv only.** `std::process::Command` with `.arg()` per element. No
   `sh -c`, no `cmd /c`, no `CreateProcess` command-line string on Windows —
   Windows uses the raw-argument API with explicit quoting rules so the
   `"` / `\` / `^` re-parsing hazards are handled once, in one place, with tests.
2. **Typed substitution.** `{line}`/`{col}` are `u32` formatted by omt.
   `{path}` is a `PathBuf`. `{workspace_root}` is a `CanonicalPath`. No template
   variable is ever concatenated into a larger string containing a separator; an
   element is either fully literal or exactly one substituted value. `--goto {path}:{line}:{col}`
   is the one exception and is built by `format!` from three *typed* values, not
   from raw match text.
3. **A path is never allowed to look like a flag.** If the resolved path's first
   byte is `-`, omt prefixes it with `./` (or passes it after a `--` separator
   when the program is known to accept one). `-rf` as a filename does not become
   an option.
4. **`program` is resolved from `PATH` at config load**, and the resolved
   absolute path is what is spawned and what the confirm sheet displays. A
   `PATH` change mid-session cannot redirect an editor launch, and the user sees
   `/usr/local/bin/code`, not `code`.
5. **Environment is not inherited wholesale**: the spawn gets a filtered
   environment (`PATH`, `HOME`, `USER`, `LANG`, `DISPLAY`/`WAYLAND_DISPLAY`,
   `TERM` when `in_pane`, plus `open.editor.env`), so a session's injected
   agent credentials do not leak into an editor process.
6. **Fuzz + corpus.** §10.
7. **`SPAWNS_PROCESS` drives the confirmation** on every surface via
   [08 §2.3](08-web-client.md#23-effects-drive-ui-policy-not-just-audit), so a
   phone activating an editor sees a sheet naming the resolved program — which
   is the correct amount of friction for "spawn a process on my laptop".

### 8.4 Rules as untrusted input

A user-supplied regex is a DoS surface. The `regex` crate has no catastrophic
backtracking, but it does have compilation blowup: each rule compiles under a
`size_limit` (1 MiB) and `dfa_size_limit` (2 MiB), and the whole `RegexSet` is
compiled at config-load time so a bad rule is a **config error with a line and
column** ([P7](01-principles.md#p7--configuration-is-data-and-errors-are-precise)),
never a runtime hang. A rule whose confirm pass exceeds `open.match.rule_budget_us`
(default 200 µs) on a single line is disabled for the remainder of the session
with a diagnostic naming it.

### 8.5 Audit

Every `open.activate` emits an event carrying actor, device, match text, rule id,
resolved target, chosen handler, declared effects and outcome. "What did my phone
open on my laptop, and when" is answerable, per [D2](decisions.md#d2--remote-is-exactly-equivalent-to-local)'s
attribution consequence.

---

## 9. Capabilities

Declared in [03](03-capability-catalog.md)'s style; all in group `open`.

**These signatures are authoritative**, and
[04 §8.4](04-terminal-core.md#84-semantic-click-targets) says so ("this document
does not restate their shapes"). Its sketch `open.targets.list { session,
position } -> [Target]` is illustrative and out of date on three counts, each of
which matters: a `position` input can only ever return the matches on one line,
so hint mode — which needs **every match in the viewport** in one round trip —
could not be built on it; the return must be `Match`, not `Target`, because a
phone needs the *span* to draw the hint and the `MatchId` to name what it then
resolves; and `generation` is what lets a client know its cached match list is
still valid without re-fetching. `TargetScope` and `MatchRef` are the shapes that
follow. 04's sketch is a cross-document edit that 04's owner should apply.

```rust
capability! {
    /// Every match in a scope, with spans and rule ids. Pure; no filesystem access.
    name  = "open.targets.list",
    group = "open", verb = "targets-list",
    kind  = Query, role = Role::Viewer,
    input  = TargetsList { session: SessionId, scope: TargetScope /* Viewport | Block(BlockId) | Line(Position) */,
                           kinds: Option<Vec<MatchKind>>, limit: Option<u32> },
    output = TargetsListOut { matches: Vec<Match>, truncated: bool, generation: Generation },
    effects = [],
    since = "0.5",
}

capability! {
    /// Resolve one match to a concrete target: cwd resolution, stat (cached),
    /// sensitivity, and the ordered list of applicable actions. Idempotent.
    name  = "open.resolve",
    group = "open", verb = "resolve",
    kind  = Query, role = Role::Viewer,
    input  = ResolveIn { session: SessionId, match_ref: MatchRef /* ById(MatchId) | AtPosition(Position) | RawText(String) */,
                         for_client: ClientProfile /* surface + editor capabilities, so ranks are correct */ },
    output = ResolvedTarget,
    effects = [Effects::READS_FS],
    since = "0.5",
}

capability! {
    /// Run a handler against a resolved target. The one mutating entry point.
    /// Declares the *union* of its handlers' effects; the audit record carries
    /// the actual handler's effects, and the mobile confirm sheet is driven by
    /// the `ActionOffer.effects` returned from `open.resolve`, not by this.
    name  = "open.activate",
    group = "open", verb = "activate",
    kind  = Command, role = Role::Operator,
    input  = ActivateIn { session: SessionId, match_ref: MatchRef, handler: Option<HandlerId>,
                          confirm_token: Option<ConfirmToken>, agent_pane: Option<PaneId> },
    output = ActivateOut { activation: Activation, handler: HandlerId },
    effects = [Effects::READS_FS, Effects::SPAWNS_PROCESS, Effects::WRITES_PTY, Effects::NETWORK],
    since = "0.5",
}

capability! {
    /// The registry contents, with per-target applicability when a match is named.
    name  = "open.handlers.list",
    group = "open", verb = "handlers-list",
    kind  = Query, role = Role::Viewer,
    input  = HandlersList { session: Option<SessionId>, match_ref: Option<MatchRef> },
    output = HandlersListOut { handlers: Vec<HandlerInfo>, offers: Option<Vec<ActionOffer>> },
    effects = [],
    since = "0.5",
}

capability! {
    /// Enter hint mode for one client's viewport. Instance-owned so the TUI,
    /// the browser and the phone share one label assignment and one state machine.
    name  = "open.hints.begin",
    group = "open", verb = "hints-begin",
    kind  = Command, role = Role::Operator,
    input  = HintsBegin { session: SessionId, scope: TargetScope, kinds: Option<Vec<MatchKind>>,
                          alphabet: Option<String>, action: HintAction /* Default | Menu | Handler(HandlerId) */ },
    output = HintsBeginOut { hint_session: HintSessionId, hints: Vec<Hint /* { label, span, kind, exists } */> },
    effects = [],
    since = "0.5",
}

capability! {
    /// Feed keystrokes into an open hint session. Returns the narrowed set, or
    /// the activation when a label resolves uniquely.
    name  = "open.hints.select",
    group = "open", verb = "hints-select",
    kind  = Command, role = Role::Operator,
    input  = HintsSelect { hint_session: HintSessionId, keys: String },
    output = HintsSelectOut { remaining: Vec<Hint>, chosen: Option<MatchId>, activation: Option<Activation> },
    effects = [Effects::READS_FS, Effects::SPAWNS_PROCESS, Effects::WRITES_PTY, Effects::NETWORK],
    since = "0.5",
}

capability! {
    name  = "open.hints.cancel",
    group = "open", verb = "hints-cancel",
    kind  = Command, role = Role::Operator,
    input  = HintsCancel { hint_session: HintSessionId },
    output = Ack,
    effects = [],
    since = "0.5",
}
```

Plus four smaller ones, same pattern:

| Capability | Kind/Role | Input → Output | Effects |
|---|---|---|---|
| `open.rules.list` | Q / Viewer | `{}` → `{rules: Vec<RuleInfo>}` (id, kind, precedence, enabled, source layer) | — |
| `open.rules.test` | Q / Viewer | `{text, rules?}` → `{matches, overlaps_resolved}` — powers `omt open rules test` and the config editor's live preview | — |
| `open.remote.list` | Q / Viewer | `{}` → `{files: Vec<RemoteMirror>}` (the §6.3 sidecars: remote path, host, fetched-at, hash, read_only, writeback state) | `READS_FS` |
| `open.remote.discard` | C / Operator | `{path}` → `Ack` — drop a mirrored file and stop its watcher, via `omt-media`'s `unpin`/discard API | `WRITES_FS` |

**Events** on the existing bus: `OpenActivated { match, handler, outcome }`,
`OpenRemoteFetched { remote_path, host, bytes }`, `OpenWritebackConflict { .. }`,
`HintSessionChanged { hint_session, remaining }`. The last one is what lets a
phone and the TUI show the same hint overlay simultaneously.

**Parity exemptions:** none. Every capability here has a TUI binding, a web
handler and a generated doc entry, and the §7 mobile design exists so that is
true rather than exempted.

---

## 10. Performance

**The rule: the matcher never runs over the scrollback.** A 100 000-line
scrollback with 25 rules is ~2.5 M regex executions and it would be triggered by
scrolling. It does not happen, by construction:

| Scope | When it runs | Bound |
|---|---|---|
| Viewport | lines entering the viewport, and completed lines only | ≤ `rows` logical lines per frame; a full-screen redraw is ≤ 60 lines |
| Block | on demand (`open.targets.list` with `scope: Block`, or §5.5's UI) | capped at `open.match.max_block_lines` (default 5 000), resumable like `SearchCursor` ([04 §8.1](04-terminal-core.md#81-search)) |
| Everything else | never | — |

Mechanics:

- **`RegexSet` prefilter.** One pass answers "which rules could match". On the
  overwhelmingly common no-match line that pass is the whole cost.
- **A byte-level pre-prefilter before even that**: a line containing none of
  `/ : . # @ h w` (a `memchr`-style class test) cannot match any built-in rule
  and skips the `RegexSet` entirely. Built from the enabled rule set at config
  load, so user rules widen it correctly.
- **Cache keyed by `(LineId, Generation)`**, LRU of `open.match.cache_lines`
  (default 4 096). Generation equality already means content equality
  ([04 §4.1](04-terminal-core.md)), so invalidation is free and correct.
- **Damage-driven.** Only lines in the frame's `Damage` are re-matched. A frame
  with `DamageKind::Scroll` re-matches only the newly exposed rows.
- **Resolution is separate and lazier still.** `stat` runs only for matches that
  are (a) visible **and** (b) `kind = Path`, behind the 2 s `StatCache`, at most
  `open.resolve.max_stats_per_frame` (default 32) per frame with the remainder
  deferred to the next. Undecided matches render as plain (not strikethrough)
  until their `stat` lands — an unknown state never renders as a definite one.
- **Remote resolution is never speculative.** For a thin client, `open.resolve`
  is issued on activation or on entering hint mode, not per frame. Hint mode
  batches: one `open.hints.begin` returns every label with its existence state
  in a single round trip, so the whole overlay costs one RTT.

Budget, on the [04 §9.1](04-terminal-core.md#91-targets) reference machine:

| Workload | Target |
|---|---|
| Match one 200-column line, 25 rules, no match | < 1.5 µs |
| Match one 200-column line with 3 matches | < 12 µs |
| Full viewport (60 lines) cold, no cache | < 400 µs p99 |
| Full viewport warm (generation hit) | < 5 µs |
| Added frame time at 60 fps, steady output | < 0.5 ms p99 |
| `open.resolve` local, warm `StatCache` | < 50 µs |
| `open.hints.begin`, 40 matches, local | < 3 ms |
| `open.hints.begin`, 40 matches, thin client over ssh | one RTT + < 3 ms |
| Block scan, 5 000 lines | < 40 ms, resumable |

Added to `benches/` next to [04 §9.5](04-terminal-core.md#95-benchmark-plan)'s
suite: `match_nomatch`, `match_dense`, `match_viewport_cold`, `match_viewport_warm`,
`hints_assign`, all under the same 10 % regression gate.

---

## 11. Testing

**1. The recognition corpus** — the centerpiece. `tests/corpus/open/` holds real
captured output, one file per producer, each with a `.expected.json` listing
every expected `(rule, span, slots)` and — equally important — **the spans that
must *not* match**. Producers: `rustc`, `cargo test`, `cargo nextest`, `clippy`,
`gcc`, `clang`, `ld`, `make`, Python tracebacks (2 nesting levels + chained
`During handling…`), `pytest -v` failures, `mypy`, `ruff`, Node/V8 stacks (CJS,
ESM, source-mapped, bundled), `jest`, `vitest`, `tsc`, `eslint`, Go build/vet/
panic/`go test -run`, Java and Kotlin stack traces, Ruby, PHP, `rg`, `rg --vimgrep`,
`grep -n`, `git diff`, `git log --oneline`, `git status`, `git blame`, `delta`,
`docker build`, `kubectl describe`, `journalctl`, `npm`/`pnpm` error output, and
a **hostile file** of adversarial lines (bidi overrides, homograph URLs, paths
starting with `-`, 4 KB single-token lines, nested quotes, ANSI-in-path).
A new rule requires a corpus entry; a corpus regression fails CI with the
producer name and the diff.

**2. Reflow stability.** For each corpus file: match at width 80, reflow
80→200→40→80, assert the match set is *identical by `Position`* and that
`selection.to_string` over each span returns the same bytes at every width. This
is the property that makes §2.5's design testable rather than asserted.

**3. Overlap determinism.** A table of hand-built lines with deliberate
ambiguity (`at /a/b.js:1:2`, `src/x.rs:5:1: error: 3:4`, an OSC 8 span
overlapping a heuristic match, two user rules at equal precedence and equal
length) asserting exactly which rule wins and why. Rule-order changes that alter
an outcome must update this table, which makes them visible in review.

**4. Injection.** A corpus of malicious *filenames and match texts* —
`; rm -rf ~`, `$(id)`, `` `id` ``, `--goto=/etc/passwd`, `-rf`, `\\?\C:\`,
`a"b`, `a'b`, `a\nb`, 4 096-byte names, NUL attempts — driven through every
editor row in §4.4 and every user-template shape in §4.6, asserting the exact
argv vector produced. A **fake editor binary** that writes its `argv` to a file
runs the real spawn path end to end, on Unix and Windows, so the assertion is
about what the OS received, not about what omt intended.

**5. Mouse-reporting interaction.** A headless harness drives a synthetic
program that enables 1000/1002/1003/1006 and records what it receives.
Assertions: plain click while reporting is on reaches the app byte-identically
to a bare terminal; Shift+click does not reach the app and does produce an
activation; drag beyond slop produces a selection and no activation; a click
sequence crossing a mode change (app disables 1000 mid-drag) does not activate.
Plus a replay of recorded vim, helix, lazygit and an agent TUI session
asserting **zero** mouse bytes differ from a control run without omt's handler.

**6. cwd correctness.** The §3.1 scenario as an integration test: a scripted
shell with OSC 133/OSC 7 integration `cd`s, produces an error, `cd`s back; the
test asserts resolution picks the block cwd. A second variant with a decoy
`src/lexer.rs` at the top level asserts omt does **not** open the decoy. A third
without shell integration asserts the low-confidence label appears.

**7. Remote flow.** A two-instance harness (both in-process, connected over the
real ssh-stdio transport against a local `sshd` in CI) covering: fetch of a
normal file; dedup on second fetch; refusal for oversize, binary, directory,
`EACCES`, fifo; `0444` mode and sidecar contents; conflict detection in
`writeback` when the remote changes underneath; watcher teardown on detach.

**8. Fuzz.** Two targets, per [P5](01-principles.md#p5--production-grade-from-the-first-commit):
`fuzz_match` (arbitrary bytes → the matcher; asserts no panic, no span outside
the line, total time under budget) and `fuzz_resolve` (arbitrary path text +
context → the resolver; asserts no path escapes `confine()` and no syscall
outside the allowed roots, checked with a syscall-recording filesystem shim).

**9. Parity.** [03 §5](03-capability-catalog.md#5-the-parity-contract)'s test
covers the capabilities. Additionally: a test enumerating `OpenHandler`
implementations and asserting each has a web-client renderer entry and a mobile
action-sheet row, so §7's "every action reachable on a phone" is mechanically
true.

**10. Third-party handler test.** `tests/third_party_impl.rs` implements
`OpenHandler` and a `Rule` from outside `omt-open` using only its public API, per
[P2](01-principles.md#p2--pluggable-extension-without-modification)'s enforcement
clause.

---

## 12. OPEN QUESTIONS

Genuine uncertainties, and the cross-document edits this file implies.

**Cross-references other documents need** (I edited no existing file):

1. **[02 — Crate map](02-crate-map.md)** — add `omt-open` to the L2 list and to
   the "Ownership for parallel implementation" table (`Semantic open` →
   `omt-open` → blocked by contracts, terminal). Move `Match`/`Target` into
   `omt-types`.
2. **[04 §8.3–8.4](04-terminal-core.md#83-hyperlinks-and-detection)** — `Target`
   moves to `omt-types` and gains `GitRef`/`Issue`; `Custom` gains named slots.
   The click-target capabilities it points at are `open.targets.list` /
   `open.resolve` from §9 — 04 links here rather than declaring its own.
   §5.4 also redefines `SelectionMode::Semantic` in terms
   of match spans, which 04 §8.2 currently leaves to `word_chars`.
3. **[03 §6](03-capability-catalog.md#6-capability-groups-initial-surface)** —
   add an `open` row to the group table.
4. **[10 §7](10-configuration.md#7-the-settings-surface)** — a new `[open]`
   section (`default`, `rank`, `binding`, `rule`, `handler`, `editor.*`,
   `mouse.*`, `hints.*`, `remote.*`, `match.*`, `issue.*`), and the §5.2 default
   keybindings in `keybindings.toml`.
5. **Resolved — [15](15-workspace-explorer.md) and `workspace.files.reveal`.**
   `workspace.files.reveal` stays as the capability 15 owns, and **the `editor`
   handler and `workspace.files.reveal` share one implementation**: this document
   owns the handler and the editor argv/template resolution (§4.4), 15 owns the
   capability surface. 15 §8.4's `editor_args` default
   (`--goto {path}:{line}:{col}`) is VS Code-specific and is superseded by the
   detected per-editor table here.
6. **[09](09-ssh-and-media.md)** — §6 adds a consumer of `media.file.pull`; no
   protocol change, but 09's §7.4 could mention it.
7. **Resolved — [16 — Input and keymap](16-input-and-keymap.md).** (a) The
   trigger grammar **does** support mouse events with modifiers:
   [16 §2.2](16-input-and-keymap.md#22-types) gives `MouseEvent` a shape
   (`kind`/`button`/`mods`/`pos`, SGR 1006) and
   [16 §2.3](16-input-and-keymap.md#23-resolution)'s `Chord` admits a
   `"shift-mouse1"`-style trigger with a `when`. (b) `mouse_reporting`,
   `hint_mode` and `open_menu_focused` are all in
   [16 §4.1](16-input-and-keymap.md#41-the-context-set)'s `ContextSet`.
   (c) `<leader> f` / `<leader> F` / `<leader> g` are registered in
   [16 §8.2](16-input-and-keymap.md#82-the-leader-namespace). Still outstanding:
   `omt doctor keys` needs the §5.3(c) outer-emulator section, which 16 owns.
8. **[00 — Overview](00-overview.md)** and any docs index need an `18` entry.

**Genuine technical uncertainties:**

**Layering — `omt-open` at L2 is unresolved, and is recorded rather than fixed
here.** [02](02-crate-map.md) says an L2 crate is "independently useful, owns no
global state" and that "L0–L2 are runtime-agnostic". §1.2 places `omt-open` at
L2, but three things cut against that: `OpenHandler` is `#[async_trait]` (a
runtime commitment), its built-ins **spawn processes**, and `open.hints.*` holds
**per-client state on the instance** (§5.2) — which is global state by any
reading. It also depends on `omt-catalog`, which the sibling L2 crate
`omt-workspace-fs` ([15 §2](15-workspace-explorer.md#2-the-crate-omt-workspace-fs-at-l2))
deliberately does not. **Proposed fix, for whoever owns
[02](02-crate-map.md) to rule on:** hint-session state moves to `omt-daemon`,
leaving `omt-open` with (a) the handler registry and (b) a **pure resolver** —
`Match` + `ResolutionContext` → `ResolvedTarget`, syscalls behind a trait — with
activation behind a `Spawner` seam exactly analogous to `omt-workspace-fs`'s
`WatchDriver`. That makes the crate testable without a runtime and restores the
L2 invariants. This document is deliberately **not** restructured on it.

9. **Shift+click passthrough is asserted, not verified.** The claim in §5.1 —
   that iTerm2, kitty, WezTerm, Ghostty, Alacritty, Terminal.app and Windows
   Terminal all let Shift bypass application mouse reporting, and that
   applications do not expect shifted clicks — is based on the long-standing
   xterm convention and on those terminals' documented selection behavior. It
   has **not** been tested emulator by emulator, nor against a corpus of TUIs.
   This is the single load-bearing empirical assumption in the document and it
   needs a hands-on matrix before the mouse path ships. If it fails on a major
   emulator, hint mode absorbs the loss without a redesign — which is a large
   part of why hint mode is primary.
10. **Which modifier the outer emulator swallows before omt sees it** is
    likewise knowledge, not detection (§5.3(a)). omt cannot observe another
    program's keymap. Is there a probe? (Send a modifier-click-shaped query and
    see whether anything arrives — probably not, since a swallowed event
    produces no signal at all.) If not, the one-shot card in §5.3(b) is the best
    available and its trigger heuristic ("unexplained gap") needs tuning against
    real usage.
11. **Zed's remote-project CLI surface** (§6.5.1) is unverified. If `zed` cannot
    be told to open one remote path at a line, it drops to `snapshot`.
12. **VS Code `--file-uri` with a line suffix.** `code -g` takes `file:line:col`
    for local paths; whether the `vscode-remote://` URI form accepts the same
    suffix is unverified. If not, handoff opens the file at line 1 and omt
    should say so rather than silently mis-position.
13. **`nvim --remote` and TRAMP latency.** Both are offered in §6.5.1; neither
    is measured. TRAMP over a slow link is famously slow, and offering an action
    that takes 20 seconds is worse than not offering it. Needs a timing gate.
14. **Basename-only resolution ranking** (§3.4) is a guess. In a monorepo with
    forty `mod.rs`, is "inside the block cwd subtree, then shortest, then most
    recently modified" the right order? Wants real-world tuning, and possibly a
    "remember my choice for this basename in this workspace" memory — which is
    state, and therefore deferred until there is evidence it is needed.
15. **`builtin.git_sha`'s false-positive rate** is unmeasured. The guard (digit
    *and* hex-letter, `git cat-file` verification) should make wrong matches
    silent rather than harmful, but a `docker` digest or a base16 UUID in a log
    will still light up spans that then quietly resolve to nothing. If the
    visual noise is bad, the rule should default to `enabled = false` outside
    blocks whose command matches `^git\b`.
16. **Hint-label stability under streaming output.** Labels are assigned in
    reading order over the viewport; a line arriving mid-hint-session shifts
    everything. Current design freezes the assignment at `hints.begin` and lets
    the content scroll under it, which is stable but can leave a label pointing
    at a match that has scrolled away. The alternative (re-assign on damage) is
    unusable because the label the user is halfway through typing changes. The
    freeze is chosen; whether it should also **pause output rendering** for the
    duration (tmux-fingers effectively does, by drawing a static overlay) is
    unresolved and is a real UX question.
17. **Does anything actually emit OSC 8 with a `line` fragment?** §2.1's
    enrichment rule exists because nothing standard does. If a convention
    emerges (`file:///p/x.rs#L88`, or a `line=` param), omt should consume it
    and the enrichment becomes a fallback rather than the norm.
