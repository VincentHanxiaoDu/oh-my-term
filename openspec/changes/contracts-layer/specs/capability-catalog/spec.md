## Purpose

Declaring an omt operation once and deriving every surface from that single
declaration — in-process dispatch, HTTP/WebSocket routes, JSON Schema, the
TypeScript client, the CLI tree and the reference docs — so that the native TUI,
the API and the web client cannot drift apart without failing the build.

## ADDED Requirements

### Requirement: A capability is declared once

Every operation an actor can perform on an instance SHALL be declared exactly
once, carrying a stable dotted name, a group and verb, its kind
(command or query), the minimum role, its declared effects, its input and output
types, and — for commands — its intent class.

#### Scenario: A declaration yields every derived artifact

- **WHEN** a capability is declared and `cargo xtask codegen` runs
- **THEN** a JSON Schema for its input and output, a TypeScript client entry, an
  HTTP route, a CLI subcommand and a reference-documentation entry are all
  produced from that one declaration
- **AND** no derived artifact is hand-written

#### Scenario: A command without an intent class fails the build

- **WHEN** a capability of kind command is declared without an intent class
- **THEN** the build fails, naming the capability

#### Scenario: Duplicate names are rejected at startup

- **WHEN** two declarations share a dotted name
- **THEN** the registry refuses to start and reports the collision
- **AND** neither declaration is silently shadowed

### Requirement: Codegen reflects what the binary actually contains

Generated artifacts SHALL be derived from the set of declarations present in the
linked binary, not from a source scan or a hand-maintained list, so that a
declaration that fails to link is impossible to miss.

#### Scenario: Generated output is deterministic

- **WHEN** codegen runs twice against an unchanged binary
- **THEN** the output is byte-for-byte identical
- **AND** the ordering does not depend on link order

#### Scenario: CI rejects stale generated files

- **WHEN** a declaration changes and the committed generated artifacts are not
  regenerated
- **THEN** CI fails and reports which artifact is stale

### Requirement: Dispatch is the single authorization point

All capability invocations — from the in-process TUI, the local CLI socket and
remote clients alike — SHALL pass through one dispatch path that applies the
role check before invoking a handler.

#### Scenario: A remote caller below the required role is refused

- **WHEN** a client whose credential maps to a role below the capability's
  required role invokes it
- **THEN** dispatch returns `unauthorized` and the handler is never invoked

#### Scenario: The local TUI is subject to the same check

- **WHEN** the TUI invokes a capability in-process
- **THEN** the same dispatch path and the same role check apply

#### Scenario: A declared capability with no handler is a startup failure

- **WHEN** the registry is sealed and any declared capability has no registered
  handler
- **THEN** startup fails, naming the capability
- **AND** the gap is found at boot rather than by a caller at runtime

### Requirement: Errors are a closed, discriminating set

Capability failures SHALL use a closed set of stable codes, each able to carry
structured detail sufficient for a caller to distinguish causes that require
different responses.

#### Scenario: A losing caller can tell why it lost

- **WHEN** a call fails with `conflict`
- **THEN** the error carries detail distinguishing "already resolved by another
  actor" from "the target was withdrawn" from "the target timed out"

### Requirement: Retry safety is declared, not inferred

Each command SHALL declare which intent class it belongs to, and dispatch SHALL
enforce the retry semantics of that class rather than leaving them to each
transport or client.

#### Scenario: A repeated request for an idempotent command replays its result

- **WHEN** a client re-sends a command carrying the same request identity after
  a lost acknowledgement
- **THEN** the original result is returned and the command is not applied twice

#### Scenario: A never-retry command refuses a repeat

- **WHEN** a repeat arrives for a command whose intent class forbids retry
- **THEN** dispatch refuses it rather than re-applying it

### Requirement: Parity is enforced for every capability

For every declared capability the build SHALL verify that a route and schema
exist, that it is **reachable** in the TUI, that the web client has a handler,
and that it appears in the generated reference — or that it carries an explicit,
listed exemption for the surfaces it omits.

TUI reachability is satisfied by palette membership **or** by an explicit
binding. It is deliberately not "a binding exists": the palette is the universal
TUI affordance, so requiring a per-capability chord would assert a guarantee the
system does not keep.

#### Scenario: A capability missing a surface fails CI

- **WHEN** a capability is declared and any required surface is absent
- **THEN** the parity check fails and names both the capability and the missing
  surface

#### Scenario: Administrative capabilities are not required on every surface

- **WHEN** a capability requires the administrative role
- **THEN** it is not required to be reachable in the TUI

#### Scenario: A capability hidden from the palette needs a real binding

- **WHEN** a capability is marked hidden, and is therefore absent from the
  palette
- **THEN** palette membership cannot satisfy its TUI reachability
- **AND** it must carry either an explicit binding or an exemption naming the
  TUI surface

#### Scenario: A binding may name a capability or a client-local action

- **WHEN** the reverse direction runs over the keymap
- **THEN** a bound name resolves against the declared capabilities *and* the
  client-local action registry
- **AND** a name in neither fails the build

#### Scenario: Declaration soundness is checked with parity

- **WHEN** the check runs
- **THEN** it also verifies that every command declares an intent class and that
  no capability's refined effects exceed its declared effects

#### Scenario: Exemptions are visible and enumerated

- **WHEN** a capability declares a parity exemption
- **THEN** the exemption names the specific surfaces it covers rather than
  waiving the check wholesale
- **AND** it appears in the generated reference with its stated reason
- **AND** it matches an explicit allow-list, so an exemption cannot be added
  silently

#### Scenario: A new capability breaks the web build until handled

- **WHEN** a capability is added and the web client has no corresponding handler
- **THEN** the web client fails to type-check

### Requirement: Capability names are stable across versions

A capability's dotted name SHALL be treated as a compatibility surface: it is
not renamed without an alias period, inputs gain only optional fields, and
outputs may gain fields that clients ignore when unknown.

#### Scenario: A client negotiates against an older instance

- **WHEN** a client connects to an instance whose catalog lacks a capability the
  client knows
- **THEN** the client learns which capabilities exist during the handshake and
  presents the intersection rather than failing
