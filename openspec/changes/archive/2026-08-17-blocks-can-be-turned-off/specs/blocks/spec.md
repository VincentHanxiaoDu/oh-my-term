# blocks Specification

## ADDED Requirements

### Requirement: Blocks can be turned off

When `terminal.blocks.enabled` is `false`, no command marks SHALL open or close
a block, and output SHALL NOT open unattributed blocks.

The setting changed nothing: the tracker was always on, and the fallback that
makes an unmarked session legible is itself a block.

#### Scenario: Disabled

- **WHEN** blocks are disabled and a marked command runs
- **THEN** no block exists afterwards

#### Scenario: Enabled, the default

- **WHEN** nothing is said
- **THEN** marks work and unmarked output opens an unattributed block, as
  before
