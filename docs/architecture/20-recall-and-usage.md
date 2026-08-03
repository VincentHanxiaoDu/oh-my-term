# Recall, Timeline and Usage — `omt-recall`

Memory and awareness. Three questions this document owns, and nothing else:

1. **Where was that?** — finding past work across sessions, workspaces and
   machines. Gap [G2](../design/scenarios.md#g2--search-and-recall-across-sessions-transcripts-and-machines--no-owner),
   requirements R45, R46, R56.
2. **What happened while I was away?** — the per-session timeline and the
   morning digest. Gap [G3](../design/scenarios.md#g3--what-happened-while-i-was-away--a-session-digest-and-timeline--no-owner),
   requirement R31; plus comparing parallel agents ([G18], R76) and noticing a
   stuck one ([G19], R23).
3. **What did it cost?** — normalized, agent-reported usage and rate limits.
   Gap [G15](../design/scenarios.md#g15--cost-usage-and-rate-limits-across-sessions--partial-owner-thin),
   requirements R73, R74, R75.

Related: [03 — Capability catalog](03-capability-catalog.md) ·
[04 — Terminal core](04-terminal-core.md) (blocks) ·
[05 — Session model](05-session-model.md) (history, persistence) ·
[06 — Agent layer](06-agent-layer.md) (`AgentEvent`, interactions, `away_summary`) ·
[07 — Remote protocol](07-remote-protocol.md) (federation) ·
[08 — Web client](08-web-client.md) (rendering) ·
[15 — Workspace explorer](15-workspace-explorer.md) (VCS, files changed) ·
**[21 — Data lifecycle](21-data-lifecycle.md)** (retention, redaction detector,
export, purge — this document consumes it and does not duplicate it).

Two binding constraints from the top of the project, restated because every
section below is shaped by them:

> **omt never talks to a model** ([P4](01-principles.md#p4--native-semantics-observe-never-re-implement),
> anti-requirement 2). Every number in a digest is *counted*. Every sentence in
> a digest is either a template omt owns or a string the agent itself produced,
> displayed verbatim and attributed.

> **omt only ever displays usage numbers the agent itself reported.** omt keeps
> no price table, applies no rate, and estimates no cost. A wrong dollar figure
> is worse than no dollar figure, because a user will believe it.

---

## 1. The recall problem, stated as questions

Search designs go wrong when they start from "we have a corpus, let us index
it". The corpus here is mostly `cargo build` output. Start from what is actually
asked, out loud, at a terminal:

| The question, as asked | What the user knows | What must be matched |
|---|---|---|
| "Which session was I doing the auth refactor in?" | a topic, roughly | agent prompt/turn text, session title, workspace name |
| "What was that ffmpeg command?" | a fragment of the command | **command text** — nothing else |
| "What did the agent change in `payments` yesterday?" | a path prefix and a day | `FileChanged` paths, time range — a *structured* query, no text at all |
| "Where did `E0499` come from?" | an exact token from output | block **output** |
| "What did Claude say about `AuthMiddleware`?" | an identifier, agent said it | assistant message text |
| "That failing test run last Tuesday" | exit≠0, a date, maybe a name | structured filter first, text second |
| "Which of the three worktrees passed?" | three sessions | not search at all — see §9 |

Three observations drive the whole design.

**(a) Most of these are structured queries wearing a text costume.** "yesterday",
"in this repo", "that failed", "the agent did it" are filters, not query terms.
A design that pushes everything through a relevance scorer answers them worse
than a `WHERE` clause does. So: **filters are first-class and are applied
before scoring**, not as post-hoc facets.

**(b) The field matters more than the term.** `ffmpeg` in a command is a strong
hit; `ffmpeg` in the middle of 40 KB of `--help` output is noise. Fields are
therefore separately weighted and separately queryable, and the default query
searches a *subset* of fields (§2.4).

**(c) Recall is anchored in time and place.** Every one of these questions
carries an implicit "recently" and usually an implicit "here". Recency and
workspace proximity are not tie-breakers; they are primary signals (§4).

### 1.1 Non-goals

- No semantic/embedding search. It requires a model (P4) and, for identifier-
  shaped queries — which is most of them — lexical search is simply better.
- No relevance learning. mcfly's learned ranker is the most satisfying and least
  predictable design in the field ([research](../research/completions-and-shell.md#33-atuins-ranking-signals));
  omt uses a transparent weighted score that `omt doctor` can print.
- No natural-language date parsing beyond a small, documented grammar
  (`today`, `yesterday`, `last week`, `2026-08-01..2026-08-03`).

---

## 2. What is indexed, and what is not

### 2.1 The size argument, first

A working day produces roughly (measured on the author's own machine, order of
magnitude only):

| Stream | Volume/day | Value per byte |
|---|---|---|
| Command text | ~400 commands × 40 B ≈ **16 KB** | extremely high |
| Block output | 50–500 **MB** | very low, extremely uneven |
| Agent assistant/user messages | ~300 KB | high |
| Agent tool call inputs | ~200 KB | high (the `command` field especially) |
| Tool *results* (file contents, build logs) | tens of MB | very low |
| Structured metadata (paths, exits, timings) | ~1 MB | high, and it is not text |

Full-text indexing costs roughly 0.4–1.2× the source size in FTS5 with a
positions index. Indexing all block output means a multi-gigabyte index per
month to make `cargo build` warnings findable. Indexing command text costs
kilobytes. The ratio between those two is the entire design.

### 2.2 The rule

> **Everything is *structurally* queryable. Only high-value, low-volume text is
> full-text indexed. Bulk output is indexed by a bounded window, and the
> unindexed remainder stays reachable through the block it belongs to.**

The last clause is what makes the truncation honest: a search never claims
completeness over output it did not index, and the UI says so (§2.5).

### 2.3 Per-source decisions

| Source | Structured | Full-text | Cap | Why |
|---|---|---|---|---|
| Block command text | yes | **yes** | none | Tiny, and it is the single highest-value field. |
| Block cwd / git branch / exit / timing / attribution | yes | no | — | Filters, not terms. Indexed as columns. |
| Block **output** | yes (byte length, line count) | **yes, windowed** | first 4 KiB + last 8 KiB of the *closed* block, after redaction | Errors live at the end; the command echo and the first diagnostics at the start. The 100 MB middle of a build log is worthless. Configurable (`recall.output_window`), `0` disables. |
| Agent `UserMessage` | yes | **yes** | 16 KiB | Prompts are how users remember sessions. |
| Agent `AssistantText` | yes | **yes** | 16 KiB per message | "What did it say about X." |
| Agent `Reasoning` | yes | **no** by default | — | High volume, low recall value, and the most privacy-sensitive text an agent produces. Opt-in (`recall.index_reasoning`). |
| Agent `ToolCall.input` | yes | **partial** — `command`, `file_path`/`path`, `pattern`, `description` only | 2 KiB | The rest is payload (whole file bodies in `Write`). |
| Agent `ToolResult.output` | yes (status, duration) | **no** | — | This is bulk output by another name; the same 4+8 KiB windowing applies only when the tool is exec-shaped and the result was an error. |
| `FileChanged` paths | yes | **yes** (path tokenizer, §3.4) | — | "what touched `payments/`" is a top-5 question. |
| `Interaction` prompt + resolution | yes | **yes** | 4 KiB | Small, and "what did it ask me" is a real query. |
| Session/workspace name, `session.note`, agent kind/model | yes | **yes** | — | Cheap anchors. |
| `away_summary` and other agent-authored summaries | yes | **yes** | — | Written by the agent, high signal. |
| Usage / rate-limit events | yes | no | — | Numbers. §11. |
| Raw PTY bytes, scrollback outside blocks | no | no | — | Reachable via [04 §8.1](04-terminal-core.md#8-search) within-session search. Not a corpus. |

### 2.4 Default field set

An unqualified query searches `command`, `user_msg`, `assistant_msg`, `path`,
`title`, `note`, `summary` — **not** `output`. Output is searched when the user
asks for it: `out:E0499`, a UI toggle ("include command output", sticky per
device), or automatically when a `field:` filter selects it. This one default
is the difference between "search finds my work" and "search finds 200 lines of
a compiler log".

### 2.5 Honesty about coverage

Every result set carries a `coverage` object, rendered as one line in every
surface:

```
searched 41 892 blocks · 12 104 agent messages · output windows only (4 KiB head + 8 KiB tail)
2 sessions excluded: persist_scrollback = false
```

If a workspace opted out of persistence ([21](21-data-lifecycle.md)), search
says so rather than silently returning nothing from it.

---

## 3. Index design

### 3.1 The decision

> **D-R1 — The recall index is SQLite with FTS5, in the same database file as
> the block/history store, using external-content tables.**

Alternatives evaluated:

| Option | Binary cost | Incremental write | Cross-platform | Licence | Verdict |
|---|---|---|---|---|---|
| **SQLite + FTS5** (`rusqlite` bundled) | ~1.2 MB, **already linked** — `omt-store`'s backend and [05 §9](05-session-model.md#9-command-history)'s history live here | Row-at-a-time; one transaction per batch; no background merge to schedule | Everywhere Rust builds, including musl and Windows, no C++ toolchain | SQLite public domain; `rusqlite` MIT — both Apache-2.0-compatible (P9/[14](14-licensing.md)) | **Chosen** |
| tantivy | +4–6 MB, new dep tree | Excellent throughput, but segment-based: needs a merge policy, a commit policy, and a separate on-disk directory to keep consistent with the SQL store across crash/restore | Good | MIT | Rejected: a second durability domain |
| Hand-rolled inverted index | small | we own every bug | we own every bug | — | Rejected on P5 |

The decisive argument is not speed — tantivy is faster at scale omt will not
reach — it is **one durability domain**. [05 §8.2](05-session-model.md#82-crash-semantics)
promises that a crash loses at most the tail of output and that a `Partial`
restore is surfaced, never silent. With FTS5 the index is written in the *same
transaction* as the block record, so "the block exists but is not findable" is
not a reachable state. With tantivy, block metadata and the index commit
independently and reconciliation after a crash becomes a real subsystem.

Secondary arguments: FTS5 gives prefix (`auth*`), phrase (`"cargo test"`),
`NEAR`, per-column weighting via `bm25(fts, w1, w2, …)`, and `snippet()` /
`highlight()` for free. External-content tables mean the text is stored once,
in the source table, not duplicated into the FTS shadow table.

Costs accepted: no fuzzy matching in the engine (§3.5 handles it), no native
faceting (SQL handles it), and single-writer semantics (omt has exactly one
writer, the daemon).

### 3.2 Schema

Lives in `omt-store`, migration `recall_v1`. `format_version` and fixture tests
per [05 §8.3](05-session-model.md#83-versioning).

```sql
-- One row per indexable unit. Deliberately one table, not one per kind:
-- ranking must compare a command against an assistant message, and a single
-- FTS table makes that a single scored query.
CREATE TABLE doc (
  id            INTEGER PRIMARY KEY,
  kind          INTEGER NOT NULL,   -- 0 block, 1 user_msg, 2 assistant_msg,
                                    -- 3 tool_call, 4 file_change, 5 interaction,
                                    -- 6 session_meta, 7 summary
  session_id    TEXT    NOT NULL,
  workspace_id  TEXT,
  binding_id    TEXT,               -- agent binding, NULL for human blocks
  block_id      INTEGER,            -- back-link into the block log (04 §6)
  event_seq     INTEGER,            -- per-session seq of the source AgentEvent
  ts            INTEGER NOT NULL,   -- unix ms, start of the unit
  duration_ms   INTEGER,
  cwd           TEXT,
  git_root      TEXT,
  git_branch    TEXT,
  exit_code     INTEGER,            -- NULL unless kind=0 and closed
  exit_signal   INTEGER,
  attribution   INTEGER NOT NULL,   -- 0 human, 1 agent, 2 unknown  (04 §6.2)
  agent_kind    TEXT,
  model         TEXT,
  -- text columns, already redacted (§5). NULL where not applicable.
  command       TEXT,
  output        TEXT,               -- windowed, see §2.3
  output_bytes  INTEGER,            -- true size before windowing; drives §2.5
  user_msg      TEXT,
  assistant_msg TEXT,
  path          TEXT,               -- file path(s), space-joined for kind=4
  title         TEXT,
  note          TEXT,
  summary       TEXT,
  redacted      INTEGER NOT NULL DEFAULT 0,   -- count of redactions applied
  deleted_at    INTEGER              -- tombstone; purge sets this then vacuums
);

CREATE INDEX doc_ts        ON doc(ts DESC) WHERE deleted_at IS NULL;
CREATE INDEX doc_ws_ts     ON doc(workspace_id, ts DESC) WHERE deleted_at IS NULL;
CREATE INDEX doc_sess_seq  ON doc(session_id, event_seq);
CREATE INDEX doc_cwd_ts    ON doc(cwd, ts DESC) WHERE deleted_at IS NULL;
CREATE INDEX doc_kind_ts   ON doc(kind, ts DESC) WHERE deleted_at IS NULL;
CREATE INDEX doc_path      ON doc(path) WHERE kind = 4;

-- External-content FTS: text is not duplicated.
CREATE VIRTUAL TABLE doc_fts USING fts5(
  command, output, user_msg, assistant_msg, path, title, note, summary,
  content = 'doc',
  content_rowid = 'id',
  tokenize = "unicode61 remove_diacritics 2 tokenchars '_-./:@'",
  prefix = '2 3 4'
);

-- Triggers keep the shadow table in step inside the caller's transaction.
CREATE TRIGGER doc_ai AFTER INSERT ON doc BEGIN
  INSERT INTO doc_fts(rowid, command, output, user_msg, assistant_msg,
                      path, title, note, summary)
  VALUES (new.id, new.command, new.output, new.user_msg, new.assistant_msg,
          new.path, new.title, new.note, new.summary);
END;
-- doc_ad / doc_au mirror the FTS5 'delete' + re-insert idiom.
```

Notes on the choices:

- `tokenchars '_-./:@'` keeps `AuthMiddleware`, `omt-term`, `crates/omt/src`,
  `E0499`, `user@host` and `--dry-run` as single tokens. This matters more for
  developer recall than any ranking tweak.
- `prefix = '2 3 4'` builds prefix indexes for 2–4 character prefixes so the
  as-you-type palette can issue `auth*` without a full scan. Costs ~15% index
  size; measured worth it.
- `remove_diacritics 2` is Unicode-correct and does not mangle CJK (which
  `unicode61` leaves as-is; CJK matching is therefore substring-poor — accepted
  limitation, recorded in §14).
- Deletion is a tombstone plus an FTS delete, because [21](21-data-lifecycle.md)
  must be able to prove a purge happened and `VACUUM` is scheduled, not inline.

### 3.3 Write path

```rust
/// Everything that enters the index goes through this one function.
/// It is the only place `doc` is written, so §5's ordering is checkable.
pub fn index(tx: &Transaction<'_>, unit: IndexUnit) -> Result<DocId, StoreError>;

pub struct IndexUnit {
    pub kind: DocKind,
    pub scope: Scope,                 // session, workspace, binding, block, seq
    pub when: Timespan,
    pub place: Place,                 // cwd, git_root, git_branch
    pub facts: Facts,                 // exit, attribution, agent_kind, model
    /// Already-redacted text. Constructed only by `Redacted::from_raw`, which
    /// is the sole public constructor; see §5.2. There is no way to build an
    /// `IndexUnit` from a `String`.
    pub text: FieldSet<Redacted>,
}
```

Batching: units are queued in an in-memory ring and committed either every
`recall.flush_interval` (default 500 ms) or every `recall.flush_docs` (default
256 units), whichever comes first — except block-close and interaction-resolve
units, which are committed in the same transaction as the block/ledger record
itself, because they must be atomic with it.

### 3.4 The path tokenizer

`path` is written twice into the same column: the full path and its suffix
chain. `crates/omt-term/src/grid.rs` is stored as

```
crates/omt-term/src/grid.rs omt-term/src/grid.rs src/grid.rs grid.rs grid rs
```

so `grid.rs`, `omt-term/src` and `grid` all hit, without a custom tokenizer
extension (which would be a C shim and a portability problem). Cost is ~3× the
path bytes, which is nothing.

### 3.5 Fuzzy matching

FTS5 has none. The [05 §9](05-session-model.md#9-command-history) rule applies
here too and is now the general rule: **FTS narrows, Rust ranks.** A fuzzy query
issues an FTS query over the trigram-ish prefix terms to get ≤2000 candidates,
then scores them in Rust with a Smith–Waterman-style matcher (the `nucleo`
crate, MIT). If the FTS query would be empty (query shorter than 2 chars), the
candidate set is the most recent 2000 rows in scope — which is exactly what the
history palette wants anyway.

---

## 4. Ranking

Deliberately a transparent weighted product, printable by
`search.explain`. No learning, no hidden state.

```
score = bm25_component
      × recency(ts)
      × proximity(scope)
      × status(exit)
      × attribution_bias(kind, attribution)
      × session_activity(session_id)
      × kind_weight(kind)
```

### 4.1 The factors

**`bm25_component`.** FTS5's `bm25()` returns a negative number where more
negative is better. Normalize: `bm25_component = 1 / (1 + max(0, -bm25(...)))`
inverted to `1 + clamp(-bm25, 0, 20)/20` so it lands in `[1, 2]`. Column weights
passed to `bm25()`:

| column | weight |
|---|---|
| `command` | 10.0 |
| `title`, `note`, `summary` | 8.0 |
| `user_msg` | 6.0 |
| `path` | 5.0 |
| `assistant_msg` | 3.0 |
| `output` | 1.0 |

**`recency(ts)`** — exponential decay with a 14-day half-life, floored so old
strong hits are not annihilated:

```
recency = 0.15 + 0.85 * 0.5f64.powf(age_days / 14.0)
```

Range `[0.15, 1.0]`. Fourteen days is chosen because it is roughly a sprint;
`recall.recency_half_life_days` tunes it.

**`proximity(scope)`** — where you are now, multiplicatively:

| condition | factor |
|---|---|
| same `session_id` as the caller's focused session | ×1.6 |
| same `workspace_id` | ×2.0 |
| same `git_root` but different workspace (a sibling worktree — [05 §13.3](05-session-model.md#13-open-questions)) | ×1.7 |
| exact `cwd` match | ×1.3 (multiplies with the above) |
| `cwd` is a prefix of, or prefixed by, the caller's cwd | ×1.15 |
| otherwise | ×1.0 |

Same-repo beating same-directory is atuin's `FilterMode::Workspace` lesson
([research §3.3](../research/completions-and-shell.md#33-atuins-ranking-signals)),
and it is right: you want the `cargo test` you ran in this repo, from any of its
directories.

**`status(exit)`** — for `kind = block`:

| exit | factor |
|---|---|
| `Code(0)` | ×1.0 |
| `Code(n≠0)` | ×0.6 |
| `Signal(2)` / `Signal(15)` (Ctrl-C, TERM) | ×0.4 — usually a mistake, not a result |
| still running | ×1.1 — it is on screen right now |
| unknown | ×0.9 |

Failures rank *down*, not out. `store_failed` is effectively always true, and
"which invocation failed before it worked" is real information — but when the
user types `cargo test` they want the one that worked. `only:failed` inverts
this for the "find that error" case, in which failure becomes ×1.5.

**`attribution_bias`** — agent-run commands are numerous and repetitive; human
commands are what a human remembers typing.

| | history/command queries | general search |
|---|---|---|
| `Human` | ×1.25 | ×1.0 |
| `Agent` | ×0.8 | ×1.0 |
| `Unknown` | ×0.95 | ×0.95 |

The asymmetry is intentional: for "what was that command", human authorship is
strong evidence; for "what did the agent change", it is not.

**`session_activity`** — sessions that are alive and recently touched are more
likely to be the one meant.

```
alive & focused           ×1.3
alive                     ×1.15
orphaned (05 §8.2)        ×1.0
closed                    ×0.9
```

**`kind_weight`** — a flat prior so a 40-line output window cannot outrank the
command that produced it: block `1.0`, session_meta `1.0`, user_msg `0.95`,
summary `0.95`, file_change `0.9`, interaction `0.85`, assistant_msg `0.8`,
tool_call `0.75`, output-only match `0.5`.

### 4.2 Grouping

Raw scoring returns twelve hits from one session. The result list is therefore
**grouped by session**, showing the session's best hit plus up to 3 more, with
`+N more in this session`. Group order is the group's best score; within a
group, chronological. This one rule does more for perceived quality than any
weight above.

### 4.3 Testability

The ranker is a pure function `fn score(hit: &Hit, ctx: &QueryCtx) -> f64` over
plain data. `crates/omt-recall/tests/ranking.rs` holds a fixture corpus (200
docs, checked in as JSON) and ~40 assertions of the form *"query `X` from
workspace `W` at time `T` ranks doc `d17` first"*. Changing a weight without
updating the fixtures fails CI, which is the point: the ranking is a spec, not a
vibe. `search.explain` returns every factor's value for a given hit, so a bad
result is diagnosable rather than arguable.

---

## 5. Redaction and the index

Requirement **R56**: *redaction is applied to any output that enters a search
index*. This section specifies the ordering guarantee; the detector itself and
the on-disk redaction of scrollback are owned by
[21 — Data lifecycle](21-data-lifecycle.md).

### 5.1 The ordering rule

> **D-R2 — Redaction happens before persistence, and the index is built from
> the persisted (redacted) copy. There is no path from raw bytes to `doc`.**

```
PTY bytes ─► omt-term (live, never redacted — the screen shows the truth)
                │
                ├─► live clients (unredacted; the user is looking at their own terminal)
                │
                └─► on block close ──► Redactor ──► redacted text
                                            │
                                ┌───────────┴────────────┐
                                ▼                        ▼
                        scrollback/block store     recall index (§3.3)
```

Two consequences worth stating plainly:

- **The live screen is never redacted.** Redacting what the user is currently
  looking at would be absurd and would break copy/paste. Redaction is a property
  of the *durable* copy.
- **The index can never be less redacted than the store**, because it is derived
  from it. That is enforced by the type system: `IndexUnit.text` is
  `FieldSet<Redacted>` and `Redacted` has exactly one constructor,
  `Redacted::from_raw(&Redactor, &str) -> Redacted`. `omt-recall` does not
  export a way to make one otherwise, and a `tests/no_raw_index.rs` compile-fail
  test (`trybuild`) asserts that `IndexUnit { text: "…".into() }` does not
  compile.

### 5.2 What the detector covers

Pattern list and per-pattern self-tests live in [21](21-data-lifecycle.md); the
classes, so this document is readable on its own:

| Class | Examples | Confidence |
|---|---|---|
| Known key shapes | `ghp_`, `github_pat_`, `gho_`/`ghu_`/`ghs_`/`ghr_`, `glpat-`, `xox[bpsa]-`, `sk_live_`/`sk_test_`, `npm_`, `nf[pcoub]_`, `pul-`, `AKIA`/`ASIA`+16, `AIza`, `ya29.` | high — vendor-defined prefixes |
| Env assignments | `AWS_SECRET_ACCESS_KEY=`, `AWS_SESSION_TOKEN=`, `*_TOKEN=`, `*_SECRET=`, `*_KEY=`, `*_PASSWORD=`, `AZURE_*_KEY=`, `GOOGLE_SERVICE_ACCOUNT_KEY=` | high |
| CLI flags | `--password X`, `--token X`, `-p X` for known clients, `mysql -pX` | medium |
| HTTP headers | `Authorization: Bearer …`, `Authorization: Basic …`, `X-Api-Key:`, `Proxy-Authorization:`, `Cookie:`/`Set-Cookie:` | high |
| Private keys and certs | `-----BEGIN (RSA |EC |OPENSSH |PGP )?PRIVATE KEY-----` … `-----END …-----`, whole block | high |
| JWTs | `eyJ` + two more base64url segments | high |
| Connection strings | `postgres://u:p@h`, `mongodb+srv://…`, `amqp://…` — the password component only | high |
| High-entropy strings | ≥ 24 chars, charset ≥ 40 distinct, Shannon entropy ≥ 4.0 bits/char, not a known hash-shaped token in a git context | **low — see below** |

The atuin design point worth copying exactly: **every pattern carries a
self-test value in the same table entry**, and one `#[rstest]` asserts each
matches both bare and embedded in surrounding text. A denylist without
per-pattern tests rots silently
([research §3.4](../research/completions-and-shell.md#34-privacy-and-secret-redaction)).

### 5.3 False positives, and the policy

High-entropy detection is the only heuristic class, and it will fire on git
SHAs, UUIDs, base64 images, minified JS and checksums. Policy:

- The entropy rule is **on for `output`, off for `command`**. Command text is
  short, human-typed, and covered well by the shape and assignment rules;
  entropy there produces noise. Output is bulk and is where a `kubectl get
  secret -o yaml` lands.
- Context exclusions come first: a token on a line matching `^[0-9a-f]{7,40}\s`
  (git log), inside a ``` fenced block tagged as a diff, or immediately after
  `sha256:`/`Digest:`/`integrity=` is not a secret.
- **Redaction is partial, never whole-line.** Only the matched span is replaced,
  with `‹redacted:kind›` — so `Authorization: ‹redacted:bearer›` retains the
  fact that an auth header was present. Whole-line redaction destroys far more
  recall value than it protects.
- A false positive costs one unfindable token. A false negative costs a secret
  in a file that gets backed up to Dropbox. The asymmetry justifies erring
  toward redaction, and the escape hatch below bounds the cost.

### 5.4 The escape hatch

Three levels, all in [21](21-data-lifecycle.md)'s config block, listed here
because search is where users hit them:

1. `recall.redaction.allow = ["<regex>"]` — spans matching are never redacted.
   Intended for `sha256:`-style project-specific noise.
2. `recall.redaction.entropy = false` — disables only the heuristic class. The
   shape/assignment/header/private-key classes cannot be disabled; that is
   deliberate.
3. `persist_scrollback = false` per session or workspace — nothing is stored, so
   nothing is indexed, and search reports the exclusion (§2.5).

There is no "redact this retroactively, it is already indexed" that is cheaper
than a purge. `data.purge { scope, matching }` ([21](21-data-lifecycle.md))
deletes the doc rows and their FTS entries in one transaction, and that is the
supported remedy.

### 5.5 What redaction cannot catch — stated plainly

- **A secret with no shape.** A 12-character database password, a customer name,
  an internal hostname, the contents of a private document the agent read. No
  pattern finds these.
- **Secrets in agent-authored prose.** If the model repeats a key back in an
  explanation, the shape rules catch it; if it paraphrases one, nothing does.
- **Structured dumps.** `kubectl get secret -o yaml` gives base64 blobs that
  entropy catches; `-o json` with short values may not.
- **Encoding.** A key split across a line wrap, or base64-of-base64, defeats
  span matching.
- **The blob store.** Pasted screenshots ([09](09-ssh-and-media.md)) are not
  text and are not scanned.

Therefore the documented position is: **redaction reduces the blast radius of a
persisted terminal; it is not a guarantee, and omt says so in the onboarding
text and in `omt doctor`.** The real controls are opt-out, retention and purge —
[21](21-data-lifecycle.md).

---

## 6. Command history as a first-class case

[05 §9](05-session-model.md#9-command-history) defines `HistoryEntry` and
`HistoryQuery` and owns them. This section says how history relates to the index
and, crucially, how it relates to the shell.

### 6.1 One store, two queries

History is not a separate corpus. `kind = 0` (block) rows with a non-NULL
`command` *are* the history. `history.query` is `search.query` with
`kinds = [block]`, `fields = [command]`, the history ranking profile (§4.1's
history column), and dedup enabled. One index, one write path, one redactor.

Dedup follows [05 §9](05-session-model.md#9-command-history) and atuin's
performance lesson — a window function inside a subquery so the timestamp-
ordered scan terminates early, never `GROUP BY`:

```sql
SELECT * FROM (
  SELECT d.*, ROW_NUMBER() OVER (
           PARTITION BY d.command, d.attribution ORDER BY d.ts DESC) AS rn
  FROM doc d
  WHERE d.kind = 0 AND d.deleted_at IS NULL AND d.command IS NOT NULL
    AND (:workspace IS NULL OR d.workspace_id = :workspace)
) WHERE rn = 1
ORDER BY /* §4 score, computed in Rust over this candidate set */
LIMIT :limit;
```

Partitioning by `(command, attribution)` and not by `(command, cwd, host)` is a
deliberate divergence from atuin: cwd is a *ranking* signal here, not an identity
signal, so the same command in two directories is one history entry ranked by
where you are. Separate seen-sets per attribution class stop an agent's 40
`cargo test` runs from evicting the human's one.

### 6.2 omt does not own the input line

This is the constraint that makes honesty necessary. Per
[the completions research](../research/completions-and-shell.md#4-syntax-highlighting-of-the-input-line--the-honest-analysis)
and anti-requirement 9, **the shell's line editor owns the input line.** omt
runs a real shell in a real PTY; it does not implement completion, a prompt, or
Ctrl-R.

Therefore:

| Thing | Who owns it | omt's role |
|---|---|---|
| Up-arrow, Ctrl-R, `!!`, `HISTFILE` | the shell | none. omt does not intercept, rewrite or supplement them |
| The shell's history file | the shell | omt never writes to it, and by default never reads it (§6.3) |
| omt's own history | omt | populated from block closure ([05 §8.1](05-session-model.md#81-what-is-snapshotted-vs-replayed)) |
| Recall of a past command | omt | via the **palette** (`⟨leader⟩ /`), the web client, and `omt history` — surfaces omt does own |
| Getting a recalled command onto the shell's line | omt, mechanically | writes the text to the PTY as if typed, `submit: false`, exactly like `session.send_text`. The shell's line editor then owns it |

So the flow is: the user opens omt's palette (an omt surface), picks an entry,
and omt *types* it into the shell's prompt without pressing Enter. That is
position-independent synthetic input under
[D3](decisions.md#d3--synthetic-input-is-bounded-by-state-dependence-not-by-tool-danger)
— it requires no inference about a highlight bar — and it is the whole
integration. omt never claims to have replaced Ctrl-R; it offers a *better,
different* recall surface that also works from a phone, which the shell's does
not.

### 6.3 Importing the shell's history

`omt history import --shell zsh|bash|fish|atuin` is a one-shot, explicit,
previewable command run at onboarding. It reads `HISTFILE` /
`fish_history` / atuin's SQLite, applies the redactor, and inserts rows with
`attribution = Unknown`, `session_id = "imported:<shell>"`, and no cwd/exit
(shells mostly do not record them; atuin does, and its columns are mapped
one-to-one). Imported rows carry `kind_weight × 0.85` because their metadata is
poorer.

It is opt-in and never automatic: reading a user's shell history without being
asked is exactly the kind of thing that erodes trust in a tool that also
persists their terminal.

### 6.4 What omt's history has that the shell's does not

Worth advertising, because it justifies the whole apparatus: exit code and
signal, duration, cwd *and* git root *and* branch, human-vs-agent attribution,
cross-session immediacy (a command run in one pane is queryable in another the
moment its block closes, with a `HistoryAppended` event so open palettes update
live), a link to the block's output, and phone access.

---

## 7. Cross-instance search

### 7.1 The decision

> **D-R3 — Federation happens in the client. Indexes are never replicated
> between instances.**

Each instance indexes only what it observed and is authoritative for it
([00 §7](00-overview.md), [07 §1](07-remote-protocol.md#1-topology-and-federation)).
The web client holds N credentials and fans out.

Why not replicate:

- **There is no server.** Replication needs a rendezvous point or full mesh;
  omt has neither, by design (anti-requirement 3). atuin's per-`(host, tag)`
  append-only encrypted log is the correct shape *if you have a sync server* —
  and omt does not want one.
- **The index is derived data.** Replicating it means replicating the redaction
  decisions, retention policy and purges of one machine onto another. A purge on
  the laptop must not require a reachable server to take effect on the desktop.
- **Fan-out is cheap and already exists.** Queries are small, results are small,
  the client already federates session lists, and the same machinery covers
  search with no new protocol.
- **Locality is a feature.** The work machine's index stays on the work machine.
  That is a compliance answer, for free.

### 7.2 Mechanics

```ts
async function federatedSearch(q: SearchQuery, signal: AbortSignal)
  : Promise<FederatedResults> {
  const targets = registry.connected().filter(i => i.available.has("search.query"));
  const settled = await Promise.allSettled(
    targets.map(i => withTimeout(i.rpc("search.query", q), 2500, signal)));
  // …merge, then re-rank across instances (§7.3)
}

interface FederatedResults {
  groups: SessionGroup[];              // each carries { instance, instanceLabel }
  reached: InstanceId[];
  unreachable: { instance: InstanceId; label: string; reason: string }[];
  timedOut: InstanceId[];
  degraded: { instance: InstanceId; reason: "no_capability" | "older_catalog" }[];
  coverage: Coverage[];                // §2.5, one per instance
}
```

Rules:

- **Results stream in.** Each instance's results render the moment they arrive;
  the list re-sorts as later ones land, with a stable key per hit so nothing
  jumps under a finger mid-tap. Slow instances never block fast ones.
- **Partial results are labelled, never silently dropped.** A banner:
  `3 of 4 instances answered · build-server: timed out (2.5 s) · retry`.
  Requirement R46 is not satisfied by returning the reachable subset without
  saying so.
- **Every hit is attributed** with an instance chip, using the same label and
  colour as the session list ([08 §3](08-web-client.md#3-multi-instance-federation)).
  A `cargo test` on the laptop and on `build-server` are different results.
- **Version skew** is handled by [07 §1.5](07-remote-protocol.md#15-federating-across-versions):
  an instance without `search.query` is listed under `degraded`, not hidden.

### 7.3 Cross-instance ranking

Scores from different instances are comparable because the ranker is a pure
function of the hit and the query context, with one exception: `proximity`
depends on "where the caller is", which is instance-local. The client therefore
sends its focus context (`focused_session`, `focused_workspace`, `cwd`,
`git_root`) in the query, and each instance applies proximity only when the
`git_root` path *string* matches — so a clone of the same repo at the same path
on two machines counts as proximate, and at different paths does not. That is
the honest limit; `search.explain` shows which factor applied.

Ties across instances break toward the instance the user's focused session is
on, then by recency.

---

## 8. Timeline and digest

The highest-value unowned feature. It requires **no new observation** — every
input already exists in [06](06-agent-layer.md)'s `AgentEvent` stream, the block
log and the interaction ledger. It is assembly.

### 8.1 The timeline

```rust
/// Derived, cached, rebuildable from the event log at any time.
pub struct Timeline {
    pub session: SessionId,
    pub binding: Option<BindingId>,
    pub span: Timespan,
    pub entries: Vec<TimelineEntry>,
    pub stats: TimelineStats,       // §8.3
}

pub struct TimelineEntry {
    pub seq: Seq,                   // the source event's per-session seq (P6 causality)
    pub at: Timestamp,
    pub duration: Option<Duration>,
    pub kind: EntryKind,
    /// Collapsed repeats: `Bash(cargo test) ×7` is one entry with n = 7.
    pub repeat: u32,
    /// Present when this entry is a roll-up of finer entries.
    pub children: Vec<TimelineEntry>,
}

pub enum EntryKind {
    SessionStart { agent: AgentKind, model: Option<String>, reason: StartReason },
    Turn { turn_id: Option<String>, prompt: String, origin: Origin,
           outcome: Outcome, tools: u32, files: u32 },
    ToolCall { name: String, summary: String, status: ToolStatus,
               duration: Option<Duration> },
    FileChanged { path: PathBuf, change: Change, tool: Option<String> },
    Block { block: BlockId, command: String, exit: Option<ExitStatus>,
            duration: Option<Duration>, attribution: Attribution },
    Interaction { id: InteractionId, kind: InteractionKindTag,
                  prompt: String, resolution: ResolutionSummary },
    Compaction { trigger: String, before: Option<u64>, after: Option<u64> },
    Usage { delta: UsageDelta },     // §11
    RateLimit { status: String, resets_at: Option<Timestamp>, kind: Option<String> },
    Error { source: SourceId, message: String },
    Note { text: String, by: Actor },        // session.note (G7)
    Gap { idle: Duration },                  // > 10 min of nothing; renders as a divider
    SessionEnd { reason: String },
}

pub struct ResolutionSummary {
    pub outcome: ResolutionOutcome,   // Answered | TimedOut | Cancelled | Abandoned
    pub by: Option<Actor>,            // who — and therefore which device
    pub device: Option<String>,       // "iPhone", "laptop TUI" — from presence (12)
    pub response: Option<String>,     // the chosen label(s), verbatim
    pub latency: Option<Duration>,    // open → resolved
    pub fidelity: Option<Fidelity>,   // Native | Synthetic — always shown (D3)
}
```

Construction rules:

- **Turns are the spine.** `TurnStart`…`TurnEnd` bracket everything between
  them; entries with a `turn_id` become that turn's `children`. Events with no
  turn (human blocks, session-level events) are siblings at the top level.
- **Repeat collapsing.** Consecutive `ToolCall`s with the same `(name, canonical
  input)` collapse into one entry with `repeat = n`. Canonicalization is
  whitespace-normalized JSON with volatile fields (timestamps, ids) dropped.
  This is the same computation the stuck detector uses (§10) — one
  implementation, two consumers.
- **Gaps.** More than `timeline.gap_threshold` (default 10 min) with no event
  inserts a `Gap`, so a 9-hour overnight session does not render as an
  undifferentiated wall.
- **Subagents** are `children` of the tool call that spawned them, keyed by
  `thread.parent` ([06 §8](06-agent-layer.md#8-ancillary-semantics)).
- **Errors** include source-level failures (a transcript reader disabling itself)
  because "the timeline is incomplete and here is why" beats a silent hole.

Storage: the timeline is **not** a stored artifact. It is computed from `doc`
rows (`doc_sess_seq` index) plus the block log and interaction ledger, and
memoized per session with an invalidation on new events. A session with 20 000
events builds its timeline in well under the §13 budget, and rebuilding from the
log means a timeline can never disagree with the events.

### 8.2 The digest

The digest answers **"what happened while I was away"** for one session, or for
an instance/workspace over a window. It is composed by counting.

> **D-R4 — The digest is assembled from counts and templates. omt does not
> generate prose. Where an agent supplies its own natural-language summary, omt
> displays it verbatim, in quotation marks, attributed to the agent.**

```rust
pub struct Digest {
    pub scope: DigestScope,           // Session | Workspace | Instance | All
    pub window: Timespan,
    pub headline: String,             // template-composed, §8.2.1
    pub sections: Vec<DigestSection>,
    /// Agent-authored text, verbatim. Never edited, never truncated in the
    /// middle of a sentence, always attributed.
    pub agent_summaries: Vec<AgentSummary>,
    pub counts: DigestCounts,
}

pub struct AgentSummary {
    pub session: SessionId,
    pub agent: AgentKind,
    pub source: &'static str,         // "away_summary", "TurnEnd.last_message"
    pub text: String,
    pub at: Timestamp,
}

pub struct DigestCounts {
    pub duration: Duration,           // wall clock, first to last event
    pub working: Duration,            // summed time in AgentState::Working
    pub turns: u32,
    pub tool_calls: u32,
    pub files_changed: u32,           // distinct paths
    pub files_by_change: BTreeMap<Change, u32>,
    pub blocks: u32,
    pub blocks_failed: u32,
    pub interactions_opened: u32,
    pub interactions_answered_by: BTreeMap<ActorKind, u32>,
    pub interactions_timed_out: u32,
    pub compactions: u32,
    pub errors: u32,
    pub usage: Option<UsageTotals>,   // §11 — only if the agent reported it
    pub rate_limited: Option<RateLimitState>,
    pub ended: Option<(Timestamp, Outcome)>,
}
```

#### 8.2.1 Composition rules — exact

The headline is built by concatenating clauses from a fixed table, **in this
order**, omitting any clause whose count is zero or whose data is absent. Each
clause is a `const` format string; the set is closed and enumerated in
`crates/omt-recall/src/digest/clauses.rs`, one test per clause.

| # | Condition | Clause |
|---|---|---|
| 1 | always | `{duration:h m}` (e.g. `4 h 12 m`) |
| 2 | `working < duration * 0.8` | `({working} working)` |
| 3 | `turns > 0` | `{turns} turn(s)` |
| 4 | `files_changed > 0` | `{files_changed} file(s) changed` |
| 5 | `interactions_opened > 0` | `{opened} question(s)` + `, {n} answered from {devices}` + `, {n} timed out` |
| 6 | `blocks_failed > 0` | `{blocks_failed} of {blocks} commands failed` |
| 7 | `compactions > 0` | `{compactions} compaction(s)` |
| 8 | `usage.cost_usd` reported | `{cost} reported by {agent}` |
| 9 | `rate_limited.is_some()` | `rate limited until {resets_at}` |
| 10 | `errors > 0` | `{errors} observation error(s)` |
| 11 | `ended` | `finished at {time} ({outcome})` — else `still running` |

Pluralization is a small closed function, not a library. Durations render as
`4 h 12 m`, `47 s`, `2 d 3 h` — never `4.2 hours`. Times render in the viewer's
local zone with the instance's zone appended when they differ.

Rules that keep it honest:

- **A number omt did not count is not shown.** No "approximately", no
  interpolation, no carrying a stale value forward.
- **A zero is an omission, not a "0".** `0 files changed` is noise.
- **Unknown ≠ zero.** If an agent reports no usage, the cost clause is absent
  and the usage section reads `not reported by {agent}` (§11.4) — never `$0.00`.
- **No judgement words.** Not "successfully", not "productive", not "stuck".
  The stuck signal (§10) is a separate, explicitly-thresholded object.

#### 8.2.2 Example output — overnight session (J40)

```
claude · feat/payments-retry · desktop-2

4 h 12 m (3 h 51 m working) · 38 turns · 47 files changed · 3 questions
(2 answered from iPhone, 1 timed out) · 6 of 41 commands failed ·
2 compactions · $4.18 reported by Claude Code · finished 03:24 (completed)

Claude Code's own summary, 03:24:
  "Retry logic is implemented for the three payment providers and the
   integration tests pass. Stripe's idempotency-key handling still needs a
   decision — I used a per-attempt key, which may double-charge on a
   partial failure. Next step is confirming that with the payments team."

Files          38 modified · 8 created · 1 deleted        [ view diffstat ]
Commands       41 run · 6 failed
               cargo test ×12 (2 failed, then passed)
               cargo clippy ×5 (all passed)
               git status ×9
Questions      02:41  "Which retry backoff?"  → "Exponential, 5 attempts"
                      answered by you on iPhone, 4 m 12 s later, native
               03:02  "Overwrite the fixture?" → timed out after 5 m,
                      agent proceeded with its default
               03:19  "Run the migration?"     → "No", answered on iPhone
Context        compacted twice: 187k → 42k (02:10), 191k → 48k (03:07)
Usage          in 1 204 331 · out 88 402 · cache read 8 991 022
               $4.18 · reported by Claude Code · omt computes no prices
Rate limits    five_hour: allowed · resets 06:30
```

Every figure above is a `COUNT(*)`, a `SUM`, or a field the agent sent. The one
paragraph of prose is Claude Code's `away_summary`
([06 §8](06-agent-layer.md#8-ancillary-semantics)), quoted and attributed.

#### 8.2.3 Example — instance digest, no agent reported anything

```
laptop · since 18:00 yesterday · 4 sessions

13 h 47 m · 2 agent sessions · 61 files changed · 12 of 208 commands failed ·
still running

aider · fix/parser         2 h 04 m · 6 files · 1 question (answered, TUI)
                           usage not reported by Aider
crush · spike/wasm         0 h 51 m · 3 files · usage not reported by Crush
zsh · omt (2 sessions)     208 commands · 12 failed
                           longest: cargo build --release (4 m 31 s)

No agent-authored summaries available for these sessions.
```

This is the degraded case and it is still useful. `usage not reported` appears
once per agent, not per session, and links to a doc page explaining which agents
report what.

#### 8.2.4 Multi-session morning digest

Scope `All` with window "since I was last connected" (from presence, [12](12-collaboration.md)) is the
literal answer to J40. Sessions are ordered by *needs-you first*, then by
activity: `Blocked` sessions, then sessions that ended with `Outcome::Error`,
then sessions with failed commands, then the rest. That ordering is the digest's
only editorial act, and it is a fixed rule, not a judgement.

### 8.3 Rendering per surface

**Phone (the most valuable place for it).** The digest is the *entry screen*
when the app is opened after ≥ 30 minutes away — before the session list, not
buried in a tab. One card per session, headline clause chain wrapped to 3 lines
max with the rest behind "more", the agent's verbatim summary in a quoted block
with the agent's name and icon, and two thumb-sized actions: **Open session**
and **Timeline**. Counts are tappable and each drills into the filtered
timeline. A `Blocked` session's card carries the interaction card inline, so
answering is one tap from the digest — which is the whole product in one
gesture. Web Push notification text is the headline clause chain truncated to
120 characters, never generated.

**Desktop web.** The digest is a two-column panel: counts and sections left,
timeline right, scroll-linked — clicking `6 of 41 commands failed` scrolls the
timeline to the first failure and filters to failures. The timeline is
virtualized (`@tanstack/virtual`, per [08](08-web-client.md)) because a long
session has tens of thousands of entries.

**TUI.** `⟨leader⟩ t` opens the timeline in a pane as a collapsible tree;
`⟨leader⟩ g` prints the digest. `omt digest` and `omt timeline <session>` render
the same content to stdout, with `--json` for scripting. The digest is plain
text with no box drawing so it pipes and pastes cleanly — this is the form users
will paste into a standup channel, so it is designed to be pasted.

---

## 9. Comparing parallel agent results (G18)

Three worktrees, three agents, one task. The workflow ends in a comparison omt
currently does not support.

> **D-R5 — omt presents comparable facts side by side. It does not rank,
> score, recommend, or judge which result is better.**

That is not modesty; judging requires understanding the code, which requires a
model (P4), and a plausible-looking wrong recommendation about which branch to
keep is exactly the failure mode that would cost a user real work.

```rust
pub struct ComparisonRow {
    pub session: SessionId,
    pub instance: InstanceId,
    pub workspace: WorkspaceId,
    pub branch: Option<String>,
    pub base_ref: Option<String>,           // resolved merge-base, §9.1
    pub agent: Option<AgentKind>,
    pub model: Option<String>,
    pub state: AgentState,
    pub elapsed: Duration,

    pub diffstat: Option<DiffStat>,         // files, insertions, deletions (15 §3.2)
    pub files_touched: Vec<PathBuf>,        // union: git diff ∪ AgentEvent::FileChanged
    pub files_disagreement: Vec<PathBuf>,   // in one set and not the other — §9.2

    pub test_runs: Vec<TestRun>,            // §9.3
    pub timeline_entries: u32,
    pub turns: u32,
    pub interactions: u32,
    pub interactions_timed_out: u32,
    pub failed_blocks: u32,
    pub usage: Option<UsageTotals>,         // agent-reported only
    pub agent_summary: Option<AgentSummary>,// verbatim
}
```

### 9.1 The base ref

`git merge-base <session branch> <base>` where `base` is, in order: the user's
explicit choice, the launch configuration's recorded base
([10 §9.1](10-configuration.md#91-launch-configurations)), the repo's default
branch, else `HEAD` at workspace open. The resolved ref is **displayed**, because
a diffstat against an unstated base is meaningless. Read-only git only — no
mutation ([15 §1.1](15-workspace-explorer.md), anti-requirement 6).

### 9.2 Files touched, and the disagreement column

Two sources: `git diff --name-status <base>...HEAD` (truth about the tree) and
the union of `AgentEvent::FileChanged` (attribution — what the agent *says* it
did). They differ, and the difference is informative: a file in the agent's set
but not in git's was reverted or written identically; a file in git's set but not
the agent's was changed by something else (a build, a formatter, the user). Both
are shown, with the disagreement called out rather than reconciled.
[15 §8.3](15-workspace-explorer.md#83-files-the-agent-changed-this-session) owns
the per-session view; this reuses it.

### 9.3 Tests run, and their outcome

Detected without any test-framework integration, from block records only:

```rust
pub struct TestRun {
    pub block: BlockId,
    pub command: String,          // verbatim
    pub exit: Option<ExitStatus>,
    pub duration: Option<Duration>,
    pub detected_by: DetectedBy,  // ConfigPattern | Builtin
}
```

A block is a test run if its command matches `recall.test_patterns` — default
`^(cargo (test|nextest)|npm (test|run test)|pnpm test|yarn test|go test|pytest|jest|vitest|mix test|bundle exec rspec|ctest|make test)\b`.
The **outcome is the exit code**, nothing else. omt does not parse test output
to count passed/failed assertions: every framework's format differs, they change
between versions, and a wrong "12 passed" is worse than an honest
`exit 0`/`exit 101`. If no block matches, the cell reads `no test command
detected` — not "tests failed", not blank.

### 9.4 The view

Web/TUI: dashboard multi-select → **Compare**. A table with one column per
session, one row per fact, differing cells emphasized. Below it, each session's
agent summary verbatim, side by side. A single **Open diff** action per column
routes to the explorer's existing diff view. There is no "pick this one" button,
and the empty space where it would go is the point.

---

## 10. Detecting a stuck or looping agent (G19)

`working` for 22 minutes is indistinguishable from `working` for 20 seconds
except by elapsed time. All the signals needed are already counted; none require
screen-scraping.

### 10.1 The detector

```rust
pub struct StuckDetector {
    cfg: AttentionConfig,
    per_binding: HashMap<BindingId, BindingStats>,
}

pub struct BindingStats {
    /// Rolling p50/p90 of this binding's own completed turn durations.
    turn_duration: Quantiles,
    /// Rolling p90 of this binding's own gaps between consecutive events.
    event_gap: Quantiles,
    working_since: Option<Timestamp>,
    last_event_at: Timestamp,
    /// Canonicalized recent tool calls (§8.1's canonicalizer, shared).
    recent_calls: VecDeque<(u64 /*hash*/, Timestamp, ToolStatus)>,
    consecutive_failures: u32,
    tokens_since_file_change: u64,
    last_file_change_at: Option<Timestamp>,
}

pub struct AttentionSignal {
    pub session: SessionId,
    pub binding: BindingId,
    pub reasons: Vec<StuckReason>,   // all that fired, never just the first
    pub since: Timestamp,
    pub confidence: Confidence,      // Weak | Strong  — see §10.3
    /// Rendered text, template-composed. e.g. "Bash(cargo test) ×7".
    pub detail: String,
}

pub enum StuckReason {
    /// Elapsed in `working` far beyond this binding's own baseline.
    LongWorking { elapsed: Duration, baseline_p90: Duration, ratio: f64 },
    /// Same canonicalized tool call n times within a window.
    RepeatedCall { name: String, canonical: String, count: u32, window: Duration },
    /// n consecutive tool results or blocks with an error/non-zero exit.
    RepeatedFailure { count: u32, last: String },
    /// No event of any kind for longer than this binding's p90 gap × k.
    Silent { since: Duration, baseline_p90: Duration },
    /// Usage climbing with no FileChanged and no assistant text.
    BurnWithoutProgress { tokens: u64, since: Duration },
}
```

### 10.2 Thresholds and tunability

Defaults, all in `[attention]` config ([10](10-configuration.md)):

| Reason | Default trigger |
|---|---|
| `LongWorking` | `elapsed > max(baseline_p90 × 4, 8 min)` |
| `RepeatedCall` | `count ≥ 4` identical canonical calls within 10 min |
| `RepeatedFailure` | `count ≥ 3` consecutive failures |
| `Silent` | `no event for > max(event_gap_p90 × 6, 5 min)` while `Working` |
| `BurnWithoutProgress` | `≥ 150 000 tokens` reported since the last `FileChanged`, and `≥ 10 min`, and no `AssistantText` in that window |

**Relative to the binding's own baseline, not to a global constant.** A session
running a 40-minute test suite has a p90 turn duration measured in tens of
minutes and will not fire; a session that normally answers in 20 seconds and has
been silent for 6 minutes will. Baselines need ≥ 8 completed turns before the
relative rules arm; until then only the absolute floors apply.

### 10.3 Output: a signal, never an action

> **D-R6 — The detector produces an attention signal. It never interrupts,
> never prompts the agent, never answers anything, and never kills a session.**

Auto-interrupting would be omt exercising policy over the agent, against the
spirit of [D1](decisions.md#d1--omt-adds-no-policy-layer-over-an-agents-permission-semantics),
and it would eventually kill a long-but-fine job. The user interrupts, from any
surface, with `agent.interrupt`.

Rendering: an amber chip on the session card with the reason's `detail`
(`Bash(cargo test) ×7`, `no output 11 m`), and the dashboard's Working section
sorted by elapsed. It is a *chip*, not a modal, not a sound. `Confidence::Strong`
(two or more reasons firing, or `RepeatedCall` with `count ≥ 8`) is the only
level that may raise a push notification, and only if the user opted into
`attention.push = true`, default off.

### 10.4 Not crying wolf

- **Hysteresis.** A signal clears only after `2 × ` the trigger condition stops
  holding, so a chip does not blink.
- **One signal per binding.** Reasons accumulate into `reasons`; they do not
  each produce a separate alert.
- **Never during `Blocked`.** A session waiting on a human is not stuck; it is
  waiting, and it already has a much louder affordance.
- **Snooze, per binding, per reason**, for 30 min / this session / never for
  this session. Snoozes are persisted with the binding.
- **Known-slow commands are exempt** from `Silent`: a `ToolCall` currently
  running whose command matches `attention.slow_commands` (default: `cargo
  build|cargo test|make|docker build|npm ci|bazel|pytest`) suppresses `Silent`
  and `LongWorking` while it runs. The block is running; that is not silence.
- **`omt attention explain <session>`** prints every reason's inputs, the
  binding's baselines, and which thresholds were compared — the same
  falsifiability discipline as `agent.explain`
  ([06 §4](06-agent-layer.md#4-merging-confidence-tiers-not-voting)).

---

## 11. Usage and cost (G15)

### 11.1 The rule, stated where nobody can miss it

> **D-R7 — omt displays only what the agent reported. omt holds no price table,
> applies no rate, and computes no cost. Where an agent reports tokens but not
> money, omt shows tokens and says the cost was not reported. It does not
> multiply.**

Prices change without notice, plans differ (subscription vs. API vs. enterprise),
caching and batch discounts move the number, and a confidently wrong dollar
figure on a dashboard is worse than an absent one. Every cost figure in every
surface carries the attribution `reported by <agent>`, and a tooltip: *"omt does
not calculate prices."*

Out of scope, explicitly: budgets, spend alerts, throttling, and halting an
agent when a threshold is crossed. Those are policy over the agent (D1), and
omt cannot bill anything.

### 11.2 Normalization

Source data, per [the research](../research/agent-clis.md):

| Agent | Tokens | Cost | Context window | Rate limits |
|---|---|---|---|---|
| Claude Code | `assistant.message.usage` (`input_tokens`, `output_tokens`, `cache_creation_input_tokens`, `cache_read_input_tokens`), `result.modelUsage` per model | **yes** — `total_cost_usd`, `modelUsage[*].costUSD` | `modelUsage[*].contextWindow` | **yes** — `rate_limit_event` `{status, resetsAt, rateLimitType, overageStatus, isUsingOverage}` |
| Codex | `event_msg.token_count`: `total_token_usage` / `last_token_usage` with `input`, `cached_input`, `cache_write`, `output`, `reasoning_output` | no | `model_context_window` | `rate_limits` `{limit_id, credits}` |
| opencode | `session.tokens_{input,output,reasoning,cache_read,cache_write}`, `message.tokens` | `session.cost` | no | no |
| Gemini / Qwen | **uncertain** | uncertain | uncertain | uncertain |
| Cursor | **uncertain** | uncertain | uncertain | uncertain |
| Amp, Aider, Crush, Goose | none observed | none | none | none |

```rust
pub struct UsageEvent {
    pub session: SessionId,
    pub binding: BindingId,
    pub thread: ThreadRef,           // subagent-aware
    pub at: Timestamp,
    pub model: Option<String>,
    pub tokens: Tokens,
    /// Present only when the agent stated it. `None` is never rendered as 0.
    pub cost_usd: Option<f64>,
    pub context_window: Option<u64>,
    /// Whether the numbers are cumulative-for-session or a delta.
    pub accounting: Accounting,      // Cumulative | Delta
    pub reported_by: AgentKind,
    pub source: SourceId,            // which EventSource observed it
}

pub struct Tokens {
    pub input: u64, pub output: u64,
    pub cache_read: u64, pub cache_write: u64,
    pub reasoning: u64,
}
```

**Cumulative vs. delta is the one real hazard.** Claude Code's per-message
`usage` is per-request; Codex's `total_token_usage` is cumulative for the thread;
opencode's `session.tokens_*` are cumulative for the session. Summing a
cumulative series double-counts by orders of magnitude. So the adapter declares
`accounting` per source, and the store normalizes on write: `Cumulative` events
are converted to deltas against the previous value **for the same
`(binding, thread, model)` key**, with a negative delta (a reset, a `/clear`)
recorded as a new baseline and a `UsageReset` marker on the timeline rather than
a negative number. This rule has its own fixture test per agent.

### 11.3 Storage and query

Usage events are `doc`-adjacent, in their own narrow table (they are numbers,
not text, and they are written at a much higher rate):

```sql
CREATE TABLE usage_event (
  id INTEGER PRIMARY KEY,
  session_id TEXT NOT NULL, binding_id TEXT, thread_id TEXT,
  workspace_id TEXT, ts INTEGER NOT NULL,
  agent TEXT NOT NULL, model TEXT,
  d_input INTEGER NOT NULL DEFAULT 0, d_output INTEGER NOT NULL DEFAULT 0,
  d_cache_read INTEGER NOT NULL DEFAULT 0, d_cache_write INTEGER NOT NULL DEFAULT 0,
  d_reasoning INTEGER NOT NULL DEFAULT 0,
  cost_usd REAL,                 -- NULL means NOT REPORTED, not zero
  context_window INTEGER,
  deleted_at INTEGER
);
CREATE INDEX usage_sess_ts ON usage_event(session_id, ts);
CREATE INDEX usage_ws_ts   ON usage_event(workspace_id, ts);
CREATE INDEX usage_day     ON usage_event(ts);

CREATE TABLE rate_limit_state (
  agent TEXT NOT NULL, kind TEXT NOT NULL,     -- 'five_hour', 'codex', …
  status TEXT NOT NULL, resets_at INTEGER,
  detail TEXT, observed_at INTEGER NOT NULL,
  PRIMARY KEY (agent, kind)
);
```

`usage.query { scope, since, until, group_by }` where `scope` is
`Session | Workspace | Instance | All` and `group_by` is
`Day | Session | Workspace | Agent | Model`. Output:

```rust
pub struct UsageReport {
    pub rows: Vec<UsageRow>,
    pub totals: UsageTotals,
    /// Agents active in the window that reported nothing. Rendered explicitly.
    pub not_reporting: Vec<AgentKind>,
    /// True if any row in the window had `cost_usd = NULL`. Forces the
    /// "partial" label on any total; a total over a mixed set is a lie
    /// without it.
    pub cost_partial: bool,
}
```

### 11.4 Rendering, including the empty case

```
Today · all instances

Tokens     in 3 204 118 · out 214 883 · cache read 22 118 402 · reasoning 41 002
Cost       $9.41 reported by Claude Code · partial — 2 agents reported no cost
           omt does not calculate prices.

By session
  claude · feat/payments      1 204 331 in    88 402 out   $4.18
  claude · fix/parser           901 002 in    61 118 out   $3.09
  codex  · spike/wasm           788 441 in    41 233 out   cost not reported
  aider  · docs                 —                          usage not reported

Rate limits
  Claude Code · five_hour   allowed · resets 06:30 (in 2 h 11 m)
  Codex       · codex       credits 41% remaining · observed 04:12
```

When an agent reports nothing at all, the row reads `usage not reported` and the
agent appears once in a footnote listing which agents expose usage data and
which do not, linking to the reference page. It never reads `$0.00`, never
`0 tokens`, and there is never an omt-computed estimate beside it.

**Rate limits (R74)** are surfaced in three places: the session card while
`status != allowed`, the instance chip, and the digest's headline clause 9. When
`resets_at` is in the past, the state is shown as `stale (observed HH:MM)`
rather than silently cleared — a stale limit is information.

---

## 12. Capabilities

Declared per [03 §2](03-capability-catalog.md#2-declaring-a-capability). All
`Query` unless stated. `V`iewer < `O`perator < `A`dmin.

### 12.1 `search.*`

| Capability | Kind | Role | Input | Output | Effects |
|---|---|---|---|---|---|
| `search.query` | Q | V | `SearchQuery` | `SearchResults` | — |
| `search.explain` | Q | V | `{ query: SearchQuery, doc: DocId }` | `{ factors: [(name, f64)], total: f64, bm25: f64 }` | — |
| `search.suggest` | Q | V | `{ prefix, scope, limit }` | `{ terms: [String], recent_queries: [String] }` | — |
| `search.stats` | Q | V | `{}` | `{ docs, bytes, oldest, newest, by_kind, excluded_sessions }` | — |
| `search.reindex` | C | **A** | `{ scope, since? }` | `{ job: JobId }` | `READS_FS`, `WRITES_FS` |

```rust
pub struct SearchQuery {
    pub text: String,                     // may embed `field:` and `-negation`
    pub fields: Option<Vec<Field>>,       // default per §2.4
    pub kinds: Option<Vec<DocKind>>,
    pub scope: SearchScope,               // All | Workspace(id) | Session(id) | Cwd(path)
    pub time: Option<Timespan>,
    pub attribution: Option<AttributionFilter>,
    pub exit: Option<ExitFilter>,         // Any | Success | Failure
    pub agent: Option<AgentKind>,
    pub path_prefix: Option<PathBuf>,
    pub mode: MatchMode,                  // Literal | Prefix | Phrase | Fuzzy
    pub context: FocusContext,            // §7.3 — for proximity scoring
    pub group_by_session: bool,           // default true
    pub limit: u32,
    pub cursor: Option<SearchCursor>,
}

pub struct SearchResults {
    pub groups: Vec<SessionGroup>,        // §4.2
    pub total_estimate: u64,
    pub coverage: Coverage,               // §2.5
    pub took_ms: u32,
    pub cursor: Option<SearchCursor>,
}

pub struct Hit {
    pub doc: DocId, pub kind: DocKind,
    pub session: SessionId, pub block: Option<BlockId>,
    pub at: Timestamp, pub score: f64,
    /// FTS5 snippet with match offsets, already redacted by construction.
    pub snippet: String,
    pub match_ranges: Vec<Range<usize>>,
    /// Everything needed to jump: pane, scrollback position, timeline seq.
    pub anchor: Anchor,
}
```

The query mini-language is small and documented: `field:term`, `-term`,
`"exact phrase"`, `term*`, `path:crates/omt-term`, `exit:fail`, `by:agent`,
`by:me`, `since:yesterday`, `in:workspace`, `out:` (opt into output). Anything
unrecognized is treated as a literal term, never an error — a search box that
rejects input is a bad search box.

### 12.2 `history.*`

| Capability | Kind | Role | Input | Output |
|---|---|---|---|---|
| `history.query` | Q | V | `HistoryQuery` ([05 §9](05-session-model.md#9-command-history)) | `{ entries, next_cursor }` |
| `history.get` | Q | V | `{ id }` | `HistoryEntry` + `{ block, output_available }` |
| `history.import` | C | O | `{ shell, path?, dry_run }` | `{ imported, skipped, redacted, preview }` — `READS_FS`, `WRITES_FS` |
| `history.forget` | C | O | `{ matching: HistorySelector }` | `{ removed }` — `DESTRUCTIVE`, `WRITES_FS` |

`history.forget` deletes rows *and* their FTS entries in one transaction and is
the targeted counterpart to [21](21-data-lifecycle.md)'s bulk purge; the
"I just pasted a token into a command" case needs to be one command, not a
retention-policy change.

### 12.3 `timeline.*` / `digest.*`

| Capability | Kind | Role | Input | Output |
|---|---|---|---|---|
| `timeline.get` | Q | V | `{ session, from_seq?, limit, collapse: bool, kinds? }` | `{ entries, stats, next_seq }` |
| `timeline.stats` | Q | V | `{ session }` | `TimelineStats` |
| `digest.get` | Q | V | `{ scope, window }` | `Digest` |
| `digest.since_last_seen` | Q | V | `{ device? }` | `Digest` — window from presence |
| `compare.sessions` | Q | V | `{ sessions: [SessionId], base_ref? }` | `{ rows: [ComparisonRow], base_resolved }` |

`compare.sessions` is single-instance; multi-instance comparison is client-side
fan-out over the same capability, merged into one table (§7).

### 12.4 `usage.*`

| Capability | Kind | Role | Input | Output |
|---|---|---|---|---|
| `usage.query` | Q | V | `{ scope, since, until?, group_by }` | `UsageReport` |
| `usage.limits` | Q | V | `{ agent? }` | `{ limits: [RateLimitState] }` |
| `usage.session` | Q | V | `{ session }` | `{ totals, by_model, context_window, last_event }` |

### 12.5 `attention.*`

| Capability | Kind | Role | Input | Output |
|---|---|---|---|---|
| `attention.list` | Q | V | `{ scope }` | `{ signals: [AttentionSignal] }` |
| `attention.explain` | Q | V | `{ session }` | `{ baselines, reasons, thresholds, snoozes }` |
| `attention.snooze` | C | O | `{ session, reason?, until }` | `{ snoozed_until }` |
| `attention.clear` | C | O | `{ session }` | `{ cleared: u32 }` |

Events emitted on the shared bus ([03 §4](03-capability-catalog.md#4-events-are-the-read-side-twin)):
`HistoryAppended`, `DocIndexed { session, kind, seq }` (coalesced, ≤ 1/s per
session), `AttentionRaised`/`AttentionCleared`, `UsageUpdated { session,
totals }` (coalesced, ≤ 1/2 s), `DigestAvailable { scope }`.

---

## 13. Performance budgets and backpressure

### 13.1 Budgets

Measured on the reference machine (2021 M1 Pro, NVMe) in
`crates/omt-recall/benches/`; CI fails on a > 20% regression.

| Operation | Budget | Notes |
|---|---|---|
| Index one closed block (12 KB window) | **< 400 µs** p99, in-transaction | dominated by FTS5 tokenization |
| Index one agent message (2 KB) | < 120 µs p99 | |
| Redaction pass over 12 KB | **< 250 µs** p99 | one compiled `RegexSet` pass + entropy scan |
| Steady-state indexer CPU, hot stream | **< 3% of one core** | see §13.2 for how this is held |
| `search.query`, 1 M docs, single term, scoped | **< 40 ms** p95 | |
| `search.query`, 1 M docs, unscoped fuzzy | < 150 ms p95 | FTS narrow + Rust rank |
| `history.query` (palette keystroke) | **< 15 ms** p95 | the strictest, because it is per-keystroke |
| `timeline.get`, 20 000-event session, first page | < 80 ms p95 | |
| `digest.get`, 24 h, 8 sessions | < 120 ms p95 | pure aggregation |
| Index size | **≤ 12% of persisted scrollback bytes** | if exceeded, the output window is the dial |

The per-keystroke budget is why prefix indexes exist and why the palette
debounces at 40 ms and cancels in-flight queries — never queues them.

### 13.2 The hot stream, and backpressure

A `cargo build` at full tilt produces tens of MB/s. The indexer must never be in
that path.

**Rule 1 — indexing is off the PTY path entirely.** `omt-term` consumes PTY
bytes and closes blocks; block closure enqueues an `IndexUnit` onto a bounded
channel. The PTY reader never touches SQLite. This is the same discipline as
[05](05-session-model.md)'s "the state machine never spawns a task".

**Rule 2 — the queue is bounded and shedding, not blocking.**

```rust
pub struct IndexQueue {
    cap: usize,                     // recall.queue_capacity, default 4096 units
    dropped: Counter,
    policy: ShedPolicy,
}

pub enum ShedPolicy {
    /// Drop the *output* field of the oldest queued unit, keeping its metadata
    /// and command. Applied first: it reclaims ~99% of the bytes and loses the
    /// least valuable field.
    ShedOutput,
    /// Then drop whole low-value units, oldest first, in this order:
    /// tool_call → assistant_msg → file_change → block-output → never
    /// (command, user_msg, interaction, session_meta are never dropped).
    ShedUnits,
}
```

> **The backpressure rule: when output outpaces the indexer, omt degrades the
> *index*, never the *terminal*. Dropped units are counted, surfaced in
> `search.stats` as `deferred`, and re-indexed by a low-priority background pass
> when the stream goes quiet. The terminal never stalls and the block record
> itself is never dropped.**

Re-indexing is possible precisely because the block record and its scrollback
are already durable — the index is derived data, so a shed unit is a *latency*
problem, not a *loss* problem. `search.stats.deferred > 0` renders as a small
"indexing (N pending)" indicator, and `search.reindex` forces it.

**Rule 3 — one writer, batched.** All index writes go through a single task with
a `WAL`-mode connection, `synchronous = NORMAL`, batching per §3.3. Readers use
separate connections from a pool and never block the writer (WAL gives that).

**Rule 4 — big blocks are truncated at the source.** A block whose output
exceeds `recall.max_block_output` (default 32 MiB) has its window taken from the
first 4 KiB and the last 8 KiB of the *stream* without materializing the middle,
so a 4 GB log costs 12 KiB of index and one pass of redaction.

---

## 14. Open questions

1. **History scope across worktrees.** Carried from
   [05 §13.3](05-session-model.md#13-open-questions). §4.1 proposes `git_root`
   proximity ×1.7 as the answer — same repo, different worktree, ranked just
   under same-workspace. Needs agreement with 05 and 15, and the `git_root` of a
   linked worktree must resolve to the *main* repo, which needs verification
   against `git rev-parse --path-format=absolute --git-common-dir`.
2. **CJK and other unsegmented scripts.** `unicode61` does not segment Chinese
   or Japanese, so CJK search degrades to whole-run matching. Options: ship
   FTS5's `trigram` tokenizer as a second index for CJK-detected text (roughly
   doubles index size for that text), or accept the limitation. Undecided;
   P10 keeps the *UI* English but users' content is not.
3. **Does the output window actually catch errors?** The 4 KiB-head /
   8 KiB-tail assumption is based on the shape of compiler and test output. It
   should be measured against a real corpus before the defaults are frozen —
   `rustc`'s error summary is at the tail, but `make -j` interleaves.
4. **Entropy detection's real false-positive rate** on ordinary developer output
   (git logs, `npm ls`, base64 assets, docker digests). Needs a measured figure
   before the entropy rule stays on-by-default for `output`.
5. **Timeline memoization invalidation.** Recomputing a 20 000-event timeline on
   every new event is wasteful; incremental append is easy for the tail but the
   repeat-collapsing rule (§8.1) can retroactively merge the last two entries.
   Needs a defined incremental algorithm or a bounded-recompute-window rule.
6. **`digest.since_last_seen` when the device is new.** A phone added this
   morning has no "last seen". Fall back to 12 hours? To the last session
   activity gap? Currently unspecified.
7. **Does Gemini/Qwen or Cursor report usage at all?** Both are `UNCERTAIN` in
   the research. Until verified, they fall into §11.4's "not reported" path,
   which is correct but may be needlessly pessimistic.
8. **Cumulative-vs-delta on resume.** When Claude Code resumes a session, does
   `total_cost_usd` restart or continue? If it restarts, §11.2's reset rule
   turns a resume into a new baseline and the session total under-reports.
   Needs a measured answer, and a `UsageReset` marker either way.
9. **Cross-instance dedup.** The same repo on two machines produces near-
   identical history. Should federated results dedup by `(command, git_root)`
   across instances, or is per-machine attribution more valuable? Current lean:
   no dedup, because "which machine did I run it on" is usually the question.
10. **Should `search.query` be exempt from the writer-token discipline for
    `Orphaned` sessions?** It already is — search is a read — but jumping to a
    hit in an orphaned session raises the "restart?" affordance
    ([05 §8.2](05-session-model.md#82-crash-semantics)), and where that prompt
    belongs in the search flow is unspecified.
11. **Digest as a push payload.** §8.3 truncates the headline to 120 characters
    for Web Push. Whether the phone should instead receive a structured payload
    and compose locally (better, but duplicates the clause table in TypeScript)
    is open. Codegen from the clause table is the likely answer.
