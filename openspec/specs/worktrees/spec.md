# worktrees Specification

## Purpose
TBD - created by archiving change git-worktrees-and-diff. Update Purpose after archive.
## Requirements
### Requirement: A worktree is a real git worktree and becomes a workspace

Adding a worktree MUST create one through git, and the result MUST be usable as
a workspace without any special case.

#### Scenario: A worktree is added
- **WHEN** a worktree is added for a branch
- **THEN** a git worktree exists at that path
- **AND** opening it as a workspace yields the id its canonical path derives

#### Scenario: A branch that already exists
- **WHEN** a worktree is added for a branch that exists
- **THEN** it checks that branch out rather than failing

#### Scenario: A path that is already a worktree
- **WHEN** a worktree is added at a path that already holds one
- **THEN** it is refused with what is there, rather than overwriting it

### Requirement: Removing a worktree is explicit and refuses to lose work

Removing a worktree MUST NOT discard uncommitted changes unless the caller says
so, and MUST say what would be lost.

#### Scenario: A clean worktree
- **WHEN** a worktree with no changes is removed
- **THEN** it is removed

#### Scenario: A worktree with uncommitted changes
- **WHEN** a worktree with changes is removed without force
- **THEN** it is refused, naming that there are uncommitted changes

### Requirement: The diff of a file is available, not only its name

omt MUST be able to report the hunks of a changed file — the lines, not only
that the file changed.

#### Scenario: A modified file
- **WHEN** hunks are asked for a file with changes
- **THEN** the added and removed lines are reported with their positions

#### Scenario: A file with no changes
- **WHEN** hunks are asked for an unchanged file
- **THEN** it reports none rather than failing

#### Scenario: Staged and unstaged are separate
- **WHEN** hunks are asked for staged changes
- **THEN** unstaged changes to the same file are not included

### Requirement: A fan-out runs one prompt across several worktrees

Starting a fan-out MUST create a worktree per agent and report every arm,
including any that failed to start.

#### Scenario: Three agents, one prompt
- **WHEN** a fan-out is started for three agents
- **THEN** three worktrees exist, one per agent
- **AND** every arm is reported with its branch and worktree

#### Scenario: One agent cannot start
- **WHEN** one arm fails to start
- **THEN** that arm is reported as failed with the reason
- **AND** the others are unaffected

### Requirement: Choosing an arm records the choice and merges nothing

Choosing MUST record which arm won and MUST NOT modify any branch.

#### Scenario: An arm is chosen
- **WHEN** an arm is chosen
- **THEN** the fan-out reports it as chosen, naming its branch
- **AND** no branch has been merged, committed to, or checked out

