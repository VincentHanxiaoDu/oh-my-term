# iTerm2 Architecture Research (reference for `oh-my-term`)

Source tree studied: `/Users/hanxiao.du/Desktop/vincent/projects/oh-my-term/.research/iterm2`
(gnachman/iTerm2, Objective-C / Objective-C++ / Swift, macOS-only, GPLv2).
Commit at time of reading: `0da24d6` ("Fully linkify long URLs…").

All paths below are relative to that research checkout. Where I could not verify a
claim from source I say so explicitly ("not verified"). Nothing here is inferred
from memory of iTerm2 docs — everything cited has a file path.

---

## 0. Orientation: how the code is laid out

`sources/` has ~136 top-level subdirectories, grouped by feature rather than layer.
The ones that matter for a terminal implementer:

| Directory | What lives there |
|---|---|
| `sources/VT100/` | Byte stream → token parsers, `VT100Terminal` (mode/state machine), token execution/scheduling |
| `sources/VT100Screen/` | Screen model: grid + scrollback + marks + resizing |
| `sources/LineBuffer/` | Scrollback storage (`LineBuffer`, `LineBlock`), search, DWC caches |
| `sources/ScreenChar/` | The `screen_char_t` cell struct, string→cells conversion, unicode normalization |
| `sources/Marks/` | Semantic marks (prompt, command, folds, blocks, buttons) |
| `sources/InlineImages/` | OSC 1337 inline images + Kitty graphics protocol |
| `sources/ShellIntegration/` | Command/directory/host history persistence (Core Data) |
| `sources/tmux/` | tmux `-CC` control-mode gateway, layout parsing, window opener |
| `sources/API/` | Python scripting API: HTTP→WebSocket server, protobuf dispatch |
| `sources/MetalRenderer/` | GPU renderer: driver + ~25 per-feature renderers + shaders |
| `sources/Triggers/` | Regex triggers (~30 kinds) |
| `sources/SemanticHistory/` | Cmd-click / path resolution / URL actions |
| `sources/SearchingFiltering/`, `sources/GlobalSearch/` | Find-on-page, filter mode, cross-session search |
| `sources/SSH/` | The "conductor" — iTerm2's own SSH integration channel |
| `Resources/shell_integration/` | The shipped shell scripts (bash/zsh/fish/tcsh/xonsh) |
| `OtherResources/Utilities/` | `imgcat`, `it2copy`, `it2ul`, `it2dl`, `it2attention`, … |
| `proto/api.proto` | 1686-line protobuf schema for the scripting API |

---

## 1. Terminal emulation core

### 1.1 Pipeline shape

```
PTY bytes
  → VT100Parser            (sources/VT100/VT100Parser.m, 718 lines)
      → VT100ControlParser (dispatch: DCS / CSI / OSC(xterm) / ANSI / other)
      → VT100Token stream  (sources/VT100/VT100Token.h, 503 lines — ~1 token type per sequence)
  → TokenArray / TwoTierTokenQueue / TokenExecutor  (Swift, mutation queue)
  → VT100Terminal          (sources/VT100/VT100Terminal.m, 6076 lines — modes + semantics)
      → VT100ScreenDelegate calls
  → VT100ScreenMutableState (sources/VT100Screen/VT100ScreenMutableState.m, 8061 lines)
      → VT100Grid (viewport)  +  LineBuffer (scrollback)  +  IntervalTree (marks)
  → main-thread immutable snapshot (VT100Screen) → PTYTextView / Metal driver
```

The key architectural decision: **parsing and screen mutation happen off the main
thread**, and the main thread reads an immutable snapshot. `VT100ScreenMutableState`
vs `VT100ScreenState` / `VT100ScreenStateSanitizingAdapter` implement that split, and
`iTermGCD.assertMainQueueSafe` / `assertMutationQueueSafe` assertions are sprinkled
throughout (e.g. `VT100ScreenMutableState+Resizing.m:1299`).

### 1.2 Parser structure

`VT100Parser` owns a `VT100ByteStream` and dispatches per-chunk:

```objc
// sources/VT100/VT100Parser.m ~line 125
if (isMixedAsciiString(firstChar, secondChar) && !_dcsHooked) {
    ParseString(&consumer, token, encoding);          // fast ASCII path
} else if (iscontrol(firstChar) || _dcsHooked ||
           (support8BitControlCharacters && isc1(firstChar))) {
    [_controlParser parseControlWithConsumer:... incidentals:vector ...];
}
```

Notable design points worth copying:

- **Fast ASCII path.** `VT100_ASCIISTRING` / `VT100_MIXED_ASCII_CR_LF` token types exist
  so runs of plain text (and text+CRLF) bypass the full state machine.
- **Partial-parse resumption.** `_savedStateForPartialParse` (an `NSMutableDictionary`)
  lets OSC/DCS parsing suspend mid-sequence when the read buffer ends, and resume on
  the next chunk. Every sub-parser returns `VT100_WAIT` + writes saved state
  (see `VT100XtermParser.m` `kXtermParserOutOfDataState`). *This is the single most
  important structural requirement for a streaming Rust parser.*
- **"Incidentals".** A parse call can emit *extra* tokens into a `CVector` alongside the
  primary token. Used for OSC 1337 `File=` so a multi-megabyte base64 image doesn't
  have to be buffered as one token: the parser emits
  `XTERMCC_MULTITOKEN_HEADER_SET_KVP`, then N × `XTERMCC_MULTITOKEN_BODY`, then
  `XTERMCC_MULTITOKEN_END` (`VT100XtermParser.m:320-360, 445-470`).
- **Shadow SGR state on the parser thread.** `VT100Parser` keeps `_shadowRendition`,
  `_savedShadowRendition`, and a 10-entry `_shadowSGRStack` so it can pre-convert
  non-ASCII strings to styled cells *on the parser thread* (`VT100ParserMaxSGRStackEntries = 10`,
  `kMinPreconvertStringLength = 4`). It must therefore track DECSC/DECRC and
  XTPUSHSGR/XTPOPSGR itself, duplicating a bit of `VT100Terminal`.
- **Nested parsers.** `_sshParsers` is a `pid → VT100Parser` map with a `depth` counter.
  When iTerm2's SSH integration is active, remote output arrives wrapped in
  `SSH_OUTPUT` tokens and is *re-parsed* by a child parser. `SSH_TERMINATE` /
  `SSH_UNHOOK` tear those down. Also `SSH_RECOVERY_BOUNDARY` tokens for reconnect.

Sub-parsers:

| File | Handles |
|---|---|
| `VT100CSIParser.m` (878 ln) | `CSI` incl. private markers, intermediates, sub-parameters |
| `VT100XtermParser.m` (513 ln) | `OSC` and `APC` |
| `VT100DCSParser.m` (744 ln) | `DCS`, with a *hook* mechanism (below) |
| `VT100AnsiParser.m` | `ESC` + single final byte |
| `VT100OtherParser.m` | leftovers |
| `VT100StringParser.m` | text runs, encoding conversion |
| `VT100SixelParser.m` | sixel payload (hooked from DCS) |
| `VT100TmuxParser.m` | tmux control-mode payload (hooked from DCS) |
| `VT100ConductorParser.swift` | iTerm2 SSH conductor payload (hooked from DCS) |

**DCS hooks** are worth stealing. `VT100DCSParser` computes a "compact sequence" from
(private marker, intermediate, first passthrough byte) and installs a stateful hook
object that consumes all subsequent bytes until it says to unhook
(`VT100DCSParser.m:513-551`):

```objc
case MAKE_COMPACT_SEQUENCE(0, 0, 'p'):
    if ([[self parameters] isEqual:@[ @"1000" ]]) { token->type = DCS_TMUX_HOOK;
        _hook = [[VT100TmuxParser alloc] init]; }        // DCS 1000 p  → tmux -CC
    else if ([[self parameters] isEqual:@[ @"2000" ]]) { token->type = DCS_SSH_HOOK;
        _hook = [[VT100ConductorParser alloc] initWithUniqueID:_uniqueID]; }
    break;
case MAKE_COMPACT_SEQUENCE(0, 0, 'q'):
    _hook = [[VT100SixelParser alloc] initWithParameters:[self parameters]];   // DCS … q → sixel
    break;
```

Hooks carry a `_uniqueID` (UUID) so the main thread can request an unhook without
racing a session that has already re-hooked (`forceUnhookDCS:`). There's also a
`dataLooksLikeBinaryGarbage` check that turns runaway DCS passthrough into
`VT100_BINARY_GARBAGE`.

### 1.3 The cell: `screen_char_t`

`sources/ScreenChar/ScreenChar.h`. Bitfield struct (there is also a
`legacy_screen_char_t` kept for state-restoration compatibility — plan for a
versioned on-disk cell format from day one). Fields, in order:

- `code` (16-bit): either a UTF-16 code unit, or — when `complexChar` is set — an index
  into a global side table of multi-code-point strings (combining marks, emoji ZWJ
  sequences). In the `WIDTH+1` column of a line it instead holds the **continuation
  mark**: `EOL_HARD` (0), `EOL_SOFT` (1, wrapped long line), `EOL_DWC` (2, wrapped
  because of a double-width char) — `ScreenChar.h:99-101`.
- Private-use sentinel codes (`ScreenChar.h:62-77`):
  `DWC_SKIP` (padding cell at end of line before a DWC wraps), `TAB_FILLER`
  (cells between a tab stop's start and end), `DWC_RIGHT` (right half of a
  double-width char), `BOGUS_CHAR`.
- Colors: `foregroundColor/fgGreen/fgBlue` and `backgroundColor/bgGreen/bgBlue`,
  each 8 bits, plus 2-bit `foregroundColorMode` / `backgroundColorMode`. So one
  cell natively carries indexed **or** 24-bit truecolor.
- Attributes: `bold, faint, italic, blink, underline, strikethrough, invisible,
  inverse, image, guarded, virtualPlaceholder, complexChar`, plus a 3-bit
  underline style split across `underlineStyle0:2` + `underlineStyle1:1`
  (they ran out of contiguous bits — a lesson for layout).
- `unused : 11` bits remain.

`image` marks a cell that's part of an inline image; `virtualPlaceholder` is used for
Kitty-style unicode placeholders. `guarded` implements SPA/EPA protected areas.

**String → cells** is `StringToScreenChars()` (`ScreenChar.h:601`), whose signature is
the whole grapheme/width contract in one place:

```c
void StringToScreenChars(NSString *s, screen_char_t *buf,
                         screen_char_t fg, screen_char_t bg, int *len,
                         BOOL ambiguousIsDoubleWidth,
                         int *cursorIndex, BOOL *foundDwc,
                         iTermUnicodeNormalization normalization,
                         NSInteger unicodeVersion,
                         BOOL softAlternateScreenMode,
                         BOOL *rtlFound);
```

Takeaways for omt:
- Buffer must be **2× string length** worst case (every char double-width).
- `unicodeVersion` is a *runtime setting* (settable via OSC 1337 `UnicodeVersion=`,
  see §2), because width tables changed between Unicode versions and users need to
  match whatever the remote host thinks.
- `ambiguousIsDoubleWidth` is a per-profile toggle (CJK ambiguous width).
- Normalization modes: `None / NFC / NFD / HFSPlus`
  (`ScreenChar/iTermUnicodeNormalization.h`).
- `softAlternateScreenMode` changes conversion behavior (likely to avoid combining
  marks corrupting full-screen apps — *exact rationale not verified*).
- RTL is detected here (`rtlFound`) and there is a separate bidi pipeline
  (`iTermBidiDisplayInfo`, referenced from `LineBuffer.h`; `tools/bidi.py` generates tables).

Width/property tables are **generated** by scripts in `tools/`: `eastasian.py`,
`emoji.py`, `basechars.py`, `default_ignorable.py`, `ignorable.py`, `idn.py`,
`generate_nscharacterset.py` → `iTermCharacterSets.m.template`. omt should likewise
generate tables rather than hand-roll, and should pin a Unicode version.

### 1.4 Grid (viewport)

`sources/VT100/VT100Grid.m` (3306 lines) + `VT100LineInfo.m`.

- Fixed `size` (`VT100GridSize`), array of lines of `screen_char_t[width+1]`
  (the +1 is the continuation mark).
- Scroll regions (top/bottom, and left/right when DECSLRM/mode 69 is enabled).
- Per-line **damage tracking** in `VT100LineInfo`: `dirtyRange` (a
  `VT100GridRange`), `dirtyIndexes` (`NSIndexSet`), and a per-line **generation**
  counter — `"Equal generations imply equal content"` (`VT100LineInfo.h:30`). There's
  an O(1) "all dirty" representation via never-reused identities (`VT100LineInfo.h:17`).
  This is exactly the primitive a ratatui/xterm.js diff layer wants.
- `iTermTemporaryDoubleBufferedGridController` gives a second grid used to freeze
  output momentarily (used around resize and for the "blinking/animation" path).

### 1.5 Scrollback: `LineBuffer` / `LineBlock`

`sources/LineBuffer/` — this is the most transferable subsystem.

Model:

- **`LineBuffer`** owns an ordered `iTermLineBlockArray` of **`LineBlock`s**.
  Default block size is 8 KiB of `screen_char_t`:
  ```objc
  // LineBuffer.m:149
  const int BLOCK_SIZE = 1024 * 8;
  ```
- **`LineBlock`** stores *raw, unwrapped* lines contiguously in one buffer
  (`iTermCharacterBuffer`), plus a `LineBlockMetadataArray` of per-raw-line metadata
  (timestamps, continuation, external attributes like hyperlink IDs, bidi info).
  A raw line is a logical line; wrapping to a display width is computed on demand.
- Wrapping is queried, never stored:
  ```objc
  - (const screen_char_t *)getWrappedLineWithWrapWidth:(int)width
                                              lineNum:(int *)lineNum
                                           lineLength:(int *)lineLength
                                    includesEndOfLine:(int *)includesEndOfLine
                                              yOffset:(int *)yOffsetPtr
                                         continuation:(screen_char_t *)continuationPtr
                                 isStartOfWrappedLine:(BOOL *)isStartOfWrappedLine
                                             metadata:(out iTermImmutableMetadata *)metadataPtr;
  ```
  `-getNumLinesWithWrapWidth:` is memoized per width (`hasCachedNumLinesForWidth:`),
  and `iTermCumulativeSumCache` / `iTermLineBlockArray` maintain prefix sums so
  "line N at width W" is not O(blocks).
- `mayHaveDoubleWidthCharacter` is a sticky flag: once true, a slower DWC-aware line
  counting path is used, backed by `iTermDoubleWidthCharacterCache`. Fast path
  otherwise. Good idea — most sessions never see a DWC.
- **Copy-on-write**: `LineBlock` has `progenitor` (weak), `hasBeenCopied`,
  `numberOfClients`, `invalidated`. `cowCopy` gives cheap snapshots (used for the
  main-thread immutable view and for alt-screen resize scratch buffers).
- **Two generation counters** (`LineBlock.h:31-47`):
  - `generation` — globally allocated via `iTermAllocateGeneration()`, bumped on
    mutation; used for delta-encoded state restoration (if a block's generation equals
    the DB record's, it's unchanged → skip writing).
  - `mutationCounter` — also globally unique, bumped on *every* content change
    including in-place ones that don't bump `generation`; used as the per-row draw
    cache key. The header explains why a per-block counter would collide across COW
    copies. Worth reading before designing omt's render cache.
- Dropping from the head: `-dropLines:withWidth:chars:firstSurvivorPartialOffset:`
  returns how many chars were trimmed off the *front of a partially surviving raw
  line*, because positions inside that line must be fixed up.
- `LineBufferPosition` is an opaque, width-independent position (with an
  `extendsToEndOfLine` flag). Selections, marks, and search results are stored as
  positions and re-resolved to (x,y) at the current width. **This is the mechanism
  that makes reflow non-destructive** and omt should adopt it.

Search lives here too: `iTermCoreSearch.m`, `FindContext`, `LineBufferSorting.mm`.

### 1.6 Reflow on resize

`sources/VT100Screen/VT100ScreenMutableState+Resizing.m` (1552 lines).
`-reallySetSize:...` at line 1294 is the whole algorithm. Sequence:

1. `[linebuffer beginResizing]` / `endResizing` bracket the operation.
2. Snapshot: saved-cursor position (converted to an *absolute* coord using
   `totalScrollbackOverflow`), visible line range, selection, alt grid copy.
3. If the **alt screen** is showing, build a temporary `LineBuffer` containing the
   *primary* screen's content (`prepareToResizeInAlternateScreenMode:`) so alt-screen
   selections/marks can be reflowed against the right content.
4. **Push the grid into the line buffer** — `-appendScreen:toScrollback:withUsedHeight:newHeight:`
   (line 141). The number of rows pushed is chosen carefully:
   ```objc
   if (grid.size.height - newHeight >= usedHeight) n = MAX(usedHeight, newHeight);
   else if (newHeight < grid.size.height)          n = usedHeight;   // shrinking
   else                                            n = grid.size.height;
   ```
   i.e. don't scroll used lines off the top just because the window shrank.
5. Convert every coordinate of interest through the line buffer:
   `coord --(old width)--> LineBufferPosition --(new width)--> coord`. That's
   `resizeConverter` (line ~1381) and `positionRangeForCoordRange:inLineBuffer:tolerateEmpty:`.
   Selections are first **trimmed of leading/trailing nulls** (`trimSelectionFromStart:…`)
   because nulls aren't in the line buffer.
6. Set `currentGrid.size = newSize`.
7. Broadcast the converter so *everything holding a coordinate* can update:
   `iTermRCResizeNotification` + `screenResizeResilientCoordinates:`. Marks/annotations
   use "resilient coordinates" (`sources/VT100/ResilientCoordinate.swift`) bound to a
   data source; the alt-screen case rebinds them to a second pool for the duration of
   the broadcast and then rebinds back (lines 1408-1480). This is fiddly and clearly
   hard-won.
8. `restoreScreenFromLineBuffer:withDefaultChar:maxLinesToRestore:` pulls rows back
   out of the line buffer into the new grid.
9. Alt-screen restore computes `linesMovedUp` = number of rows lost off the top when
   alt content reflows narrower, and shifts selections/marks by it.
10. Restore saved cursor from the absolute coord; recompute `commandStartCoord` from
    the last prompt mark (`startCoordOfCurrentCommand`).

Design lesson: iTerm2 does **not** implement a bespoke reflow algorithm on the grid.
It reuses one mechanism — "append grid to scrollback, then re-wrap at the new width" —
and expresses every other object's position as a line-buffer position. omt should do
the same.

Note the `EOL_HARD` vs `EOL_SOFT` distinction is what makes this correct: a hard
newline terminates a raw line, a soft one does not, so re-wrapping is purely a
function of raw lines + width.

### 1.7 Token execution / backpressure

`sources/VT100/TokenExecutor.swift` (877 ln), `TokenArray.swift`,
`TwoTierTokenQueue.swift`, `TokenExecutorHelpers.m`.

- **Two priority tiers** (`TwoTierTokenQueue.numberOfPriorities = 2`). High priority
  is drained first — used for in-band signaling tokens (SSH conductor, tmux) that must
  not be starved by bulk output.
- **Coalescing**: a `TokenArrayGroup` marked `coalescable` is flattened into a single
  `VT100_GANG` token whose `subtokens` are executed as a batch — cheap when a burst is
  all plain text/SGR.
- **Throughput estimation**: `iTermThroughputEstimator(historyOfDuration: 5.0/30.0, …)`
  (`TokenExecutor.swift:326`).
- **Side effects** are queued (`iTermTaskQueue`) and flushed to the main thread by a
  `PeriodicScheduler` at 1/30 s — or 1 s when the session is in the background
  (`TokenExecutor.swift:345`). Big win: a backgrounded tab costs ~nothing.
- **Pause/unpause** with an `Unpauser` token object (RAII-ish); there's also a
  *global* pause used when the "maximize throughput" preference is off
  (`TokenExecutor.swift:459-460`), i.e. a deliberate fairness-vs-throughput knob.
- `SlownessDetector` is injected into the executor and drives user-visible
  "this session is producing output too fast" behavior.

---

## 2. Escape sequence coverage

### 2.1 Token inventory

`sources/VT100/VT100Token.h` is effectively the capability list. Highlights beyond
plain VT100:

- **C1 8-bit controls** (`VT100CC_C1_IND/NEL/HTS/RI/SS2/SS3/DCS/SPA/EPA/SOS/DECID/CSI/ST/OSC/PM/APC`)
  — but only honored when encoding is ASCII/Latin-1 (`support8BitControlCharacters`),
  which is the right call for UTF-8 safety.
- Rectangular ops: `DECCARA, DECRARA, DECSACE, DECCRA, DECFRA, DECERA, DECRQCRA`.
- `DECSLRM` (left/right margins), `DECSTBM`, `DECSTR`, `DECALN`, `DECSCUSR`.
- `DECDHL/DECDWL/DECSWL` double-height/width lines (and `iTermLineAttribute`,
  `iTermEffectiveLineWidth()` in `ScreenChar.h:48-53`).
- `REP` (repeat), `SPA`/`EPA` guarded areas.
- `XTPUSHSGR / XTPOPSGR`, `XTPUSHCOLORS / XTPOPCOLORS / XTREPORTCOLORS`,
  `XTSMGRAPHICS`, `XTREPORTSGR`.
- DA / DA2 / DA3 / XDA (extended DA, mintty issue 881).
- `SET_MODIFIERS` / `RESET_MODIFIERS` (`CSI > Ps ; Pm m`).
- Window ops: `XTERMCC_WINDOWSIZE`, `WINDOWSIZE_PIXEL`, `WINDOWPOS`, `ICONIFY`,
  `DEICONIFY`, `RAISE`, `LOWER`, `REPORT_WIN_STATE/POS/PIX_SIZE/SIZE/SCREEN_SIZE`,
  `REPORT_ICON_TITLE`, `REPORT_WIN_TITLE`, `PUSH_TITLE`, `POP_TITLE`.
- Kitty keyboard protocol: referenced at `VT100Terminal.m:2965`
  (`https://sw.kovidgoyal.net/kitty/keyboard-protocol/#progressive-enhancement`).
  *I did not verify how complete the implementation is.*

### 2.2 OSC / APC dispatch table

Verbatim from `sources/VT100/VT100XtermParser.m:268-320`:

| OSC | Token | Meaning |
|---|---|---|
| `0` | `XTERMCC_WINICON_TITLE` | set icon + window title |
| `1` | `XTERMCC_ICON_TITLE` | icon title |
| `2` | `XTERMCC_WIN_TITLE` | window title |
| `4` | `XTERMCC_SET_RGB` | set palette entry |
| `6` | `XTERMCC_PROPRIETARY_ETERM_EXT` | Eterm ext; also **proxy icon** if arg isn't a digit |
| `7` | `XTERMCC_PWD_URL` | current working directory as `file://` URL |
| `8` | `XTERMCC_LINK` | **hyperlinks** |
| `9` | `ITERM_USER_NOTIFICATION` | growl-style notification |
| `10/11/12` | text fg / text bg / cursor color | |
| `17/19` | highlight bg / highlight fg | |
| `22` | `XTERMCC_SET_POINTER_SHAPE` | mouse pointer shape |
| `50` | `XTERMCC_SET_KVP` | Konsole font code, aliased to the 1337 handler |
| `52` | `XTERMCC_PASTE64` | **clipboard** |
| `104` | `XTERMCC_RESET_COLOR` | |
| `110/111/112/117/119` | reset fg/bg/cursor/highlight-bg/highlight-fg | |
| `133` | `XTERMCC_FINAL_TERM` | **semantic prompt / FinalTerm** |
| `134` | `XTERMCC_FRAMER_WRAPPER` | iTerm2 "framer" wrapper |
| `1337` | `XTERMCC_SET_KVP` | **iTerm2 proprietary key=value** |
| `21337` | `XTERMCC_SET_TAB_STATUS` | tab status indicator |
| APC (`ESC _`) | `VT100_APC` | Kitty graphics + tmux |

Terminators: BEL, ST, and (in 8-bit mode) C1 ST. The parser has special-cased quirks
documented in comments at `VT100XtermParser.m:151-200`, including that a `File=` KVP
may finish before the true end of the OSC, and that very long OSCs are truncated
(`"Truncate very long OSC"`, line 258).

**OSC 8 hyperlinks**: token `XTERMCC_LINK`; per-cell hyperlink identity lives in
"external attributes" carried in `LineBlockMetadataArray` / `iTermMetadata` rather
than in `screen_char_t` (which has no spare bits). omt should plan the same: a
side-table keyed per run of cells.

**OSC 52 clipboard** — `VT100Terminal.m:1487-1532, 2633-2644`:
- Write: base64 payload is decoded (`apr_base64_decode`), then **sanitized**: NUL
  terminates; control chars other than TAB/LF/CR are stripped. Then
  `terminalCopyStringToPasteboard:`.
- Read (`?`): the comment says *"Now read access is not implemented due to security
  issues"* — it returns nil and instead routes to `terminalReportPasteboard:`, which
  is gated behind `terminalShouldSendReport:`. So OSC 52 read is effectively
  policy-gated, not free. **omt should copy this: never allow silent clipboard reads.**
- All OSC 1337 clipboard/file paths are additionally gated on
  `[_delegate terminalIsTrusted]`.

**OSC 133 semantic prompt** — handled in `VT100Terminal.m` around lines 4720-4800.
Supported forms and attribute parsing:
- `OSC 133 ; A` — prompt start. Attribute `k=<kind>` → `VT100PromptKind`:
  `i`nitial / `s`econdary / `c`ontinuation / `r`ight / unknown (`promptKindFromArgs:`).
- `OSC 133 ; B` — prompt end / command start-of-input.
- `OSC 133 ; C` — command output begins.
- `OSC 133 ; D [; exitcode]` — command finished. `exitCodeFromArgs:` walks args past
  the command char and takes **the first positional (non-`key=value`) arg that parses
  as an int**, so `D;1;aid=x`, `D;aid=x;1`, and `D;aid=x` all work.
- `aid=<id>` — opaque application/session id, parsed by `aidFromArgs:` in any position.
  Empty value ⇒ nil. Other keys (`cl=`, `redraw=`, `click_events=`) are accepted and
  ignored.

This tolerant "sweep all args, ignore unknown keys" parser is a good pattern for omt.

### 2.3 DECSET / DECRST modes

Verbatim inventory from `VT100Terminal.m:747-995` (`executeDecSetReset:`), plus the
report path at `:5556-5645`:

| Mode | Behavior in iTerm2 |
|---|---|
| 1 | DECCKM cursor keys application mode |
| 2 | **deliberately not implemented** — comment says VT52 mode "breaks everything from the user's POV" |
| 3 | DECCOLM 80/132, gated on `allowColumnMode`; honors `preserveScreenOnDECCOLM` |
| 4 | smooth scroll — ignored |
| 5 | DECSCNM reverse video |
| 6 | DECOM origin mode |
| 7 | DECAWM wraparound |
| 8 | autorepeat |
| 9 | X10 mouse — **TODO, not implemented** |
| 12 | cursor blink |
| 20 | removed (no-op) |
| 25 | DECTCEM cursor visible |
| 40 | allow column mode (DECCOLM enable) |
| 41 | "more fix" (xterm curses hack) |
| 45 | reverse wraparound |
| 47 / 1047 / 1048 / 1049 | alt screen variants — the xterm truth table is reproduced as a comment at line 906 |
| 66 | DECNKM keypad |
| 69 | DECLRMM left/right margins — **requires VT level ≥ 400** |
| 80 | sixel display mode (DECSDM) |
| 95 | DECNCSM — **requires VT level ≥ 500** |
| 1000/1002/1003 | mouse: normal / button-event / any-event (1001 highlight **not implemented**) |
| 1004 | **focus reporting**, gated on `terminalFocusReportingAllowed` |
| 1005 | mouse format UTF-8 ext |
| 1006 | **SGR mouse** |
| 1007 | alternate scroll, gated on `terminalAllowAlternateMouseScroll` |
| 1015 | urxvt mouse format |
| 1016 | **SGR-pixel mouse** |
| 1036 | meta sends escape |
| 1337 | iTerm2 ext: **report key-up events** |
| 2004 | **bracketed paste**, gated on `allowPasteBracketing` |
| 2026 | **synchronized output** (`synchronizedUpdates`) |
| 2031 | color-palette-update notifications (contour spec) → unsolicited dark-mode DSR |
| 2048 | in-band resize notifications (rockorager gist) |

Note the pattern: several modes are **capability-gated by a VT emulation level**
(`iTermEmulationLevel400/500`) and several are **policy-gated by the delegate**. omt
should have both axes from the start.

Truecolor: native in `screen_char_t` (24-bit fg/bg + 2-bit mode). `XTERMCC_SET_RGB`
(OSC 4) and the SGR 38/48 `;2;r;g;b` path both feed it.

DSR / DECDSR handled at `VT100Terminal.m:1257-1362` — including `?997` (color scheme
report), `?1337` (iTerm2 ext), `?75` data integrity, `?85` multi-session config.

### 2.4 Graphics

- **Sixel**: `VT100SixelParser.m`, hooked from `DCS … q`. `libsixel` is vendored
  (`submodules/libsixel`). DECSDM (mode 80) is tracked as `sixelDisplayMode`.
- **Kitty graphics**: `sources/InlineImages/KittyImageCommand.swift` +
  `KittyImageController.swift` + `MetalRenderer/Renderers/KittyImageRenderer.swift`.
  Parsed from APC (`ESC _ G …`) at `VT100Terminal.m:5421-5425`. The Swift model covers
  `a=` actions, `ImageTransmission` (`f=` format, `t=` medium, `o=` compression,
  `m=` more, `i=`/`q=` verbosity), `ImageDisplay` (incl. cursor movement policy and
  **unicode placeholder** creation — hence `screen_char_t.virtualPlaceholder`),
  `AnimationFrameLoading`, `AnimationFrameComposition`, `AnimationControl`,
  `DeleteImage`. That's a fairly complete implementation.
- **iTerm2 inline images**: see §4.

---

## 3. Shell integration

### 3.1 The scripts

`Resources/shell_integration/iterm2_shell_integration.{bash,zsh,fish,tcsh,xonsh}`.
Also a submodule `submodules/iTerm2-shell-integration` and injection support at
`sources/ShellIntegration/ShellIntegrationInjection.swift` +
`Bundle+ShellIntegration.swift` (iTerm2 can inject the script into a new shell without
the user editing rc files).

The zsh script is the cleanest read. Exact emissions (`iterm2_shell_integration.zsh`):

```zsh
iterm2_prompt_mark()      { printf "\033]133;A;aid=%s\007" "$ITERM2_CURRENT_AID" }   # line 78
iterm2_ps2_mark()         { printf "\033]133;A;k=s;aid=%s\007" "$ITERM2_CURRENT_AID" } # line 88
iterm2_prompt_end()       { printf "\033]133;B;aid=%s\007" "$ITERM2_CURRENT_AID" }   # line 92
iterm2_before_cmd_executes() {                                                        # line 35
    printf "\033]133;C;aid=%s\r\007" "$ITERM2_CURRENT_AID"   # (\r variant when needed)
}
iterm2_after_cmd_executes() {                                                         # line 72
    printf "\033]133;D;%s;aid=%s\007" "$STATUS" "$ITERM2_CURRENT_AID"
    iterm2_print_state_data
}
iterm2_print_state_data() {                                                           # line 59
    printf "\033]1337;RemoteHost=%s@%s\007" "$USER" "$_iterm2_hostname"
    printf "\033]1337;CurrentDir=%s\007" "$PWD"
    iterm2_print_user_vars
}
iterm2_set_user_var() {                                                               # line 43
    printf "\033]1337;SetUserVar=%s=%s\007" "$1" $(printf "%s" "$2" | base64 | tr -d '\n')
}
# at install time:
printf "\033]1337;ShellIntegrationVersion=17;shell=zsh\007"                           # line 222
```

Mechanics:
- zsh: hooks into `precmd_functions` and `preexec_functions`.
- bash: vendors **bash-preexec.sh** (MIT, v0.6.0) inside the script to synthesize
  preexec/precmd from the `DEBUG` trap + `PROMPT_COMMAND`. It appends
  `__iterm2_prompt_command` as the *last* element of `PROMPT_COMMAND` (not
  `precmd_functions`) specifically so a pre-existing `PROMPT_COMMAND` can't clobber the
  `PS1` it decorates — comment at `iterm2_shell_integration.bash:48-56`.
- `PS1` is decorated by wrapping the marks in `%{…%}` (zsh zero-width) so prompt
  width math stays correct; `PS2` gets the `k=s` secondary mark unless
  `ITERM2_SQUELCH_PS2_MARK` is set.
- It detects that the user changed `PS1` behind its back by comparing against
  `ITERM_PREV_PS1`.
- **^C during prompt entry**: if `preexec` never ran but `precmd` did, it synthesizes
  `iterm2_before_cmd_executes` so the state machine doesn't desync
  (`.zsh:167-178`). This edge case matters for omt's block model.
- Guarded off entirely when `TERM` is `screen`/`tmux-256color`/`linux`/`dumb`, when
  non-interactive, or when already installed; bash refuses if `shopt extdebug` is on.
- `ITERM2_CURRENT_AID` is a per-prompt-cycle id, pre-seeded before the first
  `precmd` so install-time emissions have a valid aid.

### 3.2 Consumption side

- `sources/VT100/PromptStateMachine.swift` — the terminal-side state machine that
  turns A/B/C/D into prompt/command/output regions. (I did not read it in full.)
- `sources/Marks/`: `VT100ScreenMark` (prompt/command/exit code, `promptRange`,
  `commandRange`, `firstLineOfCommand`), `iTermCapturedOutputMark`, `FoldMark`,
  `BlockMark`, `ButtonMark`, `PathMark`. Marks are stored in an **IntervalTree**
  keyed by absolute positions, with doppelgangers for the main thread.
- `sources/ShellIntegration/iTermShellHistoryController.m` — Core Data persistence of
  command history, recent directories (`iTermRecentDirectoryMO`), host records
  (`iTermHostRecordMO`), and a `iTermDirectoryTree` for directory autocomplete.
  Also `iTermLatestVersionByShell.h` for nagging about stale shell integration.
- `VT100ScreenMutableState.startCoordOfCurrentCommand` (Resizing.m:1532) recomputes the
  command start after resize by deltaing against the prompt mark — evidence that
  block boundaries must survive reflow.

For omt's agent-state detection: OSC 133 `C`→`D` bracketing plus `aid=` gives you
"command running / finished / exit code" without heuristics. `OSC 1337;SetUserVar=` is
the sanctioned channel for arbitrary shell→terminal state (base64-encoded value).

---

## 4. Inline images / imgcat wire format

### 4.1 Wire format (from `OtherResources/Utilities/imgcat`)

```
OSC 1337 ; File = <key>=<value> [; <key>=<value> …] : <base64 data> ST
```

`imgcat`'s emitter, lines 57-105:

```sh
printf "1337;"
printf "File"                # or "MultipartFile"
printf "=inline=%s" "$2"                       # inline=1 → render; 0 → download
printf ";size=%d" $(printf "%s" "$3" | b64_decode | wc -c)   # DECODED byte count
[ -n "$1" ] && printf ";name=%s" "$(printf "%s" "$1" | b64_encode)"  # base64 filename
[ -n "$5" ] && printf ";width=%s"  "$5"
[ -n "$6" ] && printf ";height=%s" "$6"
[ -n "$7" ] && printf ";preserveAspectRatio=%s" "$7"
[ -n "$8" ] && printf ";type=%s" "$8"
# monolithic: printf ":%s" "$base64"
# multipart:  fold -w 200; per chunk:  OSC 1337;FilePart=<chunk> ST ; then OSC 1337;FileEnd ST
```

Terminator handling (also used by `it2copy`, `it2dl`, `it2ul`):

```sh
print_osc() { if [[ $TERM == screen* || $TERM == tmux* ]];
                then printf "\033Ptmux;\033\033]"; else printf "\033]"; fi }
print_st()  { if [[ $TERM == screen* || $TERM == tmux* ]];
                then printf "\a\033\\";            else printf "\a"; fi }
```

i.e. **inside tmux, wrap in `DCS tmux; … ST` and double every ESC**; tmux only accepts
`ESC \` as ST. It keys off `TERM` rather than `$TMUX` because `TERM` survives ssh.
Base64 is emitted with `-w0` on GNU coreutils, plain otherwise.

### 4.2 Terminal-side argument parsing

`VT100Terminal.m:3930-4010`. Args are split on `;` then on the first `=`.
Recognized keys:

- `inline` — `0`/`1`. If 0 ⇒ it's a **file download**, not an image.
- `size` — decoded byte count (used for progress + a size cap).
- `name` — base64 of the filename (`stringByBase64DecodingStringWithEncoding:NSUTF8StringEncoding`);
  defaults to `"Unnamed file"`.
- `width`, `height` — integer with optional suffix; units resolved to
  `kVT100TerminalUnitsCells` (bare number), `…px` → pixels, `…%` → percentage,
  or the literal `auto` → `kVT100TerminalUnitsAuto`.
- `preserveAspectRatio` — bool.
- `type` — MIME type hint.
- `insetTop`, `insetLeft`, `insetBottom`, `insetRight` — doubles, **fractions of a cell**
  (`ScreenChar.h:632`: "Insets should be specified as a fraction of cell size … in [0,1]").
- `mode=wide` → `forceWide`.

Chunked variants: `MultipartFile=` opens (`_receivingMultipartFile = YES`),
then `FilePart=<base64chunk>`, then `FileEnd` (`VT100Terminal.m:4218-4265`).
The parser side emits these as multitoken incidentals (§1.2) so memory stays bounded.

Rendering: `ImageCharForNewImage(name, width, height, preserveAspectRatio, insets)`
allocates a global image code and returns a `screen_char_t` with `image=1`;
`SetPositionInImageChar(&c, x, y)` stamps the cell's (col,row) within the image. So
**an image occupies real grid cells** and therefore scrolls, reflows, and gets
selected like text. Supporting code: `sources/InlineImages/{ImageRegistry.swift,
iTermImageInfo, iTermImageCache, iTermAnimatedImageInfo, VT100InlineImageHelper}` and
`MetalRenderer/Renderers/iTermImageRenderer.m`.

`iTermImageMark` (in `sources/Marks/`) records image locations in the interval tree.

### 4.3 Paste and drag-and-drop

- Drag-and-drop is implemented in `sources/TerminalView/PTYTextView.m`,
  `SessionView.m`, `PseudoTerminal.m` (`draggingEntered`, `NSFilenamesPboardType`).
  I did not trace the full behavior; the common documented behavior (drop a file →
  its escaped path is typed) is consistent with those files but **I did not verify it
  line-by-line**.
- Pasting non-text: `sources/Pasting/iTermNonTextPasteHelper.swift`,
  `NSPasteboard+iTerm.m`, `PasteboardReporter.swift`. Paste in general goes through
  `iTermPasteHelper` with a `PasteContext` (chunked, rate-limited, with bracketed-paste
  wrapping and "paste special" transforms: tab→spaces, remove newlines, base64-encode,
  escape shell chars). The chunk/delay pacing is important for slow ptys and is worth
  copying.
- `sources/Clippings/` and `sources/Snippets/` hold clipboard history and snippets.

---

## 5. Clipboard and file transfer over SSH

This is the section most relevant to omt's "paste an image while ssh'd in" goal.
iTerm2 has **three** distinct mechanisms; understand all three.

### 5.1 OSC 52 — limited

As covered in §2.2: write works (sanitized, trust-gated); **read is refused** by the
decoder for security. Also subject to whatever size limits the ssh/tmux path imposes
and to the "Truncate very long OSC" guard in the parser. Not suitable for images.

### 5.2 OSC 1337 `Copy=` — chunked clipboard (`it2copy`)

`it2copy` avoids one giant OSC by using a segmented protocol with a UID
(`OtherResources/Utilities/it2copy:44-70`):

```
OSC 1337 ; Copy=2;<uid>            ST     # begin segmented
OSC 1337 ; Copy=3;<uid>:<line>     ST     # …repeated per line
OSC 1337 ; Copy=4;<uid>            ST     # end segmented
# monolithic form:
OSC 1337 ; Copy=:<base64>          ST     # (mode defaults to 1)
```

Terminal side (`VT100Terminal.m:4266-4310`): mode 1 = monolithic, 2 = begin segmented
(records `uidForCopyMode`), 3 = segment (dropped if the uid doesn't match — replay
protection), 4 = end. Everything gated on `terminalIsTrusted`. There's also a legacy
`CopyToClipboard` / `EndCopy` KVP pair, and the *parser* itself watches for those to
flip `_saveData` (`VT100Parser.m:152-158`) — a rare case of the parser needing
semantic knowledge.

### 5.3 remote → local download: `it2dl`

`it2dl` reuses the **inline-image transport with `inline=0`**. From
`OtherResources/Utilities/it2dl`:

- Directories are tar+gzipped to stdout first (`tar -czf - -C <dirname> <basename>`).
- Chunked path: `OSC 1337;MultipartFile=name=<b64>;` then repeated
  `OSC 1337;FilePart=<chunk>` then `OSC 1337;FileEnd`.
- Monolithic path: `\033]1337;File=name=<b64>;` … (line 98).
- Same tmux DCS wrapping / `TERM`-sniffing as imgcat.

So: **a download is just an inline image with `inline=0`.** One transport, two
behaviors. Good simplification for omt.

### 5.4 local → remote upload: `it2ul` (the interesting one)

`OtherResources/Utilities/it2ul` implements a *request/response* flow, which is what
makes "paste a local image into a remote session" possible:

```sh
send_request_for_upload() {
  print_osc
  printf '1337;RequestUpload=format=tgz;version=%s' "$(tar --version | head -1 | b64_encode)"
  print_st
}
send_request_for_upload
read status                     # terminal types "ok" or "abort" back on stdin
if [[ $status == ok ]]; then
  data=$(read_base64_stanza)    # terminal types a base64 blob followed by a blank line
  decode "$data" | tar -x -z -C "$location" -f - $*
fi
```

Terminal side: `RequestUpload` → `[_delegate terminalRequestUpload:value]`
(`VT100Terminal.m:4313-4316`), gated on `terminalIsTrusted`. iTerm2 shows a local file
picker, tars the selection, and **writes `ok\n` + base64 + blank line into the pty as
if typed**. The remote side just reads stdin.

Implication for omt: the "paste image over ssh" feature does **not** need a side
channel. A tiny remote helper + a terminal-driven synthetic-input reply is enough, and
it works through any transport that carries a tty (ssh, docker exec, tmux, nested
shells). The costs are: the payload goes through the pty at pty speed, it must be
base64, and the helper must be present remotely.

### 5.5 The conductor (iTerm2's richer alternative)

`sources/SSH/Conductor.swift` + friends, hooked from **`DCS 2000 p`**
(`VT100DCSParser.m:527-536` → `DCS_SSH_HOOK` → `VT100ConductorParser.swift`).
Started via `OSC 1337;it2ssh=…`, fed with `SendConductor`, torn down with `EndSSH`
(`VT100Terminal.m:4438-4445`). Once running:

- Remote output arrives as `SSH_OUTPUT` tokens tagged with a pid, and is dispatched to
  a **nested `VT100Parser`** (§1.2) — so each remote process gets its own parser.
- `SSH_TERMINATE`, `SSH_UNHOOK`, `SSH_RECOVERY_BOUNDARY` manage lifecycle and
  reconnection (`ConductorRecovery.swift`, `SSHReconnectionInfo.swift`).
- On top of it: `SSHEndpoint`, `ConductorFileTransfer`, `TarJob.swift`,
  `SSHFilePanel*` (a full remote file browser), `SSHFilePromiseProvider` (drag files
  out of the remote host into Finder), `SSHProcessInfoProvider`, `SecretServer.swift`.

This is a full multiplexed control channel over the pty, in the same spirit as tmux
control mode. If omt wants first-class remote-file UX it will need something like it;
if it only wants image paste, §5.4 suffices.

---

## 6. tmux `-CC` control mode

`sources/tmux/` — `TmuxGateway.m` is the protocol core; `TmuxController.m` maps tmux
objects to iTerm2 objects.

Entry: iTerm2 runs `tmux -CC`; tmux replies with `DCS 1000 p` which the DCS parser
hooks into `VT100TmuxParser` (`VT100DCSParser.m:515-526`, token `DCS_TMUX_HOOK`).

**Framing.** Command responses are bracketed:
```
%begin <command_id> <timestamp> [<flags>]
…response lines…
%end <command_id>          (or %error <command_id>)
```
`TmuxGateway.m:657-690` parses `%begin` with regex `^%begin ([0-9]+) [0-9]+( [0-9]+)?$`
and asserts that a `%begin` is always closed. Everything between goes into
`currentCommandResponse_` / `currentCommandData_` (both a string and raw data copy).
There's a workaround: if a session is destroyed, tmux may not print `%end` before
`%exit`, so `%exit` force-closes the pending command (`:772-783`).

**Notifications handled** (`TmuxGateway.m:800-870`), a complete list:

| Notification | Handling |
|---|---|
| `%output %<pane> <data>` | pane output (data is octal-escaped) |
| `%extended-output %<pane> <latency> [args] : <data>` | tmux ≥3.2 with latency |
| `%layout-change @<win> <layout> [visible_layout] [flags]` | re-lay-out the tab |
| `%window-add @<win>` / `%unlinked-window-add` | open a native tab |
| `%window-close` / `%unlinked-window-close` | |
| `%window-renamed` / `%unlinked-window-renamed` | |
| `%session-changed $<id> <name>` | gates `acceptNotifications_`; iTerm2 waits for this before writing, to distinguish an interactive attach from a one-shot `tmux -CC list-windows` (`:405-413`) |
| `%session-renamed`, `%sessions-changed` | |
| `%window-pane-changed` | tmux 2.5+ |
| `%session-window-changed`, `%client-session-changed`, `%client-detached` | |
| `%pause` / `%continue` | tmux 3.2 flow control |
| `%subscription-changed` | tmux 3.2 format subscriptions |
| `%paste-buffer-changed` | |
| `%pane-mode-changed` | tmux 2.5+, ignored |
| `%noop` | ignored |
| `%exit [reason]` | disconnect; on ≥3.2 write a newline first |
| unknown `%…` | logged; non-`%` lines either abort or accumulate in `strayMessages_` depending on `tolerateUnrecognizedTmuxCommands` |

**Version gating** is explicit (`versionAtLeastDecimalNumberWithString:@"3.2"`).

Supporting parsers: `TmuxLayoutParser.m` (tmux's layout string → tree),
`iTermTmuxLayoutBuilder.m` (the reverse — iTerm2 splits → tmux layout),
`TmuxHistoryParser.m` (`capture-pane` output → scrollback),
`TmuxStateParser.m`, `TSVParser.m` (tmux's `-F` TSV format output).
`iTermTmuxBufferSizeMonitor.m` implements client-side backpressure;
`iTermTmuxOptionMonitor.m` and `iTermTmuxStatusBarMonitor.m` subscribe to formats;
`TmuxWindowOpener.m` materializes a tmux window as a native tab;
`TmuxControllerRegistry.m` maps sessions; `TmuxDashboardController` is the UI.
Keys are sent to tmux hex-encoded byte-by-byte (`keyEncodedByte:` → `0x%02x`).

**Mapping**: tmux session → iTerm2 window group; tmux window → native tab; tmux pane →
native split pane; tmux layout string is the source of truth and iTerm2 pushes layout
changes back via `select-layout`. This is a good model for omt's workspace concept:
treat the multiplexer as authoritative state and the UI as a projection.

---

## 7. Session / window / tab / pane model, restoration, and the scripting API

### 7.1 Object model

- `sources/PTYSession/` — `PTYSession` is the unit (one pty + one screen + one
  view). `sources/TerminalView/` has `PTYTextView`, `SessionView`, `PseudoTerminal`
  (window controller), `PTYTab` / split tree.
- `sources/Channels/`, `sources/BuriedSessions/`, `sources/Workgroups/`,
  `sources/SplitPanel/`, `sources/Swipe/` — additional session containers.
- `sources/ExternalSessionState/`, `sources/StateRestoration/` — persistence.
  Restoration is delta-encoded against `LineBlock.generation` (§1.5) and stored via
  Core Data / SQLite (`sources/Databases/`, `submodules/fmdb`,
  `Model.xcdatamodeld`). `tools/analyze_restorable_state.py` exists for debugging.
- Saved arrangements are a separate concept, exposed over the API
  (`SavedArrangementRequest` in `proto/api.proto:572`).

### 7.2 Python API

Transport (`sources/API/`):
- `iTermAPIServer.m` runs an HTTP server; `iTermHTTPConnection.m` upgrades to
  WebSocket (`iTermWebSocketConnection`, `iTermWebSocketFrame`). Frames are
  **binary** opcode (`iTermAPIServer.m:1439`).
- Auth: a cookie/key handshake (`websocketKeyForConnectionKey:`) plus a
  **library-version check** — connections with too-old a Python library are rejected
  with `iTermWebSocketConnectionLibraryVersionTooOldString` (`:605`).
- `iTermInProcessAPIConnection.m` lets in-app code use the same API surface without a
  socket. `it2api` / `it2cli` are the CLI entry points; a bundled Python runtime is
  downloaded on demand (`iTermPythonRuntimeDownloader.m`) and scripts run sandboxed-ish
  via `iTermAPIScriptLauncher.m`.

Protocol (`proto/api.proto`, 1686 lines): a single
`ClientOriginatedMessage` / `ServerOriginatedMessage` envelope with a `oneof` of
request types and a correlating `id`. Request families:

- Lifecycle/layout: `CloseRequest`, `SplitPaneRequest`(*), `ActivateRequest`,
  `ReorderTabsRequest`, `SetTabLayoutRequest`, `RestartSessionRequest`,
  `SavedArrangementRequest`, `FocusRequest`, `ListProfilesRequest`.
- Content: `GetBufferRequest`, `SelectionRequest`, `InjectRequest` (inject bytes as if
  from the pty), `SendTextRequest`(*).
- State: `VariableRequest`, `GetPropertyRequest`, `SetPropertyRequest`,
  `PreferencesRequest`, `ColorPresetRequest`, `ProfileChangeRequest`.
- Extension points: `RegisterToolRequest` (a web-view tool in the toolbelt),
  `RPCRegistrationRequest` (register a Python function callable from iTerm2 —
  this is how keybindings/triggers/status-bar components call into scripts),
  `StatusBarComponentRequest`, `MenuItemRequest`, `SetBroadcastDomainsRequest`.
- tmux passthrough: `TmuxRequest` / `TmuxResponse`.

Notifications (server→client), the useful ones for a plugin API:
`NewSessionNotification`, `TerminateSessionNotification`, `LayoutChangedNotification`,
`FocusChangedNotification`, `ScreenUpdateNotification`, `KeystrokeNotification`,
`VariableChangedNotification`, `ProfileChangedNotification`,
`BroadcastDomainsChangedNotification`, `LocationChangeNotification`,
`CustomEscapeSequenceNotification`, and the semantic-prompt trio
`PromptNotificationPrompt` / `PromptNotificationCommandStart` /
`PromptNotificationCommandEnd` inside `PromptNotification`.
`ServerOriginatedRPC` + `ServerOriginatedRPCResultRequest` implement iTerm2 calling
*into* the script.

(*) I saw these families in the message list; I did not enumerate every one.

Design notes worth stealing for omt's plugin API:
1. **Variables + a small expression language** are the extension substrate. Almost
   everything (badge text, status bar, titles, triggers) is driven by
   `VariableRequest`/`VariableMonitorRequest` over a namespaced variable tree, and
   `RPCRegistrationRequest` lets a script *become* a variable provider or an action.
2. **Custom escape sequences as a plugin channel**: `OSC 1337;Custom=…`
   (`VT100Terminal.m:4406`) → `CustomEscapeSequenceNotification`. A program in the
   terminal can talk to a plugin. Very cheap, very powerful.
3. Registration is versioned and the server rejects stale clients rather than
   half-working.

---

## 8. Productivity feature inventory

### 8.1 Search / filter

- `sources/SearchingFiltering/SearchEngine.swift` + `FindContext` +
  `LineBuffer`'s `iTermCoreSearch.m`. Search runs against the **line buffer at raw-line
  granularity**, incrementally (a `FindContext` is a resumable cursor), so a search over
  a huge scrollback doesn't block. `TailFindController.swift` keeps a search live as new
  output arrives.
- `AsyncFilter.swift` / `FilterTextField.swift` — "filter" mode: show only matching
  lines, live-updating.
- `GlobalSearch/iTermGlobalSearchEngine.m` — search across all sessions, with a
  per-session cursor (`iTermGlobalSearchEngineCursor`) so results stream in.
- `SearchResultSoftBoundaryExtender.swift` — extends matches across soft-wrapped
  boundaries (necessary because a match can span a wrap).
- `FoldSearchEngine.swift` — search inside folded regions.
- `iTermFindPasteboard.m` / `iTermSearchHistory.m` — shared find pasteboard, history.

### 8.2 Command history / autocomplete

- `sources/ShellIntegration/iTermShellHistoryController.m` — Core Data store of
  `iTermCommandHistoryEntryMO` (+ `iTermCommandHistoryCommandUseMO` per invocation,
  which is how "commands used in this directory" works), `iTermRecentDirectoryMO`,
  `iTermHostRecordMO`. `iTermDirectoryTree` indexes directories for prefix search.
- `sources/Popups/`, `sources/CompletionsUI/`, `sources/OpenQuickly/` —
  the autocomplete popup, the "open quickly" omnibar.
- `sources/Composer/` — the multiline command composer.

### 8.3 Triggers

`sources/Triggers/` — a regex is matched against each line (or each command), and one
of ~30 actions fires. Action inventory (file names): `Alert, Annotate, Bell, Bounce,
BufferInput, Capture, Coprocess, Fold, Highlight, Hyperlink, Inject,
InjectJavascript, Mark, SetNamedMark, Password, SGR, Script, SendText, SetDirectory,
SetHostname, SetTabStatus, SetUserVariable, Stop, EnterWorkgroup, ExitWorkgroup,
ReaderMode, Reload` (several have Browser variants for the built-in browser).
`PTYTriggerEvaluator.m` drives evaluation. `CaptureTrigger` feeds
`iTermCapturedOutputMark` — captured output shows up in the toolbelt.

Notable: triggers can run **per-command** as well as per-line, and `StopTrigger`
short-circuits further trigger evaluation for the line.

### 8.4 Smart selection & semantic history

- Smart selection: `sources/ContentAnalysis/SmartMatch.{h,m}` +
  `sources/Settings/SmartSelectionController.m`. An ordered list of (regex, precedence,
  actions); double-click picks the highest-precedence match around the click point.
  Actions attached to a match become the context menu.
- Semantic history (Cmd-click): `sources/SemanticHistory/`
  - `iTermPathFinder.m` — finds a plausible path around a click, trying prefixes and
    suffixes; `iTermPathCleaner.m` normalizes; `iTermCachingFileManager.m` caches
    `stat` results (important — naive existence checks are the perf killer here).
  - `URLAction.m` / `iTermURLActionFactory.m` — decides what a click means
    (open file at line, open URL, run a command, look up in the smart-selection rules).
  - `iTermSemanticHistoryController.m` — the configurable action (open in editor with
    `file:line`, run a command, etc.).
- Line/column extraction from `file:line:col` patterns is part of the URL action
  factory. The recent commit at HEAD is about linkifying long/IDN URLs, so URL
  detection lives near here too.

### 8.5 Other inventory (not investigated in depth)

`sources/Annotations/`, `sources/Portholes/` (rich inline content),
`sources/CopyMode/` (vim-ish keyboard selection), `sources/MultiCursor/`,
`sources/StatusBar/` (composable components, scriptable),
`sources/Toolbelt/`, `sources/Snippets/`, `sources/PasswordManager/`,
`sources/Hotkey/` (hotkey windows), `sources/InputBroadcasting/`,
`sources/AutomaticProfileSwitching/` (profile follows hostname/user/path),
`sources/DVR/` (instant replay — a ring buffer of frames),
`sources/SessionNotes/`, `sources/Printing/`, `sources/QuickLook/`,
`sources/Browser/` + `WebExtensionsFramework/` (an embedded browser!),
`sources/AITerm/` + `iTermAI/` + `ClaudeCode/` (LLM integration).

---

## 9. Performance techniques

1. **Off-main-thread parse + mutate, immutable main-thread snapshot.**
   `VT100ScreenMutableState` (mutation queue) ⇄ `VT100ScreenState` (main thread)
   with explicit `iTermGCD` queue assertions. Side effects are batched and delivered
   at 1/30 s (1 s if backgrounded) — `TokenExecutor.swift:345, 363`.

2. **Fast paths in the parser.** `VT100_ASCIISTRING` and `VT100_MIXED_ASCII_CR_LF`
   avoid the state machine for the common case; `isMixedAsciiString()` peeks two bytes.
   Non-ASCII strings ≥ `kMinPreconvertStringLength` (4) are converted to cells on the
   parser thread using shadow SGR state, with a global byte budget
   (`sOutstandingPreconvertBytes`).

3. **Token coalescing + two-tier priority queue** so bulk output can't starve control
   traffic and so a burst executes as one `VT100_GANG`.

4. **Damage tracking at line granularity**: `VT100LineInfo` keeps `dirtyRange`,
   `dirtyIndexes`, and a content **generation**; equal generations ⇒ equal content
   (`VT100LineInfo.h:30`), with an O(1) all-dirty representation.
   `LineBlock.mutationCounter` extends the same idea to scrollback rows for the draw
   cache.

5. **Memoized wrap math**: per-width line counts cached on `LineBlock`, prefix sums in
   `iTermCumulativeSumCache` / `iTermLineBlockArray`, DWC positions in
   `iTermDoubleWidthCharacterCache`, and a sticky `mayHaveDoubleWidthCharacter` flag
   that keeps the fast path for the 99% case.

6. **Copy-on-write scrollback blocks** — snapshots for the renderer and for
   alt-screen resize are O(1).

7. **Metal renderer** (`sources/MetalRenderer/`):
   - `iTermMetalDriver.m` builds an `iTermMetalFrameData` per frame, extracts row data
     on the main thread (stat `MtExtractFromApp`), then does
     `BuildRowData → UpdateRenderers → CreateTransientStates → PopulateTransientStates`
     on a **private queue** before encoding. Frames are pipelined.
   - ~25 independent renderers, each with a "transient state" per frame:
     background color (with RLE — `iTermBackgroundColorRLETestHelper`), background
     image, text, underline (+ composite), cursor, cursor guide, mark, line-style mark,
     image, Kitty image, badge, broadcast stripes, margin, highlight row, indicator,
     full-screen flash, offscreen command line, block, pill background, terminal
     button, rectangle, copy-background.
   - Glyph atlas with expiry (`expireNonASCIIGlyphs`) — ASCII glyphs are kept, others
     age out.
   - Per-stage timing via `iTermPreciseTimerStats` + histograms, resettable on
     `applicationDidBecomeActive`. Also `iTermMetalUnavailableReason.m` — the renderer
     degrades to `PTYTextView` (CoreText) with a *user-visible explanation*.
   - `tools/compare-rendering.sh` for regression-checking the two renderers against
     each other.

8. **Compressible buffers** (`sources/CompressibleBuffers/`) for cold scrollback, and
   `sources/Codecs/` — *I did not read these; mechanism not verified.*

9. Benchmarks/tests exist as first-class code: `PerformanceTests/`,
   `iTermAttributedStringBuilderBenchmark.m`, `tools/perf`.

---

## 10. Checklist distilled for omt

**Must-have core (in rough dependency order)**

- [ ] Cell struct with: 16-bit code + complex-char side table, 24-bit fg/bg + color
      mode, attribute bits, and a `width+1` continuation slot holding
      `EOL_HARD | EOL_SOFT | EOL_DWC`.
- [ ] Sentinel codes for `DWC_RIGHT`, `DWC_SKIP`, `TAB_FILLER`.
- [ ] `StringToScreenChars` equivalent: grapheme clustering, wcwidth with
      configurable `unicode_version` and `ambiguous_is_double_width`, normalization
      mode, RTL detection. Generate tables from UCD; don't hand-write.
- [ ] Scrollback = blocks of raw (unwrapped) lines + per-line metadata; wrapping
      computed on demand with per-width memoized counts and prefix sums.
- [ ] Opaque, width-independent `Position` type; store selections, marks, search
      results, and hyperlink anchors as positions.
- [ ] Reflow = "append grid to scrollback, re-wrap at new width, restore grid", with a
      coordinate converter broadcast to every position holder.
- [ ] Streaming parser with explicit saved state for partial OSC/DCS/CSI, plus a
      DCS/APC **hook** mechanism (sixel, kitty, tmux, ssh-conductor all reuse it).
- [ ] Multitoken emission for huge OSC payloads so images never buffer whole.
- [ ] Per-line dirty ranges + content generation counters for the renderer diff.

**Escape sequence priority for a "serious" terminal**

- Tier 1: full CSI/SGR incl. truecolor, alt screen 1049, DECSTBM, bracketed paste
  (2004), mouse 1000/1002/1003 + SGR 1006, focus 1004, synchronized output 2026,
  OSC 0/1/2 titles, OSC 4/10/11/12 + resets, OSC 7 cwd, OSC 8 hyperlinks,
  OSC 52 write (sanitized, no read), OSC 133 A/B/C/D with `aid=`/`k=`.
- Tier 2: DECSLRM + rectangular ops, REP, DECSCUSR, XTPUSH/POPSGR, 1016 pixel mouse,
  2048 in-band resize, 1007 alternate scroll, OSC 1337 `File=`/`SetUserVar=`/
  `CurrentDir=`/`RemoteHost=`/`SetMark`/`UnicodeVersion`.
- Tier 3: sixel, Kitty graphics, Kitty keyboard, 2031 palette notifications,
  double-width/height lines, guarded areas.

**Policy gates to build in from day one** (iTerm2 has all of these):
`is_trusted` session flag; per-capability allow lists for clipboard write, clipboard
read, focus reporting, paste bracketing, window ops, alternate scroll, file
transfer/upload; and a VT emulation level that gates DECSLRM/DECNCSM.

**For the "paste image over ssh" feature**: implement the `it2ul` pattern —
`OSC 1337;RequestUpload=format=tgz;version=<b64>` from a remote helper, terminal
responds by *typing* `ok\n` + base64 + blank line into the pty. Plus `it2dl`'s
`File=…;inline=0` for the reverse. Reserve OSC 52 for small text only.

---

## 11. Gaps / things I did not verify

- `PromptStateMachine.swift` internals (how A/B/C/D are turned into blocks and how
  desync is recovered) — read the file before designing omt's block model.
- Exact drag-and-drop semantics in `PTYTextView.m` / `SessionView.m`.
- Completeness of the Kitty keyboard protocol implementation.
- `CompressibleBuffers/` and `Codecs/` mechanisms.
- Whether the Metal text renderer does subpixel AA / how it handles ligatures.
- `SplitPaneRequest` / `SendTextRequest` field details in `api.proto` (I enumerated
  message names, not all fields).
- `VT100Terminal.m` is 6076 lines and I read roughly a third of it; the SGR handler,
  charset (SCS/G0-G3) handling, and the report/DSR paths deserve a closer read before
  implementation.
