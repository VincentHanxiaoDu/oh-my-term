# Changelog

## 0.1.0 — unreleased

The first release. What follows is what works, and the section after it is what
does not — because the second list is the one that decides whether this is
useful to you.

### Terminal

- A terminal emulator with scrollback, reflow, wide characters and damage
  tracking. Renders only what changed, so an idle screen costs nothing over
  ssh.
- **Command blocks** from OSC 133, and *without* it: output with no open block
  opens one, so a bare `ssh` to a host with no shell integration produces
  blocks rather than an empty session. Exit codes 130 and 141 are not failures.
- Panes: `Ctrl-A s` splits, `Ctrl-A o` cycles, `Ctrl-A x` closes. A split is
  refused when there is no room rather than producing panes too small to read.
- A hint line that says one thing, chosen from what is true now, and stops
  offering the introduction once you have used the prefix.
- Command-line highlighting.

### Agents

Eleven agent CLIs, each at the tier omt can actually deliver — and it says
which:

| Tier | Agents |
|---|---|
| Protocol | Codex, GitHub Copilot, Gemini CLI, opencode, Qwen, Goose |
| Hook | Claude Code, Cursor |
| Heuristic | Aider, Amp, Crush, **and anything omt has never heard of** |

- Structured question and permission cards from the agent's own words, never
  scraped from a screen. Answering is exactly-once: a second answer is refused,
  and answerability comes from whether omt has a channel rather than from
  whether the card is open.
- Subagent threads, and the state that says which one needs a human.
- Usage and rate limits, when the agent reports them. Cost only when the agent
  stated one — omt never computes it.

### Remote

- A web client that installs as a PWA, takes touch, and receives push. Verified
  in a browser, not assumed.
- `omt serve` and `omt attach`: sessions outlive every terminal, and survive the
  daemon restarting as readable orphans with a one-call restart.
- `omt ssh` for another machine; TLS for a browser.
- File transfer in chunks, resumable, with progress.

### Git

- Worktrees: `worktree.add` / `list` / `remove`. Each is a workspace by the id
  its path derives, with no special case anywhere.
- `git.hunks` — the changed lines of a file, not only which files changed.
- Fan-out: one prompt, an agent per worktree. Choosing an arm records the choice
  and **merges nothing**.

### Everything else

- 58 capabilities, declared once and reaching the TUI, the socket and the
  browser — enforced by a parity gate that is proven to fail.
- Plugins: install with an explicit grant, and a token whose role comes from
  what was granted rather than what was asked for.
- Configuration in layers with provenance, plus `config.schema` so a settings
  screen can be built without knowing the key names.
- Themes, including a built-in one, and import from common formats.
- Scheduled jobs, command history, dictation over a pluggable speech provider.

## What this release does not do

- **No signed or published mobile app.** The iOS app builds and runs on a
  simulator; the Android app builds to an APK. Neither is signed, and neither
  has been on a device.
- **No side-by-side comparison view** for fan-out results. The data is there
  (`git.hunks`); the view is not.
- **No merge, no commit, no push.** Everything git here is read-only except
  creating and removing worktrees.
- **No speech engine in the box.** Bring a key; omt makes no network call
  without one.
- **Not published to crates.io.** Build from a clone.
