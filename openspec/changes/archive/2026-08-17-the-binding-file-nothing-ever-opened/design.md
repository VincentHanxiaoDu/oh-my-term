# Design

## Crates

- `omt-input` (L2) — the grammar, the context set, compilation and resolution.
  It already owns chords, the keymap and the reserved list, and it is below
  everything that will consume this.
- `omt-config` (L3) — where the file is, in the layer order it already
  implements for `config.toml`. It may depend on `omt-input`, which is L2.
- `omt-tui` (L5), `omt` (L6) — the leader and its actions come from the
  resolved map.

## The types, before the internals

```rust
// omt-input::context
pub struct ContextSet(u32);           // 16 §4.1's names, exactly
pub enum Predicate { Always, Name(Context), Not(..), And(..), Or(..) }
impl Predicate { pub fn parse(&str) -> Result<Self, PredicateError>; pub fn holds(&self, ContextSet) -> bool; pub fn terms(&self) -> u16; }

// omt-input::bindings
pub struct BindingFile { pub leader: String, pub keymap: String, pub bindings: Vec<Binding> }
pub struct Binding {
    pub trigger: String,              // as written, before <leader> expansion
    pub when: Option<String>,
    pub action: BindingAction,        // Capability { name, args } | Unbind
    pub force: bool,
    pub platform: Vec<String>,
    pub requires: Option<String>,
    pub modes: Vec<String>,
    pub repeatable: bool,
    pub source: Source,               // which layer and file it came from
}
pub struct Compiled { pub steps: Vec<Chord>, pub when: Predicate, pub action: BindingAction, pub specificity: u16, pub source: Source }
pub struct Resolver { /* compiled, ordered */ }
pub enum ChordResolution<'a> { Dispatch(&'a Compiled), Pending { .. }, Passthrough }
```

`BindingFile` keeps its name and its two top-level fields; `bindings` changes
from `Vec<(String, String)>` to `Vec<Binding>`. The pair form cannot carry
`when`, `args` or a gate, and every one of those is in the document the type
cites.

## Grammar

One parser, `bindings::parse(text, source) -> (BindingFile, Vec<BindingProblem>)`,
because a file with one bad line still has to install its other forty. Problems
carry the trigger, a code, a message and a fix.

- Top level: `leader`, `keymap`, `[[binding]]`, and every other key is a
  trigger whose value is a string or an inline table.
- A trigger is one or more space-separated steps. `<leader>` in any step
  expands to the `leader` chord. A single step may be a mouse trigger
  (`shift-mouse1`, `wheel-up`, `alt-mouse3-drag`).
- `"none"` is `BindingAction::Unbind`, which stops resolution and falls through
  to passthrough — distinct from having no binding, so a user can remove a
  default without knowing which layer put it there.

## Why `toml_edit` in `omt-input`

`omt-config` already depends on it, and the alternative — parsing in
`omt-config` and handing `omt-input` a JSON value — puts the file's grammar in
the crate that knows nothing about chords, which is where a grammar goes to
drift from the thing it describes. The licensing gate counts one more dependent
crate on a dependency already accounted for.

## Resolution, per 16 §2.3

Ordered once at compile time, first match wins: pending chord, then modal
context, then specificity (`Predicate::terms`), then layer, then passthrough.
Passthrough as the default rather than the fallback is the invariant the whole
crate is built on, and it is why an unmatched mouse event passes through exactly
as an unmatched key does.

## The leader, in the TUI

`omt-tui::app::leader_action` is a hard-coded `match` over characters and
`PREFIX_KEY` is a constant. Both become lookups into a table the TUI is handed:
the defaults produce exactly today's table, so a user with no binding file sees
no change — which is the property the screen tests assert.

## What is reported rather than dropped

- `OMT-C408` — a gate (`platform`, `requires`) is unmet, so the binding is not
  installed. A *user's* binding in that state is named; a builtin's is not,
  because the user did not ask for it.
- `OMT-C410` — a reserved chord bound globally without `force`.
- `OMT-C421` — `modes` on a binding under a non-modal keymap.
- An unknown `when` name is an error, not `false`. A predicate that silently
  never holds is a binding that silently never fires, which is the class of
  defect this whole change exists to end.
