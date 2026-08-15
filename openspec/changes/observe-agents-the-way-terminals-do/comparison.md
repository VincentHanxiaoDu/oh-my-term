# What two mature terminals do, and what omt does now

Read from the source of two products that observe third-party agent CLIs, and
checked line by line against omt. Neither is named here: the point is the
technique, and a competitor's name in this repository is a liability rather
than a citation.

They disagree about almost everything and agree about the one thing omt was
missing.

## How an agent is identified

| | them (A) | them (B) | omt before | omt now |
|---|---|---|---|---|
| Its own protocol | a sentinel URL and a versioned JSON payload, emitted by a plugin they ship for the agent; the host advertises support by environment variable | — | hooks and protocols, already built | unchanged |
| Terminal title | — | read; the highest-priority rule in every rule set | parsed into a rename event and **discarded** | a `Marker` reading: identity from the name, state from the leading glyph |
| Process table | — | not used for identity | the only source, and it failed | fallback, and repaired |
| Screen | — | region rules | not read at all | region rules |

**The finding.** omt had the top of the ladder and the bottom, and nothing in
between — and the middle is where every agent on a plain pty lives.

## How the screen is read

| | them (B) | omt now |
|---|---|---|
| Regions | `osc_title`, `osc_progress`, `whole_recent`, `bottom_non_empty_lines(n)`, `after_last_horizontal_rule`, `prompt_box_body`, and several more | `Whole`, `BottomNonEmpty(n)`, `AfterLastRule`, `PromptBox` |
| Combinators | `contains`, `regex`, `line_regex`, `any`, `all`, `not` | `Contains`, `LineStartsWith`, `Any`, `All`, `Not` |
| Priorities | explicit, 250–1100 | explicit |
| "Say nothing about now" | `skip_state_update`, for an agent showing its own history | `ScreenState::Hold` |
| Where the rules live | versioned TOML, updatable without a release | versioned TOML, with the Rust values as the floor |
| Coverage | 19 agents | Claude Code and Gemini, from captured screens |

Rules as data is built. A file per agent under the config directory replaces
the shipped set for that agent; anything wrong with it — missing, malformed,
too new, describing another agent, or containing a rule with no test at all —
leaves the shipped rules in place and says why, because the shipped rules are
better than nothing in each of those cases.

The remaining difference is remote update. They fetch rule sets; omt does not,
and should not without deciding what it means to run somebody else's matcher
against your screen.

Coverage is deliberately short. Rules for an agent nobody watched would be the
"a wrong rule is confident" failure this whole change exists to avoid.

## What a person is shown

| | them (B) | omt before | omt now |
|---|---|---|---|
| States | blocked, working, idle, unknown | the same four | the same four |
| Finished and unseen | `(Idle, seen: false)` renders as **done**, filled mark | collapsed into "idle" | **done**, filled mark |
| The word "unknown" | never shown; renders as "idle" with a dim mark | printed next to a found agent | never shown |
| Ordering | blocked 4, done 3, working 2, idle 1, unknown 0 | blocked, then working, then everything | the same five ranks, on both surfaces |
| Marks | shape *and* colour | colour only, in places | shape and colour |
| Agent panel | a persistent sidebar, 2848 lines, shared with their phone layout | a list behind a key | a bounded, bordered, counted panel behind a key |

The persistent panel is the largest thing not built, and it is not an oversight:
19 §2.1 gives omt **one row** of chrome and says so twice. A sidebar would
contradict a document whose reasoning holds — every cell of that row has to
justify its columns, and a panel that is always present has to justify a good
deal more. What the document does sanction is a modal list, so that list was
made worth looking at instead.

If a persistent panel is wanted, it is a change to 19 rather than a change
around it.

## How a frame is drawn

| | them (B) | omt before | omt now |
|---|---|---|---|
| Buffering | one buffer, diffed once | per-pane diff, with chrome and overlays written over the result | per-pane diff, whole frame buffered and written once |
| Splits | a BSP tree with ratios and directional movement | N equal shares in one direction | a BSP tree with ratios and directional movement |
| Synchronized output | DEC 2026 around every frame | none — every frame could show this frame's status bar above the last frame's pane | DEC 2026, and a frame that drew nothing writes nothing |
| Cursor | frame state, emitted by the diff | written unconditionally every tick — 619 bytes a second into an idle terminal | only when it moved |
| When to draw | only when something changed, rate-limited | every 8 ms | every 8 ms, but an unchanged frame costs no bytes |
| Cursor visibility | derived: focused, and in terminal mode, and not scrolled back | always placed in the pane | unchanged |

Single-buffer rendering is the other structural difference. It is what makes an
overlay part of the picture rather than something painted on top of it, and it
is the prerequisite for the panel above.

## What was wrong underneath all of it

Three defects, each of which made omt read the wrong thing and believe it:

1. **`tcgetpgrp` answers about the caller** in the window before a child takes
   the terminal. The probe read its own argv and environment and reported
   whatever was in them. Every agent-detection test in this project passed on
   exactly this, because the suite runs inside an agent session.
2. **A self-updating agent's name is only in `argv[0]`.** Its executable is
   `.../versions/2.1.233`; omt matched fingerprints against the executable's
   base name and consulted `argv[0]` only when the executable could not be read
   at all.
3. **Observation lived in the TUI's loop.** A session opened from a phone was
   never looked at by anything.
