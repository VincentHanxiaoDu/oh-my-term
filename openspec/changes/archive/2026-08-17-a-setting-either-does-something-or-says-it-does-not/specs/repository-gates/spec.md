# repository-gates Specification

## ADDED Requirements

### Requirement: A setting has a reader or a declared reason

Every setting in the schema SHALL either be read by something, or be declared
unwired with the reason, and the build SHALL fail otherwise.

A settings screen that shows a key, validates it, records which file it came
from and then changes nothing is worse than no setting at all: it fails in the
one way nobody checks for, because it looks exactly like a setting that worked.

#### Scenario: A setting nothing reads

- **WHEN** a setting has no reader and no declaration
- **THEN** the gate fails, naming the key and both ways to answer it

#### Scenario: A declaration that has gone stale

- **WHEN** a setting declared unwired is given a reader
- **THEN** the gate fails, because a stale excuse is how the declaration stops
  meaning anything

#### Scenario: A name that collides with something else

- **WHEN** a key is spelled the same as an unrelated key elsewhere in the tree
- **THEN** that spelling alone does not count as a reader
