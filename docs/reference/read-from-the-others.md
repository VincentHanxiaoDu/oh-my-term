# What the other two do, feature by feature

Two comparable products were read directly — their source, not their marketing
— and every user-facing surface in them was checked against omt. This is that
checklist. It exists because the previous comparisons were written from
*architecture* (how they observe an agent, how they draw a frame) and that is
not a feature inventory: it found real defects and it could not find absences.

Neither product is named here, in this file or anywhere else in this
repository. What is borrowed is a **question** — "does omt answer this?" — and
occasionally a wire format that is already public, like an escape sequence. No
code was copied, and their names have no place in omt's history.

A verdict per row, and the ones that say *no* say why. An honest absence is a
decision; an unrecorded one is a gap nobody has counted.

## 1. Observing an agent

| Their surface | omt | Verdict |
|---|---|---|
| Hook ingress from the agent's own hook system | built | **parity** — omt's is the flagship path and reaches the ledger, the transcript and the roster |
| Structured events over the terminal (`OSC 777`, JSON behind a marker) | built | **taken** — the only structured source that needs nothing installed; at `Marker` tier, because anything in the pane can write those bytes ([06 §3.6b](../architecture/06-agent-layer.md)) |
| Terminal title as a state marker | built | **parity**, and omt asks the *state* of an already-identified agent so a title that omits the name still contributes |
| Process and environment inspection | built | **parity**, with three failure modes theirs does not handle (signed binaries, versioned paths, the foreground race) |
| The agent's own session file on disk | built | **neither of them tails it**, and the row was wrong to call omt's absence a gap against them. One reads the whole file at save points to upload it and write it back so a resume works on another machine, keeping every record opaque; the other never opens it and uses the path its hook reports purely as a resume reference. What is taken is the *discovery*: the agent says where its file is, so nothing derives a path. What is added is reading it — [06 §3.6c](../architecture/06-agent-layer.md) |
| Per-agent resume, by the agent's own session id | built | **taken** — each CLI spells it differently, so it is per-adapter and never a template |

## 2. Telling somebody

| Their surface | omt | Verdict |
|---|---|---|
| Notification raised through the terminal emulator | built | **taken**; omt adds recognized-or-silent, because a sequence an emulator does not understand is drawn as text |
| A sound on state change | built | **taken with a difference** — omt ships no audio and plays what the machine already has |
| Notification from a program *inside* a pane (`OSC 9`) | built | **taken** — the long build that finished while its window was behind a browser |
| Confirming before quitting ends running work | built | **taken**, and it found a defect: omt's key said *detach* and killed every pty ([05 §4.1b](../architecture/05-session-model.md)) |
| Push to a phone when the terminal is closed | omt's answer is a web client with its own session | **different shape** — [07 §8](../architecture/07-remote-protocol.md) |

## 3. Reading and editing

| Their surface | omt | Verdict |
|---|---|---|
| Scrollback copy mode with search | built | **taken, and now past them.** omt's coordinates are absolute rows, because screen-relative ones move under a reader. Word motions, line-wise selection, backward search and smart case came from reading theirs. **Jumping between prompts came from neither** — it needs the shell to have marked them, and one of the two never parses that at all; omt already carries the row each prompt began on |
| Named registers in copy mode | no | **deliberate, and not a gap against them.** Neither has registers in a scrollback copy mode — one has no copy mode, and the other's belong to its editor's vim mode. omt's copy goes to the system clipboard over `OSC 52` because that is what survives ssh; a second clipboard only omt can paste from is a place to lose text in |
| Mouse selection, and the mouse in general | built | **taken with their defects left out.** Both give the program in the pane priority, and one of them makes Shift the universal escape hatch, which is the convention and is what omt does. What neither does correctly: one collapses the three tracking modes so a click-only program is sent motion, drops the modifier bits so Ctrl+click arrives as a plain click, has no middle button, and forwards a right-press whose release never follows; the other translates the wheel to arrow keys for *every* alternate-screen program instead of asking whether 1007 was set, which moves an editor's cursor when somebody scrolls ([18 §5.1b](../architecture/18-semantic-open.md)) |
| Clipboard over the terminal (`OSC 52`) | built | **parity** — write only, and read refused unconditionally |
| Images a program draws in a pane | seen, reserved, and shown as a card; not yet drawn | **partial, and their scores are lower than they look.** Neither implements sixel. One of them parses the iTerm2 inline-image key only far enough to log it as unimplemented, which on screen is a silent drop. The multiplexer among them does not rasterize at all — it keeps placements per pane and re-emits kitty's protocol to the host terminal, clipped and diffed per frame, and that feature is experimental and **off by default** in their build. omt does the layer underneath first: an image is *seen* (the parser used to swallow APC with no callback at all), it reserves the rows it needs, and those rows say what arrived. Re-emitting to the host is the next step, not this one ([09 §7.2](../architecture/09-ssh-and-media.md)) |
| An editor inside the terminal | no | **not applicable** — omt hosts the programs people already use; a text editor in a multiplexer is a second editor to learn |
| Language-server integration | no | **not applicable**, same reason |
| Markdown, diff and code-block views for agent output | web client has them | **partial** — the terminal renders the transcript as lines, and the rich views are the web client's |

## 4. Running agents

| Their surface | omt | Verdict |
|---|---|---|
| Multi-agent orchestration with a tab bar | omt's is the session tree plus the agent panel | **different shape** — omt groups by workspace and orders by what needs a person, with no second navigation axis |
| Per-agent identity colouring | no | **deliberate** — theirs assigns a colour and glyph per agent from a stable hash. omt marks *state* by shape because a colour is the first thing a terminal theme takes away, and a second glyph column would make state and identity ambiguous in twenty-four columns. What tells omt's rows apart is already stable and already shown: the pane number in the bar, the workspace heading in the panel |
| Question cards answered from another surface | built for protocol agents; refused for pty agents | **deliberate** — selecting an option in a pty card means moving a highlight omt cannot see |
| Permission prompts, plan review | modelled in the ledger | **parity of model**, and the terminal shows them in the conversation view |
| Subagent trees | modelled (`ThreadRoster`) | **parity of model** |
| Running an agent in a container or a sandbox | no | **not applicable today** — omt runs what is on your machine; isolation is the agent's own concern and a much larger design |
| Cloud execution | no | **not applicable** — omt's premise is your machine, reachable from your phone |

## 5. Input

| Their surface | omt | Verdict |
|---|---|---|
| Classifying typed input as a shell command or a question for the agent | no | **not applicable** — omt has no prompt of its own. You type into the agent's own interface, so there is nothing to route |
| Vim and emacs keymaps | both built, for copy mode | **ahead, and their scores here are low.** Neither ships a preset keymap: one hardcodes a vim-ish copy mode that is not remappable at all, the other writes three bindings from a banner and keeps its vim mode inside its editor. omt documented `keymap = "vim" \| "emacs"` and read the field nowhere, which is a setting somebody sets and watches do nothing — that is what this fixed ([16 §6.8b](../architecture/16-input-and-keymap.md)) |
| Voice input | capability exists (`omt-stt`) | **parity of surface** |
| Slash commands of the agent, surfaced by the host | built | **taken, adapted**: theirs completes into their own composer as you type `/`; omt has no composer, so `⟨leader⟩ /` lists what the agent reported and *types* the choice into the agent's own interface ([19 §2.1e](../architecture/19-onboarding.md)) |
| Command history menu | built | **taken, adapted**: theirs opens on Up in their own composer and restores the buffer on dismiss; omt has no composer and Up belongs to the program in the pane, so it is `⟨leader⟩ r` — and what it offers is *every session's* history, which is the part no shell can offer. Looking for it found that nothing recorded anything, and that blocks carried no command text ([19 §2.1f](../architecture/19-onboarding.md)) |

## 6. Where omt is ahead

Recorded for the same reason as the gaps: a comparison that only lists what is
missing is a comparison nobody can act on.

- **A capability catalog every surface shares.** Both of theirs have a terminal
  and a client that drift; omt's parity gate refuses to build when they do.
- **A tier ladder with a stated weakest-first rule.** Both of theirs mix sources
  and neither says which wins when they disagree.
- **Removing a worktree counts what would be lost** — modified, staged and
  untracked, separately — where theirs asks git and reads the error.
- **The screen is tested through omt's own emulator.** Their TUI tests assert on
  view models; omt drives a real pty and reads what a terminal would.
