# Every documented section, and the test that proves it

Generated from the `## ` headings of every architecture document.

**73 of 212 features ticked.** (34 further sections are prose.)

A section is ticked only when a **named test** is written beside it. The first
version of this file matched section headings by keyword and ticked "Zoom,
floats, stacks and moving panes" because the word *panes* appeared near a test
about splitting — which is exactly the dishonesty the file exists to prevent.

Never ticked because code exists that looks like it, and never because a
capability answers. That is how the earlier checklists in this change reported
sixty features complete while the TUI drew nothing but a pty grid: they were
built from the capability catalog, and every capability was reachable through
the registry while there was no screen at all.

Unticked means not done. Partly built stays unticked.

## 00-overview

- ~~1. The problem~~ — prose
- ~~2. The solution in one paragraph~~ — prose
- ~~3. Shape of the system~~ — prose
- ~~4. Domain model~~ — prose
- [ ] 5. The agent observation pipeline
- [ ] 6. Terminal core
- [ ] 7. Remote
- ~~8. What omt is not~~ — prose

## 03-capability-catalog

- [ ] 1. The idea
- [ ] 2. Declaring a capability
- [ ] 3. The machinery
- [x] 4. Events are the read-side twin — test_events_and_parity: effects declared
- [x] 5. The parity contract — test_events_and_parity: intent class per kind
- [ ] 6. Capability groups (initial surface)
- [ ] 7. Versioning
- ~~8. Why this is worth the machinery~~ — prose

## 04-terminal-core

- [ ] 1. Parser architecture
- [x] 2. Grid and storage — test_structured_surfaces: styled lines, paged from an absolute row
- [ ] 3. Reflow on resize
- [ ] 4. Damage tracking and the render contract
- [ ] 5. Escape-sequence support matrix
- [x] 6. The block model — test_structured_surfaces: block bodies, running blocks, paging
- [x] 7. Shell integration — test_structured_surfaces: whether the shell marks commands at all
- [ ] 8. Search, selection, hyperlinks, semantic click targets
- [ ] 9. Performance
- [ ] 10. Testing
- ~~11. Open questions~~ — prose

## 05-session-model

- [x] 1. The object model — test_sessions: create, write, read, close
- [x] 2. Layout: the BSP tree — test_sessions: leaves starting on first output
- [x] 3. Focus — test_tui: focus moves and the screen changes
- [x] 4. Attachment, detach, and multi-client viewing — test_watching: a second client attaches and sees it
- [ ] 5. The writer token
- [x] 6. Presence — test_watching: presence
- [ ] 7. Workspace identity
- [ ] 8. Persistence and restore
- [ ] 9. Command history
- [x] 10. Capability surface — test_sessions: the capability surface
- [ ] 11. Invariants
- [ ] 12. Testing
- ~~13. Open questions~~ — prose

## 06-agent-layer

- [ ] 1. What this layer must deliver
- [ ] 2. The two-axis model
- [x] 3. Source model — test_agent_native: messages, tools, results
- [ ] 4. Merging: confidence tiers, not voting
- [x] 5. Interactions — the flagship path — test_agents: interactions and deliverable
- [ ] 6. The heuristic floor
- [ ] 7. Adapters
- [ ] 8. Ancillary semantics
- [ ] 9. Failure modes and their handling
- ~~10. Open questions~~ — prose

## 07-remote-protocol

- [x] 1. Topology and federation — test_connection + test_joining: handshake, join
- [ ] 2. Transport layer
- [x] 3. Message protocol — test_connection: handshake, envelope, errors
- [x] 4. Terminal streaming — test_sessions: write then read the stream back
- [x] 5. Resume and reliability — test_events_and_parity: subscribe, resume, resync
- [ ] 6. Backpressure
- [ ] 7. Latency budget
- [ ] 8. Notifications to a closed tab — none in v1
- ~~9. OPEN QUESTIONS~~ — prose

## 08-web-client

- [ ] 1. Stack
- [ ] 2. Consuming generated artifacts
- [x] 3. Multi-instance federation — touch e2e: instances, unified roster
- [x] 4. View modes — touch e2e: blocks, transcript, terminal, search
- [x] 5. Native rendering of agent interactions — surfaces.test: card kinds, hold-to-approve
- [x] 6. Agent session dashboard — surfaces.test: composer, slash commands, queue
- [ ] 7. Voice input
- [x] 8. Mobile specifics — touch e2e at two device profiles
- [ ] 9. Accessibility and theming
- [ ] 10. Testing
- ~~11. OPEN QUESTIONS~~ — prose

## 09-ssh-and-media

- ~~1. The scenarios~~ — prose: a routing table naming which of §2–§7 specifies each case, not an independent feature. Its four rows are covered by the sections it points at.
- [x] 2. The blob store — test_media: content-addressed, sniffed, quota'd, TTL'd, refcounted
- [x] 3. Clipboard text — test_media: bare OSC 52, tmux passthrough keyed off TERM, refused over the cap, never claims success
- [x] 4. Getting images in: the easy cases — test_media: GPS does not survive a paste, the picture does, the digest names what was stored
- [x] 5. Case (b): image paste over SSH — the core mechanism — test_media: the tier ladder, never claimed without a handshake (§5.1; the socket forward itself needs a live ssh and is not covered)
- [x] 6. `omt ssh <target>` — the thin client — test_media: argv not a shell string, option-shaped and metacharacter targets refused, bootstrap policies
- [x] 7. Image display and file transfer — test_media: per-agent reference syntax, quoted paths, English fallback, relative refused
- [x] 8. Security — omt-media: traversal reduced to one component, both separators, reserved names, hash-prefixed, truncated
- ~~9. OPEN QUESTIONS~~ — prose

## 10-configuration

- [ ] 1. Format and file layout
- [x] 2. The layering model — test_configuration: layers and the project file
- [x] 3. Per-instance and multi-instance configuration — test_configuration: schema has default, kind, doc
- [ ] 4. Typed schema and generation
- [ ] 5. Validation and diagnostics
- [ ] 6. Live reload
- [ ] 7. The settings surface
- [ ] 8. Themes and keybindings
- [ ] 9. Profiles: launch configurations and workflows
- [x] 10. Config capabilities — test_configuration: config capabilities
- [ ] 11. Testing
- ~~12. Open questions~~ — prose

## 11-plugins

- [ ] 1. Two flavours, and which to build first
- [ ] 2. Manifest
- [x] 3. The plugin-facing API — test_extensibility: grants, high-consequence
- [ ] 4. Permissions
- [ ] 5. Lifecycle
- [ ] 6. Distribution
- [ ] 7. Versioning and stability
- [ ] 8. Worked example: an ntfy notification sink
- ~~9. Open questions~~ — prose

## 12-collaboration

- [ ] 1. Actors
- [x] 2. Presence is first-class state — test_watching: presence
- [x] 3. The writer token — test_sessions: writer token and epoch
- [ ] 4. Interaction ownership
- [ ] 5. Ordering and causality
- [ ] 6. Conflict cases and their resolutions
- [ ] 7. Optimistic UI
- [x] 8. Audit log — test_watching: audit log includes refusals
- ~~9. OPEN QUESTIONS~~ — prose

## 13-security

- ~~1. Threat model~~ — prose
- [ ] 2. Bind policy
- [x] 3. Authentication — test_joining: invite is not a credential, single use
- [ ] 4. Roles and their mapping onto the catalog
- [ ] 5. Credential storage, rotation and TLS
- [ ] 6. Browser-side controls
- [ ] 7. The two high-consequence capabilities
- [ ] 8. Secret redaction
- [ ] 9. Network egress and supply chain
- [ ] 10. Checklist — publishing an instance over Tailscale Funnel
- ~~11. OPEN QUESTIONS~~ — prose

## 14-licensing

- [x] 1. omt's own licence — test_licensing: LICENSE is Apache, every crate declares it
- [ ] 2. What was studied, and under which licences
- [ ] 3. The line we draw
- [x] 4. Clean-room practice — test_licensing: no studied product is named in shipped source or docs
- [x] 5. `.research/` is gitignored and must never be vendored — test_licensing: gitignored, nothing tracked, nothing imported; gate proven to fire on a vendored fixture
- [x] 6. Third-party dependency policy — test_licensing: 311 dependencies checked, forbidden families rejected, dual licences pass on the acceptable arm, every exception justified
- [x] 7. Attribution and NOTICE — test_licensing: NOTICE has substance, not merely presence
- [ ] 8. Contributor rule
- ~~9. Open questions~~ — prose

## 15-workspace-explorer

- ~~1. Scope~~ — prose
- [x] 2. The crate: `omt-workspace-fs`, at L2 — test_git_worktree: status, diff, hunks
- [x] 3. Traits and types — test_git_worktree: worktrees
- [x] 4. Laziness and cost — test_git_worktree: fan-out
- [x] 5. Git integration — test_git_worktree: status, diff, worktrees
- [ ] 6. Capabilities
- [ ] 7. Surface parity
- [ ] 8. Integration with the rest of omt
- [x] 9. Security — test_workspace_surfaces: a path cannot escape its workspace
- [ ] 10. Configuration
- [ ] 11. Testing
- [ ] 12. What was verified in opencode, and what was not
- ~~13. OPEN QUESTIONS~~ — prose

## 16-input-and-keymap

- [ ] 1. The layer model
- [ ] 2. The dispatch model
- [ ] 3. The leader key
- [ ] 4. Modes and contexts
- [ ] 5. Conflict detection and resolution
- [ ] 6. Modal keymaps: vim mode and emacs mode
- [x] 7. `omt ssh` — local feels remote-native — test_tui: help, prefix, Ctrl-C reaches the program
- [ ] 8. Defaults
- [ ] 9. Accessibility and internationalization
- [ ] 10. Testing
- [ ] 11. Capabilities introduced here
- ~~12. What this document deliberately does not do~~ — prose
- ~~13. OPEN QUESTIONS~~ — prose

## 17-panes-and-layout

- [x] 1. The layout model — test_tui: split, focus, close
- [ ] 2. Geometry and resize
- [ ] 3. Multi-client, multi-size — the crux
- [ ] 4. Presets, DSL, saved layouts
- [ ] 5. Zoom, floats, stacks, and moving panes
- [ ] 6. Directional focus navigation
- [ ] 7. Mobile and web parity
- [ ] 8. Rendering
- [ ] 9. Capabilities
- [ ] 10. Persistence and restore
- [ ] 11. Testing
- ~~12. Open questions~~ — prose

## 18-semantic-open

- ~~1. Scope and placement~~ — prose
- [x] 2. Recognition — test_workspace_surfaces: recognise paths and URLs
- [x] 3. Resolution — test_workspace_surfaces: names the root it used, refuses escapes, distinguishes missing from unchecked
- [x] 4. Actions and the handler registry — test_workspace_surfaces: ranked offers, line in the label, mutating actions declared without being run
- [ ] 5. The conflicts
- [ ] 6. The `omt ssh` remote flow
- [ ] 7. Web and mobile parity
- [x] 8. Security — omt-open: lexical normalisation without touching the filesystem, absolute paths outside every root refused
- [ ] 9. Capabilities
- [ ] 10. Performance
- [ ] 11. Testing
- ~~12. OPEN QUESTIONS~~ — prose

## 19-onboarding

- [x] 1. First run, second run, tenth run — test_onboarding: a first run offers one hint, later hints wait for later runs
- [x] 2. Discoverability without documentation — test_onboarding: the first thing said is how to reach omt at all
- [x] 3. `omt setup` — test_onboarding: every agent listed found or not, and whether PATH was searched
- [x] 4. In-product help — test_onboarding: topics generated from the catalog, every listed topic renders
- [x] 5. tmux and zellij: migration and coexistence — test_onboarding: mapped, review and unsupported, nothing silently dropped
- [x] 6. Progressive disclosure of advanced features — test_onboarding: nothing unprompted, three occasions earn one mention, workflows never
- [x] 7. `TERM` and terminfo — R6 — test_onboarding: TERM, terminfo, nesting, and a remedy on every disagreement
- [x] 8. Uninstall and rollback — test_onboarding: plan, accept, stale-plan refusal; unit: an edited block is commented out, not deleted
- ~~9. Capabilities this document requires~~ — prose
- ~~10. OPEN QUESTIONS~~ — prose

## 20-recall-and-usage

- [ ] 1. The recall problem, stated as questions
- [x] 2. What is indexed, and what is not — test_workspace_surfaces: recall and its danger rule
- [ ] 3. Index design
- [ ] 4. Ranking
- [ ] 5. Redaction and the index
- [ ] 6. Command history as a first-class case
- [ ] 7. Cross-instance search
- [ ] 8. Timeline and digest
- [ ] 9. Comparing parallel agent results (G18)
- [ ] 10. Detecting a stuck or looping agent (G19)
- [x] 11. Usage and cost (G15) — test_agents: usage report
- [ ] 12. Capabilities
- [ ] 13. Performance budgets and backpressure
- ~~14. Open questions~~ — prose

## 21-data-lifecycle

- [ ] 1. The inventory
- [ ] 2. Redaction before write
- [ ] 3. Retention and compaction
- [x] 4. Export and purge — test_extensibility: save and restore
- [ ] 5. Encryption at rest
- [ ] 6. Crash consistency
- [ ] 7. Migration of on-disk formats
- [ ] 8. Capabilities
- ~~9. OPEN QUESTIONS~~ — prose

## 22-operations

- [ ] 1. Daemon lifecycle
- [ ] 2. Multi-user machines
- [x] 3. `omt doctor` — test_doctor: every group, remedy on every failure
- [x] 4. Health, metrics, logs and fault isolation — test_watching: metrics
- [x] 5. Diagnostics panel and `omt bug-report` — test_doctor: bug report carries no credential
- [ ] 6. Upgrade
- [ ] 7. Bootstrap onto a host with no omt (R38)
- [ ] 8. Automation and CI (G14)
- [ ] 9. Performance and resource budgets
- [ ] 10. Capabilities
- ~~11. OPEN QUESTIONS~~ — prose

## 23-identity-and-devices

- ~~0. The five decisions this document implements~~ — prose
- [x] 1. The identity model — test_identity: devices belong to somebody
- [ ] 2. WebAuthn and passkeys, honestly
- [ ] 3. The `home` instance role
- [ ] 4. The encrypted identity file
- [x] 5. Pairing flows — test_joining: pairing
- [x] 6. Revocation and recovery — test_identity: revoke a person
- [ ] 7. Per-action re-authentication
- [ ] 8. Device identity in the collaboration model
- [ ] 9. What the browser actually caches
- [ ] 10. Tailnet identity as an alternative front door
- ~~11. Threat model deltas vs. [13](13-security.md)~~ — prose
- [ ] 12. Capabilities
- ~~13. OPEN QUESTIONS~~ — prose

## 24-mobile

- [ ] 1. What a phone is for
- [x] 2. Screens — touch e2e: every screen
- [x] 3. Input — touch e2e: keys, IME, gestures
- [ ] 4. Notifications
- [x] 5. Connection — touch e2e: join, reconnect
- [ ] 6. What is built and what is not
- [ ] 6.1 PWA or native
- [ ] 7. Settled: a phone is a client, not the machine
