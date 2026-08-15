# pane-control Specification

## Purpose
TBD - created by archiving change match-their-key-surface. Update Purpose after archive.
## Requirements
### Requirement: One pane can fill the screen and come back

omt SHALL be able to expand the focused pane to the whole area and restore the
arrangement exactly.

The most-used key in every multiplexer, because splitting is for comparing and
zooming is for working, and both happen in the same minute.

#### Scenario: Zooming

- **WHEN** somebody zooms a pane in a split arrangement
- **THEN** that pane has the whole area and the others are not drawn

#### Scenario: Coming back

- **WHEN** they zoom out
- **THEN** the arrangement is exactly what it was, including the ratios

#### Scenario: Zooming with one pane

- **WHEN** there is nothing to zoom over
- **THEN** nothing changes, and nothing is said

### Requirement: Focus can return to where it just was

omt SHALL be able to move focus to the previously focused pane.

Moving between two panes by direction means thinking about which direction;
this is how everybody actually moves between two things.

#### Scenario: Going back

- **WHEN** focus moves from one pane to another and then back
- **THEN** it lands on the first

#### Scenario: Nowhere to go back to

- **WHEN** focus has never moved
- **THEN** nothing changes

### Requirement: A pane can be moved rather than the focus

omt SHALL be able to swap the focused pane with its neighbour in a direction.

#### Scenario: Swapping

- **WHEN** a pane is swapped with the one to its right
- **THEN** the two exchange places and focus follows the pane

#### Scenario: Nothing in that direction

- **WHEN** there is no neighbour that way
- **THEN** the arrangement is unchanged

### Requirement: A session can be given a name

A session SHALL be renameable, and the name SHALL be what every surface shows.

A roster of six things called `shell` is what omt shows today. Detection
answers it for agents; for everything else, the answer is letting somebody say
what a thing is.

#### Scenario: Naming one

- **WHEN** a session is renamed
- **THEN** the terminal, the panel and the web client all show the new name

#### Scenario: An empty name

- **WHEN** the name given is empty
- **THEN** it is refused, rather than leaving a row with nothing in it

### Requirement: Focus can jump to the next agent that wants something

omt SHALL be able to move focus to the next session whose agent needs a person,
without opening a list.

This is the product's subject. Needing a list and a choice to reach it is two
steps for the thing omt exists to make one.

#### Scenario: Something is waiting

- **WHEN** an agent is blocked and somebody presses the key
- **THEN** focus moves to it

#### Scenario: Several are waiting

- **WHEN** the key is pressed again
- **THEN** focus moves to the next one, and wraps

#### Scenario: Nothing is waiting

- **WHEN** no agent needs anybody
- **THEN** focus does not move, and omt says why rather than appearing stuck

