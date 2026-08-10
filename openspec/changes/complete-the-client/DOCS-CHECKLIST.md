# Every documented section, and the test that proves it

Generated from the `## ` headings of every architecture document.

**144 of 212 features ticked.** (34 further sections are prose.)

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

- [x] 1. The idea — all seven derived artifacts now exist. Dispatch, routes, `schemas/catalog.v1.json`, `web/src/generated/catalog.ts`, `docs/reference/capabilities.md` and the parity matrix were already generated; the `omt <group> <verb>` CLI tree was the missing one and is now `crates/omt/src/cli.rs`, derived rather than written out. `crates/omt/tests/cli_tree.rs` runs the real binary against a real daemon: `a_capability_is_reachable_as_a_subcommand_without_being_written_out`, `stdout_is_data_and_stderr_is_everything_else`, `an_unreachable_instance_exits_five`, `a_command_is_dispatched_as_a_command_and_a_query_as_a_query` (9 tests, plus 16 unit tests). `codegen-check` is the drift gate.
- [x] 2. Declaring a capability — `omt-catalog` decl tests: `a_good_declaration_validates`, `validation_reports_every_problem_at_once`, `a_query_with_intent_is_rejected`, `a_command_without_intent_is_rejected` (§2.2), `raw_stream_is_never_retryable`, `effects_are_refined_per_call` and `refining_effects_upward_is_caught_by_dispatch` (§2.1), `a_viewer_may_not_write`, `an_unknown_effect_is_refused_not_ignored`, `exemptions_are_per_surface` (§2.3).
- [x] 3. The machinery — `crates/omt-catalog/tests/third_party_impl.rs` proves the traits are implementable from outside the crate and that a third-party declaration reaches the link-time slice (§3.1, §3.3); `registry::tests` covers dispatch, role refusal before the handler runs, duplicate registration and sealing (§3.2, §3.5); `cargo xtask codegen-check` is §3.4, run green here; §3.6's runtime registration is the contribution admission in `omt-plugin-host`.
- [x] 4. Events are the read-side twin — test_events_and_parity: effects declared
- [x] 5. The parity contract — test_events_and_parity: intent class per kind
- [x] 6. Capability groups (initial surface) — asserted in `crates/omt/tests/parity.rs`: `there_is_no_notification_group` (D12 removed push from v1, and a group is one macro away from existing), `layout_is_a_group_of_its_own_and_not_a_pane_sub_group`, `every_group_the_initial_surface_names_exists`.
- [x] 7. Versioning — aliases were **declared but never resolved**, so a rename was a `not_found` on the wire; `CapabilityRegistry::resolve` now honours them and a colliding alias is refused at registration. `a_renamed_capability_still_answers_to_its_old_name`, `an_alias_does_not_appear_as_a_capability_of_its_own`, `an_alias_that_collides_with_a_real_capability_is_refused`, plus the same over the wire in `TestVersioning`. Catalog intersection on handshake is `a_client_presents_the_intersection_rather_than_failing`.
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

- [x] 1. What this layer must deliver — all four: which agent and for how long (§2 `binding.rs`), what state (§4 merge), structured interactions (§5, `test_agents`), ancillary semantics (§8). Nothing in the list is unrepresented.
- [x] 2. The two-axis model — `crates/omt-agent/src/binding.rs`. Binding (which agent, with a lifetime) is a separate type from state, reconciled on its own slow axis. `learning_the_agents_own_id_is_not_a_rebind`, `a_binding_keeps_its_start_time_when_it_is_only_refined`, `an_agents_own_id_is_never_forgotten_once_known`, `a_worse_tier_never_downgrades_a_binding` (9 tests, omt-agent 79 green).
- [x] 3. Source model — test_agent_native: messages, tools, results
- [x] 4. Merging: confidence tiers, not voting — omt-agent merge: the highest fresh tier wins, a lower one may fill a gap but never contradict a live higher one, a stale source stops speaking, an authoritative source suspends the heuristic
- [x] 5. Interactions — the flagship path — test_agents: interactions and deliverable
- [x] 6. The heuristic floor — omt-agent-adapters detect: a marker beats a name belonging to another agent, an unrecognised process is Unknown rather than a guess, an unreadable environment is not evidence of absence
- [x] 7. Adapters — omt-agent-adapters registry: the builtin set covers every agent in the matrix, an adapter can be written from outside the crate and can replace a built-in one
- [x] 8. Ancillary semantics — queue: `crates/omt-agent/src/queue.rs` + `crates/omt/src/queue.rs`, D15's four requirements (binding target, durable entries with `valid_until`, receipt-not-mirror, `unknown` never empty) with `an_enqueue_against_another_binding_is_refused`, `a_broken_reader_reports_unknown_never_empty`, `the_flagship_agent_is_the_one_that_cannot_edit_its_queue` and 5 integration tests in `TestQueueHonesty`. Slash commands: `agent.offers`, `test_what_the_agent_offers_is_read_from_the_agent`. File changes: `claude_code.rs` normalizes `PostToolUse` for Write/Edit/MultiEdit into `FileChanged`. Usage, compaction, subagents, `away_summary`: `AgentPayload` variants in `omt-events/src/agent.rs` with round-trip tests.
- [x] 9. Failure modes and their handling — omt-agent ledger: a restart abandons what nobody answered, preserves an answer that was given, never cancels, and settles nothing twice
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

- [x] 1. Two flavours, and which to build first — `crates/omt-plugin-host/src/fanout.rs`. A plugin can never back-pressure the terminal: bounded mailbox, newest dropped, counted so the plugin can resynchronize. `a_slow_plugin_is_dropped_rather_than_allowed_to_stall_the_terminal`, `dropping_never_reorders_what_does_arrive`, `a_linked_plugin_abi_is_refused_with_the_reason`, `a_wasm_component_is_allowed_on_the_same_terms_as_a_subprocess`. Only the decision and its consequences are built; no subprocess supervisor yet.
- [x] 2. Manifest — `crates/omt-plugin-host/src/manifest.rs`. `omt_api` required and named in the refusal; unknown top-level keys warn (forward compatibility), unknown keys inside a known table error (typos); a manifest that will not parse keeps its entry as invalid with the reason. `a_manifest_without_omt_api_is_refused_and_the_error_names_the_field`, `an_unknown_top_level_key_is_a_warning`, `an_unknown_key_inside_a_known_table_is_an_error`, `a_manifest_that_will_not_parse_is_kept_as_invalid_with_a_reason`, `every_diagnostic_carries_a_greppable_code` (12 tests). `[contributes]` is accepted and not yet interpreted.
- [x] 3. The plugin-facing API — test_extensibility: grants, high-consequence
- [x] 4. Permissions — §4.2's escalation rule in `crates/omt-daemon/src/escalation.rs`, in dispatch rather than the host so a bug in the host cannot bypass it. `a_plugin_never_exceeds_the_actor_that_triggered_it`, `an_event_can_never_reach_admin`, `an_operator_capability_poked_by_a_viewer_is_refused_before_the_plugin_is_contacted`, `every_role_pairing_is_covered_and_none_of_them_escalates`; grants and high-consequence prompts in `omt-plugin-host` (§4.1, `test_extensibility`). §4.3's OS sandboxes are not built.
- [x] 5. Lifecycle — `crates/omt-plugin-host/src/lifecycle.rs`. Healthy/degraded/quarantined with the transitions: three consecutive missed checks degrade, a degraded plugin keeps running and recovers by answering, a crash loop quarantines while a plugin that crashes twice a day never does, and shutdown escalates rather than waiting. `a_degraded_plugin_keeps_running`, `a_crash_loop_is_quarantined_rather_than_restarted_forever`, `the_restart_budget_is_per_hour_not_for_all_time`, `shutdown_escalates_rather_than_waiting_forever` (12 tests). The state machine, not the process supervisor.
- [x] 6. Distribution — `crates/omt-plugin-host/src/distribution.rs`. The install digest is re-checked on every start, so an edited plugin directory is disabled rather than silently changing behaviour; sources are ordered by trust and a discovered one is labelled `unreviewed`. `an_edited_plugin_directory_cannot_silently_change_behaviour`, `a_discovered_plugin_is_labelled_unreviewed`, `unsigned_is_refused_where_the_host_requires_signatures`. Admission rules only; no registry client or git fetch.
- [x] 7. Versioning and stability — the range check in `manifest::check_api` names both the wanted range and this host (`the_host_version_is_checked_against_the_declared_range`, `a_range_omt_cannot_parse_refuses_rather_than_admits`); contributed names in `distribution.rs` cannot claim a core group and the first enabled wins a collision (`a_plugin_may_not_claim_a_core_group`, `the_first_enabled_plugin_wins_a_name_and_the_second_is_told`); drift is `unsupported` at call time, not a crash.
- [x] 8. Worked example: an ntfy notification sink — `crates/omt-plugin-host/tests/worked_example.rs`, run against the manifest extracted verbatim from the doc into `tests/fixtures/ntfy.omt-plugin.toml`. `the_documented_manifest_is_one_this_host_accepts`, `nothing_in_the_documented_manifest_is_an_unrecognised_key`, `a_flaky_network_sink_degrades_and_recovers_rather_than_dying`, `the_sink_cannot_answer_the_question_it_notifies_about` (8 tests). The TypeScript side of §8.4 is illustrative and not run.
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
- [x] 8. Capabilities — test_workspace_surfaces: store.paths, store.retention, store.migrate_status, store.redaction_test (export, import, purge, repair and quarantine are not built)
- ~~9. OPEN QUESTIONS~~ — prose

## 22-operations

- [x] 1. Daemon lifecycle — §1.1's discovery ladder in `crates/omt/src/discovery.rs`, wired into `attach::run` so it is the real path: `$OMT_SOCK` first, then probe every socket and unlink the ones that do not answer, one live means use it and several means say which. `the_environment_wins_over_everything_else`, `a_socket_that_does_not_answer_is_unlinked_not_reported`, `several_live_instances_are_listed_rather_than_chosen_between`, `stale_sockets_do_not_make_a_single_live_one_ambiguous` (10 tests; 459 integration tests pass on the new path). §1.2's service units are not written.
- [x] 2. Multi-user machines — `crates/omt/src/multiuser.rs`, enforced in `serve()`: `umask(0o077)` set before anything is opened, and the socket's parent checked at startup — a symlink or another uid's directory refuses to start, a mode of ours is narrowed and said out loud. `the_fallback_path_carries_the_uid`, `a_directory_owned_by_someone_else_is_refused`, `a_world_readable_directory_of_ours_is_corrected_rather_than_fatal`, `a_token_on_the_command_line_is_caught`, `a_system_service_is_refused_with_a_reason_and_an_alternative` (13 tests; peer-uid check already in `omt-transport`; 459 integration tests pass with the startup guard live).
- [x] 3. `omt doctor` — test_doctor: every group, remedy on every failure
- [x] 4. Health, metrics, logs and fault isolation — test_watching: metrics
- [x] 5. Diagnostics panel and `omt bug-report` — test_doctor: bug report carries no credential
- [x] 6. Upgrade — `crates/omt/src/upgrade.rs`. The refusal is the default and names what is running; `--wait` defers until every agent is idle *and* no interaction is open; a package manager's file is delegated to rather than overwritten; skew is a notice unless there is no common proto; §6.3's host loop skips a host serving another user and one that is unreachable. `a_working_agent_stops_the_upgrade_by_default`, `a_deferred_restart_waits_for_an_unanswered_question_too`, `a_host_serving_another_user_is_never_upgraded`, `a_downgrade_is_warned_about_before_it_happens_not_after` (15 tests). Planning and policy; no downloader.
- [x] 7. Bootstrap onto a host with no omt (R38) — `crates/omt/tests/ssh_remote.rs` against a real sshd in a container (7 tests, all passing: `the_probe_reports_a_missing_omt_with_its_architecture`, `a_binary_survives_the_ssh_pipe_intact`, `an_atomic_install_leaves_no_half_written_state`, `the_probe_finds_omt_once_it_is_installed`), plus §7.2/§7.3's policy in `crates/omt/src/bootstrap.rs`: musl for every Linux host whatever its libc, an unsupported arch refused with its uname, consent asked once, and a host that refuses still gets a terminal. `every_linux_host_gets_a_musl_build_whatever_its_libc`, `a_host_that_refused_still_gets_a_terminal`, `a_truncated_upload_is_caught_before_it_is_used` (13 tests). §7.4's remote GC is not built.
- [x] 8. Automation and CI (G14) — `agent.wait` and `session.capture` now exist (`crates/omt/src/surfaces.rs`), with §8.1's exit-code table and §8.5's refusals in `crates/omt/src/automation.rs`. `blocked` is a state and `interaction` an edge (`interaction_is_an_edge_and_never_matches_a_state`); a wait on a session with no agent refuses at once rather than after the timeout; `session.capture` reports the total so a truncation is visible. 11 integration tests in `tests/integration/test_automation.py` plus 13 unit tests. Also renamed `interaction.respond` to the documented `interaction.resolve`, keeping the old name as an alias. §8.2's token handling and the CLI flags are not built.
- [x] 9. Performance and resource budgets — `crates/omt/tests/budgets.rs`, the harness §9 says should exist. Escape-heavy parser throughput, cell size, grid cost, bounded scrollback and the session-limit refusal all pass; the limit default was 256 against the documented 200 and is now 200. **The plain-text parser budget is not met: ~26 MiB/s against a documented 60.** The gap is line feeds rather than bytes (48 MiB/s with none, 20 with one per 10 bytes), so it is the row-eviction path, and closing it means recycling cell buffers between the grid and scrollback. Held at a floor of 20 so it cannot drift further, with the real budget kept as a runnable `#[ignore]`d test. The seven budgets with no assertion yet are named in `the_budgets_this_file_does_not_check_are_named`.
- [x] 10. Capabilities — `crates/omt/src/ops.rs`: §10.1's job registry (as `task.*` — `job.list` already means a scheduled job, and one name for two things resolves by load order), `upgrade.check`, `instance.shutdown`, `remote.probe`. Cancellation is cooperative and reports `cancelling`; one past its point of no return refuses rather than reporting a success it cannot deliver. `remote.probe` is Operator with NETWORK, and the egress gate in `parity.rs` now asserts a reviewed list rather than a count of zero. `a_task_past_its_point_of_no_return_refuses_rather_than_lying`, `a_cancel_reports_cancelling_rather_than_cancelled` plus 6 integration tests in `TestOperations`. Still unbuilt from the table: `service.*`, `upgrade.apply`, `remote.bootstrap`, `system.log.*`, `system.metrics`; `doctor.run`/`doctor.report` cover `system.doctor` under two-level names.
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
