# terminal-streaming Specification

## ADDED Requirements

### Requirement: A bell in a pane is the configured bell

The Bell event SHALL be consumed according to `terminal.bell`.

It was emitted by the daemon and consumed by no one, so a bell did nothing
whatever the setting said.

#### Scenario: Audible

- **WHEN** the setting is `audible` or `both` and a pane rings
- **THEN** the outer terminal's bell is written

#### Scenario: Off

- **WHEN** the setting is `off`
- **THEN** nothing is written and nothing is flashed

### Requirement: A large paste is asked about

A paste that exceeds `paste_confirm_lines`, or that ends in a newline when
`paste_confirm_on_newline` is on, SHALL wait for an answer rather than reach
the program, and y SHALL hand it over with any other key dropping it.

It reached the program immediately, so a two-hundred-line pasted script ran
its first line before anything could look at it.

#### Scenario: Over the threshold

- **WHEN** a paste has more lines than the threshold
- **THEN** it is held, the hint row names it and asks, and y sends it

#### Scenario: Answered anything else

- **WHEN** the answer is not y
- **THEN** the paste is dropped and the program sees nothing

### Requirement: Blocks open only where they may

When `terminal.blocks.heuristic_fallback` is off, unmarked output SHALL NOT
open a block, and marked commands SHALL.

#### Scenario: Fallback off, marks on

- **WHEN** the fallback is off and a marked command runs
- **THEN** exactly one attributed block exists

### Requirement: Reflow is what the setting says

`reflow.on_resize` SHALL decide what a resize re-writes. `none` cuts the screen
at the new size; the other two reflow, which in this model is the same
operation — the scrollback is width-independent by construction.

#### Scenario: None

- **WHEN** the setting is `none` and a resize changes the width
- **THEN** the screen is cut and history scrolled away is gone

### Requirement: A new pane starts where the setting says

`terminal.default_cwd` SHALL choose the directory a new pane starts in.

#### Scenario: Home

- **WHEN** the setting is `home`
- **THEN** the pane starts in `$HOME`

#### Scenario: Workspace, the default

- **WHEN** the setting is `workspace`
- **THEN** the pane starts in the directory omt was started in
