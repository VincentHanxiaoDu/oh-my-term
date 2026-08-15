# scrollback-reading Specification

## Purpose
TBD - created by archiving change close-the-unwired-surfaces. Update Purpose after archive.
## Requirements
### Requirement: What has scrolled away can be read and copied

omt SHALL provide a mode that shows a session's history, moves a cursor through
it, selects a span and copies it.

The bindings, the documentation and the help overlay have described this since
before there was anything underneath it.

#### Scenario: Reaching what left the screen

- **WHEN** somebody enters copy mode and goes to the top
- **THEN** output that scrolled off the screen is shown

#### Scenario: Entering

- **WHEN** copy mode is entered
- **THEN** the view starts at the end of the history, where the session is

#### Scenario: A visible way out

- **WHEN** copy mode is on screen
- **THEN** the keys that leave it are shown

#### Scenario: Keys do not fall through

- **WHEN** a key with no meaning in copy mode is pressed
- **THEN** nothing happens, and nothing reaches the program in the pane

#### Scenario: Copying

- **WHEN** a selection is yanked
- **THEN** the text goes to the clipboard through the terminal, which is the
  only one reachable over ssh
- **AND** the padding a terminal adds to every row is not included

#### Scenario: History that has aged out

- **WHEN** a selection covers rows the scrollback no longer holds
- **THEN** those rows are omitted rather than rendered as blank lines

### Requirement: The history can be searched

omt SHALL search the history from copy mode and move the cursor to a match.

Scrolling by hand through a thousand lines of an agent's output to find the one
error is the thing a scrollback exists to avoid.

#### Scenario: Finding something

- **WHEN** a query is typed and submitted
- **THEN** the cursor moves to the next line containing it, and the view
  follows

#### Scenario: Case

- **WHEN** the query differs from the text only in case
- **THEN** it still matches

#### Scenario: Finding nothing

- **WHEN** nothing in the history matches
- **THEN** the cursor does not move

#### Scenario: An empty query

- **WHEN** the search field is submitted empty
- **THEN** no search is run and the previous query is kept

