# Feature gap — omt against `stablyai/another tool`

A second pass, wider than [another tool.md](another tool.md)'s first read. That one asked "what
did they get right that we got wrong". This asks "what do they *have*".

**Method, so the confidence is legible.** Rows marked ✓read were checked against
source. Rows marked ~dir are inferred from a module name in `src/main/` and its
file listing; that tells us the concern exists, not how well it is solved.

## 1. Their module surface

`src/main/` has 70+ modules. Grouped by what omt would call them:

| Their concern | omt today |
|---|---|
| `agent-hooks`, `claude`, `codex`, `opencode`, `gemini`, `cursor`, `amp`, `droid`, `pi`, `grok`, `kimi`, `minimax`, `mimo`, `devin`, `copilot`, `antigravity` ✓read | `omt-agent-adapters` — 3 adapters covering 8 agents |
| `pty`, `ghostty` ~dir | `omt-term` + `omt-pty` |
| `ssh`, `ports` ~dir | designed, not built |
| `git`, `source-control`, `github`, `gitlab`, `gitea`, `bitbucket`, `azure-devops` ~dir | **nothing** |
| `jira`, `linear` ~dir | **nothing** |
| `claude-usage`, `codex-usage`, `opencode-usage`, `rate-limits`, `*-accounts` ✓read | **nothing** |
| `automations` (cron, headless dispatch, precheck) ✓read | **nothing** |
| `browser`, `computer` ~dir | **nothing**, deliberately undecided |
| `ai-vault`, `providers` ~dir | `omt-auth` covers credentials, not provider keys |
| `speech` ~dir | `omt-stt` |
| `skills`, `plugins` ~dir | `omt-plugin-host` |
| `memory` ~dir | **nothing** |
| `i18n` ~dir | **nothing** |
| `attribution` ~dir | `Actor` on every mutation |
| `observability`, `telemetry`, `diagnostics`, `crash-reporting` ~dir | **nothing** |
| `hang-watchdog` ✓read — watches its *own* main thread | **nothing**, and a different thing is missing (§3) |
| `project-groups` ~dir | workspaces, no grouping above them |
| `dock`, `tray`, `menu`, `window`, `emulator` ~dir | n/a — no desktop shell |
| `sqlite` ~dir | `omt-store` |

## 2. What omt has that they appear not to

Not scored as wins; scored as things to keep.

- **A capability catalog with a parity gate.** Nothing in their tree looks like
  a check that a capability reaching desktop also reaches mobile. Their mobile
  app is a separate React codebase, which is exactly the shape that drifts.
- **The tier ladder as an enforced invariant.** They read hooks and state
  files; omt additionally forbids a heuristic source from producing structured
  content and tests it over the whole registry.
- **The interaction ledger.** Exactly-once resolution by CAS, and delivery
  confirmed by observing the agent record the answer — including the case where
  the local user answered differently a moment earlier. Their steering model is
  "send a follow-up prompt", which does not need this and cannot do it.
- **One binary, no Electron.**

## 3. Gaps worth closing, ranked by whether they are architecture or product

**Architecture — cheap now, expensive later.**

1. **Bound transcript reads.** They cap agent state files by bytes, structural
   tokens *and* nesting depth before parsing. omt bounds hook payloads and does
   not bound transcript reads at all. An agent's own session file is
   attacker-adjacent input the moment a repo can influence it.
2. **Agent hang detection.** Their watchdog watches their own UI thread. The
   thing omt actually needs is different and it has nothing for it: an agent
   that is neither working nor blocked nor exited — wedged. The merge machine
   reports `Unknown` when every source goes stale, which is honest but not
   actionable. `Unknown` for 90 s while the process is alive is a distinct
   state a user must be told about.
3. **Usage and rate limits.** Reading `AgentPayload::Usage` already exists;
   nothing aggregates it, and nothing knows about a subscription's limits. On a
   phone "you have 8% of your window left" is the difference between sending a
   follow-up and not.

**Product — real gaps, each a deliberate decision.**

4. **Worktree fan-out.** Their headline. One prompt, N agents, N worktrees,
   compare and merge. `WorkspaceId` is derived from a canonical path so N
   worktrees are already N workspaces; what is missing is the fan-out, the
   comparison surface and the merge.
5. **Subagent grid for mobile — done.** The level is *threads*, not sessions.
   Claude Code's desktop client has a subagent switcher that cycles each
   subagent's transcript; **its mobile client shows one session at a time even
   with five agents running**. `ThreadRef` already carried `is_subagent` and
   `parent`, so what was missing was the roster and the grid, not the model.

   Both are built. `ThreadRoster` folds events into per-thread state and
   attributes each raised card to the subagent that raised it — five subagents
   blocked on five questions is five answers to give, not one session that
   vaguely "needs you". The grid sorts blocked first, because spawn order
   buries the only cell that matters, and carries a distinct glyph per tone so
   it is readable without colour.

   **What steering honestly is.** A running subagent has no free-text input
   channel: the parent agent drives it and nothing exposes a prompt for one.
   What is actionable is answering its cards and interrupting — which is most
   of the value and is real. `actionsFor()` never offers a prompt, for the same
   reason a surface never offers an answer button for an undeliverable card.
6. **Scheduled and triggered runs** (`automations`): cron, headless dispatch,
   run-on-precheck.
7. **SCM and issue-tracker integration.** Large, and arguably not a terminal's
   job — but "open the PR this branch belongs to" is one capability, not an
   integration.
8. **Diff review with line comments fed back to the agent.**
9. **Design Mode / embedded browser / computer use.** Depends on shipping a
   browser. Not decided.
10. **i18n.** Not yet, but the capability catalog is the right place for it and
    every user-facing string currently assumes English.

## 4. Not adopting

- **`star-nag`.** They ship a module that asks users to star the repo.
- **Electron.** The reason their terminal needs WebGL to feel fast.
- **A separate mobile codebase.** The parity gate exists to prevent this.
