# Every documented section, and the test that proves it

Generated from the `## ` headings of every architecture document.

**108 of 212 features ticked.** (34 further sections are prose.)

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

- [x] 1. The layer model — omt-input layers: reserved OS and terminal chords audited against omt's defaults in every mode, and the audit proven to fire
- [x] 2. The dispatch model — omt-input dispatch: original bytes pass through unmodified, undecodable sequences are never claimed, leader-leader sends the leader on
- [x] 3. The leader key — test_tui: the leader opens a pending sequence, pressing it twice sends it on, the cheatsheet names it
- [x] 4. Modes and contexts — omt-input modal: the governed modes are named, not implied; a binding outside them is refused
- [x] 5. Conflict detection and resolution — test_tui: names the program and what the key does there, only critical refuses, spelling-independent, every entry version-stamped
- [x] 6. Modal keymaps: vim mode and emacs mode — test_tui: vim motions in copy mode, never in a passing-through pane; emacs is an alternative not a layer
- [x] 7. `omt ssh` — local feels remote-native — test_tui: help, prefix, Ctrl-C reaches the program
- [x] 8. Defaults — test_tui: keys.audit — omt's defaults take nothing critical, and the audit reports how many programs it checked against
- [x] 9. Accessibility and internationalization — omt-input layout: matches on base-layout key or produced character, says which; defaults avoid AltGr or carry an alias
- [x] 10. Testing — omt-input: 99 tests across key, keymap, dispatch, modal, layout, layers and conflicts; every audit paired with a test that it fires
- [x] 11. Capabilities introduced here — test_tui: keys.list, keys.explain, keys.registry, keys.conflicts, keys.audit, keys.flavour, keys.cheatsheet (keys.capture and keys.probe need a live terminal and are not built)
- ~~12. What this document deliberately does not do~~ — prose
- ~~13. OPEN QUESTIONS~~ — prose

## 17-panes-and-layout

- [x] 1. The layout model — test_tui: split, focus, close
- [x] 2. Geometry and resize — omt-tui geometry: no gaps at any ratio, no overlaps, index stable across resize, absurd ratios clamped
- [x] 3. Multi-client, multi-size — the crux — omt-tui geometry: a pane below its minimum is dropped and named, not drawn as an unreadable sliver
- [x] 4. Presets, DSL, saved layouts — omt-tui geometry: even presets give equal shares, the main pane keeps its share as others are added, no panes is not a layout
- [x] 5. Zoom, floats, stacks, and moving panes — omt-tui geometry: zoom leaves the tree untouched, floats clamp into the screen, a stack occupies one slot and reflows nothing, moving is a swap
- [x] 6. Directional focus navigation — test_workspace_surfaces: geometric over placed rects, hidden panes skipped, deterministic, no wrap by default
- [x] 7. Mobile and web parity — omt-tui geometry: a narrow screen shows one readable pane rather than nothing, and the same tree gives different answers per client
- [x] 8. Rendering — omt-tui geometry: dividers derived from placements, spanning only where panes meet, grabbable with slop; edges known for border drawing
- [x] 9. Capabilities — test_workspace_surfaces: pane.navigate, layout.save, layout.restore
- [x] 10. Persistence and restore — test_workspace_surfaces: saved layouts hold no rects, restore keeps what survives and names the rest, a phone-sized restore drops what will not fit
- [x] 11. Testing — omt-tui: 47 geometry tests — no gaps, no overlaps, stable indices, degradation, presets, zoom, floats, stacks, swaps, save and restore
- ~~12. Open questions~~ — prose

## 18-semantic-open

- ~~1. Scope and placement~~ — prose
- [x] 2. Recognition — test_workspace_surfaces: recognise paths and URLs
- [x] 3. Resolution — test_workspace_surfaces: names the root it used, refuses escapes, distinguishes missing from unchecked
- [x] 4. Actions and the handler registry — test_workspace_surfaces: ranked offers, line in the label, mutating actions declared without being run
- [x] 5. The conflicts — test_workspace_surfaces: a plain click inside a mouse-reporting app is forwarded, shift is the one omt claims, alt is not
- [x] 6. The `omt ssh` remote flow — omt-media mirror layout (basename verbatim, path-keyed, hostile components reduced) + test_workspace_surfaces: list and discard
- [x] 7. Web and mobile parity — web/test/targets.test.ts: 44px hit areas centred on the row, width not padded, overlapping taps disambiguated rather than guessed
- [x] 8. Security — omt-open: lexical normalisation without touching the filesystem, absolute paths outside every root refused
- [x] 9. Capabilities — test_workspace_surfaces: open.resolve, open.rules_list, open.rules_test; every listed rule proven to fire (open.remote.* is §6, not covered)
- [x] 10. Performance — omt-open: byte prefilter proven never to skip a matching line; a screenful under 8ms; a 16k line bounded
- [x] 11. Testing — crates/omt-open/tests/corpus.rs over tests/corpus/open/: 28 cases, negatives outnumber positives, hostile file, reflow stability
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

- [x] 1. The recall problem, stated as questions — omt-recall policy: every question in the table is answerable under the default settings, and turning output indexing off costs one nameable question
- [x] 2. What is indexed, and what is not — test_workspace_surfaces: every source has a stated treatment, reasoning excluded until opted in, tool fields are an allow list, output windowed not truncated
- [ ] 3. Index design
- [x] 4. Ranking — test_workspace_surfaces: every factor reasoned, the factors multiply to the score, a huge text match cannot swamp the rest, the order is total
- [x] 5. Redaction and the index — omt-recall redact: D-R2 enforced by the type (no constructor takes a string), the screen keeps the truth, ordinary output untouched
- [x] 6. Command history as a first-class case — omt-recall: history dedupes so a habit is one row, dangerous commands never suggested, recency decays over a week
- [x] 7. Cross-instance search — omt-recall federate: one row naming both instances, scores summed not maxed, an unreachable instance is a gap not an absence
- [x] 8. Timeline and digest — test_agents: consecutive repeats collapse with a count, two different calls never do, stats count events not lines, digest is facts not prose
- [x] 9. Comparing parallel agent results (G18) — test_agents: nothing ranked or recommended, both directions of file disagreement flagged, different bases called incomparable, unreported usage is not zero
- [x] 10. Detecting a stuck or looping agent (G19) — test_agents: every reason reported not just the first, baselines per binding so a slow agent is not a permanent alarm, a first turn is not evidence
- [x] 11. Usage and cost (G15) — test_agents: usage report
- [ ] 12. Capabilities
- [x] 13. Performance budgets and backpressure — omt-recall backpressure: offering never blocks, bulk shed before commands, equal value does not churn, every drop counted and reported
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
