## Purpose

How an omt instance and a client talk to each other — framing, handshake and
capability negotiation, capability calls, event subscription and resume,
terminal byte streams, and the local ingress through which an agent's own hook
reports what it saw.

## ADDED Requirements

### Requirement: A connection states what it can do before it is used

Every connection SHALL begin with a handshake in which the parties exchange
protocol version, feature support and the instance's capability set, so that a
client knows what is available rather than discovering it by failing.

#### Scenario: A client and instance on different versions still work

- **WHEN** a client connects to an instance whose protocol version or capability
  set differs from its own
- **THEN** they agree on a common version and the client learns the instance's
  capabilities
- **AND** the client presents what is supported rather than erroring

#### Scenario: An incompatible peer is refused with a reason

- **WHEN** no common protocol version exists
- **THEN** the connection is refused with a message naming the versions each side
  supports

### Requirement: Authorization happens before any state is reachable

A connection SHALL be authenticated and mapped to a role before it can invoke a
capability or subscribe to events.

#### Scenario: An unauthenticated connection reaches nothing

- **WHEN** a client attempts a capability call or a subscription before
  authenticating
- **THEN** the request is refused and no state is disclosed

#### Scenario: A local peer's established identity is carried, not re-asserted

- **WHEN** a connection arrives over the local socket
- **THEN** the protocol carries the peer identity the transport established, and
  provides no way for a message to claim an identity of its own

### Requirement: Requests are identified stably across reconnection

A request identity SHALL remain valid across a dropped and re-established
connection, so that a client which loses an acknowledgement can determine
whether its request was applied.

#### Scenario: A client learns the outcome of an in-flight call

- **WHEN** a connection drops after a command was sent but before its result
  arrived, and the client reconnects and repeats it
- **THEN** the client receives the original outcome rather than applying the
  command a second time

### Requirement: Subscriptions can be filtered and resumed

A client SHALL be able to subscribe to a subset of events by scope and kind, and
to resume a subscription from a stated position.

#### Scenario: A phone avoids traffic it cannot use

- **WHEN** a client subscribes with a filter
- **THEN** it receives only matching events

#### Scenario: Resume beyond the retained window is handled explicitly

- **WHEN** a client resumes from a position the instance no longer retains
- **THEN** the protocol encodes a resynchronize directive carrying a reference to
  the snapshot that replaces the gap and a count of what was missed
- **AND** there is no encoding by which a resume silently returns less than was
  asked for

### Requirement: A slow consumer degrades visibly, never silently

When a subscriber cannot keep up, the instance SHALL apply a stated policy that
never silently drops an event a client believes it received.

#### Scenario: A client is told when it fell behind, and by how much

- **WHEN** a subscriber falls behind and the instance sheds or collapses what it
  had buffered
- **THEN** the protocol carries a notice naming what class of data was affected
  and how much
- **AND** there is no encoding in which data is dropped without such a notice

#### Scenario: There is an encoding for giving up on a subscriber

- **WHEN** a subscriber remains unable to keep up beyond the policy's limit
- **THEN** the protocol encodes a close that carries a stated reason
- **AND** no encoding exists for abandoning a subscriber silently

### Requirement: Terminal bytes carry ordering and consumption information

The terminal byte stream SHALL be framed with a sequence position wide enough
not to wrap in any reachable regime, and SHALL carry the highest client input
position the instance has consumed.

#### Scenario: A long-lived busy session never wraps its counter

- **WHEN** a session produces output continuously at high rate for an extended
  period
- **THEN** the sequence position does not wrap
- **AND** a resume after that period rejoins at the correct point

#### Scenario: A client can tell what the instance has consumed of its input

- **WHEN** a client sends input and receives subsequent output frames
- **THEN** those frames report how much of the client's input has been consumed

### Requirement: Client input is never blindly replayed

Raw terminal input SHALL NOT be re-sent on reconnection; a client that loses a
connection mid-input SHALL fail loudly rather than risk applying a fragment
twice.

#### Scenario: Terminal input has no resend encoding

- **WHEN** a connection is re-established
- **THEN** the protocol offers no message by which previously-sent terminal input
  can be replayed
- **AND** a client that lost input therefore cannot silently re-apply it

### Requirement: An agent's hook reports over a local ingress

The protocol SHALL define messages by which an agent's own hook reports an
observation to the instance, carrying the agent's payload verbatim, and SHALL
encode a directive back to the hook that is safe to act on when the instance is
slow, unreachable or absent. The hook binary's own behaviour is delivered by the
change that builds it; what is specified here is the encoding it depends on.

#### Scenario: The report carries the agent's payload unmodified

- **WHEN** a hook report is encoded
- **THEN** it carries the agent's own payload byte-for-byte, alongside the event
  name the agent used, rather than a normalized rendering of either
- **AND** a payload too large to carry is marked as truncated rather than
  silently shortened

#### Scenario: The safe directive requires no reply to construct

- **WHEN** the instance is unreachable, or does not answer within the budget the
  request carries
- **THEN** the directive that lets the agent proceed unchanged is one the hook can
  produce without having received anything
- **AND** no encoding requires a round trip in order to fail safely

#### Scenario: An unrecognized hook event is recorded, not discarded

- **WHEN** a hook reports an event name the instance does not recognize
- **THEN** the raw event is retained rather than dropped, so it is diagnosable

#### Scenario: The hook is not a privileged actor

- **WHEN** a hook reports an observation
- **THEN** it does so as an observation and cannot invoke capabilities or mutate
  state on an actor's behalf
