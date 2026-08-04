# Research — running an agent *on* a phone

> Status: research in progress. This file exists so [24 — Mobile](../architecture/24-mobile.md)
> can link to it; the findings are being gathered and will replace this note.

## The question

Could a phone be the machine rather than a window onto one? That is: could
Claude Code, opencode or Codex CLI run **locally** on iOS or Android, in a
sandbox or a unified workspace, with omt supplying the runtime dependencies?

If yes, the architecture changes materially — a phone becomes an instance
rather than a client, and the whole remote protocol becomes optional for
single-device use.

## What is being investigated

- **iOS**: subprocess limits, JIT and W^X, what a-Shell / iSH / Blink actually
  achieve and under which entitlements, and what App Store review permits an
  app to execute.
- **Android**: Termux under `targetSdk` 29+ exec restrictions, proot, and
  whether the Android Virtualization Framework terminal is a real path.
- **Browser / WebAssembly**: WASI, container2wasm, WebVM, WebContainers — what
  they can genuinely run, and what network and filesystem access costs.
- **The reality check**: whether *any* shipping mobile coding-agent product
  runs an agent locally, or whether all of them run it remotely.

Nothing in the architecture assumes an answer either way until this lands.
