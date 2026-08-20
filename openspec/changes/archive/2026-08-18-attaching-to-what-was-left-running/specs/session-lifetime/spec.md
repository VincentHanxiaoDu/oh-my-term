# session-lifetime Specification

## ADDED Requirements

### Requirement: An orphan accepts a resize

`session.resize` SHALL accept a session restored from a snapshot as if it had
a pty, because an attach's first action is a resize and the orphan is the
session an attach exists to come back to.

#### Scenario: Resizing an orphan

- **WHEN** an attach resizes a session that is an orphan
- **THEN** the resize is accepted, and the attach proceeds

#### Scenario: Writing to an orphan

- **WHEN** somebody types into an orphan
- **THEN** the write is refused, naming the reason
