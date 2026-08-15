# agent-panel Specification

## Purpose
TBD - created by archiving change a-panel-worth-the-columns. Update Purpose after archive.
## Requirements
### Requirement: A panel earns its columns or does not appear

The agent panel SHALL be off by default, and SHALL refuse to draw when the
panes it would take space from would be left too narrow to work in.

19 §2.1's rule is that every cell of chrome must justify the space it takes. A
panel is chrome that takes columns rather than rows, and the same test applies:
on a wide terminal twenty-four columns buys a list of every agent on the
machine, and on a narrow one it buys nothing worth having.

#### Scenario: A wide terminal

- **WHEN** the panel is enabled and the terminal is wide enough that every pane
  keeps a workable width
- **THEN** the panel is drawn

#### Scenario: A terminal too narrow to share

- **WHEN** drawing the panel would leave a pane below the minimum width
- **THEN** the panel is not drawn, and the panes have the whole screen

#### Scenario: Nobody asked for it

- **WHEN** the setting has not been turned on
- **THEN** there is no panel, and the screen is exactly what it was

### Requirement: The panel says what each row wants

Each row SHALL carry a mark for what it wants, its agent's name where there is
one, and where it is. Rows SHALL be ordered by attention.

#### Scenario: An agent waiting for a person

- **WHEN** an agent is blocked
- **THEN** its row is first, and marked

#### Scenario: An agent that finished while nobody looked

- **WHEN** an agent went idle and its pane has not been seen since
- **THEN** its row is above the ones still working, and says done

#### Scenario: Something omt cannot describe

- **WHEN** omt has no state for a session
- **THEN** the row is last, and does not say "unknown"

### Requirement: Both surfaces render the same entries

The entries the panel draws SHALL come from one model, shared with the web
client rather than computed again beside it.

Two surfaces building their own lists from the same data is how they came to
call one state "blocked" and "waiting on you" in the same product.

#### Scenario: The same session on two surfaces

- **WHEN** a session is listed in the TUI panel and on the web client
- **THEN** both name the same agent, the same state word, and the same order

### Requirement: The panel is toggled and remembered

Turning the panel on or off SHALL take one key, and SHALL persist.

#### Scenario: Turning it on

- **WHEN** somebody presses the toggle
- **THEN** the panel appears, and is still there in the next session

#### Scenario: Turning it off

- **WHEN** somebody presses it again
- **THEN** the panel goes, and stays gone

