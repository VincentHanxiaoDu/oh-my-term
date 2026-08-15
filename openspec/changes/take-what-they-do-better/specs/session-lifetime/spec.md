## ADDED Requirements

### Requirement: Quitting says what it is about to end

omt SHALL ask before ending a terminal that holds running sessions, and the
question SHALL list them with what each agent is doing.

The key is bound where every multiplexer binds "detach", and in omt it ends
every pty the process holds. A word that promised otherwise cost somebody an
agent mid-turn.

#### Scenario: Something is running

- **WHEN** the quit key is pressed and a session is live
- **THEN** omt does not exit
- **AND** it shows what will end, ordered by what needs a person first

#### Scenario: Answering

- **WHEN** the question is on screen and Enter is pressed
- **THEN** omt exits

The answer is handled by the list itself: an open overlay takes every key, so a
confirmation that let its answer through to the keymap could never be answered.

#### Scenario: Changing your mind

- **WHEN** the question is on screen and Esc is pressed
- **THEN** omt stays, and everything is still running

#### Scenario: Nothing to lose

- **WHEN** no session is live
- **THEN** the key exits without asking

### Requirement: The key says what it does

omt SHALL label this key `quit` for as long as quitting ends the sessions.

`detach` is a promise that the session survives, and it is the promise a
multiplexer exists to make. Making it while breaking it is worse than not
making it.

#### Scenario: The key strip

- **WHEN** the leader strip is shown
- **THEN** the label is `quit`
