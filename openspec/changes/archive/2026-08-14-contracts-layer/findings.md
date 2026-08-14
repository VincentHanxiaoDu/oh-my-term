# What implementing this change found

Task 9.4: every correction the implementation forced, recorded here so the
architecture documents stay the source of truth rather than quietly diverging.

Nothing below is a change of design. Each is a place where the code and the
documents disagreed and the documents were right — which is the outcome this
record exists to make visible, because the opposite outcome is the one that
happens silently.

---

## 1. `backpressure` was in the wrong crate, not in the wrong place

[02 §L1](../../../docs/architecture/02-crate-map.md) already said `omt-events`
holds "`EventBus` … with per-subscriber backpressure policy and
resume-from-seq". The policy lived in `omt-server` at L5.

No crate boundary moved. The code was on the wrong side of one that was
already documented, which meant every crate that publishes events but is not
the server had no access to the policy that governs publishing.

**No document change.** The code moved to where 02 already put it.

---

## 2. Six schemas out of two hundred and eighty

[03 §5](../../../docs/architecture/03-capability-catalog.md) describes
`schemas/catalog.v1.json` as "every input/output schema, plus the declaration
metadata". `dump()` hand-listed six — the three proving capabilities' inputs
and outputs — and dropped the generator's `$defs` entirely, so even those six
were `$ref`s resolving to nothing.

A client generating types from the committed artifact got types for three
capabilities out of a hundred and forty, and no error to explain why.

The fix made schemas reachable through the erased handler so `dump()` walks
the registry. **No document change**: 03 §5 described the intent correctly and
the code did not implement it.

---

## 3. `PermissionMode` cannot be an enum

Task 2.3 lists it among the "domain enums", following
[02 §L0](../../../docs/architecture/02-crate-map.md), which names it beside
`Role` and `SessionMode`.

It cannot be one. [10 §8](../../../docs/architecture/10-configuration.md) says
the mode is *passed through* and that omt never overrides it silently, and
[06 §8](../../../docs/architecture/06-agent-layer.md) carries the agent's own
list verbatim — `acceptEdits`, `auto`, and whatever the next release adds. A
closed enum would refuse a mode the agent supports, which is the opposite of
passing it through.

Implemented as a newtype over `String`: a named type that cannot be confused
with the other strings on the same struct, without closing a vocabulary that
is not omt's to close.

**Suggested document change:** 02 §L0 lists `PermissionMode` among types whose
shape it does not state, so nothing there is wrong. Worth a word in the
glossary that it is deliberately open.

---

## 4. The parity allow-list file was not built, deliberately

Task 3.3 asks for "the allow-list file it is checked against, whose path and
format this task fixes".

Decided against. An exemption lives on the `Decl` beside the capability it
excuses, where it cannot drift from the declaration, and where the generated
reference can print it. A second file is a second place to forget.

What was missing was the visibility half: `Parity::Exempt`'s own doc comment
promised its reason is "printed in the generated reference, so it is argued in
public", and it was printed nowhere. `cargo xtask codegen` now emits every
exemption with its reason, and says "None" when there are none — a missing
section reads as "not checked", where "None" is the claim actually being made.

---

## 5. The recent-results cache did not exist at all

[03 §3.5](../../../docs/architecture/03-capability-catalog.md) gives a table of
what a repeated `RequestId` means per intent class, and puts the cache in
dispatch. There was no cache, and `Intent` decided nothing at dispatch time.

This is the one finding with safety weight.
[D15](../../../docs/architecture/decisions.md)'s rule — never retry an
injection, by any actor except a human who can see the screen — was written
down in the decisions document and enforced nowhere. A retry loop that means
well is exactly the actor that would send one.

`ConflictState` needed a new variant. `AlreadyResolved` says somebody *else*
got there first; nothing said the caller itself did, and a surface deciding
whether to offer a human a retry button has to tell those apart.

**Suggested document change:** 03 §3.5's table should name the conflict state
each refusal returns, so a client implementer does not have to infer it.

---

## 6. `capabilities-draft.md` outlived the generator

The file said of itself that it was "hand-written and temporary" and would be
replaced when the codegen landed. The codegen landed; the file stayed, and
four architecture documents still linked to it as the consolidated list.

Deleted, with the four links repointed at the generated file. Two lists of
capabilities is one list that is wrong, and the hand-written one is always
the wrong one.

---

## 7. P1's file-length rule had no check

[01 §P1](../../../docs/architecture/01-principles.md) names the enforcement:
"a CI check on file length (warn at 1200 lines, fail at 2000)". It did not
exist, and three files were over the failing threshold — one of them at 6454
lines, which P1 explicitly calls a design smell.

The rule was written down, every individual addition was small, and nothing
ever added them up.

The check exists now and the three files are split. **No document change**:
P1 was right and unenforced.
