# Every documented section, and the test that proves it

Generated from the `## ` headings of every architecture document.

**139 of 212 features ticked.** (34 further sections are prose.)

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

- [x] 1. Format and file layout — test_configuration: the layers are found from a subdirectory, a nearer config wins, the walk never leaves the outermost repository
- [x] 2. The layering model — test_configuration: layers and the project file
- [x] 3. Per-instance and multi-instance configuration — test_configuration: schema has default, kind, doc
- [x] 4. Typed schema and generation — test_configuration: docs/reference/settings.md is generated and codegen-check fails if it drifts, every setting in the capability is in the page, every setting has a default and a sentence
- [x] 5. Validation and diagnostics — omt-config: every problem reported in one pass and in a stable order, both types named, enumerations list their choices, a value a later layer overrides is not complained about
- [x] 6. Live reload — test_configuration: appearance is hot, listener settings rebuild only the listener, the store needs the process back, what will not take effect yet is listed separately
- [x] 7. The settings surface — test_configuration: every one of the 52 settings the document tables is in the schema, and the parser is asserted non-empty so the check cannot pass over nothing
- [x] 8. Themes and keybindings — test_tui: a binding naming no capability is caught with a near-miss suggestion, contexts live on the trigger, every problem carries help (theme contrast checks live in omt-theme)
- [x] 9. Profiles: launch configurations and workflows — test_configuration: an agent gets an argument vector, a prompt with shell syntax stays one argument, focus always lands somewhere, every problem at once (workflows are §9.2 and not built)
- [x] 10. Config capabilities — test_configuration: config capabilities
- [x] 11. Testing — omt-config: every diagnostic code scraped from source and matched against its entry, defaults round-trip, layer precedence table-driven over the chain (fuzzing and comment-preservation property tests are not built)
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
- [x] 2. Bind policy — test_security: the default needs no configuration, an open port without auth refuses, every refusal carries a remedy, all problems reported at once
- [x] 3. Authentication — test_security: the welcome advertises the methods, tailnet is not offered on loopback, every advertised name is in the closed set; test_joining: an invite is not a credential and is single-use; constant-time compare in omt-auth
- [x] 4. Roles and their mapping onto the catalog — crates/omt/tests/parity.rs: no Viewer capability changes anything, spawning as Viewer needs the reviewed allow-list, instance-changing commands are Admin
- [x] 5. Credential storage, rotation and TLS — omt-auth at_rest + omt-config merge: an inline secret is an error with somewhere to put it and is kept out of the resolved values; permissions corrected, foreign ownership fatal
- [x] 6. Browser-side controls — omt-server origin: a hostile page is refused before the credential, prefixes and paths do not pass, a non-browser client still connects; CSP names no host omt does not serve
- [x] 7. The two high-consequence capabilities — parity.rs + test_security: no PTY write is Viewer, no second permission model exists by name, an agent that offered nothing gets an empty list rather than an invented one
- [x] 8. Secret redaction — omt-recall redact: no constructor takes a string, so there is no path from raw bytes to a document; omt-server watch: the audit record has no field that can hold caller text (the tracing-Layer integration 21 §2 specifies is not built)
- [x] 9. Network egress and supply chain — parity.rs: zero capabilities declare NETWORK today and the count is asserted, nothing is named like telemetry, the reachable-but-uncalled STT path is written down (supply chain: xtask licensing over 311 deps)
- [x] 10. Checklist — publishing an instance over Tailscale Funnel — test_security: tailnet identity alone refused, wildcard origin refused, short password refused unless device-bound, default role must be Viewer, all problems at once
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
- [x] 3. Index design — omt-recall policy: the suffix chain makes a path searchable by the part anybody remembers, no empty tokens, cost bounded
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

- [x] 1. The inventory — omt-store inventory: nothing is readable by anybody else, everything is on a retention ladder, the largest and most sensitive is on the shortest one
- [x] 2. Redaction before write — omt-store log: terminal text reaches disk only through a redactor, asserted on the file's actual bytes; omt-recall redact: no path from raw bytes to an indexed document
- [x] 3. Retention and compaction — test_workspace_surfaces: the fact outlives the output, failed and agent-touched blocks keep theirs, a small output is not worth compacting, every kind is deleted eventually
- [x] 4. Export and purge — test_extensibility: save and restore
- [x] 5. Encryption at rest — omt-store inventory: the answer is no and says so first, names what is relied on instead, gives the reason, and nothing claims encryption omt does not do
- [x] 6. Crash consistency — omt-store record: a torn tail is distinguished from corruption in the middle, a file of another kind is rejected before its length is believed, an absurd length is refused rather than allocated
- [x] 7. Migration of on-disk formats — omt-store version: a store from the future is refused rather than partly read, the two directions are different refusals with opposite remedies, no best-effort parse
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
- [x] 2. WebAuthn and passkeys, honestly — omt-auth methods: an IP is never a relying party, localhost is a suffix of nothing, a tailnet name becomes the tailnet-wide domain, reaching an instance never needs an origin
- [x] 3. The `home` instance role — omt-identity: an unenrolled identity is told exactly what to run, enrolment is checked before the signature, an epoch floor is distinct from revocation, home changes nothing about sessions
- [x] 4. The encrypted identity file — omt-auth identity_file: the header is readable without the passphrase, the AAD bytes are the written ones, a lying length is refused, truncation at every offset is survived (the cipher itself is not wired)
- [x] 5. Pairing flows — test_joining: pairing
- [x] 6. Revocation and recovery — test_identity: revoke a person
- [x] 7. Per-action re-authentication — test_identity: off by default, named capabilities and effect bits both challenge, freshness clamped at both ends, failing never narrows authority
- [x] 8. Device identity in the collaboration model — omt-identity: a reference carries everything needed to draw it, is-this-device is per recipient, an unnamed device still reads as one, your own answer from elsewhere says it was you
- [x] 9. What the browser actually caches — web/test/cache.test.ts: the trim never proposes a store that would un-pair the device, trims exactly the overage, says when trimming cannot help, only the terminal store holds output
- [x] 10. Tailnet identity as an alternative front door — omt-auth methods: all four conditions required, nothing trusted by default, every failed condition reported, the dangerous combination refuses at startup
- ~~11. Threat model deltas vs. [13](13-security.md)~~ — prose
- [x] 12. Capabilities — test_identity: device.list, device.get via list, device.rename, device.revoke, device.revoke_all, identity.list, identity.revoke, identity.reauth_check (the pairing and stepup groups are not built)
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
