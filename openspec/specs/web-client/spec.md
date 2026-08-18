# web-client Specification

## Purpose
TBD - created by archiving change web-client-rebuild. Update Purpose after archive.
## Requirements
### Requirement: A screen shows one state at a time

Every screen MUST render exactly one of loading, empty, error or content. Two
statements about the same thing on one screen means one of them is wrong, and
the user cannot tell which.

#### Scenario: A list that is still loading
- **WHEN** a screen's data has been requested and has not arrived
- **THEN** it shows a loading state
- **AND** it does not simultaneously show an empty state

#### Scenario: A list that came back empty
- **WHEN** data arrives and is empty
- **THEN** it shows one empty state saying why, not a count and a contradiction

#### Scenario: A header agrees with its body
- **WHEN** a screen reports a count in its header
- **THEN** that count is derived from what the body renders

### Requirement: A tap opens what the session is

Tapping a session MUST open the screen that matches what it is. A shell has no
agent threads, and routing it to a thread grid shows an empty screen for the one
thing the user opened it for.

#### Scenario: A plain shell
- **WHEN** a session with no agent is tapped
- **THEN** its terminal opens

#### Scenario: A session running an agent
- **WHEN** a session with agent threads is tapped
- **THEN** its threads open

### Requirement: Navigation has a way back

Every screen except the roster MUST offer a way back, and the browser's own back
gesture MUST work.

#### Scenario: A screen below the roster
- **WHEN** any screen other than the roster is showing
- **THEN** a back affordance is present

#### Scenario: The device's back gesture
- **WHEN** the user triggers the browser's back
- **THEN** the client returns to the previous screen rather than leaving the app

### Requirement: An error belongs to what caused it and clears

A failed call MUST be reported against the thing that failed and MUST clear when
that thing succeeds. Errors MUST NOT accumulate as text under unrelated content.

#### Scenario: A call fails
- **WHEN** a capability call fails for a screen
- **THEN** that screen shows the error in place of its content

#### Scenario: A retry succeeds
- **WHEN** the same call later succeeds
- **THEN** the error is gone

#### Scenario: An unrelated screen
- **WHEN** one screen's call fails
- **THEN** other screens do not show that error

### Requirement: Presentation comes from tokens

Type sizes, spacing, colour roles and elevation MUST be defined once and
referenced, so that a screen is composed rather than hand-styled and two screens
cannot disagree about what a heading is.

#### Scenario: A new screen is added
- **WHEN** a screen is built from the system's components
- **THEN** it inherits the type scale, spacing and colours without new values

#### Scenario: Touch targets
- **WHEN** any interactive element is rendered
- **THEN** it is at least 44 CSS pixels on its smallest side

### Requirement: Starting work from a client with no filesystem

The client SHALL let somebody open a project and start a session in it without
a laptop, offering only locations the instance already knows.

A path text field SHALL NOT be offered. A phone cannot browse to check a path,
cannot complete one, and learns the path was wrong only after the typing —
which is the whole cost of the feature paid before any of the benefit.

#### Scenario: The workspaces the instance already has

- **WHEN** somebody opens the project picker
- **THEN** every workspace from `workspace.list` is offered, with its name and
  its root
- **AND** each workspace's worktrees from `worktree.list` are offered under it

#### Scenario: A workspace whose directory is gone

- **WHEN** a remembered workspace's root no longer exists
- **THEN** it is shown, disabled, with the reason
- **AND** it is never silently omitted, because a list that quietly shrinks
  reads as omt having lost the workspace

#### Scenario: Starting a session takes one tap and asks nothing

- **WHEN** somebody picks a workspace
- **THEN** a session is created with the default kind and mode
- **AND** no form is shown, because the local `omt` asks neither question and a
  client that asked both would be teaching a vocabulary that exists only here

#### Scenario: Creating a workspace at an arbitrary path is refused here

- **WHEN** somebody looks for a way to open a directory omt has never seen
- **THEN** the client states that this is done by running `omt` in that
  directory
- **AND** offers no file browser rooted outside the known workspaces

### Requirement: Attachments in an agent composer

The client SHALL let somebody attach a photo, a file, or the clipboard to an
agent session, through the pipeline `09 §4.3` owns.

#### Scenario: A photo from the device

- **WHEN** somebody taps attach and chooses a photo
- **THEN** it is staged, shown before it is sent, and transferred through
  `media.put`
- **AND** the agent receives it in that agent's own reference syntax

#### Scenario: A file the workspace already has

- **WHEN** somebody chooses a workspace file rather than a device file
- **THEN** it is referenced rather than uploaded, because the instance already
  has the bytes

#### Scenario: Every attachment control is a visible control

- **WHEN** the client is used on a device with no keyboard
- **THEN** paste is reachable from a button and not only from a paste event
- **AND** no attachment path exists that a gesture alone can reach

#### Scenario: An attachment that fails says which one and why

- **WHEN** a transfer fails
- **THEN** the failing attachment is named, with the reason, and the composer's
  text is not lost

### Requirement: Files and diffs on a phone

The client SHALL render the workspace tree and the diff of a file, following
`15 §7.3` and `15 §7.4`.

#### Scenario: Browsing a workspace

- **WHEN** somebody opens the files sheet for a workspace
- **THEN** its direct children are listed from `fs.list`, one directory at a
  time
- **AND** a directory is expanded only when it is opened, because the tree of a
  real repository is not a thing to fetch whole

#### Scenario: Reading what changed

- **WHEN** somebody opens the changes view
- **THEN** the changed files are listed from `git.status`
- **AND** opening one shows its hunks from `git.hunks` with line numbers

#### Scenario: A binary or oversized file

- **WHEN** a changed file is binary or over the diff limit
- **THEN** the client says which of the two it is
- **AND** does not render an empty diff, which is indistinguishable from no
  change

#### Scenario: Reading is all the client does

- **WHEN** somebody looks for a way to stage, unstage or commit
- **THEN** there is none, per `15 §1.1`

### Requirement: Worktrees

The client SHALL list a workspace's worktrees and let somebody open a session
in one.

#### Scenario: Seeing which worktrees exist

- **WHEN** somebody opens a workspace that has worktrees
- **THEN** each is listed with its branch, from `worktree.list`

#### Scenario: Removing one with changes

- **WHEN** somebody removes a worktree that has uncommitted changes
- **THEN** the instance refuses and the client says what would be lost

### Requirement: Settings

The client SHALL render every setting the instance declares, whether or not
anyone has set it.

#### Scenario: Every setting is listed

- **WHEN** somebody opens settings
- **THEN** every key from `config.schema` is shown with its type, default and
  description
- **AND** a key nobody has set is shown at its default rather than omitted

#### Scenario: Where a value came from

- **WHEN** a value differs from its default
- **THEN** the layer it came from is named, from `config.sources`

### Requirement: Cost and context on a session's screen

The client SHALL show what an agent has spent on the session's own screen, and
SHALL NOT show it on the roster.

#### Scenario: Tokens and context

- **WHEN** an agent session is open
- **THEN** one line under the title states tokens in and out and how much of
  the context window is used

#### Scenario: A cost omt was not told

- **WHEN** the agent reported no cost
- **THEN** no currency is shown
- **AND** no price is computed, because plans differ and a wrong number is
  worse than none

#### Scenario: The roster stays quiet

- **WHEN** the roster is on screen
- **THEN** no cost appears on it, because a number changing every few seconds
  competes with the one signal the roster exists to carry

### Requirement: Language

The client SHALL render omt's own words in English or Simplified Chinese,
chosen from the browser and overridable, and SHALL NOT translate an agent's
words.

#### Scenario: Choosing without being asked

- **WHEN** the client is opened for the first time
- **THEN** the language follows the browser
- **AND** a setting can override it

#### Scenario: An agent's words are the agent's

- **WHEN** a card carries options an agent wrote
- **THEN** those options are rendered verbatim in whatever language they arrived
  in
- **AND** the controls around them are in the reader's language

#### Scenario: A missing translation is usable

- **WHEN** a phrase has no translation for the chosen language
- **THEN** the English is rendered
- **AND** the key is never rendered, because a screen of identifiers is a
  screen nobody can use

#### Scenario: A phrase with a number in it

- **WHEN** a phrase includes a count
- **THEN** the whole phrase is one template with its own plural rule
- **AND** it is not assembled by concatenating a number with a fragment

#### Scenario: Layout measured in characters

- **WHEN** text is truncated or a bar is drawn to a width
- **THEN** the measurement counts display columns, where a CJK glyph is two

### Requirement: An attachment lands outside the workspace

The client SHALL transfer an attachment through `media.put` and `media.materialize`,
and SHALL NOT write it into the workspace.

#### Scenario: A photo sent from a phone

- **WHEN** somebody attaches a photo to a session
- **THEN** the bytes are stored under their digest and written out under omt's
  managed root
- **AND** the repository's working tree is unchanged

#### Scenario: A file already in the workspace

- **WHEN** somebody attaches a file the instance already has
- **THEN** it is referenced by path and nothing is transferred

#### Scenario: The reference syntax comes from the instance

- **WHEN** the client tells an agent about a path
- **THEN** it asks `media.attach` which syntax that agent wants, and falls back
  to a bare path only when the instance cannot say

### Requirement: A refused worktree removal says what it would cost

The instance SHALL report how many files are modified, staged and untracked
when it refuses to remove a worktree, and the client SHALL render that count
before offering to force it.

#### Scenario: Removing a worktree an agent has been working in

- **WHEN** somebody asks to remove a worktree with uncommitted changes
- **THEN** the refusal names the counts and the path
- **AND** the option to remove it anyway is offered against those counts rather
  than against a generic warning

### Requirement: One directory is one project

The instance SHALL canonicalize a workspace root before deriving its id.

#### Scenario: A directory reached through a symlink

- **WHEN** a workspace is opened at `/tmp` and again at `/private/tmp`
- **THEN** both resolve to one workspace, and the project list shows it once

### Requirement: An orphaned session can be restarted or closed

The roster and the session screen SHALL offer restart and close on a session
whose process is gone, through `session.restart` and `session.close`.

05's session model already says an orphan's surface offers to restart it. No
surface did, so the only way to be rid of one was to delete the snapshot file.

#### Scenario: Restarting a session that came back from a snapshot

- **WHEN** somebody opens an orphaned session and asks to restart it
- **THEN** the same command is respawned
- **AND** the old output stays above a separator, because it is why they kept
  the session

#### Scenario: Closing one

- **WHEN** somebody closes an orphaned session
- **THEN** it leaves the roster and does not come back on the next restart

#### Scenario: A live session offers neither

- **WHEN** a session is running
- **THEN** restart is offered and close is offered, and neither is labelled as
  a repair — an orphan's controls are the same controls, not special ones

### Requirement: The roster does not bury a live session under dead ones

The roster SHALL group orphaned sessions below live ones and SHALL say how
many there are.

#### Scenario: A machine that has been restarted a few times

- **WHEN** the roster holds two running sessions and sixteen orphans
- **THEN** the two running ones are at the top
- **AND** the subtitle says how many are running and how many are orphaned,
  as separate numbers

#### Scenario: Every session is an orphan

- **WHEN** nothing is running
- **THEN** the roster says so plainly rather than showing a list that looks
  like work in progress

### Requirement: Clearing every orphan is one action that names its cost

The roster SHALL offer to close all orphaned sessions at once, and that offer
SHALL state how many it will close.

Sixteen confirmations for sixteen corpses is a control nobody uses. One
unlabelled "clear" is one somebody presses by accident and then goes looking
for the scrollback.

#### Scenario: Clearing them

- **WHEN** somebody taps the offer
- **THEN** it says how many sessions it will close before closing any
- **AND** a running session is never among them, at any count

#### Scenario: Nothing to clear

- **WHEN** there are no orphans
- **THEN** the offer is not on the screen at all, rather than present and inert

### Requirement: The theme is the configured one

`theme.get` SHALL serve the theme `appearance.theme_mode` names — the light
half for `light`, the dark half otherwise — with `appearance.theme_light`
naming the light half.

#### Scenario: Light mode

- **WHEN** `appearance.theme_mode` is `light`
- **THEN** the light theme is served

#### Scenario: Dark and auto

- **WHEN** the mode is `dark` or `auto`
- **THEN** the dark theme is served, as it always was

### Requirement: The appearance settings render

The web client SHALL apply the `appearance.*` and `font.*` settings it is
given, with the previous rendering as the default when none are present.

#### Scenario: A configured face

- **WHEN** a font family and size are configured
- **THEN** the terminal renders them

#### Scenario: Nothing configured

- **WHEN** no appearance settings resolve
- **THEN** the client renders exactly what it always did

