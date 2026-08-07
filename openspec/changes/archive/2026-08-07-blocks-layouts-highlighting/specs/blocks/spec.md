## ADDED Requirements

### Requirement: Output with no open block still produces one

Output arriving when no block is open MUST open one rather than being dropped.
A shell that emits no OSC 133 — or emits it late, or is `ssh` on the far side
of a connection that has none — must still produce something a surface can
show, or every one of those sessions appears empty.

#### Scenario: A shell with no shell integration at all
- **WHEN** output arrives and no block has ever been opened
- **THEN** a block is opened covering that output
- **AND** it is marked as having no command attributed to it

#### Scenario: The marks arrive later
- **WHEN** an early block exists and a prompt mark then arrives
- **THEN** the early block is closed at that row
- **AND** a new block opens for the marked command

#### Scenario: A marked session is unaffected
- **WHEN** output arrives while a marked block is open
- **THEN** no early block is created

### Requirement: An unattributed block says so

A block opened from output alone MUST be distinguishable from one whose command
the shell reported. A surface offering "re-run this" must not offer it for a
block whose command omt never saw.

#### Scenario: A surface asks what it can re-run
- **WHEN** a block was opened from output with no command mark
- **THEN** it reports that it has no command
- **AND** its command text is empty rather than guessed from the screen

### Requirement: A workspace can open as a layout

A launch configuration MUST describe a recursive split of panes with the command
each runs, and opening it MUST produce that layout.

#### Scenario: A two-pane configuration
- **WHEN** a launch configuration describing two panes is opened
- **THEN** two sessions start with the commands it names
- **AND** both are in the workspace's view

#### Scenario: A configuration naming a directory that is gone
- **WHEN** a pane names a working directory that does not exist
- **THEN** opening reports which pane failed rather than silently starting it elsewhere

### Requirement: A command line is highlighted from one table

Highlighting MUST classify tokens from the same table that names commands, so
that a token highlighted as a command and a token completed as a command are
the same set.

#### Scenario: A command line with a flag and a path
- **WHEN** `cargo test --workspace crates/omt` is classified
- **THEN** `cargo` is a command, `test` a subcommand, `--workspace` a flag, and the rest an argument

#### Scenario: A quoted string containing a flag
- **WHEN** a quoted argument contains something that looks like a flag
- **THEN** it is classified as part of the string rather than as a flag
