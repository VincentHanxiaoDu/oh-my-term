# Tasks

- [x] Write the `Codex` adapter: fingerprint, spawn env with `CODEX_NOTIFY`,
      event mapping, interrupt, path mention
- [x] Write the `Cursor` adapter: fingerprint, spawn env, lifecycle hook
      mapping including turn start from the prompt hook
- [x] Register both in `builtin()`
- [x] Widen the registry test to the whole documented matrix
- [x] Tests: tier honesty, verbatim command carrying, unknown events erroring
- [ ] Codex `app-server` client, which is what would raise its tier to protocol
- [ ] opencode HTTP (`serve` REST + SSE) source, listed in the matrix as
      `OpencodeHttp` and currently served by generic ACP instead
