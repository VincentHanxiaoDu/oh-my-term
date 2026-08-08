# Tasks

- [x] `omt-workspace-fs`: worktree add, list, remove over `git worktree`
- [x] `worktree.add` / `list` / `remove` capabilities — named `worktree.*`
      rather than `git.worktree.*` because the catalog derives a name from its
      group and verb, and a three-level name cannot
- [x] `git.hunks` capability over the existing parser
- [x] `fanout.start` / `status` / `choose` capabilities creating real worktrees
- [x] Tests against a real repository: add, list, remove, refuse-with-changes,
      hunks of a modified file, a fan-out that partially fails
- [x] Wire into the web client; parity gate
- [x] Docs: roadmap entry, `docs/status.md`, the design docs
