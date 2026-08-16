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

### Requirement: Copy mode moves by word, by half a page, and to the first thing on a line

Copy mode SHALL provide word-forward, word-back and word-end motions, a motion
to the first non-blank character of a line, and half-page motions.

Reaching one argument in a long command line character by character is not
reading, it is typing. Half a page keeps the reader's place in view in a way a
whole page does not.

#### Scenario: Moving by word

- **WHEN** the word-forward motion is used
- **THEN** the cursor moves to the start of the next word on the line

#### Scenario: A word motion at the end of a line

- **WHEN** there is no next word on the line
- **THEN** the cursor stops rather than wrapping to a line the reader was not
  looking at

#### Scenario: The first thing on the line

- **WHEN** the first-non-blank motion is used on an indented line
- **THEN** the cursor lands on the first character that is not a space

### Requirement: A whole line can be selected

Copy mode SHALL offer a line-wise selection in which every row between the
anchor and the cursor is taken in full.

What somebody copies out of a scrollback is a command or a line of output far
more often than it is a span within one.

#### Scenario: Selecting lines

- **WHEN** a line-wise selection is made across several rows
- **THEN** each of those rows is copied in full, whatever column the cursor is
  in

#### Scenario: One line

- **WHEN** a line-wise selection is made and the cursor has not moved
- **THEN** that whole line is copied

### Requirement: Search runs in both directions and is smart about case

Search SHALL run backwards as well as forwards, repeats SHALL follow the
direction the search was made in, and a query without a capital letter SHALL
match either case while a query containing one SHALL match exactly.

Somebody looking for `error` wants every spelling of it. Somebody who typed
`ERROR` typed the capitals deliberately, and matching anything else means
walking them through every line that was not what they meant.

#### Scenario: Searching backwards

- **WHEN** a backward search is run
- **THEN** the cursor moves to the nearest match before it

#### Scenario: Repeating a backward search

- **WHEN** a backward search has been made and the repeat key is used
- **THEN** the cursor moves to the next match in that same direction

#### Scenario: A query with no capitals

- **WHEN** the query is typed in lower case
- **THEN** it matches text of either case

#### Scenario: A query with a capital

- **WHEN** the query contains a capital letter
- **THEN** only text with that capital matches

### Requirement: Copy mode jumps between prompts

Copy mode SHALL move the cursor to the previous or next prompt, and SHALL skip
any block for which omt never saw a command.

Scrolling through a thousand lines to find where the last command began is what
a scrollback is used for. It requires knowing where prompts are, which requires
the shell having said so — and a block opened from output alone has no prompt,
so jumping to it would land somewhere arbitrary while looking deliberate.

#### Scenario: Back to the last command

- **WHEN** the previous-prompt motion is used
- **THEN** the cursor moves to the row the previous prompt began on

#### Scenario: Nothing before it

- **WHEN** there is no earlier prompt
- **THEN** the cursor does not move

#### Scenario: A block omt never saw a command for

- **WHEN** a block was opened from output alone
- **THEN** the prompt motions do not stop at it

#### Scenario: A shell with no integration

- **WHEN** no prompts have been marked at all
- **THEN** the prompt motions move nothing, rather than guessing at a row

### Requirement: The keymap setting selects the copy-mode keys

The `keymap` setting SHALL choose which key table copy mode uses, and
`emacs` SHALL provide the emacs motion set.

The setting is documented in the configuration reference and parsed into the
binding file, and nothing has ever read it — so it is a setting somebody can
set and watch do nothing.

#### Scenario: The emacs keymap

- **WHEN** the keymap is `emacs`
- **THEN** the emacs motion, search, mark and copy keys move the cursor and
  make selections

#### Scenario: The default keymap

- **WHEN** the keymap is unset or `default`
- **THEN** copy mode behaves exactly as it did

