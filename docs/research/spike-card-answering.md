# Spike: can omt answer Claude Code's cards position-independently?

**Date:** 2026-08-03 · **Subject:** Claude Code v2.1.220
(`~/.local/share/claude/versions/2.1.220`, arm64 Bun-compiled binary).

This spike settles the highest-priority open risk in the project. Per
[D3](../architecture/decisions.md#d3--synthetic-input-is-bounded-by-state-dependence-not-by-tool-danger),
omt may only synthesize input when the answer is **position-independent** — when
producing it does not require omt to have inferred where a highlight currently
sits. If Claude Code's cards were arrow-key-only, [D9](../architecture/decisions.md)'s
headline claim would be off by default in its headline case.

> **This document replaces an earlier static-only draft** of the same name. That
> draft's verdict was right, but three of its specific claims were wrong; they
> are corrected in §9 so nobody re-imports them.

Two methods were used, and every finding is tagged with the one that produced it:

- **VERIFIED-LIVE** — observed by driving the real binary under a Python `pty`
  harness in a scratch directory, capturing the raw byte stream, replaying it
  through a small ANSI screen model, and sending exact bytes. No
  `--dangerously-skip-permissions`; all work confined to a scratch dir.
- **VERIFIED-STATIC** — read out of the shipped JS. The Bun module graph sits
  between the runtime and the `\n---- Bun! ----\n` trailer near EOF and is
  greppable in place; it is React-Compiler output, so component and handler
  bodies are recoverable verbatim. Quotes below are literal.
- **INFERRED** — reasoned from the above, not directly observed.

---

## Verdict

**Yes — for the two card types that matter, and by a stronger mechanism than
hoped.** Claude Code's selection widget has a hard-coded, non-rebindable numeric
accelerator: an ASCII digit `1`–`9` resolves the option at that *absolute* index
in the full option list and **submits in the same keystroke** — no Enter, and
completely independent of where the highlight sits. Proven live by pressing
`↓ ↓ 1` on a three-option permission card and getting option 1, not option 3.

- **`AskUserQuestion`, single-select: YES, fully.** The card's option list is the
  `options` array from the tool input, in order, so omt computes the digit purely
  from the `PreToolUse` payload it already holds. One byte resolves the whole
  tool call. D9's headline case is **on by default**.
- **Tool-permission prompt (Bash, Write, Edit, MCP): YES for "allow", NO for a
  specific "deny".** Option 1 is always `Yes`. But the list is 2–4 options
  depending on local state invisible to the hook payload, so the index of `No`
  is not derivable. `Esc` is a position-independent abort and is the only safe
  negative.
- **`AskUserQuestion`, multiSelect: PARTIALLY.** Digits toggle checkboxes
  position-independently, but *submitting* the set requires navigating to a
  Submit row. Not position-independent; do not offer it remotely.
- **Plan review (`ExitPlanMode`): NO.** 2–5 options, every one conditional, and
  the final "No, keep planning" is a text-input option.
- **`y`/`n` do nothing** on any of these cards — VERIFIED-LIVE. another tool's
  `^\s*❯?\s*yes\b` heuristic is wrong for Claude Code; its `^\s*1\.\s*yes\b`
  form is right, and the digit really does select.

D9's headline claim survives. D3 needs one clarification, D13 needs four extra
preconditions, D14 needs an amendment, D15 needs a third resolver.

---

## 1. The mechanism — VERIFIED-STATIC

All of Claude Code's option cards render through one component (`jr` → `ZXs`),
whose keyboard handler is `HXs`. The relevant branch, verbatim:

```js
if(t!==!0){
  if(o&&Hue(y.key)===" "&&r.focusedValue!==void 0){ /* multi-select space toggle */ }
  if(t!=="numeric"&&/^[0-9]$/.test(T)){
    y.preventDefault();
    let C=parseInt(T)-1;
    if(C>=0&&C<r.options.length){
      let A=r.options[C];
      if(A.disabled===!0)return;
      if(A.type==="input"){
        if((l?.get(A.value)??"").trim()){r.onChange?.(A.value);return}
        if(A.allowEmptySubmitToCancel){r.onChange?.(A.value);return}
        r.focusOption(A.value);return
      }
      r.onChange?.(A.value);return
    }
  }
}
```

Properties that matter to omt:

- `r.options` is the **full** list, not the visible window. The index is
  absolute; scroll position is irrelevant.
- `r.onChange?.(A.value)` is the *resolution* callback — the same one Enter
  eventually calls. Digit = select **and** submit, one keystroke. It never reads
  `focusedValue`. This is exactly the property D3 requires and exactly what
  arrow-key counting lacks.
- `parseInt("0")-1 === -1` fails `C>=0`, so **`0` is a no-op**. Usable range is
  `1`–`9`; options past the ninth have no accelerator.
- `T = yW(y.key)`, and `yW` only folds full-width digits `U+FF10–U+FF19` to
  ASCII. Nothing else is normalised.
- The gate `t` is `disableSelection`, computed in `ZXs` as
  `NXs||(rz?"numeric":!1)` — numeric selection is switched off exactly when the
  caller passes `hideIndexes`, which is *also* what suppresses the printed
  `1.` `2.` `3.` prefixes. **If omt can see the number on the row, the digit
  works; if it cannot, it does not.** That equivalence is the single most useful
  runtime check available (§4).
- Digits are **not** in the keybinding registry. The `Select` context binds only
  `up/down/j/k/ctrl+n/ctrl+p/pageup/pagedown/home/end/enter/escape`, so a user's
  `~/.claude/keybindings.json` cannot rebind digits away. (`home`/`end` are
  inert: `select:first`/`select:last` are declared action names for which `HXs`
  never registers handlers.)

### 1.1 Proof that it is position-independent — VERIFIED-LIVE

The card, rendered from the captured byte stream:

```
 Do you want to create probe_downdown1.txt?
 ❯ 1. Yes
   2. Yes, allow all edits during this session (shift+tab)
   3. No

 Esc to cancel · Tab to amend
```

| bytes sent | result |
|---|---|
| `3` | `⎿  User rejected write to spike_probe2.txt` — immediate, no Enter |
| `1` | `⎿  Wrote 1 line to probe_one.txt` |
| `\x1b[B` `\x1b[B` `1` | **`Wrote 1 line`** — chose *Yes* with the highlight on *No* |
| `y` | nothing; card unchanged, highlight still on 1 |
| `n` | nothing; card unchanged, highlight still on 1 |
| `\x1b` | `User rejected write to probe_esc.txt` |

The third row is the decisive one. The same was reconfirmed on the
`AskUserQuestion` card: `↓ ↓ 2` returned `Which colour? → Green`.

### 1.2 Two transport rules the harness exposed — VERIFIED-LIVE

**Bracketed paste destroys it.** Claude Code enables `ESC[?2004h` at startup, in
every capture. Sending `\x1b[200~2\x1b[201~` to the `AskUserQuestion` card **did
nothing at all** — the card stayed up, unchanged, while a bare `2` resolved it.
omt must write the digit as a raw byte outside any paste bracketing. If omt's
remote-input path wraps client text in `ESC[200~ … ESC[201~` (correct for pasted
prose), synthetic answers must bypass that path entirely.

**One key per write.** Sending `b"13"` as a single `write(2)` to the multi-select
card toggled **nothing**; sending `b"1"`, then `b"3"` a second later toggled both
Red and Blue. Coalesced bytes arriving in one read are not decoded as two key
events. Any multi-byte answer (digit+Enter, free text) must be written as
separate, ordered writes — while still satisfying D13's "a partial write is never
permitted", i.e. the sequence is one transaction holding the writer token
throughout.

---

## 2. Per-card findings

### 2.1 `AskUserQuestion`, single-select — VERIFIED-LIVE

Prompted with three options `Red, Green, Blue`:

```
 ☐ Colour
Which colour?
❯ 1. Red
     Red
  2. Green
     Green
  3. Blue
     Blue
  4. Type something.
────────────────────────────────────────────────────────────────
  5. Chat about this
Enter to select · ↑/↓ to navigate · Esc to cancel
```

Hint line, verbatim: `Enter to select · ↑/↓ to navigate · Esc to cancel`. It
gains `· ctrl+g to edit in VS Code` once the free-text row is focused, and
`Tab/Arrow keys to navigate` replaces `↑/↓ to navigate` when there is more than
one question. **The card does not advertise digit selection** — the capability is
real but undocumented in the UI (a stability signal; see §5).

Option list construction (VERIFIED-STATIC, `Pdi`):

```js
let S4S=[...fLI, gLI, ..._LI];
// fLI = KP.options.map(o => ({type:"text", value:o.label, label:o.label, description:o.description}))
// gLI = {type:"input", value:"__other__", label:"Other", placeholder:"Type something.", …}
// _LI = screen-reader mode only
```

For a question with `k` options:

| digit | meaning |
|---|---|
| `1`…`k` | `questions[q].options[i-1]`, resolved **and submitted** |
| `k+1` | the free-text "Other" row — **focuses it, does not submit** |
| `k+2` | "Chat about this" — outer handler, `yW(key)===String(k+2)` |

- **Initial highlight is deterministic**: index 0, unless a `defaultValue` was
  restored from a previously-visited question tab (VERIFIED-STATIC, `rbp`:
  `a = s ? r : i.first?.value`). omt does not need this, but it makes the
  screen check in §4 cheap.
- **Auto-submit**: `hideSubmitTab = questions.length===1 && !multiSelect`. For a
  single non-multi question, answering resolves the entire tool call at once —
  `⏺ User answered Claude's questions: · Which colour? → Green` (VERIFIED-LIVE).
  With several questions, answering advances to the next tab and the final
  `Submit` tab is a two-option confirm (`ya` → `jr`), so `1` = *Submit answers*,
  `2` = *Cancel* (VERIFIED-STATIC).
- **Free text**: digit `k+1` focuses the input (VERIFIED-LIVE — the row gained
  `❯` and the hint gained `ctrl+g to edit in VS Code`); then the text, then `\r`,
  submitting with value `__other__`. Position-independent as a sequence, **but
  see §3.1**.
- **Escape** → `answer({behavior:"deny"})`, logged
  `tengu_ask_user_question_rejected` (VERIFIED-STATIC).
- **AFK timeout**: if `askUserQuestionTimeout` is set (or `CLAUDE_AFK_TIMEOUT_MS`),
  the card **auto-advances itself** with whatever answers exist, emitting
  `tengu_ask_user_question_afk_auto_advance` (VERIFIED-STATIC). A resolver omt
  does not control — see D15 in §6.

### 2.2 `AskUserQuestion`, multiSelect — VERIFIED-LIVE

After sending `1`, then `3`:

```
←  ☒ Colours  ✔ Submit  →
Which colours?
❯ 1. [✔] Red
  2. [ ] Green
  3. [✔] Blue
  4. [ ] Type something
     Submit
Enter to select · ↑/↓ to navigate · Esc to cancel
```

Toggling is absolute and does not move focus (it stayed on row 1). Handler
(`tQs`): `if(!u&&/^[0-9]$/.test(x)){…b(r[N].value)}`. But submission is not:

```js
if(D.key==="return"||Hue(D.key)===" "){
  if(f&&l){l(d);return}                          // submit only if Submit row focused
  if(D.key==="return"&&!a&&l){l(d);return}       // …or if there is no Submit button
  if(C.focusedValue!==void 0) b(C.focusedValue);  // otherwise Enter just toggles
}
```

`AskUserQuestion` always passes a `submitButtonText`, so **Enter toggles rather
than submits**, and reaching the Submit row needs `Tab`/`↓` *from the last
option* — a navigation count. **Not position-independent; omt must not offer it.**
(Screen-reader mode fixes this — §2.6.)

### 2.3 Tool-permission prompt — VERIFIED-LIVE + VERIFIED-STATIC

Both builders feed `zCe` → `jr`, so the digit path applies to both:

- **File tools** (`tal`) — always exactly three: `Yes` / `Yes, allow all edits …`
  / `No`. `1`=allow, `3`=deny, stable.
- **Everything else, incl. Bash and MCP** (`qal`) —
  `[Yes, (Yes-and-don't-ask-again)?, (Yes-and-use-auto-mode)?, No]`. The middle
  entries are conditional on `showAlwaysAllow`, on the classifier decision
  reason, on org caps, and on auto-mode availability: **2 to 4 options.**

So `1` = *Yes* is invariant; *No* is always last but at an index omt cannot
derive from the hook payload.

Hint line, verbatim: `Esc to cancel · Tab to amend`.

**Escape semantics are builder-dependent.** Statically, `qal` wires
`onCancel → answer({behavior:"cancelled"})`; live, Esc on the *file-write* card
produced `User rejected write to probe_esc.txt`. omt should therefore present Esc
as "abort this request" rather than promising a specific `deny`, and let the
observed resolution be the record of what happened.

`y` and `n` are **inert**. The `Confirmation` keymap does define
`y:"confirm:yes", n:"confirm:no"`, but `zCe` registers handlers only for options
carrying a `keybinding` field, and neither permission builder sets one. The
actions have no handler and the keys fall through to the Select's DOM handler,
which ignores them (VERIFIED-STATIC, and VERIFIED-LIVE — both left the card
untouched).

### 2.4 Plan review (`ExitPlanMode`) — VERIFIED-STATIC

Options come from `iYf({showClearContext, showUltraplan, usedPercent,
isAutoModeAvailable, isBypassPermissionsModeAvailable})`: two to five entries,
every one conditional, and the last is
`{type:"input", label:"No, keep planning", placeholder:"Tell Claude what to change"}`
— a text-input option, so its digit focuses rather than submits.

Not reachable safely. Mirror read-only; require local resolution (or a remotely
typed free-text message).

*Not reproduced live* — I could not reliably drive the binary into plan mode
inside the harness. Labelled STATIC accordingly.

### 2.5 The documented `n`-to-comment key — VERIFIED-STATIC, qualified

It exists, but only in a **second, different** `AskUserQuestion` renderer
(`Iil`), used when `!multiSelect && options.some(o => o.preview) && !screenReader`
— i.e. only when the model attached `preview` text to its options. That renderer
draws `press n to add notes`, hints
`enter select · ↑/↓ navigate · n add notes · [tab switch questions] · esc cancel`,
and its digits behave **differently**:

```js
else if(ee.key.length===1&&ee.key>="1"&&ee.key<="9"){
  ee.preventDefault(); let te=parseInt(ee.key,10)-1;
  if(te<D.length) W(te)          // moves the highlight only
}
```

A digit **moves the highlight and does not submit**; `Enter` then selects the
highlighted index. Still position-independent *as the pair* `digit` + `Enter`,
because the digit sets an absolute index — but the single-byte resolution of §1
does not hold, and omt must send two writes.

**This is the sharpest fragility in the spike**: two renderers for the same tool,
selected by whether the model happened to emit `preview` fields, with different
submit semantics for the same key, *in the same version*.

### 2.6 Screen-reader mode replaces everything — VERIFIED-STATIC

When `--ax-screen-reader`, `CLAUDE_AX_SCREEN_READER`, or
`settings.axScreenReader: true` is active, `Ta()` (an `InternalAccessibilityContext`
fed from Ink's `isScreenReaderEnabled`) is true, and both `jr` and `V3` swap to
`I4o` / `D4o`: a **line-oriented numbered prompt with no highlight at all**.

```
Enter selection [1-5], or Escape to cancel: ▏
Invalid selection "x". Enter a number between 1 and 5.
```

and for multi-select: `Enter numbers between 1 and N, comma- or space-separated.`

Digits accumulate into a buffer and **Enter** submits, so the sequence is `3`,`\r`
— and multi-select becomes fully position-independent too (`1 3` then `\r`).
Every card in the app becomes reachable, and the §2.5 renderer fork disappears.

The mode is inherited by child processes (`wst()` returns
`{CLAUDE_AX_SCREEN_READER:"1"}`) and is gated by `tengu_ax_screen_reader`, which
defaults to enabled. **omt should offer this as the recommended configuration for
sessions the user intends to drive remotely.** It changes the local user's visual
experience, so it must be opt-in and clearly labelled — never set silently, which
would violate [P4](../architecture/01-principles.md).

---

## 3. What omt must NOT do

### 3.1 The free-text focus hazard — VERIFIED-LIVE

`HXs` returns early for every key when the **focused** option is `type:"input"`:

```js
let v=n.find(C=>C.value===r.focusedValue), b=v?.type==="input";
…
if(b){ /* only up/down/image handling */ return }   // digits never reach the numeric branch
```

Live confirmation: sending `4` (focus "Other"), then `2`, produced

```
❯ 4. 2
```

The digit was typed as a literal character into the text box. So if the local
user has arrowed onto the "Other" row — or pressed `Tab` on a permission card,
which flips `Yes`/`No` into input options (the `Tab to amend` hint) — a digit
written by omt becomes text. The remote user sees no error; the local user sees a
stray digit in a text field.

This is precisely the failure mode D3 exists to prevent. **Position-independence
is conditional on the card not being in text-entry mode**, and omt cannot learn
that from the hook payload. It must be a screen-derived precondition (§4).

### 3.2 Never do these

- Never send arrow keys followed by Enter to pick a named option.
- Never resolve a **multiSelect** `AskUserQuestion` remotely (unless screen-reader
  mode is active, §2.6).
- Never resolve a **plan review** remotely.
- Never send a digit for "No"/deny on a permission prompt — the index varies.
  Offer `Esc` and describe it as an abort.
- Never wrap the answer in bracketed paste, and never coalesce two keys into one
  write (§1.2).
- Never send `y`/`n` and assume anything happened.
- Never send `0`; never send a digit `>9`; refuse remote resolution outright when
  the option list exceeds nine entries.
- Never retry a synthetic write (§6, D15). After a successful digit the card is
  gone, so a replayed `2` lands in the chat prompt box as literal text.

---

## 4. The exact writes, and the preconditions on them

### Byte sequences

| Card | Intent | Bytes | Notes |
|---|---|---|---|
| `AskUserQuestion` single-select | choose `options[i]` | `0x31+i` (one ASCII digit, `i` 0-based, `i<9`) | resolves and submits |
| …with `preview` on any option (§2.5) | choose `options[i]` | `0x31+i`, then `\r` | separate writes |
| …free text | answer "Other" | digit `k+1`, then UTF-8 text, then `\r` | **only** if not already in input mode |
| …multi-question, Submit tab | submit | `1` | `2` = cancel |
| Tool permission | allow | `1` | invariant across all builders |
| Tool permission | abort | `\x1b` | resolution is builder-dependent (§2.3) |
| Any card, screen-reader mode | choose `options[i]` | `"{i+1}"`, then `\r` | multi-select: `"1 3"`, then `\r` |
| Any card | abandon | `\x1b` | |

### Preconditions omt must check before writing (D13 step 3)

D13 already requires a writer token, input quiescence, and re-verification.
This spike adds four checks, all against the **freshest rendered screen**, not
the hook payload. All must pass or the resolve fails `precondition_failed`,
visibly, on the surface that attempted it.

1. **The card is present and is the expected one** — match the question text from
   the hook payload against the live screen.
2. **The row omt intends to select is printed with its number**, at the index omt
   computed: literally `\s*(❯\s*)?N\.\s*<label>`. This is the highest-value check
   available. The printed number and the working accelerator are driven by the
   *same* flag (`hideIndexes` disables both), so a visible `N.` is direct evidence
   the digit will work, and a matching label is direct evidence the index is
   right. No number → refuse.
3. **The card is not in text-entry mode** — refuse if the hint line contains
   `ctrl+g to edit in`, if a cursor is parked in an input row, or if the focused
   row is the free-text option (§3.1).
4. **The pane is not on the alternate screen** (§6, D14), the option count is
   ≤ 9, and — if an AFK timeout is configured — the card is not within a few
   seconds of it, since the auto-advance would race the write.

### Confirming delivery

Do **not** confirm by re-reading the screen. Confirm from the agent event stream:
the `PostToolUse` / transcript entry for the same `tool_use_id`. For
`AskUserQuestion` the resolution appears as
`User answered Claude's questions: · <question> → <answer>` and as a `tool_result`
carrying `question → label` pairs ([agent-clis §1.3.1](agent-clis.md)); for a
permission prompt as the tool proceeding, or `User rejected …`. omt compares the
recorded answer against what it sent — **a mismatch is the signal that the
mechanism broke**, and is a loud, reportable event. If no event arrives within the
timeout, mark the intent `Undelivered` and surface it; never retry.

---

## 5. Version fragility, and how to detect breakage

The mechanism is stable in character — a numeric accelerator that is not
user-rebindable — but it is **undocumented in the UI** (§2.1) and lives in a
minified bundle, so it can change without a changelog entry. Ranked by risk:

| Risk | What would break | How omt notices |
|---|---|---|
| **High** — option lists gain/lose conditional entries (`qal` already has three) | wrong option selected | precondition 2: label at index N no longer matches |
| **High** — the §2.5 preview renderer's submit semantics already differ *today*, in this version | digit highlights instead of submitting; card sits there | delivery confirmation never arrives → `Undelivered` |
| Medium — `hideIndexes` turned on for a card | digits silently inert | precondition 2: no printed number |
| Medium — `tui: "fullscreen"` (§6) | omt's screen model is wrong | detect `ESC[?1049h` |
| Low — digits removed entirely | fails closed | `Undelivered` |

The load-bearing design choice: **omt's precondition is the rendered number and
label, not a version check.** Every high-risk change above is caught by requiring
that the screen literally shows `N. <label>` for the intended answer before
writing. omt should still record the observed `claude --version` per binding and,
on a version change, degrade to *mirror-only, resolve locally* until a stored
probe re-passes — but the per-write screen check is what prevents a silent
mis-answer, which is the thing D3 actually cares about.

A cheap self-test to ship: at binding time and on version change, run
`claude --ax-screen-reader` headlessly and confirm the numbered-prompt shape, and
assert the live card's hint line still matches one of the known strings. Any
mismatch → capability degraded, visibly, on every surface.

---

## 6. Consequences for the decision log

### D3 — clarification, not reversal

D3's allowed list already names "typing `1`/`2`/`3`", and that is exactly what
Claude Code supports. Two things should be written into it:

- **Position-independence is a property of the *(card, state)* pair, not of the
  card type.** The same card is position-*dependent* the moment focus sits on a
  text-input row (§3.1). omt must verify the state, not just the card kind.
- **Index derivability is a second, separate bound.** A keystroke can be
  position-independent and still be *wrong* if omt guessed which digit. D3
  currently conflates the two. Add: omt may synthesize a selection only when the
  intended option's index is derivable from the agent's own payload **and**
  confirmed against the rendered row. That is what rules out plan review and the
  `No` branch of permission prompts — neither of which arrow-key counting would
  have made worse.

### D9 — headline claim holds

"Answer the agent's own card from your phone while the real TUI is on screen"
works, by default, with no configuration, for `AskUserQuestion` single-select and
for permission *approval*. Qualify the claim: **deny** on a permission prompt is
available only as an abort, and multi-select and plan review are mirror-only.
A footnote, not a retraction.

### D11 — unaffected

The agent draws its own card; omt writes one byte into the PTY and the card
resolves natively. As close to "observe, never re-implement" as synthetic input
gets.

### D13 — add the preconditions, and a transport rule

Spell out step 3 as the four checks in §4, naming the printed-number check
explicitly since it is what converts a version-fragile mechanism into a
fail-closed one. Add the §1.2 transport rules: raw bytes outside bracketed paste,
one key per write, with the whole sequence held under a single writer token so
"a partial write is never permitted" still holds for the multi-byte paths.

### D14 — needs amending

D14 asserts "Claude Code is not an alt-screen program." **Confirmed as the
default** — VERIFIED-LIVE: across ten captures, including several with a card on
screen, `ESC[?1049h` appears **zero** times. What Claude Code emits at startup is
`ESC[?2004h` (bracketed paste), `ESC[?1004h` (focus events), `ESC[?2031h`, and a
kitty-keyboard push `ESC[>1u` / pop `ESC[<u`.

But it is not unconditional. v2.1.220 ships a second renderer:

```js
tui: E.enum(["default","fullscreen"]).optional().describe(
  'Terminal UI renderer. "fullscreen" uses the flicker-free alt-screen renderer
   with virtualized scrollback (equivalent to CLAUDE_CODE_NO_FLICKER=1).
   "default" uses the classic main-screen renderer.')
```

reachable via `/tui fullscreen`, `CLAUDE_CODE_NO_FLICKER=1`, or
`viewMode: "focus"` — and there is an active in-product **upsell** pushing users
toward it (`fullscreen-upsell`, `fullscreenUpsellSeenCount`), with a downsell
survey for switching back. The inline-Ink assumption is a *default*, not a
guarantee, and the alt-screen population is being deliberately grown.

Amend D14 to: the transcript surface is chosen by session kind, and omt must
**detect** the renderer at runtime (`ESC[?1049h`) rather than assume it. When a
Claude Code session enters the alt screen, block segmentation is already
suspended by [04 §6.3](../architecture/04-terminal-core.md), the transcript
surface still works (it is fed by the event stream, not the grid), and **remote
card answering must be disabled** — the screen-derived preconditions of §4 cannot
be trusted against a virtualised scrollback omt does not model.

### D15 — unaffected, and confirmed; one addition

An injected answer is squarely D15's *externally-confirmed intent* class:
at-most-once, confirmed by observation, `Undelivered` on timeout, never retried.
This spike gives that class a concrete confirmation source (the `PostToolUse` /
transcript event for the same `tool_use_id`) and a concrete reason retries are
unsafe (§3.2).

**Addition:** the AFK auto-advance (§2.1) means a card can resolve with *no actor
at all*. D15's ledger needs a third resolver alongside local and remote —
`timeout` — or omt will report a phantom conflict.

---

## 7. Reproduction

```
scratchpad/spike/
  drive.py    # pty fork, exact-byte send, raw capture
  screen.py   # minimal ANSI screen model (CUP/CUU/CUD/CUF/CUB/CHA/EL/ED/IL/DL)
  t*.py       # one probe each; t*.txt are the raw streams
```

Key symbols in 2.1.220: `HXs` (select keydown), `ZXs` (select), `tQs`
(multi-select state), `I4o`/`D4o` (screen-reader selects), `Pdi`/`Iil`
(AskUserQuestion renderers), `zCe` (permission confirm), `tal`/`qal` (permission
option builders), `iYf` (plan-review options), `Ta` (screen-reader context).

---

## 8. Remaining work

- Reproduce the plan-review card live (§2.4) and confirm its option ordering
  against a real session's local state.
- Reproduce the §2.5 preview renderer live; it is the one path where the
  single-byte assumption is already false today.
- Confirm behaviour under `tui: "fullscreen"` — specifically whether the digit
  accelerator survives (it should; the renderer changes, not the widget) and what
  omt can still observe.

## 9. Corrections to the superseded draft

The earlier static-only version of this document should not be re-imported. Three
of its claims were wrong:

1. **"Two distinct select components"** — the two handlers it quoted are
   single-select (`HXs`) and multi-select (`tQs`) of the *same* widget family, not
   alternatives for the same card. The genuinely distinct implementations are the
   screen-reader selects (§2.6) and the preview renderer (§2.5), which it missed.
2. **The gate flags were misread.** `t !== "numeric"` is not "a numeric-input
   mode where digits are text", and `!u` is not an "unidentified suppression
   flag": both are `hideIndexes`, which suppresses the printed numbers and the
   accelerator together (§1). That equivalence is the basis of omt's safety check,
   so misreading it discarded the most useful finding available.
3. **D14's reasoning.** The draft proposed replacing the alt-screen claim with
   "no shell in the loop, therefore no OSC 133". That is true but insufficient:
   the alt-screen occurrences it found are the `tui: "fullscreen"` renderer, a
   user-selectable whole-session mode being actively promoted — not incidental
   full-screen views. The consequence is a runtime **detection** requirement and a
   capability downgrade (§6), which the draft did not reach.

It also listed four open questions, all now closed: multi-select digits toggle
(§2.2); the permission prompt uses `jr` and needs no Enter (§2.3); `!u` is
`hideIndexes` (§1); and everything except the plan-review and preview paths is now
VERIFIED-LIVE.
