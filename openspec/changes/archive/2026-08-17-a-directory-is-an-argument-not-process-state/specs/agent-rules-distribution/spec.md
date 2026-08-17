# agent-rules-distribution Specification

## ADDED Requirements

### Requirement: Where rules are loaded from is a parameter

Loading a rule set SHALL accept the directory to load from, and the environment
SHALL be consulted only where a default is needed.

An environment variable is process state: two callers that want different
values at once cannot both have one, and the loser is given a wrong answer
rather than an error. Two tests doing exactly that failed about one run in
three, and the failure read as the loader ignoring a written rule set.

#### Scenario: A caller that knows the directory

- **WHEN** a caller loads rules for an agent from a named directory
- **THEN** the rules come from that directory, whatever the environment says

#### Scenario: A caller that does not

- **WHEN** a caller asks for the rules in use without naming a directory
- **THEN** the default is used, taken from the environment as before
