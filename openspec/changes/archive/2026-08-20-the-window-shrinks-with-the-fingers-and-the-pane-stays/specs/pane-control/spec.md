# pane-control Specification

## ADDED Requirements

### Requirement: A layout follows the window it is drawn in

When the terminal's size changes, the next frame SHALL be laid out at the new
size, whatever the source of the change.

The window could move under omt without its knowledge: `SIGWINCH` reaches the
pty's foreground process group, which is the shell in the pane, not omt — so
a shrunken window kept its old geometry and drew panes nobody could see.

#### Scenario: The window shrinks

- **WHEN** the terminal shrinks while a split is on screen
- **THEN** the next frames re-lay the panes at the new size, both visible

### Requirement: A pane exists while a prompt fits

A split SHALL survive a shrink down to twelve columns per pane, and a pane
denied room shall not be drawn over its neighbour.

Forty columns — the width at which `git log --oneline` looks good — made a
sixty-column window unable to hold a split: one pane took everything and the
other, still painted, overwrote the live screen with its stale grid.

#### Scenario: No room

- **WHEN** a pane's rect comes out zero
- **THEN** it is not painted, and content on screen is left alone
