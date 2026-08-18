# observability Specification

## ADDED Requirements

### Requirement: The log settings log

omt SHALL admit `log.level`'s lines to `log.file` when set, rotating at
`log.max_size`, and SHALL redact secret-shaped values when
`log.redact_secrets` is on.

The four keys were in the reference and the settings screen, and nothing
logged anything.

#### Scenario: A file is set

- **WHEN** `log.file` names a path and a line is admitted
- **THEN** the line lands in that file

#### Scenario: A level gates

- **WHEN** the level is `error`
- **THEN** an info line is written nowhere

#### Scenario: Redaction

- **WHEN** redaction is on and a line holds `token=abcdef1234567890`
- **THEN** the value is replaced and the shape survives

#### Scenario: Rotation

- **WHEN** the file passes `log.max_size`
- **THEN** the old file is rotated rather than grown past the cap
