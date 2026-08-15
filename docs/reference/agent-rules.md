# Writing agent rules

omt decides what an agent is doing from four places, strongest first: a
protocol it speaks, a hook it calls, a **marker** it sets, and a floor that
watches bytes arrive. This file is about the marker level's screen half — the
rules that read what an agent drew.

You need this when an agent moves a line, renames a prompt, or adds a step, and
omt starts reporting the wrong state. A release of omt is a bad unit of
response to somebody else's release, so the rules are a file you can edit.

## Where they go

```
$XDG_CONFIG_HOME/omt/agent-rules/<agent>.toml
~/.config/omt/agent-rules/<agent>.toml
```

One file per agent, named for it. `OMT_AGENT_RULES` points somewhere else.

A file replaces the rules omt ships for that agent — it does not add to them.
Copy the shipped set as a starting point if you only mean to change one rule.

## What a rule looks like

```toml
id = "claudecode"
version = "2026.08.15"
min_engine = 1

[[rules]]
id = "a question below the last rule"
state = "blocked"
priority = 900
region = "after_last_rule"
contains = ["esc to cancel"]

  [[rules.any]]
  contains = ["enter to select"]

  [[rules.any]]
  contains = ["enter to confirm"]

  [[rules.not]]
  contains = ["showing detailed transcript"]
```

The highest-priority rule that matches decides. `contains` is
case-insensitive and every string in it must appear. `any` holds alternatives;
`not` holds things that must be absent. `line_starts_with` matches the start of
some line, ignoring leading spaces.

## Regions, and why they are the whole thing

A rule looks at one region. This is not an optimisation — it is what separates
"this agent is asking a question **now**" from "the word *yes* appears
somewhere on the screen".

| Region | What it is |
|---|---|
| `after_last_rule` | everything below the last horizontal rule the agent drew, which is where agent CLIs put the thing they want an answer to |
| `prompt_box` | between the two rules an input box is drawn with |
| `bottom_non_empty(n)` | the last *n* lines with anything on them; blank lines are skipped so padding cannot push a status line out of view |
| `whole` | the visible screen, and the weakest — it is what makes a rule fire on scrollback |

`whole` is the default and usually the wrong choice. A rule with no region is a
rule that fires on a diff, on a file the agent is displaying, and on the
question that was answered ten minutes ago.

## States

| `state` | Means |
|---|---|
| `working` | it is doing something |
| `blocked` | it is waiting for a person |
| `idle` | it is waiting for something to do |
| `hold` | **this screen says nothing about now** |

`hold` is for an agent showing its own history: yesterday's question is on the
screen and is not being asked. Without a `hold` rule, a transcript viewer reads
as a live prompt and the agent is reported as blocked until somebody closes it.

A rule that does not match is different again: it means this screen is not one
your rules describe, and the floor decides.

## What omt refuses

Anything wrong leaves the rules omt ships in place and prints why. It never
leaves you with no rules, because the shipped ones are better than nothing in
every one of these cases:

- the file is missing, or is not valid TOML
- it declares a `min_engine` newer than this build
- its `id` is not the agent the filename names — the mistake you make copying a
  file to start a new one, which would apply one agent's rules to another
- a rule names a region or a state that does not exist
- **a rule has no test at all**, which would match every screen and report its
  state forever. This is the one mistake you cannot notice from the output,
  because the output is always confident.
- a field is misspelled. A typo that was ignored would be a rule quietly not
  doing what it says.

## What rules may not do

They decide *state*. They never produce content.

There is no field that extracts text, and there will not be one. A screen rule
knows somebody is wanted and never what for — a blocked verdict from here
carries no answerable card, because building one out of matched text is how a
change of wording, or of language, becomes a wrong "Allow" in front of
somebody.

## Getting the wording right

Write rules against a screen you captured, not one you remember. omt's own rule
for Claude Code's permission prompt was written from memory and was wrong about
the first screen anybody sees: the first-run trust prompt says *Enter to
confirm* where every later prompt says *Enter to select*. A captured screen
found it in one run.
