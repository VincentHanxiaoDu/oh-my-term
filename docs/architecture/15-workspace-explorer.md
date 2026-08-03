# Workspace Explorer — files and version control

Every workspace can show a **file tree** and its **version-control state**, on
demand, on all three surfaces. It is absent until summoned and costs nothing
while hidden.

This document specifies the crate `omt-workspace-fs`, its provider traits, the
laziness model, the `workspace.files.*` / `workspace.vcs.*` capabilities, the
three surface designs, and the security boundary.

Related: [00](00-overview.md) · [01](01-principles.md) ·
[02 — Crate map](02-crate-map.md) · [03 — Capability catalog](03-capability-catalog.md) ·
[04 §8.4 — semantic click targets](04-terminal-core.md#84-semantic-click-targets) ·
[05 §7 — workspace identity](05-session-model.md#7-workspace-identity) ·
[06 — Agent layer](06-agent-layer.md) · [08 — Web client](08-web-client.md) ·
[13 — Security](13-security.md).

Prior art studied: `sst/opencode`'s server (§12 records what was verified by
execution versus read versus inferred), plus
[iTerm2](../research/iterm2.md) `sources/SemanticHistory/` and
[another terminal](../research/another terminal.md)'s in-process `grep`/`ignore` file search. Per
[P9](01-principles.md#p9--clean-room-with-respect-to-studied-code) no code is
copied; only interface facts are reused.

---

## 1. Scope

**In:** browse the tree one directory at a time; repository summary (branch,
detached HEAD, upstream, ahead/behind, dirty counts, in-progress operation);
per-file status with staged and unstaged as *separate axes*; per-file read-only
unified diff; bounded file read; fuzzy filename search; copy path; insert a path
or an `@file` mention into an agent prompt (§8.2); open in the user's editor
(§8.4); jump to a `file:line` from terminal output (§8.1); show which files this
session's agent touched (§8.3).

**Out, with reasons:**

- **Editing file contents.** omt is not an editor
  ([00 §8](00-overview.md#8-what-omt-is-not)). A usable editor needs an undo
  model, a save model, conflict handling against the watcher, encoding
  negotiation and LSP — each its own subsystem. `workspace.files.reveal` hands
  off to the real editor instead.
- **Commit, amend, rebase, merge, cherry-pick, stash, branch, push, pull.**
  Multi-step, history-rewriting, frequently irreversible. The terminal is right
  there, with the writer token making every keystroke attributable
  ([05 §5](05-session-model.md#5-the-writer-token)). A half-built git client is
  worse than none.
- **Merge-conflict resolution.** Needs editing plus a three-way model.
  Conflicted files are *shown* (`conflicted: true`, hunks readable); resolving
  is the editor's job.
- **File management** (create/rename/move/delete/chmod/upload). File transfer
  already exists and is scoped in [09](09-ssh-and-media.md); the explorer links
  to it rather than duplicating it.
- **Repository-wide content search.** A ripgrep-shaped subsystem with its own
  index, budget and result model. See §13.2.

### 1.1 Decision: no VCS mutation in v1, including stage/unstage

**`omt-workspace-fs` ships zero write capabilities.**

The tempting minimum is `stage`/`unstage`, on the grounds that they are
index-only and reversible. That is *almost* true, and the gap is the problem:
`git add <path>` destroys a pre-existing partially-staged state for that path,
and `git restore --staged` cannot restore it — so the reversibility claim fails
exactly where a careful user would rely on it. `discard` is far worse: it
deletes uncommitted work with no reflog and no undo, and would be a one-tap
action reachable over a Tailscale funnel.

The cost of refusing is small, because the alternative is better on every axis
[P8](01-principles.md#p8--security-by-default-no-ambient-trust) cares about. The
explorer's **"send to session"** action (§8.2) inserts `git add src/main.rs`
into the focused session's composer *without submitting*. The user sees the
command; it needs the writer token; it is attributed; it lands in history
([05 §9](05-session-model.md#9-command-history)); it is undoable by the same
shell mechanisms as everything else. Parity holds — a phone can do it too —
without minting a new remote authority.

Enforced, not merely intended: every capability here is `Role::Viewer` except
`files.reveal`, and none declares `WRITES_FS` or `DESTRUCTIVE`. The CI rule in
[13 §4](13-security.md#4-roles-and-their-mapping-onto-the-catalog) — a `Viewer`
capability with `WRITES_FS`/`DESTRUCTIVE` fails the build — therefore
mechanically prevents this scope from widening by accident. Separately, a test
asserts the git argv builder only ever emits read-only subcommands (§5.2).
Revisiting is §13.1, not a TODO.

---

## 2. The crate: `omt-workspace-fs`, at L2

```
L2  subsystems   omt-term · omt-pty · omt-agent-adapters · omt-transport
                 omt-auth · omt-stt · omt-media · omt-workspace-fs   ← new
```

L2 is right: an independently useful, independently testable subsystem owning no
global state, exactly like `omt-term` and `omt-media`. Not L3, because it owns no
part of the session tree — `omt-session` keeps owning `Workspace` and its
`GitIdentity`, and this crate is handed a root path plus that `GitIdentity` and
answers questions about it.

**Depends on** `omt-types`, `omt-util`, `omt-events`; third-party `notify`,
`ignore`, `globset`, `memchr`, `thiserror`. **Not** on `omt-session`,
`omt-term`, `omt-agent`, or anything L3+. No exception to
[crate-map rule 3](02-crate-map.md#dependency-rules-mechanically-checked) is
needed. Errors are `thiserror` (`WorkspaceFsError`), never `anyhow`.

**Runtime-agnostic** per rule 5: no tokio task, no runtime. `notify` backends
create OS threads, so the crate never constructs one — it exposes a
`WatchDriver` seam that `omt-daemon` satisfies with a real `notify::Watcher`,
and a thread-free `Poll` implementation used by tests.

**It performs no I/O on behalf of a surface except through the catalog.** The
crate exposes providers, not endpoints; `omt-daemon` registers handlers over
them and dispatch applies role and effects before any syscall
([03 §3](03-capability-catalog.md#35-the-dispatch-path)). There is no path from `omt-tui`
into this crate — the TUI panel dispatches `workspace.files.list` with
`Actor::Local` exactly as a phone dispatches it over WebSocket. Path confinement
(§9.1) lives *inside* the crate, so a future second caller cannot bypass it.

---

## 3. Traits and types

Two extension points per [P2](01-principles.md#p2--pluggable-extension-without-modification),
each a trait plus a `Registry`.

```rust
pub trait FileTreeProvider: Send + Sync {
    fn id(&self) -> ProviderId;
    /// Direct children of `rel` only. Never recurses.
    fn list(&self, rel: &RelPath, opts: &ListOptions) -> Result<Listing, WorkspaceFsError>;
    fn stat(&self, rel: &RelPath) -> Result<NodeMeta, WorkspaceFsError>;
    /// Enforces `max_bytes`; sniffs binary *before* decoding (§9.2).
    fn read(&self, rel: &RelPath, opts: &ReadOptions) -> Result<FileContent, WorkspaceFsError>;
    /// Fuzzy filename search. Budgeted; returns `truncated` rather than blocking.
    fn find(&self, q: &str, opts: &FindOptions) -> Result<FindResult, WorkspaceFsError>;
    /// `Ok(None)` when this provider cannot watch (§4.4) — the surface then
    /// shows manual refresh and says why.
    fn watch(&self, d: &dyn WatchDriver, sink: FsEventSink)
        -> Result<Option<WatchHandle>, WorkspaceFsError>;
}

pub trait VcsProvider: Send + Sync {
    fn id(&self) -> ProviderId;
    fn kind(&self) -> VcsKind;                      // Git | Jujutsu | Mercurial | None
    /// Cheap: branch, HEAD, ahead/behind, dirty counts. No per-file work.
    fn summary(&self) -> Result<VcsSummary, WorkspaceFsError>;
    fn status(&self, scope: &StatusScope) -> Result<VcsStatus, WorkspaceFsError>;
    fn diff_file(&self, rel: &RelPath, base: DiffBase, opts: &DiffOptions)
        -> Result<FileDiff, WorkspaceFsError>;
    fn is_ignored(&self, rel: &RelPath) -> Result<bool, WorkspaceFsError>;
    /// `.git`, `.jj`, `.hg` — never listed as content, fed to the watcher's
    /// ignore set.
    fn internal_paths(&self) -> Vec<RelPath>;
}
```

`GitCli` and `NoVcs` ship; jj/hg are third-party. `NoVcs` is a real
implementation rather than an `Option`, because a non-git workspace is normal
([05 §7](05-session-model.md#7-workspace-identity)) and one shape is cheaper for
three surfaces than two.

### 3.1 Tree model

```rust
pub struct Listing {
    pub dir: RelPath,
    pub nodes: Vec<Node>,
    pub truncated: bool,          // cut off at ListOptions::limit (§4.3)
    pub total_seen: u32,
    /// Opaque, changes iff content changed. Enables a 304-style short circuit.
    pub etag: DirEtag,
}

pub struct Node {
    pub name: String,
    pub rel: RelPath,             // workspace-relative, `/`-separated, no trailing slash
    pub kind: NodeKind,
    pub meta: NodeMeta,
    pub vcs: Option<VcsFileState>,   // only when the caller asked for the overlay
    pub agent_touched: bool,         // §8.3
}

pub enum NodeKind {
    File,
    Dir { children_hint: Option<u32> },
    Symlink { target: SymlinkTarget },     // Inside(RelPath) | Outside | Broken
    Other,                                  // socket/fifo/device: listed, never readable
    /// A submodule or an unrelated `.git`. Not descended into by the parent's
    /// status; expandable as its own root (§5.5).
    NestedRepo { vcs: VcsKind },
}

pub struct NodeMeta {
    pub size: Option<u64>, pub modified: Option<Timestamp>, pub mode: Option<u32>,
    pub ignored: bool,
    pub sensitivity: Sensitivity,          // Normal | Sensitive — §9.4
}
```

Directories carry no trailing `/` in `rel`. opencode returns `"src/"` in one
field and `"src"` in another, forcing every client to normalize; one canonical
form plus an explicit `kind` is strictly better and free.

### 3.2 VCS model

```rust
pub struct VcsSummary {
    pub kind: VcsKind,
    pub head: Head,                     // Branch(String) | Detached { short, describe }
    pub upstream: Option<String>,
    pub ahead: Option<u32>, pub behind: Option<u32>,
    pub dirty: DirtyCount,              // { staged, unstaged, untracked, conflicted }
    pub is_worktree: bool,
    pub worktree_group: Option<WorkspaceId>,
    pub operation: Option<InProgress>,  // Merge | Rebase | CherryPick | Bisect | Revert
    pub computed_at: Timestamp,
}

pub struct VcsFileState {
    /// Two independent axes: a file can be staged *and* further modified.
    pub index: ChangeKind,      // Unmodified|Added|Modified|Deleted|Renamed|Copied|TypeChanged
    pub worktree: ChangeKind,
    pub conflicted: bool, pub untracked: bool, pub ignored: bool,
    pub lines: Option<LineStat>,        // { added, removed }; None when binary/uncomputed
    pub orig_rel: Option<RelPath>,      // rename/copy source
}

pub struct FileDiff { pub rel: RelPath, pub base: DiffBase, pub state: VcsFileState,
                      pub body: DiffBody, pub truncated: bool }

pub enum DiffBody {
    Text { hunks: Vec<Hunk>, old_path: RelPath, new_path: RelPath, eof_newline: EofNewline },
    Binary { old_size: Option<u64>, new_size: Option<u64> },
    /// Over `diff.max_file_bytes`. Carries the stat so the UI can say
    /// "+18402 −3, too large to display" and offer `files.read`.
    TooLarge { bytes: u64, lines: LineStat },
    Redacted { reason: RedactReason },   // §9.4
}

pub struct Hunk { pub old_start: u32, pub old_lines: u32,
                  pub new_start: u32, pub new_lines: u32,
                  pub header: Option<String>, pub lines: Vec<DiffLine> }

pub struct DiffLine {
    pub kind: DiffLineKind,             // Context | Added | Removed
    pub old_no: Option<u32>, pub new_no: Option<u32>,
    pub text: String,
    /// Byte ranges within `text` differing from the paired line. Computed
    /// server-side so all three surfaces highlight identically (§7.4).
    pub intra: SmallVec<[Range<u32>; 4]>,
}
```

**Decision: structured hunks on the wire, not a unified-diff string.** opencode
returns `git diff` output verbatim as a `patch` string, which forces every
client to re-implement a patch parser to render anything but a `<pre>`. Three
surfaces would mean three parsers and three sets of off-by-one bugs. We parse
once, in Rust, and ship line numbers — which also turns "jump to `file:line`
inside a diff" (§8.1) into a lookup instead of a scan.

---

## 4. Laziness and cost

The requirement is explicit and load-bearing: **hidden must be free.**

### 4.1 The zero state

While no client has an open explorer for a workspace: no `WorkspaceFsState` is
allocated (the daemon's `HashMap<WorkspaceId, _>` has no entry); no watcher, OS
thread, file descriptor, inotify/FSEvents registration, or `git` process exists;
and **no syscall is issued on the workspace root by this subsystem, ever,
including at workspace open**. `GitIdentity` is computed by `omt-session` for
its own reasons and is reused, not recomputed.

State is created by the first `workspace.files.list` or `workspace.vcs.summary`
and by nothing else. A test asserts it: open twenty workspaces, run a session in
each, assert the map is empty and no `git` process was spawned.

### 4.2 Lazy per-directory expansion; status is separate and coarser

`list` reads exactly one directory. No recursive walk on the request path, no
background indexing. This matches opencode (verified: `/file?path=` returns only
direct children) and is right — a full walk of a monorepo is seconds of I/O for
a panel the user may close immediately. Restoring an expansion set therefore
costs N calls, which clients batch into one `files.list_many` round trip; the
same call reveals a deep path from ancestors computed client-side from the path
string, with no server walk.

Status, by contrast, is **not** per-directory, because `git status` on one
subdirectory of a large repo costs nearly what it costs on the whole repo (the
index scan dominates). `workspace.vcs.status` computes the whole worktree once
and caches it; `files.list { include_vcs: true }` joins against that cache,
computing it once if cold. Invalidated by the watcher (§4.6) or, with no
watcher, by `vcs.status_ttl` (default 3 s).

### 4.3 Caps

Config keys under `workspace.explorer.*`, defaults shown:

| Cap | Default | At the limit |
|---|---|---|
| `list.max_entries` | 2 000/dir | `truncated`, `total_seen` set; UI offers `find` |
| `tree.max_cached_nodes` | 20 000/workspace | LRU evict by directory, oldest-collapsed first |
| `read.max_bytes` | 2 MiB | `payload_too_large` with the real size; UI offers `media.file.pull` |
| `read.max_binary_bytes` | 256 KiB | enough for an image preview, not a transfer channel |
| `diff.max_file_bytes` | 1 MiB | `DiffBody::TooLarge` |
| `diff.max_total_bytes` | 8 MiB/response | rest are `TooLarge`; response sets `budget_exhausted` |
| `diff.context_lines` | 3 | client may request 0–32 |
| `find.max_results` / `find.max_walk_ms` | 200 / 300 ms | `truncated` |
| `status.max_files` | 10 000 | summary only; overlay disabled with `status_too_large` |
| `watch.max_watched_dirs` | 8 000 | coarse mode (§4.6) |
| `vcs.command_timeout` | 10 s | `deadline_exceeded`; child killed |

**Read-size precedence, so the three limits are not confused for each other:**
`read.max_bytes` (2 MiB) bounds an *inline* read through
`workspace.files.read`; [18](18-semantic-open.md)'s `open.remote.max_bytes`
(4 MiB) bounds a *mirrored source file* fetched for a local editor; and
[09 §2](09-ssh-and-media.md#2-the-blob-store)'s `max_blob_bytes` (32 MiB) is the
absolute ceiling nothing may exceed.

Two of these come from measuring opencode and rejecting its choice. Its diff
default is `--unified=2147483647` — the entire file as context on every hunk.
Fine for feeding a model, terrible for a phone on LTE. And its total patch
budget degrades silently to empty patches, so a client cannot distinguish
"unchanged" from "truncated"; we signal `TooLarge`/`budget_exhausted`
explicitly.

### 4.4 `.gitignore` and noise

Ignore matching uses the `ignore` crate — the full precedence chain
(`.gitignore`, `.git/info/exclude`, `core.excludesFile`, nested files,
negations), the same engine ripgrep uses, not worth reimplementing. The matcher
is built **once per workspace** and invalidated when the watcher sees an ignore
file change; opencode rebuilds it from disk on every request, which is a read
plus parse per keystroke of tree navigation.

Ignored entries are **listed but greyed and sorted last**, never hidden —
hiding them makes "why can't I see `dist/`" a support question. A `show_ignored`
toggle (default on) filters them per client.

VCS internals are never listed as content (`internal_paths()`). opencode returns
`.git` with `ignored: false` (verified live); that is a bug we are not copying,
and it matters more for us because our tree is remotely reachable.

Independently, a hard **denylist bounds the watcher only** — `node_modules`,
`target`, `dist`, `build`, `.next`, `.venv`, `__pycache__`, `vendor`, `.gradle`,
`Pods`. They remain listable on explicit expansion; the denylist bounds
watching, not browsing.

### 4.5 Budgets

Reference machine per [04 §9.1](04-terminal-core.md#91-targets), fixtures §11.

| Operation | Repo | p50 | p99 |
|---|---|---|---|
| `files.list` cold, 200 entries | any | 1.5 ms | 6 ms |
| `files.list` cold, 2 000 entries (capped) | monorepo | 8 ms | 25 ms |
| `files.list` warm (etag hit) | any | 0.1 ms | 0.4 ms |
| `vcs.summary` | linux.git | 12 ms | 40 ms |
| `vcs.status` full worktree | linux.git, clean | 45 ms | 180 ms |
| `vcs.status` full worktree | 500k files, fsmonitor on | 90 ms | 400 ms |
| `vcs.diff` 300-line file | any | 6 ms | 20 ms |
| `files.find`, 3-char query | 500k files | 40 ms | 300 ms (cap) |
| Watcher subscribe → armed | 20k dirs | 30 ms | 120 ms |
| Cold open → first paint | 500k files | — | 150 ms |

Memory: ≤ 4 MiB resident per open workspace at the node cap (20 000 × ~200 B)
plus ≤ 1 MiB status cache. **Zero** when closed. `tests/budget.rs` asserts the
node-cache ceiling with a counting allocator; latencies are criterion benches
failing CI at +25 %.

### 4.6 File watching

**Lifecycle is reference-counted and client-driven.** `workspace.files.watch`
increments a per-workspace refcount; the **0→1 transition** builds the `notify`
backend, walks the bounded directory set to register, and arms the debouncer.
`unwatch`, client detach, or transport loss decrements; the **1→0 transition
drops the watcher**, joins backend threads, releases registrations and clears
the status cache. No lingering watcher, no keep-warm grace — the whole point is
that closing the panel returns the process to §4.1. Registrations are per client
(a phone closing its sheet does not tear down the laptop's panel) and carry a
lease (`watch.lease`, default 120 s) refreshed by any `files.*`/`vcs.*` call, so
a client that vanishes without a clean detach — the normal mobile case —
releases within one lease.

**Backend:** `notify` native mode — FSEvents on macOS, inotify on Linux; those
are the v1 platforms
([D10](decisions.md#d10--platform-targets-macos-and-linux-windows-via-wsl2), which
also covers Windows-via-WSL2, where inotify applies). A native-Windows backend is
not a v1 commitment; the `WatchDriver` seam (§2, §3) is where one would go. We
coalesce ourselves rather than using `notify-debouncer-full`, because our
invalidation unit is a *directory* and its unit is a path, and because of the
overflow behaviour below.

- FSEvents is recursive and directory-granular: one root registration covers
  everything, and its events name a directory rather than a file — which *is*
  our invalidation unit, so nothing is lost. macOS is the cheap case.
- inotify is per-directory and non-recursive. Registration is O(directories) and
  consumes `max_user_watches` (commonly 8 192, shared with every editor the user
  has open) — hence the 8 000 cap and the denylist. On `ENOSPC` we do not fail:
  we drop to coarse mode and emit `WatchDegraded` naming
  `fs.inotify.max_user_watches` and the value to raise it to.

**What is watched:** only directories currently expanded by some client plus
their direct children; plus, for VCS, exactly `HEAD`, `index`, `refs/**`,
`MERGE_HEAD`, `REBASE_HEAD` — and nothing else inside `.git`, because a
`git fetch` writes thousands of objects. Collapsing unregisters after
`watch.collapse_grace` (10 s), absorbing accordion clicking.

**Networked filesystems.** Detected by mount type (`statfs` `f_type` /
`getmntinfo`): NFS, SMB/CIFS, SSHFS, 9p, virtiofs, any FUSE. inotify and
FSEvents do not observe remote writers there, so watching would produce a tree
that is silently, confidently stale — the worst outcome. **Decision: no watcher
on a networked mount.** `watch()` returns `Ok(None)`, the capability replies
`{ "mode": "manual" }`, and the surface shows a refresh control and says why. An
opt-in `watch.network_poll` (default `off`) exists for users who want it and
know what it costs. Same path when the backend is unavailable in a container.

**Coalescing.** Raw events fold into a per-workspace accumulator of
`Dir(RelPath) | Vcs | Coarse`: a file event folds into `Dir(parent)`; an
ancestor `Dir` absorbs a descendant `Dir`; anything under a VCS internal path
folds to `Vcs`; an ignore-file change drops the matcher and folds to `Coarse`.
Flushed on a **150 ms trailing debounce with a 1 s maximum delay**, so a
`cargo build` writing 40 000 files produces a bounded event rate and a `touch`
still feels instant.

**Overflow → coarse.** If one window exceeds `watch.max_dirs_per_flush`
(default 64) distinct directories, or the backend reports its own overflow
(inotify `IN_Q_OVERFLOW`, FSEvents `kFSEventStreamEventFlagMustScanSubDirs`),
the batch collapses to one `Coarse` invalidation. Clients re-fetch only what
they are currently *showing* — at most a screenful — rather than re-walking.
This is the branch-switch and `git clean` case, and collapsing it is strictly
cheaper than being precise about it.

**Events** (new `omt-events` payloads; derived from state changes per
[03 §4](03-capability-catalog.md#4-events-are-the-read-side-twin), envelope with
`session: None`, `workspace: Some(id)`, numbered in the workspace's `Seq`
space so a gap resolves to `TreeResyncRequired`, which is always safe):

```rust
pub enum WorkspaceFsEvent {
    TreeInvalidated { dirs: Vec<RelPath> },
    TreeResyncRequired { reason: CoarseReason },
    VcsStatusChanged { summary: VcsSummary, changed: Vec<RelPath>, full: Option<VcsStatus> },
    /// Branch/HEAD/operation only — cheap and frequent enough to deserve its
    /// own variant so a header updates without a status recompute.
    VcsHeadChanged { summary: VcsSummary },
    WatchDegraded { mode: WatchMode, reason: String, remedy: Option<String> },
}
```

---

## 5. Git integration

### 5.1 Decision: shell out to `git`

| Concern | `git` CLI | `gix` | `git2`/libgit2 |
|---|---|---|---|
| Status on huge repos | reference impl; uses untracked cache, `core.fsmonitor`, split index, sparse cones | status is young; sparse/split-index/fsmonitor not at parity | correct but ignores fsmonitor and the untracked cache — minutes on 500k files |
| Linked worktrees | native | supported | supported, fiddly |
| Submodules | native | partial | supported |
| `.gitattributes`, clean/smudge, LFS | applied automatically | not applied | not applied |
| Config precedence (`includeIf`, `core.excludesFile`) | native | good | partial |
| Warm cost | ~2–5 ms spawn, then C speed | no spawn, fastest small | no spawn |
| Binary size | 0 | +~2 MiB | +~1.5 MiB, plus a C toolchain and vendored zlib/openssl |
| Licence | external process (GPL-2.0, no linking) | MIT/Apache-2.0 | GPL-2.0 **with a linking exception** — usable, but an exception to reason about per [14](14-licensing.md) |
| Divergence failure mode | matches the user's own terminal by definition | silent wrong answer | silent wrong answer |

**We shell out.** The deciding row is the last. This subsystem's entire value is
telling the user the truth about their repository, and the CLI is the only
implementation definitionally in agreement with the `git status` they will type
in the pane next to it. The costs — spawn latency, argv parsing, a `git`
dependency — are real and bounded; a status that disagrees with the user's own
git in a sparse-checkout monorepo is unbounded. The LFS row is close to
disqualifying on its own: an LFS pointer diff instead of the real diff is
actively misleading.

It is also consistent with [05 §7](05-session-model.md#7-workspace-identity),
which already shells out for `GitIdentity` — we are not introducing a second git
stack — and matches verified prior art (opencode shells out for every VCS
operation). `gix` is reconsidered when its status reaches parity on fsmonitor
and sparse checkouts; `VcsProvider` is exactly the seam that makes that a
contained change.

### 5.2 Invocation discipline

One builder, `fn git(&self, args: &[&str]) -> Command`, unconditionally applies:
`-C <worktree_root>` and `--git-dir <git_dir>` from `GitIdentity` (so a linked
worktree is addressed with no cwd dependence); `--no-optional-locks` and
`GIT_OPTIONAL_LOCKS=0` (already mandated by [05 §7](05-session-model.md#7-workspace-identity))
so we never take `index.lock` from under the user's own git;
`-c core.quotepath=false` so non-ASCII paths return as UTF-8 rather than octal
escapes; `-c color.ui=false`, `--no-ext-diff`, `--no-pager`;
`GIT_ASKPASS=/bin/false`, `GIT_TERMINAL_PROMPT=0`, `SSH_ASKPASS=` so no
read-only command can block on a credential prompt; closed stdin; a hard timeout
and a 16 MiB output cap, both killing the child.

`core.fsmonitor` is left **untouched** — unlike opencode, which disables it.
fsmonitor is the single biggest status win on large repos, and disabling it
optimizes for reproducibility over the user's actual experience.

| Purpose | Command |
|---|---|
| summary | `status --porcelain=v2 --branch --show-stash -z --untracked-files=normal` |
| per-file status | `status --porcelain=v2 -z --untracked-files=all --renames` |
| line stats | `diff --numstat -z HEAD --` |
| diff worktree/index/ref | `diff [--cached] [<ref>] --patch --unified=<n> -z -- <path>` |
| untracked diff | `diff --no-index --patch --unified=<n> -- /dev/null <path>` |
| worktrees | `worktree list --porcelain -z` |
| merge base | `merge-base <default_branch> HEAD` |

`--porcelain=v2` rather than v1 is deliberate: it gives index and worktree
change kinds as *separate fields*, the submodule state, and the branch and
ahead/behind header in one call — exactly the two-axis `VcsFileState` of §3.2.
v1 (which opencode uses) forces a second call for ahead/behind and loses the
submodule detail. Ignore checks go through the `ignore` crate, never
`git check-ignore`.

`tests/git_argv.rs` asserts the set of first arguments the builder can ever
produce is a subset of `{status, diff, worktree, merge-base, rev-parse,
symbolic-ref, for-each-ref, cat-file}` — the read-only allow-list backing §1.1.

### 5.3 Worktrees

[05 §7](05-session-model.md#7-workspace-identity) already makes each linked
worktree its own workspace — the hard part. This crate only has to be correct
about the split: content operations use `worktree_root`; ref watches use
`common_dir/refs` and `common_dir/packed-refs` (shared) **plus** `git_dir/HEAD`
(per-worktree). Getting this wrong means either a branch switch that never
updates, or every worktree recomputing on every other worktree's commit.
`VcsSummary::worktree_group` lets the explorer header group siblings, matching
the grouping `workspace.list` already returns.

### 5.4 Detached HEAD, in-progress operations, unborn branches

Detached → `Head::Detached { short, describe }`, rendered `⚠ detached @ a1b2c3d`
rather than pretending there is a branch; `describe` is cached and refreshed only
on `VcsHeadChanged`. Unborn (fresh `git init`) → `Head::Branch("main")` with no
upstream, no ahead/behind, every tracked file `index: Added`, and diffs against
the empty tree. In-progress merge/rebase/cherry-pick/bisect/revert is detected
from `MERGE_HEAD`, `rebase-merge/`, `CHERRY_PICK_HEAD`, `BISECT_LOG`,
`REVERT_HEAD` and surfaced as `VcsSummary::operation` with a persistent banner
("rebase in progress, 3 of 7") — a user on a phone looking at a half-rebased
tree needs to know that before they read anything else.

### 5.5 Submodules, nested repos, and no repo at all

A submodule is `NodeKind::NestedRepo` with the parent's `--porcelain=v2`
submodule flags (`S<c><m><u>`) mapped to a compact badge. We do **not** recurse
status into submodules by default — `git status --recurse-submodules` is the
classic multi-second stall, and this panel must open instantly. Expanding a
`NestedRepo` lazily instantiates a *second* `VcsProvider` scoped to that path,
which is why `VcsProvider` is constructed per root rather than per workspace. A
vendored repo or accidental `git init` is the same case, handled the same way.

Not a repository → `NoVcs`: `summary` returns `{ "kind": "none" }`, no status
column, no diff affordance, ignore matching falls back to `.ignore` files, and
the explorer works completely. No error, no banner, no degraded-mode messaging —
a plain directory is not a degraded repository.

---

## 6. Capabilities

Declared in the style of [03 §2](03-capability-catalog.md#2-declaring-a-capability);
handlers live in `omt-daemon` over the providers.

**One new effect bit, already adopted catalogue-wide.** The effect set is
`WRITES_PTY, SPAWNS_PROCESS, READS_FS, WRITES_FS, NETWORK, DESTRUCTIVE`
([03 §2](03-capability-catalog.md#2-declaring-a-capability)) — the earlier
`TOUCHES_FS` bit was split into `READS_FS` and `WRITES_FS` for this document's
sake, and every prior declaration has been migrated
(`open.resolve` → `READS_FS`, `workspace.worktree.add` →
`WRITES_FS`, `media.file.push`/`media.clipboard.write` → `WRITES_FS`,
`media.file.pull` → `READS_FS`, `config.set` → `WRITES_FS`). `READS_FS` is
permitted for `Viewer`; `WRITES_FS` joins the
[13 §4](13-security.md#4-roles-and-their-mapping-onto-the-catalog) CI rule that
fails the build when paired with `Viewer`.

**One name.** The branch/dirty/ahead-behind summary that
[05 §10.1](05-session-model.md#101-workspace) relies on is
`workspace.vcs.summary`, declared here, returning
`GitIdentity + { dirty, ahead, behind }`.

```rust
capability! {
    /// List the direct children of one directory. Never recurses (§4.2).
    name  = "workspace.files.list",
    group = "workspace", verb = "files-list",
    kind  = Query, role = Role::Viewer,
    input  = FilesList { workspace: WorkspaceId, path: RelPath, include_vcs: bool,
                         include_ignored: bool, limit: Option<u32>,
                         if_none_match: Option<DirEtag> },
    output = FilesListOut { listing: Listing, not_modified: bool },
    effects = [Effects::READS_FS],
    since = "0.4",
}

capability! {
    /// Read a bounded slice of one file. Binary is detected before decoding
    /// and returned base64 with a sniffed media type (§9.2).
    name  = "workspace.files.read",
    group = "workspace", verb = "files-read",
    kind  = Query, role = Role::Viewer,
    input  = FilesRead { workspace: WorkspaceId, path: RelPath,
                         range: Option<LineRange>, max_bytes: Option<u64> },
    output = FilesReadOut { content: FileContent, total_lines: Option<u64>, truncated: bool },
    effects = [Effects::READS_FS],
    since = "0.4",
}

capability! {
    /// Structured unified diff for one file (§3.2). Never a raw patch string.
    name  = "workspace.vcs.diff",
    group = "workspace", verb = "vcs-diff",
    kind  = Query, role = Role::Viewer,
    input  = VcsDiffIn { workspace: WorkspaceId, path: RelPath, base: DiffBase,
                         context_lines: Option<u32>, max_bytes: Option<u64>,
                         intra_line: bool },
    output = FileDiff,
    effects = [Effects::READS_FS, Effects::SPAWNS_PROCESS],
    since = "0.4",
}

capability! {
    /// Begin watching on behalf of this client. Ref-counted: the 0→1 transition
    /// constructs the watcher, the 1→0 drops it (§4.6). A Command, not a Query,
    /// because it mutates instance state — declaring it a Query would assert it
    /// is "cacheable and safe to retry", which is false.
    name  = "workspace.files.watch",
    group = "workspace", verb = "files-watch",
    kind  = Command, role = Role::Viewer,
    input  = FilesWatch { workspace: WorkspaceId, lease_secs: Option<u32> },
    output = FilesWatchOut { mode: WatchMode, lease_expires_at: Timestamp, note: Option<String> },
    effects = [Effects::READS_FS],
    since = "0.4",
}
```

The remaining nine follow the same pattern:

| Capability | Kind/Role | Input → Output | Effects |
|---|---|---|---|
| `workspace.files.list_many` | Q / V | `{workspace, paths[], include_vcs, if_none_match[]}` → `{listings[], unchanged[]}` | `READS_FS` |
| `workspace.files.stat` | Q / V | `{workspace, path}` → `{node: Option<Node>}` | `READS_FS` |
| `workspace.files.find` | Q / V | `{workspace, query, kinds, limit, include_ignored}` → `FindResult` | `READS_FS` |
| `workspace.files.unwatch` | C / V | `{workspace}` → `Ack` | — |
| `workspace.files.reveal` | C / **Operator** | `{workspace, path, line?, col?}` → `{launched, program}` | `READS_FS`, `SPAWNS_PROCESS` |
| `workspace.vcs.summary` | Q / V | `{workspace, refresh}` → `VcsSummary` | `READS_FS`, `SPAWNS_PROCESS` |
| `workspace.vcs.status` | Q / V | `{workspace, scope, include_line_stats}` → `VcsStatus` | `READS_FS`, `SPAWNS_PROCESS` |
| `workspace.vcs.diff_many` | Q / V | `{workspace, paths[], base, context_lines, intra_line}` → `{diffs[], budget_exhausted}` | `READS_FS`, `SPAWNS_PROCESS` |
| `workspace.vcs.worktrees` | Q / V | `{workspace}` → `{worktrees[]}` | `READS_FS`, `SPAWNS_PROCESS` |

**`workspace.vcs.*` is `Viewer` *and* declares `SPAWNS_PROCESS`, and that is
legal.** It is the case that motivates the read-only-subprocess carve-out in
[13 §4](13-security.md#4-roles-and-their-mapping-onto-the-catalog): these five
capabilities spawn a fixed argv (`git`, no shell, request data only ever filling
whole named arguments), declare `READS_FS` and no write bit, and the subprocess
mutates nothing. Each one is listed in the read-only subprocess allow-list that
CI checks, so the exemption is a reviewed entry rather than a handler's own
assertion. `workspace.files.reveal` is *not* covered — it launches the user's
editor or file manager, which is why it is `Operator`.

`diff_many` powers "review everything the agent changed" in one round trip.
There is no `stage`, `unstage`, `discard`, `apply` or `commit` (§1.1).

**Subscription** rides `events.subscribe` with a new topic, so existing auth,
resume and schema generation apply unchanged:

```json
{ "t": "subscribe", "id": "r7", "sub": "sub_3",
  "filter": { "workspaces": ["w_9f3c2a1b"], "kinds": ["workspace_fs"] },
  "since_seq": { "w_9f3c2a1b": 41208 } }
```

The frame shape is [07 §3.7](07-remote-protocol.md#37-subscriptions)'s — a
`Subscribe` message with a coarse `(sessions × workspaces × kinds)` filter, not a
bespoke topic type. `files.watch` and the subscription are deliberately separate:
the subscription
says "deliver me events on this topic", the watch says "make the events exist".
A client that subscribes without watching receives `VcsHeadChanged` only (from
the cheap ref watch the session layer already carries) and is told
`mode: "manual"`.

### 6.1 Wire examples

```json
{ "t": "call", "id": "r1", "name": "workspace.files.list",
  "input": { "workspace": "w_9f3c2a1b", "path": "crates/omt-term/src",
             "include_vcs": true, "include_ignored": true } }
```
```json
{ "t": "result", "id": "r1", "ok": true, "output": {
  "not_modified": false,
  "listing": { "dir": "crates/omt-term/src", "etag": "d1:8a41c0e2",
    "truncated": false, "total_seen": 6, "nodes": [
      { "name": "grid", "rel": "crates/omt-term/src/grid",
        "kind": { "type": "dir", "children_hint": 4 },
        "meta": { "modified": "2026-08-03T09:14:02Z", "ignored": false, "sensitivity": "normal" },
        "vcs": { "index": "unmodified", "worktree": "modified", "conflicted": false,
                 "untracked": false, "ignored": false, "lines": null },
        "agent_touched": true },
      { "name": "lib.rs", "rel": "crates/omt-term/src/lib.rs", "kind": { "type": "file" },
        "meta": { "size": 4211, "modified": "2026-08-03T09:14:02Z", "mode": 33188,
                  "ignored": false, "sensitivity": "normal" },
        "vcs": { "index": "unmodified", "worktree": "modified", "conflicted": false,
                 "untracked": false, "ignored": false, "lines": { "added": 12, "removed": 3 } },
        "agent_touched": true } ] } } }
```
```json
{ "t": "result", "id": "r2", "ok": true, "output": {
  "rel": "crates/omt-term/src/lib.rs", "base": { "type": "head" }, "truncated": false,
  "state": { "index": "unmodified", "worktree": "modified", "conflicted": false,
             "untracked": false, "ignored": false, "lines": { "added": 2, "removed": 1 } },
  "body": { "type": "text", "old_path": "crates/omt-term/src/lib.rs",
    "new_path": "crates/omt-term/src/lib.rs", "eof_newline": "both",
    "hunks": [ { "old_start": 88, "old_lines": 5, "new_start": 88, "new_lines": 6,
      "header": "impl Terminal", "lines": [
        { "kind": "context", "old_no": 88, "new_no": 88, "text": "    pub fn resize(&mut self, size: GridSize) {", "intra": [] },
        { "kind": "removed", "old_no": 89, "new_no": null, "text": "        self.reflow(size);", "intra": [[8, 14]] },
        { "kind": "added",   "old_no": null, "new_no": 89, "text": "        self.reflow_budgeted(size);", "intra": [[8, 23]] },
        { "kind": "added",   "old_no": null, "new_no": 90, "text": "        self.damage.mark_all();", "intra": [] },
        { "kind": "context", "old_no": 90, "new_no": 91, "text": "        self.cursor.clamp(size);", "intra": [] }
      ] } ] } } }
```
```json
{ "t": "event", "sub": "sub_3", "session": null, "workspace": "w_9f3c2a1b", "seq": 41211,
  "ts": "2026-08-03T09:14:02.418Z", "source": "workspace_fs",
  "payload": { "type": "vcs_status_changed",
    "summary": { "kind": "git", "head": { "type": "branch", "name": "feat/reflow" },
      "upstream": "origin/feat/reflow", "ahead": 2, "behind": 0,
      "dirty": { "staged": 0, "unstaged": 3, "untracked": 1, "conflicted": 0 },
      "is_worktree": true, "worktree_group": "w_11ab77e0", "operation": null,
      "computed_at": "2026-08-03T09:14:02.410Z" },
    "changed": ["crates/omt-term/src/lib.rs", "crates/omt-term/src/grid/mod.rs"],
    "full": null } }
```

A confinement failure — note the code, not a 500 (§9.1):

```json
{ "t": "result", "id": "r3", "ok": false,
  "error": { "code": "not_found", "message": "No such path in this workspace.",
             "detail": { "path": "../../etc/passwd" } } }
```

---

### 6.2 The `workspace.explorer.*` group — surface navigation

The four above plus the nine in the table are the *data* capabilities. The
explorer is also a **surface**, and [16 §8.2](16-input-and-keymap.md#82-the-leader-namespace)
binds keys to opening and moving around it. Those bindings must resolve to
declared capability names or they fail
[03 §5](03-capability-catalog.md#5-the-parity-contract)'s parity test, so the
group is declared here — 15 owns the explorer, including its surface.

| Capability | Kind/Role | Input → Output | Effects |
|---|---|---|---|
| `workspace.explorer.toggle` | C / V | `{workspace, visible: Option<bool>}` → `{visible}` | — |
| `workspace.explorer.reveal` | C / V | `{workspace, path: RelPath}` → `{revealed: bool}` | — |
| `workspace.explorer.goto` | C / V | *(prefix map root; §7.1's `g`)* `{workspace, target: GotoTarget}` → `Ack` | — |
| `workspace.explorer.cycle_filter` | C / V | `{workspace, filter: Option<TreeFilter>}` → `{filter}` | — |

**These declare no effects and mutate no instance state**, because the explorer's
visibility and filter are *per client per workspace* (§7.2). They are in the
catalog for the same reason `ui.open_command_palette` is: parity is checked
against the catalog, and "reveal this file in the explorer" is an action a phone,
the web client and the TUI must all offer. `workspace.explorer.reveal` is
distinct from `workspace.files.reveal` — the former moves omt's own tree, the
latter spawns the user's editor, which is why only the latter is `Operator` with
`SPAWNS_PROCESS`. [18 §3.3](18-semantic-open.md#33-existence-confinement-and-sensitivity)
relies on exactly that distinction.

---

## 7. Surface parity

[P3](01-principles.md#p3--parity-one-capability-three-surfaces) requires a
matching affordance on all three surfaces, not a subset.

### 7.1 TUI panel

A side panel toggled by `<leader>e` (`workspace.explorer.toggle`, §6.2). **It does not exist until toggled** — no
widget, no state, no capability call. Toggling off dispatches `files.unwatch`
and drops the panel state, returning to §4.1.

| Key | Action |
|---|---|
| `j`/`k`, `↓`/`↑` | move · `l`/`→`/`Enter` on a dir: expand · `h`/`←`: collapse, else jump to parent |
| `H` | collapse all — returns the tree to root-only, freeing the node cache |
| `Enter` on a file | diff view if changed, read view if not |
| `/`, `n`, `N` | fuzzy filter over `files.find`, debounced 120 ms; next/prev match |
| `g s` | cycle tree filter: all → changed only → agent-touched only |
| `g d` / `g D` | diff this file / diff everything changed as one scrollable list |
| `y y` / `y a` | copy workspace-relative / absolute path |
| `i` / `@` | insert path / `@file` mention into the focused session's composer (§8.2) |
| `o` / `r` / `q` | open in `$EDITOR` · manual refresh · close |

The diff view is a full-pane reader: hunk headers, a line-number gutter, `+`/`-`
gutter marks, intra-line ranges as a brighter background, `]c`/`[c` to jump
hunks, `<Tab>` to fold one. Colors come from the theme's diff slots
([08 §9.2](08-web-client.md#92-theming)), so the TUI and the web client render
visually the same diff.

### 7.2 Web desktop panel

A resizable left rail beside the session view, remembered per workspace per
device, **collapsed by default on first visit**. Same three-way filter as a
segmented control, same fuzzy box, same actions in a context menu and a palette
entry. The diff opens in a right-hand split with a `unified | split` toggle —
split view is desktop-only and is the one legitimate divergence, because the
*capability* (read a diff) is identical and only the layout differs. Rows are
virtualized with the same windowing the block list uses
([08 §4.2](08-web-client.md#42-block-view)), so a 2 000-entry directory scrolls
at 60 fps.

### 7.3 Mobile sheet

A bottom sheet with two detents (half, full), dismissible by downward drag,
matching [08 §8.1](08-web-client.md#81-touch-targets-and-reach). Reached from
the session header's workspace chip and from the dashboard.

- **Rows are 48 px tall** with a 44 px minimum target; the chevron and the label
  are *separate* targets so expanding and opening are never confused.
- **Indentation is capped at 4 levels**, then the sheet **reroots** at the
  current directory and shows a tappable `… / omt-term / src` breadcrumb. This
  is the most important mobile-specific choice here: a phone browses by
  drilling, not by indenting — deep nesting at 390 px is unreadable.
- **Swipe** left reveals `Diff` and `Copy path`; right reveals `Insert @` and
  `Open`. All four also live in the long-press menu, per
  [08 §8.4](08-web-client.md#84-gestures) — no gesture is the only path.
- **Pull-to-refresh** issues a manual refresh, which doubles as the affordance
  carrying the `mode: "manual"` case (§4.6).
- **The default mobile view is "changed files", not the tree** — a flat grouped
  list (`Agent-touched` / `Staged` / `Modified` / `Untracked`) with a `+12 −3`
  chip per row. Browsing a full tree on a phone is rarely the goal; reviewing
  what changed is. The tree is one tap away in the same sheet.

### 7.4 Reading a diff on a phone

Where a naive port fails, so it is specified concretely.

- **Unified only.** Side-by-side at 390 px means two 20-column columns. Unified
  plus intra-line highlighting carries the same information in one column.
- **No horizontal scroll by default.** Long lines soft-wrap with a hanging
  indent and a continuation marker. A per-diff `wrap`/`scroll` toggle exists for
  code where alignment matters; `scroll` puts each line in its own
  `overflow-x: auto` container so the page never scrolls sideways.
- **Line numbers are a narrow dimmed gutter**, and `+`/`-` are conveyed by
  background tint *and* a leading sign, never color alone
  ([08 §9.1](08-web-client.md#91-accessibility)).
- **Hunks collapse to headers** when a file has more than 3, each showing
  `+n −m`. You tap the one you care about.
- **Intra-line ranges are the payoff.** On a small screen "this line changed" is
  not useful; "this identifier changed" is. Computed server-side (§3.2), so it
  costs the phone nothing.
- **Sticky header** with filename, `+/−` stat, and prev/next-file chevrons, so
  reviewing 8 changed files is 8 swipes rather than 8 navigations.
- Fetched at `context_lines: 3`; a "more context" control refetches at 12. The
  common payload is a few KB, which is what matters on LTE.

---

## 8. Integration with the rest of omt

### 8.1 `file:line` from terminal output

[04 §8.4](04-terminal-core.md#84-semantic-click-targets) already produces
`Target::Path { raw, line, col }`, and
[18 §9](18-semantic-open.md#9-capabilities)'s `open.resolve` returns a
`ResolvedTarget` with the path resolved against the owning block's `cwd`. This
document contributes one type to it:

```rust
pub struct ExplorerRef { pub workspace: WorkspaceId, pub rel: RelPath,
                         pub line: Option<u32>, pub col: Option<u32> }
```

`ResolvedTarget` carries `explorer: Option<ExplorerRef>`. The struct itself is
defined once, in [18 §3](18-semantic-open.md#3-resolution), which owns it; this
document owns only `ExplorerRef` and the explorer behaviour behind it.

Clicking, tapping, or `gf`-ing `src/main.rs:42:8` in a stack trace opens the
explorer at that node, scrolled to line 42, showing the diff if the file is
modified and the content if not. Membership is a prefix test against the open
workspaces' canonical roots — which is why
[05 §7](05-session-model.md#7-workspace-identity)'s canonicalization matters
here too, so `/tmp/x` and `/private/tmp/x` land in the same workspace. If the
path is outside every root, `explorer` is `None` and the only offered action is
the `Operator`, locally-scoped `files.reveal`.

### 8.2 Inserting a path or `@file` into an agent prompt

The workflow the explorer exists to enable: *you are on your phone, an agent is
mid-task, and you want to point it at a file.*

Two row actions. **Insert path** puts the workspace-relative path (shell-quoted
if needed) into the focused session's composer. **Insert mention** puts the
agent's *native* file-reference syntax there — which needs per-agent knowledge,
so it goes on the adapter rather than being guessed, per
[P4](01-principles.md#p4--native-semantics-observe-never-re-implement):

```rust
// Added to `AgentAdapter`, owned by [06 §7](06-agent-layer.md#7-adapters).
// Defaulted to `None`, so third-party adapters keep compiling.
fn path_mention(&self, rel: &RelPath) -> Option<String> { None }
```

Claude Code and opencode return `@crates/omt-term/src/lib.rs`; an agent with no
mention syntax returns `None` and the UI relabels the action "Insert path"
rather than offering a mention that will be read as literal text.

**Insertion is never a submit.** It targets the composer, not the PTY: if the
session has a live binding with a native prompt channel, the text goes to that
session's omt-side composer buffer, which the user edits and sends via
`agent.prompt`; otherwise it is `session.send_text { submit: false }`, which
requires the writer token and appears as typed characters the user can see and
edit. Multi-select inserts N mentions space-separated, and "insert all changed
files" is one action from the changed-files list — the actual phone gesture for
"review these". The same mechanism backs `git add` from §1.1.

### 8.3 Files the agent changed this session

The explorer consumes `AgentEvent::FileChanged { path, change, tool, turn_id }`
and maintains a per-*binding* set of touched paths.

`AgentEvent::FileChanged { path, change, tool, turn_id }` is declared in
[06 §8](06-agent-layer.md#8-ancillary-semantics), which owns the payload;
`change` is `Created | Modified | Deleted | Renamed { from }`.

The set is keyed by `BindingId` and **cleared when the binding ends**, matching
the `clear_retained()` discipline in [06 §2](06-agent-layer.md#2-the-two-axis-model)
so a new agent never inherits the previous one's attribution. `agent_touched` is
*attribution*, not truth about the filesystem — git status is the truth. When
they disagree (agent wrote a file then reverted it), the UI shows the git status
plus a muted agent glyph and does not editorialize. The mobile changed-files
list gains an **"Agent" group** at the top when the set is non-empty; this is the
highest-value screen in this document — "what did Claude just do to my repo",
answerable in one tap from a lock-screen notification.

### 8.4 Open in editor

`files.reveal` runs the configured editor on the *daemon's* machine
(`editor = "${EDITOR}"`, `editor_args = ["--goto", "{path}:{line}:{col}"]`).

**`workspace.files.reveal` and [18](18-semantic-open.md)'s `editor` handler share
one implementation:** 18 owns the handler and the editor argv/template
resolution (its detected per-editor table replaces the VS Code-specific
`editor_args` default above), and this document owns `workspace.files.reveal` as
the capability surface — so the explorer's "open in editor" and
`open.activate { handler: "editor" }` cannot diverge in behaviour.

The template substitutes **positional argv values, never a shell string** — no
`sh -c`, so a path containing `;` or `$()` is inert. The program is resolved
from `PATH` once and the resolved path is shown in the confirmation. Because it
is `SPAWNS_PROCESS` and `Operator`, the web client's generic effects wrapper
([08 §2.3](08-web-client.md#23-effects-drive-ui-policy-not-just-audit)) already
produces a confirm sheet naming the program. From a phone this is "open that
file on my laptop" — genuinely useful and genuinely a process spawn, so the
confirmation is correct rather than friction. Disabled when `editor` is unset;
greyed out with a reason for a credential whose scope
([13 §4.1](13-security.md#41-credential-scope)) excludes
`workspace.files.reveal`, per the degradation rule in
[08 §3.4](08-web-client.md#34-graceful-degradation-across-catalog-versions).

### 8.5 Diffs inside permission cards

[06 §5](06-agent-layer.md#5-interactions--the-flagship-path) declares
`InteractionKind::Permission { diff: Option<UnifiedDiff>, .. }`. That field
becomes `Option<FileDiff>` (§3.2), so a permission card renders with the *same*
component as the explorer's diff view on all three surfaces — one renderer, one
palette, one intra-line highlighter, one mobile behaviour.

Source, in priority order: (1) **the agent's own payload** — a Claude Code
`Edit`/`Write` `PreToolUse` hook carries `old_string`/`new_string`, an ACP
permission request carries the proposed change. This is authoritative, being
what the agent will actually do, and wins whenever present. (2) **Computed by
this crate** against the current file when the payload gives only a path,
marked `source: "computed"` so a user knows it is omt's reconstruction.

It is never *only* computed for a destructive tool: a card omt cannot render
faithfully shows the raw tool input, not a guessed diff. Rendering a wrong diff
that a user taps "Allow" on is precisely the failure mode
[06 §6](06-agent-layer.md#6-the-heuristic-floor) exists to prevent.

---

## 9. Security

Named plainly: **a `Viewer` credential on a published instance can read any
non-gated file under any open workspace root.** That is the intended power of
the feature — a phone showing you a diff is reading your source — and it is
bounded as follows.

### 9.1 Path confinement

One function inside the crate, used by every provider method:

```rust
fn confine(root: &CanonicalPath, rel: &RelPath) -> Result<PathBuf, WorkspaceFsError>
```

1. **Reject at parse.** `RelPath` has a validating constructor: no leading `/`,
   no `.`/`..` component, no NUL, no backslash on Unix, no ADS/`\\?\`/device
   names on Windows, no component over 255 bytes, total under 4 096. Absolute
   and traversal inputs are rejected before any syscall.
2. **Lexical join**, then assert `root` is still a prefix.
3. **`realpath` the join** and assert `realpath(root)` is still a prefix. This
   catches a symlink inside the workspace pointing at `/etc` or another user's
   home.
4. **`openat` from a retained root fd, `O_NOFOLLOW` on the final component**,
   closing the TOCTOU window between 3 and the open. A symlink is *listed*
   (`NodeKind::Symlink`), its target followed only when inside the root;
   `SymlinkTarget::Outside` renders as an explained dead end rather than being
   silently omitted.
Steps 2–3 mirror the two-step check verified in opencode's core (lexical
containment then a post-`realpath` test); 1 and 4 are ours.

**Steps 1–4 are the whole check on every v1 platform.** macOS and Linux are the
targets, and Windows is supported through WSL2, which *is* Linux — the guest sees
a Linux filesystem through Linux syscalls, so `openat`/`O_NOFOLLOW` and
`realpath` are the right primitives there and no additional step is needed
([D10](decisions.md#d10--platform-targets-macos-and-linux-windows-via-wsl2)).

A **native**-Windows backend would need a fifth step — reparse points resolving
outside the root are NTFS's version of the symlink escape, and comparison must be
case-normalized per volume, since `confine()`'s prefix assertions are
byte-comparisons that a case-insensitive volume defeats. That step is **reserved,
not promised**: native Windows is not a v1 target, and the seam for it is the
same `WatchDriver`/path-backend boundary §4.6 names. Recording it here means the
requirement is not rediscovered later; it is not a claim that the path is
implemented. (Step 1's rejection of ADS, `\\?\` and device names is unconditional
and stays — it is input validation against a hostile client, not a platform
behaviour, and a Windows-shaped path is never legitimate over the wire.)

**Failures return `not_found`, never `unauthorized` and never a 500.** Per
[13 §4](13-security.md#4-roles-and-their-mapping-onto-the-catalog) an
`unauthorized` answer confirms existence. (opencode returns an opaque HTTP 500
here — verified live — which leaks little but is indistinguishable from a real
fault.) Scoped credentials compose: `scope = Workspaces([w1])` cannot name `w2`
in any of these capabilities and gets `not_found`.

### 9.2 Size limits and binary detection

`read.max_bytes` is enforced **before** reading from the `stat` size, and again
as a hard copy cap for files whose size is not knowable. Over the limit is
`payload_too_large` carrying the real size. opencode reads the entire file into
memory with no cap and only then decides what it is; on a 4 GiB core dump that
is a daemon OOM.

Binary is sniffed **from the first 8 KiB before decoding**: a NUL in the prefix,
failure to decode as UTF-8 with over 5 % replacement, or a known magic number.
Binary returns base64 with a sniffed media type, capped at
`read.max_binary_bytes` — enough for an inline image preview, not enough to be a
file-transfer channel; larger routes to `media.file.pull`
([09](09-ssh-and-media.md)) with its own quotas.

Content is returned **byte-exact**. opencode `.trim()`s text content before
returning it (verified live — the trailing newline is gone), which silently
corrupts anything that round-trips. `read` never follows a fifo, socket or
device, so a `mkfifo` in the tree cannot hang a handler.

### 9.3 Cost limits as a security control

The §4.3 caps are also DoS bounds: an authenticated `Viewer` looping on
`files.find` is bounded by `find.max_walk_ms` and by the per-credential rate
limits in [13](13-security.md). `vcs.*` additionally takes a per-workspace
semaphore of 1, so N concurrent clients produce one `git status`, not N.

### 9.4 Sensitive files

`NodeMeta::sensitivity` is `Sensitive` when the name matches a configurable set
defaulting to `.env`, `.env.*`, `*.pem`, `*.key`, `*.p12`, `id_rsa*`,
`id_ed25519*`, `.netrc`, `.npmrc`, `.pypirc`, `credentials`, `*.kdbx`,
`.aws/**`, `.ssh/**`, `.docker/config.json`, `secrets.*`, `terraform.tfstate*`,
`*.jks`.

| | `Viewer` | `Operator` | `Admin` |
|---|---|---|---|
| See the node, badged 🔒 | yes | yes | yes |
| `files.read` its content | refused (`forbidden_sensitive`) | allowed | allowed |
| `vcs.diff` its content | `DiffBody::Redacted` | allowed | allowed |
| Line stats (`+n −m`) | yes | yes | yes |

Listing without content is the right trade for a *shared* credential: hiding it
makes the tree lie, showing it makes a shared read-only link a credential leak.

The gate is on `Viewer` only, and that is deliberate. `Viewer` is the sharing
role ([13 §4](13-security.md#4-roles-and-their-mapping-onto-the-catalog)); the
owner's own devices are `Operator`, and per
[D2](decisions.md#d2--remote-is-exactly-equivalent-to-local) an `Operator` on a
phone is equivalent to sitting at the TUI, which can `cat .env` without
ceremony. Refusing the owner's phone would be a capability that works locally
and not remotely, which D2 forbids. An owner who wants a stricter phone mints a
narrower credential deliberately (`13 §4.1`); it is not the default.

Separately, the redactor from [13 §8](13-security.md#8-secret-redaction) runs
over **diff and file content** on the way out, catching high-entropy secrets in
files that matched no name pattern. That is a genuine extension of that
section's scope, which today covers logs, audit entries and events but not
capability *outputs* — hence §13.5. Its stated limits apply here too and
honestly: a secret in an unusual format will not be caught.

### 9.5 Audit

Every `files.read`, `vcs.diff` and `files.reveal` from a non-`Local` actor is
audited with actor, credential, workspace, path and byte count. `files.list` is
sampled (high-volume, low-signal), but the *first* list per workspace per
credential is always recorded, so "this token started browsing my repo at 03:14"
is answerable.

---

## 10. Configuration

All caps in §4.3 are `workspace.explorer.*` keys, plus `editor`/`editor_args`
(§8.4), `show_ignored`, `default_mobile_view` (`"changed" | "tree"`),
`watch.{lease, collapse_grace, debounce, debounce_max, network_poll, deny_dirs}`,
`vcs.{command_timeout, status_ttl, max_files}` and `sensitive.patterns` (§9.4).

`workspace.explorer.enabled = false` is a real off switch: the capabilities are
not registered, so they do not appear in the handshake list and a federating
client greys them out per [03 §7](03-capability-catalog.md#7-versioning). An
instance that must never expose file contents remotely is one config line away.

---

## 11. Testing

**Fixture repositories** in `tests/fixtures/repos/`, built deterministically
(fixed author and dates, so hashes are assertable): `plain/` (not a repo);
`simple/`; `both-axes/` (staged *and* further modified — the two-axis model);
`unborn/`; `detached/`; `conflict/` (mid-merge, `MERGE_HEAD` present);
`rebase/` (stopped at 3 of 7); `worktrees/` (two linked, different branches);
`submodules/` (clean, new-commits, dirty-content); `nested/` (unrelated `.git`);
`unicode/` (combining marks, RTL, emoji, a `"` and a `\n` in filenames);
`symlinks/` (inside, outside, broken, loop); `sensitive/`; `binary/` (PNG, ELF,
UTF-16, a lone NUL at byte 9 000); `crlf/` and `lfs/` (verifying the §5.1
filter argument pays off); `sparse/` (cone mode, most of the tree absent);
`huge/` (generated: 500 000 files across 40 000 directories, with
`node_modules`).

**Performance** (feature-gated benches): `huge/` drives every number in §4.5,
plus a `max_cached_nodes` ceiling test with a counting allocator.

**Watcher tests** use a `WatchDriver` fake with a virtual clock, so folding,
debouncing and overflow-to-coarse are deterministic and run in microseconds. A
separate `#[ignore]`d tier drives real `notify` on each OS in CI:
create/modify/delete/rename, branch switch, `git stash`, atomic-rename save (the
editor pattern), 40 000 files touched at once (expects exactly one `Coarse`),
injected inotify `ENOSPC` (expects `WatchDegraded` with the remedy string), and
a subscribe/unsubscribe cycle asserting the fd count returns to its start.

**Traversal-attack tests** are a table-driven corpus against `confine()`:
`../`, `..\\`, `%2e%2e/`, `....//`, absolute and UNC paths, `CON`/`NUL` on
Windows, NUL bytes, over-long components, a symlink to `/etc` created *between*
the `realpath` and the open (injected), a symlink loop, and a `.git/config` read
attempt. Every case must return `not_found` and must not `open` outside the root
— asserted with an `openat`-counting shim in the test build. This corpus is also
a **fuzz target** per [P5](01-principles.md#p5--production-grade-from-the-first-commit),
alongside a fuzzer over the `git diff` output parser, which is genuinely
untrusted: a malicious filename can inject `diff --git` lines into a patch,
which is exactly how patch-splitting parsers get confused.

**Third-party impl test** ([P2](01-principles.md#p2--pluggable-extension-without-modification)):
a `VcsProvider` for a toy VCS written outside the crate using only its public
API. If it needs a `pub(crate)` item opened up, the abstraction leaked.

**Parity**: the standard catalog test covers all thirteen capabilities declared
in §6 (four written out, nine tabulated). Playwright
adds a 390×844 audit asserting every explorer row and diff control meets 44 px,
and an E2E that opens the sheet, filters, opens a diff, inserts an `@` mention
and asserts the composer text — mirrored by a TUI test driving the same sequence
through the same capabilities.

---

## 12. What was verified in opencode, and what was not

**Verified by execution** against `opencode 1.18.9`, live server, throwaway git
repo: the route set (`GET /file`, `/file/content`, `/file/status`, `/find`,
`/find/file`, `/find/symbol`, `/vcs`, `/vcs/status`, `/vcs/diff`,
`/vcs/diff/raw`, `POST /vcs/apply`, `/api/fs/*`, SSE `/event`); that
`/file/status` and `/find/symbol` are stubs returning `[]` despite published
schemas; that `/file/content` never populates its declared `diff`/`patch`
fields, `.trim()`s text, and base64s binary with a sniffed mime type; that
traversal and absolute paths both return an opaque HTTP 500; that `/file` lists
`.git` with `ignored: false`; that directory `path` values carry a trailing `/`;
that `/vcs/diff` returns literal `git diff` output as a per-file string; and
that **no file-watcher events arrived over SSE** in ~50 s of live file mutation,
with or without the experimental flag.

**Read in source, not independently exercised:** that VCS operations shell out
to `git` with a fixed hardening prefix (`--no-optional-locks`,
`core.fsmonitor=false`, `core.quotepath=false`, …) and the exact argv vectors;
that the default diff context is `2147483647`; that the 10 MB patch budget
degrades silently to empty patches; that confinement is a lexical check followed
by a post-`realpath` check; that the general watcher is `@parcel/watcher`, gated
behind `OPENCODE_EXPERIMENTAL_FILEWATCHER`, requires VCS, and has **no**
server-side debouncing (coalescing is client-side, keyed on "is this path
already loaded"); that only `.git/HEAD` is watched unconditionally, for branch
detection; that `.gitignore` is re-read and the matcher rebuilt per request;
that search has two backends (a bundled ripgrep and a native Rust indexer) with
the latter as default.

**Inferred:** that the installed binary and the cloned HEAD are behaviourally
identical for this surface (strong evidence — routes, parameter bounds and both
stubs match exactly — but they are same-day builds); and the reason no watcher
events fired (most likely the native binding failing to load; not isolated).

**From iTerm2:** `sources/SemanticHistory/` owns Cmd-click path resolution, and
OSC 7 plus OSC 133 supply the `cwd` a relative path resolves against — omt
already encodes both in [04 §8.4](04-terminal-core.md#84-semantic-click-targets),
so §8.1 only adds the workspace-relative hop. **From another terminal:** a `FileTree`
feature flag exists, and its palette does file search via `crates/a ripgrep crate`
using the `grep`/`ignore` crates **in-process** rather than spawning `rg` — the
precedent for using `ignore` directly in §4.4 instead of `git check-ignore`.

---

## 13. OPEN QUESTIONS

1. **Does `stage`/`unstage` ever earn its way in?** §1.1 says no for v1. The
   strongest counter-case is the phone review flow: read the agent's diff, stage
   the good files, tell the agent to commit. If revisited, the shape is
   `workspace.vcs.stage` at `Operator` with `WRITES_FS`, reachable to any
   `Operator` credential (there is no per-credential approval policy — see
   [13 §7.2](13-security.md#72-remotely-resolving-an-agent-interaction)), and a
   refusal when the path has a partially-staged state `git add` would destroy.
   `discard` and `commit` stay out regardless. Needs v1 usage data.

2. **Repository-wide content search.** Out of scope (§1), but the obvious next
   request, and it interacts with these caps and the watcher. another terminal does it
   in-process with the `grep` crate; opencode ships a vendored ripgrep plus a
   native indexer. Leaning: a `workspace.files.grep` over the `grep` crate,
   budgeted like `find`, no index. Deciding late is cheap (a new capability, not
   a change to these) — but it must be decided before v1 if we want the tree
   filter to search contents.

3. **Should the watcher survive a brief unsubscribe?** §4.6 tears down at 1→0
   with no grace, so a user toggling the panel repeatedly pays full
   re-registration (~30 ms on 20k dirs). A 5 s grace would fix that at the cost
   of violating "free when hidden" for 5 s. Leaning: keep the hard teardown,
   because the property *is* the feature; revisit if re-arm measures worse than
   budget on Linux.

4. **`vcs.status` on a 500k-file monorepo without fsmonitor.** §4.5 budgets
   400 ms p99 *with* it. Without, `git status` is multi-second, making
   `include_vcs: true` a trap. Options: detect the cost on first call and
   disable the overlay with a `status_too_slow` flag (symmetric with
   `status_too_large`); or offer to enable fsmonitor, which is a config write to
   the user's repo and needs consent. Needs a measurement on a real monorepo.

5. **Redaction over capability outputs.** §9.4 extends
   [13 §8](13-security.md#8-secret-redaction) from logs/audit/events to file and
   diff outputs — a scope change to the security model with a real performance
   cost (regex over every diff line) and a real false-positive cost (a redacted
   line in a diff is confusing). Needs a call with the security-model owner:
   always on, non-`Admin` only, or name-pattern gating alone.

6. **Resolved — `AgentEvent::FileChanged`** is now declared in
   [06 §8](06-agent-layer.md#8-ancillary-semantics). Until adapters emit it,
   `agent_touched` is always `false` and the mobile "Agent" group does not
   appear — a clean degradation.

7. **Resolved — `AgentAdapter::path_mention`** is now a *defaulted* method on the
   trait in [06 §7](06-agent-layer.md#7-adapters), which keeps it compatible per
   [P2](01-principles.md#p2--pluggable-extension-without-modification). The
   rejected alternative was a central `MentionStyle` table keyed by `AgentKind`;
   the adapter won because mention syntax is agent-native knowledge (P4) and a
   plugin-contributed adapter must be able to supply it without editing
   `omt-core`.

8. **What is the default diff base on the mobile changed-files screen?**
   `Worktree` (uncommitted only) or `MergeBase(default_branch)` (the whole
   branch)? The latter is the question a reviewer actually asks and is constant
   in a worktree-per-branch workflow, and costs one extra `merge-base` call.
   Leaning `Worktree` for the agent-review case with a one-tap toggle. Needs a
   UX call alongside [08](08-web-client.md).
