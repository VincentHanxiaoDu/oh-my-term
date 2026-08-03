# Spike: can omt answer Claude Code's cards position-independently?

**Date:** 2026-08-03 · **Subject:** Claude Code v2.1.220
(`~/.local/share/claude/versions/2.1.220`, bundled JS executable, ~256 MB).

This spike settles the highest-priority open risk in the project. Per
[D3](../architecture/decisions.md#d3--synthetic-input-is-bounded-by-state-dependence-not-by-tool-danger),
omt may only synthesize input when the answer is **position-independent** — when
producing it does not require omt to have inferred where a highlight currently
sits. If Claude Code's cards were arrow-key-only,
[D9](../architecture/decisions.md#d9--positioning-what-omt-may-and-may-not-claim)'s
headline claim would be off by default in its headline case.

Evidence is marked **VERIFIED-STATIC** (read out of the shipped bundle),
**VERIFIED-LIVE** (observed in a running process) or **INFERRED**.

---

## Verdict

**Yes. Digit keys select directly, by index, without reference to the current
highlight.** Both select components in the bundle implement it, and the mapping
is from the *option array order* — which omt already receives verbatim in the
`PreToolUse` hook payload — not from any screen state omt would have to guess.

The flagship path is therefore **live and on by default**: a phone can answer the
agent's own card while the real TUI is on screen, within D3's bound. D9 needs no
amendment on this point, and the D5 build ordering stands.

Two guards exist and must be respected (§3). One version-fragility risk is real
and needs a detector (§6).

---

## 1. The mechanism — VERIFIED-STATIC

Two distinct select components, both handling a bare digit. The second is the
clearest:

```js
if (!u && /^[0-9]$/.test(x)) {
  D.preventDefault();
  let N = parseInt(x) - 1;
  if (N >= 0 && N < r.length) b(r[N].value);   // select + submit, by index
  return
}
if (D.key === "escape") { i(); D.stopImmediatePropagation() }
```

and the first, which is the richer variant used where options can be
free-text inputs:

```js
if (t !== "numeric" && /^[0-9]$/.test(T)) {
  y.preventDefault();
  let C = parseInt(T) - 1;
  if (C >= 0 && C < r.options.length) {
    let A = r.options[C];
    if (A.disabled === true) return;
    if (A.type === "input") {
      if ((l?.get(A.value) ?? "").trim()) { r.onChange?.(A.value); return }
      if (A.allowEmptySubmitToCancel) { … }
    }
    …
  }
}
```

**Why this satisfies D3.** `b(r[N].value)` resolves the choice out of the
options array by ordinal. It never reads `focusedValue`, never moves a cursor,
and is unaffected by where the highlight happens to be. omt knows the array —
the hook payload carries `input.questions[].options[]` verbatim, in order — so
the digit for a given option is computable from data omt already holds. There is
no inference step, which is precisely the property D3 requires and precisely
what arrow-key counting lacks.

`parseInt(x) - 1` also fixes the mapping: **1-based digit → 0-based index**.
Option 1 is `1`, and there is no option 0.

## 2. What the card advertises — VERIFIED-STATIC

The `AskUserQuestion` card's own hint region, read from the bundle's string
table in layout order:

```
enter · select · down · navigate · Tab/Arrow keys to navigate
ctrl+g · edit in $EDITOR · escape · cancel · Submit · Next
```

Note what is absent: **the card does not advertise digit selection.** The
capability is real but undocumented in the UI. That is fine for omt — omt is
not a human reading hints — but it is a signal about stability (§6).

`multiSelect` and `isMultiSelect` both appear adjacent to this region, so the
multi-select path exists in the same component; whether a digit *toggles* rather
than *submits* under `multiSelect: true` is **not established by this spike** and
is listed as remaining work.

## 3. The two guards omt must respect

Both digit handlers are gated:

| Guard | Meaning | Consequence for omt |
|---|---|---|
| `t !== "numeric"` | a numeric-input mode where digits are text, not selectors | omt must not send a digit when the focused option is a numeric input |
| `!u` | an unidentified suppression flag on the simpler component | unresolved; treat as a reason to verify before writing |

Combined with `A.type === "input"` handling (the free-text / "Other" path, which
has its own submit semantics), this means **omt cannot assume every option is
digit-selectable**. The safe rule: digit-select only plain choice options, and
fall back to reporting the card as *view-only, answer in the terminal* when the
target option is an input type. That is the honest degradation
[06 §4](../architecture/06-agent-layer.md) already specifies for cards omt
cannot safely answer.

## 4. Permission prompts — VERIFIED-STATIC, partially

Numbered options are confirmed present in the bundle (`3. No, keep prompting me`,
`3. No, keep everything`), consistent with another tool's manifest regexes
(`^\s*1\.\s*yes\b`) quoted in [another tool research §3.2](another tool.md). These prompts
render through the same select machinery, so the digit path applies.

**Not established:** whether the permission prompt uses the first or second
component, and therefore whether digit-select submits immediately or requires a
subsequent Enter. omt's gated transaction
([D13](../architecture/decisions.md#d13--synthetic-delivery-is-a-gated-transaction-never-a-bare-write))
must confirm by observation either way, so this does not block — but it changes
the exact byte sequence and must be settled before the responder is written.

## 5. Alternate screen — VERIFIED-STATIC, and it refines D14

Claude Code **does** contain alt-screen machinery — five `?1049h`/`?1049l`
occurrences, an `altScreenActive` flag, `altScreenMouseTracking`, and a
`resetFramesForAltScreen()` path:

```js
… + (this.altScreenActive ? "" : "\x1B[?1049h") + "\x1B[?1004l" + … + "\x1B[2J\x1B[H"
…this.options.stdout.write("\x1B[?1049l" + …)
```

So the claim "Claude Code never uses the alternate screen" is **too strong**. It
enters alt screen conditionally, for full-screen views.

**[D14](../architecture/decisions.md#d14--agent-sessions-get-a-transcript-surface-blocks-are-for-shell-work)
is unaffected, and mildly reinforced.** Its conclusion does not rest on
inline-vs-alt-screen; it rests on there being **no shell in the loop**, so no
OSC 133 ever arrives, so the heuristic segmenter's close conditions can never
fire. Alt-screen usage only adds intervals where segmentation is suspended
outright. Either way an agent session yields no usable blocks, and the
transcript surface is required.

D14's prose should be corrected on this detail: say *"no shell in the loop, so
no OSC 133"* as the reason, and drop the assertion that Claude Code is not an
alt-screen program.

## 6. Version fragility, and the detector omt needs

The digit path is **undocumented in the UI** (§2) and lives in a minified
bundle. It could change or be re-gated in any release without a changelog entry,
and D3's whole point is that a wrong answer is invisible to a remote user.

omt must therefore **never trust this blind**. Required:

1. **Confirm by observation, always** — already mandated by D13 and
   [D15](../architecture/decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism):
   the state goes `Submitted` on write and only becomes `Resolved` when omt sees
   the agent record *the answer omt sent*. A silently-changed keymap shows up as
   `Undelivered`, not as a wrong answer.
2. **Verify the resolved answer matches the intended one.** The `tool_result`
   text carries `"question"="label"` pairs
   ([agent-clis §1.3.1](agent-clis.md)); omt compares against what it sent. A
   mismatch is a loud, reportable event — it is the signal that the mechanism
   broke.
3. **A capability probe per agent version.** Record the version a responder was
   validated against; on an unrecognised version, degrade to view-only and say
   so, rather than guessing.

## 7. Consequences for the decision log

| Decision | Effect |
|---|---|
| **D3** | Satisfied. Claude Code choice cards are `StateDependence::Independent` for plain options; input-type options are excluded. |
| **D9** | Headline claim holds; no amendment. |
| **D13** | Unchanged and now load-bearing — it is what converts an undocumented keymap from a silent-wrong-answer risk into a visible failure. |
| **D14** | Correct conclusion, wrong stated reason. Amend to "no shell in the loop, therefore no OSC 133"; remove the not-an-alt-screen-program claim. |
| **D5** | Ordering stands. |

## 8. Remaining work

- Multi-select: does a digit toggle or submit under `multiSelect: true`?
- Which component backs the permission prompt, and whether Enter is needed after
  the digit.
- Identify the `!u` suppression flag.
- Confirm all of the above **live** in a PTY harness; this spike is
  static-analysis-only, and every finding above should be re-marked
  VERIFIED-LIVE before the responder ships.
