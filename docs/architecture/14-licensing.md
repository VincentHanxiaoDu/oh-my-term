# Licensing and Provenance

The policy behind **P9 — Clean-room with respect to studied code**
([01 — Principles](01-principles.md)). Short, because it must be memorable, and
precise, because it is the kind of rule that is only useful if it is unambiguous
at 2 a.m. during a refactor.

---

## 1. omt's own licence

`oh-my-term` is **Apache-2.0**. Every source file carries no per-file header; the
repository root carries `LICENSE` (Apache-2.0) and `NOTICE`. Contributions are
accepted under Apache-2.0 §5 (inbound = outbound); there is no CLA.

Apache-2.0 rather than MIT because of the explicit patent grant and the
`NOTICE`/attribution mechanism, both of which matter for a project that ships a
protocol and a plugin ABI.

---

## 2. What was studied, and under which licences

Organised by licence class rather than by product, because the licence is what
constrains omt and the product name is not. The specific projects are recorded
in the commit history of the work that referenced them; what matters here is
the rule each class imposes.

| Licence class | What omt took | What that permits |
|---|---|---|
| **AGPL-3.0** terminal emulators | Interface facts only: shell-integration escape-sequence numbers and payload shapes, YAML theme schema, workflow YAML schema, keymap file grammar, layout-configuration schema, and the *pattern* of a declarative settings registry | Reimplementation from public documentation. No code, no adaptation, no translation |
| **GPL-2.0** terminal emulators | Interface facts only: VT and xterm escape-sequence coverage, DSR/DECRQM behaviour, `.itermcolors` property-list key names | As above |
| **Apache-2.0** agent tooling | Interface facts, and ideas where useful — plugin manifest shape, detection-manifest concept, configuration-diagnostic approach | Compatible with omt's own licence, and omt still takes no code; see §3 |
| **Coding-agent CLIs**, various | Documented hook payload schemas, ACP and app-server protocol shapes, transcript file formats | Reimplementation from published protocol documentation and from bindings the tools generate for their own clients |

The AGPL and GPL classes are the ones that constrain omt. **Nothing in omt is
derived from AGPL or GPL code**, which is what lets omt be Apache-2.0. Even the
permissively licensed crates in those projects are untouched: omt uses ratatui
and its own grid, not a UI stack from anywhere else.

Checkouts of studied code are never committed — see §5.

---

## 3. The line we draw

**Interfaces are facts. Code is expression.**

Reimplementable — these carry no copyright as such, and reproducing them is how
interoperability works:

- Escape-sequence numbers and their grammar (OSC 133, OSC 52, OSC 7, OSC 1337,
 kitty graphics, the OSC 9277/9280 payload framing, DEC private modes).
- Wire protocols and message shapes (ACP, agent app-server methods, hook payload
 JSON, SSE event names).
- File formats and schemas (YAML theme keys, workflow YAML keys,
 keybinding file grammar, launch-configuration structure, iTerm2's
 `.itermcolors` plist keys, agent transcript JSONL fields).
- Field names, key names, enum spellings and default values that are *required*
 for a file written for the other tool to load in ours.
- Documented behaviour: "exit code 130 is not a failure", "swallow prompt hooks
 while the alt screen is active".

Not reimplementable — do not copy, adapt, translate, or paraphrase-into-code:

- Any Rust, Objective-C, Swift, C, TypeScript or shell **source** from a studied
 repository, including small helper functions and constant tables that are not
 themselves interface facts.
- Algorithms as *written there* — a reflow routine, a sum-tree implementation, a
 regex set. (The *idea* "use a sum tree for block heights" is fine; the code
 must come from an appropriately licensed crate or be written by us.)
- Comments, doc strings, test fixtures, and asset files.
- Line-by-line "port to Rust" of GPL/AGPL logic. Translation is derivation.

Ambiguous cases resolve toward *do not copy*. If a fact cannot be stated in the
research notes as a schema, a table, or a sentence — if it can only be conveyed
by showing the code — treat it as expression, not interface, and write our own.

Where a research note quotes source verbatim (it does, in short excerpts, marked
`[SRC]`), that is fair-dealing quotation for the purpose of describing an
interface. Those quotes are **not** an input to implementation: they are context
for the schema tables next to them.

---

## 4. Clean-room practice

1. **Research produces notes, not patches.** A research task reads the upstream
 code and writes a document in `docs/research/` describing interfaces: schemas,
 sequence numbers, state machines, defaults, file layouts. Notes mark every
 claim `[SRC]` (read from source), `[DOC]` (from public documentation) or
 `[INF]` (inference, labelled).
2. **Implementation reads the notes.** An implementation change cites the note
 section it implements. It does not need, and should not have, the upstream
 checkout open.
3. **The two roles are separated in time, and preferably by person.** For the
 AGPL and GPL targets this separation is the point. For
 Apache-2.0 targets it is good hygiene that keeps the provenance story
 uniform.
4. **Every generated artifact is ours.** Themes imported from the YAML format are
 parsed by our parser into our format; we ship no other terminals code and no YAML theme
 files. The importer's test fixtures are either files we wrote or files from a
 permissively licensed community corpus, recorded in `NOTICE`.

---

## 5. `.research/` is gitignored and must never be vendored

Upstream checkouts live in `.research/` at the repo root, which is in
`.gitignore` and additionally guarded:

- A CI job fails if any path under `.research/` is tracked, or if a commit adds
 a file matching known upstream markers (a vendor's own crate names, `iTerm2.xcodeproj`,
 `other terminals-plugin.toml` outside a test fixture).
- A CI job greps the diff for long verbatim runs against a hash index of the
 studied trees, and fails on a match outside `docs/research/`.
- Release tarballs and container images are built from a clean checkout; there is
 no build step that reads `.research/`.

Vendoring an AGPL tree into an Apache-2.0 repository would be the single fastest
way to make omt unshippable. The directory is a scratch space for a human reading
code, and nothing downstream may depend on it.

---

## 6. Third-party dependency policy

Applies to the Cargo workspace and to `web/`'s npm tree.

**Allowed** without review: `Apache-2.0`, `MIT`, `BSD-2-Clause`, `BSD-3-Clause`,
`ISC`, `Zlib`, `Unicode-3.0`, `CC0-1.0`, `MPL-2.0` (file-level copyleft; we do
not modify MPL files, and if we ever do, those files stay MPL).

**Forbidden**: `GPL-*`, `AGPL-*`, `LGPL-*` (static linking makes LGPL
impractical for a Rust binary), `SSPL`, `BUSL`, `Elastic-2.0`, anything
non-commercial, anything unlicensed or with an unresolvable expression.

**Case by case**, requiring an entry in `deny.toml` with a justification and a
reviewer: dual licences where one arm is forbidden, dependencies with a `NOTICE`
requiring propagation, and any crate whose licence field is `NONE`.

Enforcement:

```toml
# deny.toml (excerpt)
[licenses]
version = 2
allow = ["Apache-2.0", "MIT", "BSD-2-Clause", "BSD-3-Clause", "ISC",
 "Zlib", "Unicode-3.0", "CC0-1.0", "MPL-2.0"]
confidence-threshold = 0.93
exceptions = [] # every entry needs a comment and a reviewer

[bans]
multiple-versions = "warn"

[sources]
unknown-registry = "deny"
unknown-git = "deny"
allow-git = [] # git dependencies require an explicit entry
```

CI runs `cargo deny check licenses bans sources advisories` on every pull request
and on a nightly schedule (so a newly-published advisory fails the tree even
without a code change). The npm tree runs `license-checker-rspec` against the
same allow-list. A pull request that adds a dependency with a new licence fails
until `deny.toml` is updated in the same PR — which is the point: the licence
decision is reviewed alongside the dependency.

Runtime-only external programs (an agent CLI, `tailscale`, `ffmpeg`) are not
dependencies in this sense: omt shells out to what the user installed and links
nothing. Bundling any of them would change that analysis.

---

## 7. Attribution and NOTICE

- `NOTICE` at the repo root lists: omt's copyright, the crates whose licences
 require notice propagation, any third-party data corpus we redistribute (theme
 files, detection manifests), and an "interfaces studied" section naming other terminals,
 iTerm2 and other terminals **as interface references**.
- `NOTICE` is shipped in every release artifact and printed by
 `omt --licenses`, which also emits the full generated dependency licence
 report (`cargo about generate`, committed as `docs/reference/licenses.md` and
 diffed in CI like every other generated artifact).
- Naming other terminals or iTerm2 in `NOTICE` is a courtesy and a provenance record. It is
 **not** an admission of derivation, and the wording says so: *"omt implements
 file formats and escape-sequence protocols compatible with the following
 projects; no code from them is included."*
- Trademarks are not licensed: omt does not use the other terminals or iTerm2 names or
 marks in its branding, only in factual statements about compatibility.

---

## 8. Contributor rule

Every pull request that cites a studied repository must cite it **as an interface
reference, never as a source of code.**

Concretely, the PR template asks:

> - [ ] This change contains no code copied, adapted or translated from
> `other terminals/other terminals`, `gnachman/iTerm2`, or any GPL/AGPL source.
> - [ ] Any upstream behaviour it reproduces is documented in `docs/research/`
> and cited by section.
> - [ ] New dependencies are covered by the allow-list in `deny.toml`.

A PR description saying "ported from `app/src/terminal/foo.rs`" is rejected on
sight and the branch is not merged after rewording — the concern is the code, not
the sentence. A PR saying "implements the OSC 133 handling described in
[research/other terminals.md §2.4](../design/terminal-ux.md)" is exactly right.

Reviewers are expected to ask, for any suspiciously well-shaped constant table or
state machine, "where did this come from?" — and an answer of "I had the file
open" means the change is rewritten from the notes.

---

## 9. Open questions

- **OPEN QUESTION — bundling a community theme corpus.** `other terminals/themes`
 ships thousands of theme YAMLs under a permissive licence, but the individual
 themes have their own authors. Do we redistribute converted copies (with
 per-theme attribution in `NOTICE`), or ship only the importer and let users
 fetch? Leaning toward importer-only for v1.
- **OPEN QUESTION — plugin licence policy.** Plugins are separate programs
 communicating over a protocol, so a GPL plugin does not infect omt. Should the
 registry index nonetheless record and display a plugin's licence, and should
 `plugins.allow_unsigned = false` deployments be able to require an allow-list?
 Leaning yes to both.
- **OPEN QUESTION — the verbatim-run CI check.** Hashing the studied trees to
 detect copied runs requires keeping a hash index in the repo. Is an n-gram
 index of a GPL tree itself a derivative work? Probably not (it is not
 reconstructible), but it needs a second look before the check ships.
