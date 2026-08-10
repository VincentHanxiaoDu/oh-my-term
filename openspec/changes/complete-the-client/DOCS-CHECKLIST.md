# Every documented section, and whether a test proves it

Generated from the `## ` headings of every architecture document.

**35 of 212 features ticked.** (33 further sections are prose —
a problem statement, an open question, a rationale — with nothing to build.)

A section is ticked only where a test exercises it against a real instance, a
real pty or a real browser. Never because code exists that looks like it, and
never because a capability answers: this file is the answer to how the earlier
checklists kept passing while the product did not work. They were built from the
capability catalog, and a capability passing says nothing about whether anything
is on screen. The TUI proved it — every capability reachable through the
registry, and a screen with nothing on it at all.

Unticked means not done. Where a section is partly built, it stays unticked
until a test covers it.

## 00-overview

- ~~1. The problem~~ — prose, nothing to build
- ~~2. The solution in one paragraph~~ — prose, nothing to build
- ~~3. Shape of the system~~ — prose, nothing to build
- ~~4. Domain model~~ — prose, nothing to build
- [ ] 5. The agent observation pipeline
- [ ] 6. Terminal core
- [ ] 7. Remote
- ~~8. What omt is not~~ — prose, nothing to build

## 03-capability-catalog

- [ ] 1. The idea
- [ ] 2. Declaring a capability
- [ ] 3. The machinery
- [ ] 4. Events are the read-side twin
- [x] 5. The parity contract
- [ ] 6. Capability groups (initial surface)
- [x] 7. Versioning
- ~~8. Why this is worth the machinery~~ — prose, nothing to build

## 04-terminal-core

- [ ] 1. Parser architecture
- [ ] 2. Grid and storage
- [ ] 3. Reflow on resize
- [ ] 4. Damage tracking and the render contract
- [ ] 5. Escape-sequence support matrix
- [x] 6. The block model
- [ ] 7. Shell integration
- [ ] 8. Search, selection, hyperlinks, semantic click targets
- [ ] 9. Performance
- [ ] 10. Testing
- ~~11. Open questions~~ — prose, nothing to build

## 05-session-model

- [ ] 1. The object model
- [ ] 2. Layout: the BSP tree
- [ ] 3. Focus
- [ ] 4. Attachment, detach, and multi-client viewing
- [x] 5. The writer token
- [ ] 6. Presence
- [ ] 7. Workspace identity
- [ ] 8. Persistence and restore
- [ ] 9. Command history
- [ ] 10. Capability surface
- [ ] 11. Invariants
- [ ] 12. Testing
- ~~13. Open questions~~ — prose, nothing to build

## 06-agent-layer

- [ ] 1. What this layer must deliver
- [ ] 2. The two-axis model
- [ ] 3. Source model
- [x] 4. Merging: confidence tiers, not voting
- [x] 5. Interactions — the flagship path
- [ ] 6. The heuristic floor
- [ ] 7. Adapters
- [ ] 8. Ancillary semantics
- [ ] 9. Failure modes and their handling
- ~~10. Open questions~~ — prose, nothing to build

## 07-remote-protocol

- [ ] 1. Topology and federation
- [x] 2. Transport layer
- [ ] 3. Message protocol
- [ ] 4. Terminal streaming
- [ ] 5. Resume and reliability
- [ ] 6. Backpressure
- [ ] 7. Latency budget
- [ ] 8. Notifications to a closed tab — none in v1
- ~~9. OPEN QUESTIONS~~ — prose, nothing to build

## 08-web-client

- [ ] 1. Stack
- [ ] 2. Consuming generated artifacts
- [x] 3. Multi-instance federation
- [x] 4. View modes
- [x] 5. Native rendering of agent interactions
- [x] 6. Agent session dashboard
- [ ] 7. Voice input
- [x] 8. Mobile specifics
- [ ] 9. Accessibility and theming
- [ ] 10. Testing
- ~~11. OPEN QUESTIONS~~ — prose, nothing to build

## 09-ssh-and-media

- [ ] 1. The scenarios
- [ ] 2. The blob store
- [ ] 3. Clipboard text
- [ ] 4. Getting images in: the easy cases
- [ ] 5. Case (b): image paste over SSH — the core mechanism
- [ ] 6. `omt ssh <target>` — the thin client
- [ ] 7. Image display and file transfer
- [ ] 8. Security
- ~~9. OPEN QUESTIONS~~ — prose, nothing to build

## 10-configuration

- [ ] 1. Format and file layout
- [ ] 2. The layering model
- [ ] 3. Per-instance and multi-instance configuration
- [x] 4. Typed schema and generation
- [ ] 5. Validation and diagnostics
- [ ] 6. Live reload
- [ ] 7. The settings surface
- [ ] 8. Themes and keybindings
- [ ] 9. Profiles: launch configurations and workflows
- [ ] 10. Config capabilities
- [ ] 11. Testing
- ~~12. Open questions~~ — prose, nothing to build

## 11-plugins

- [ ] 1. Two flavours, and which to build first
- [x] 2. Manifest
- [ ] 3. The plugin-facing API
- [x] 4. Permissions
- [ ] 5. Lifecycle
- [ ] 6. Distribution
- [ ] 7. Versioning and stability
- [ ] 8. Worked example: an ntfy notification sink
- ~~9. Open questions~~ — prose, nothing to build

## 12-collaboration

- [ ] 1. Actors
- [x] 2. Presence is first-class state
- [x] 3. The writer token
- [ ] 4. Interaction ownership
- [ ] 5. Ordering and causality
- [ ] 6. Conflict cases and their resolutions
- [ ] 7. Optimistic UI
- [x] 8. Audit log
- ~~9. OPEN QUESTIONS~~ — prose, nothing to build

## 13-security

- ~~1. Threat model~~ — prose, nothing to build
- [ ] 2. Bind policy
- [ ] 3. Authentication
- [ ] 4. Roles and their mapping onto the catalog
- [ ] 5. Credential storage, rotation and TLS
- [ ] 6. Browser-side controls
- [ ] 7. The two high-consequence capabilities
- [ ] 8. Secret redaction
- [ ] 9. Network egress and supply chain
- [ ] 10. Checklist — publishing an instance over Tailscale Funnel
- ~~11. OPEN QUESTIONS~~ — prose, nothing to build

## 14-licensing

- [ ] 1. omt's own licence
- [ ] 2. What was studied, and under which licences
- [ ] 3. The line we draw
- [ ] 4. Clean-room practice
- [ ] 5. `.research/` is gitignored and must never be vendored
- [ ] 6. Third-party dependency policy
- [ ] 7. Attribution and NOTICE
- [ ] 8. Contributor rule
- ~~9. Open questions~~ — prose, nothing to build

## 15-workspace-explorer

- ~~1. Scope~~ — prose, nothing to build
- [ ] 2. The crate: `omt-workspace-fs`, at L2
- [ ] 3. Traits and types
- [ ] 4. Laziness and cost
- [x] 5. Git integration
- [ ] 6. Capabilities
- [ ] 7. Surface parity
- [ ] 8. Integration with the rest of omt
- [ ] 9. Security
- [ ] 10. Configuration
- [ ] 11. Testing
- [ ] 12. What was verified in opencode, and what was not
- ~~13. OPEN QUESTIONS~~ — prose, nothing to build

## 16-input-and-keymap

- [ ] 1. The layer model
- [ ] 2. The dispatch model
- [ ] 3. The leader key
- [ ] 4. Modes and contexts
- [ ] 5. Conflict detection and resolution
- [x] 6. Modal keymaps: vim mode and emacs mode
- [ ] 7. `omt ssh` — local feels remote-native
- [ ] 8. Defaults
- [ ] 9. Accessibility and internationalization
- [ ] 10. Testing
- [ ] 11. Capabilities introduced here
- ~~12. What this document deliberately does not do~~ — prose, nothing to build
- ~~13. OPEN QUESTIONS~~ — prose, nothing to build

## 17-panes-and-layout

- [x] 1. The layout model
- [ ] 2. Geometry and resize
- [ ] 3. Multi-client, multi-size — the crux
- [x] 4. Presets, DSL, saved layouts
- [x] 5. Zoom, floats, stacks, and moving panes
- [ ] 6. Directional focus navigation
- [ ] 7. Mobile and web parity
- [ ] 8. Rendering
- [ ] 9. Capabilities
- [ ] 10. Persistence and restore
- [ ] 11. Testing
- ~~12. Open questions~~ — prose, nothing to build

## 18-semantic-open

- ~~1. Scope and placement~~ — prose, nothing to build
- [ ] 2. Recognition
- [ ] 3. Resolution
- [ ] 4. Actions and the handler registry
- [ ] 5. The conflicts
- [ ] 6. The `omt ssh` remote flow
- [ ] 7. Web and mobile parity
- [ ] 8. Security
- [ ] 9. Capabilities
- [ ] 10. Performance
- [ ] 11. Testing
- ~~12. OPEN QUESTIONS~~ — prose, nothing to build

## 19-onboarding

- [ ] 1. First run, second run, tenth run
- [ ] 2. Discoverability without documentation
- [ ] 3. `omt setup`
- [ ] 4. In-product help
- [ ] 5. tmux and zellij: migration and coexistence
- [ ] 6. Progressive disclosure of advanced features
- [ ] 7. `TERM` and terminfo — R6
- [ ] 8. Uninstall and rollback
- ~~9. Capabilities this document requires~~ — prose, nothing to build
- ~~10. OPEN QUESTIONS~~ — prose, nothing to build

## 20-recall-and-usage

- [x] 1. The recall problem, stated as questions
- [ ] 2. What is indexed, and what is not
- [ ] 3. Index design
- [ ] 4. Ranking
- [ ] 5. Redaction and the index
- [x] 6. Command history as a first-class case
- [ ] 7. Cross-instance search
- [ ] 8. Timeline and digest
- [ ] 9. Comparing parallel agent results (G18)
- [ ] 10. Detecting a stuck or looping agent (G19)
- [x] 11. Usage and cost (G15)
- [ ] 12. Capabilities
- [ ] 13. Performance budgets and backpressure
- ~~14. Open questions~~ — prose, nothing to build

## 21-data-lifecycle

- [ ] 1. The inventory
- [ ] 2. Redaction before write
- [ ] 3. Retention and compaction
- [ ] 4. Export and purge
- [ ] 5. Encryption at rest
- [ ] 6. Crash consistency
- [ ] 7. Migration of on-disk formats
- [ ] 8. Capabilities
- ~~9. OPEN QUESTIONS~~ — prose, nothing to build

## 22-operations

- [ ] 1. Daemon lifecycle
- [ ] 2. Multi-user machines
- [x] 3. `omt doctor`
- [x] 4. Health, metrics, logs and fault isolation
- [x] 5. Diagnostics panel and `omt bug-report`
- [ ] 6. Upgrade
- [ ] 7. Bootstrap onto a host with no omt (R38)
- [ ] 8. Automation and CI (G14)
- [ ] 9. Performance and resource budgets
- [ ] 10. Capabilities
- ~~11. OPEN QUESTIONS~~ — prose, nothing to build

## 23-identity-and-devices

- ~~0. The five decisions this document implements~~ — prose, nothing to build
- [x] 1. The identity model
- [ ] 2. WebAuthn and passkeys, honestly
- [ ] 3. The `home` instance role
- [ ] 4. The encrypted identity file
- [x] 5. Pairing flows
- [x] 6. Revocation and recovery
- [ ] 7. Per-action re-authentication
- [ ] 8. Device identity in the collaboration model
- [ ] 9. What the browser actually caches
- [ ] 10. Tailnet identity as an alternative front door
- ~~11. Threat model deltas vs. [13](13-security.md)~~ — prose, nothing to build
- [ ] 12. Capabilities
- ~~13. OPEN QUESTIONS~~ — prose, nothing to build

## 24-mobile

- [ ] 1. What a phone is for
- [x] 2. Screens
- [x] 3. Input
- [ ] 4. Notifications
- [x] 5. Connection
- [ ] 6. What is built and what is not
- [ ] 6.1 PWA or native
- [ ] 7. Settled: a phone is a client, not the machine
