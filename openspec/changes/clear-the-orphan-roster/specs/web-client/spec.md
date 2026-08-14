## MODIFIED Requirements

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
