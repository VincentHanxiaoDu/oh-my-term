# Data Lifecycle: what omt writes, for how long, and how to destroy it

omt watches everything you type and everything your agents print, and it writes
a lot of that down. After six months of daily use it holds gigabytes about every
project the user has touched — including work under NDA, `.env` dumps that
scrolled past, and `kubectl get secret -o yaml` output nobody meant to keep.

This document is the complete accounting of that, and the design for getting it
back out or destroying it. It is the owner of gaps **G4** (data lifecycle) and
**G9** (privacy of persisted output) from
[docs/design/scenarios.md](../design/scenarios.md), and of requirements R47,
R55, R56 and R57.

Related: [01 — Principles](01-principles.md) ·
[03 — Capability catalog](03-capability-catalog.md) ·
[05 — Session model §8](05-session-model.md#8-persistence-and-restore) (the
store's crash model) · [09 — SSH and media §2](09-ssh-and-media.md#2-the-blob-store)
(the blob store) · [12 — Collaboration §8](12-collaboration.md#8-audit-log) ·
[13 — Security §5, §8](13-security.md#5-credential-storage-rotation-and-tls) ·
[20 — Recall and usage](20-recall-and-usage.md) (search, indexing, timeline —
that document owns *what the index is for*; this one owns *what may enter it and
when it is deleted*) · [22 — Operations](22-operations.md).

> **The binding decision behind this document.** Scrollback and transcripts are
> **persisted by default**. The alternative — ship with persistence off — makes
> the product worse in exactly the way it is meant to be better: an agent that
> ran for forty minutes while you were away is worth nothing if its output was
> never written down, and "reattach and read what happened" is the single most
> common thing a user does. So we persist, and we pay for that choice with the
> four controls in this document: **redaction before write**, **per-session and
> per-workspace opt-out**, **enforced retention**, and **a one-command purge
> with a printed manifest**. All files are `0600`, all directories `0700`.

---

## 1. The inventory

This is the table a security reviewer will ask for, and requirement **R57** is
exactly this table. Every byte omt writes appears here. Anything not in this
table is a bug.

Roots, all per-uid ([22 §2](22-operations.md#2-multi-user-machines)):

```
$XDG_STATE_HOME/omt/            # durable state          (default ~/.local/state/omt)
$XDG_CONFIG_HOME/omt/           # configuration          (default ~/.config/omt)
$XDG_CACHE_HOME/omt/            # regenerable caches     (default ~/.cache/omt)
$XDG_RUNTIME_DIR/omt/           # sockets, blobs, pids   (tmpfs; gone on reboot)
```

**Growth estimates** below are for a *heavy* user: one workstation, ~6 active
workspaces, ~10 sessions per day, 3 agent sessions running most of the working
day, shell integration installed. They are the numbers to design retention
against, not averages.

| # | Data | Path | Format | Growth (heavy user) | Mode | Can contain secrets? |
|---|---|---|---|---|---|---|
| 1 | Session/workspace/pane tree, layouts, focus | `state/omt/tree/snapshot-<n>.bin` + `tree.log` | postcard snapshot + length-prefixed CRC'd append log | ~200 KB steady; rewritten, not grown | 0600 | **Yes** — session `argv` and injected `env` ([05 §1.1](05-session-model.md#11-types)); redacted per §2 |
| 2 | Scrollback chunk snapshots | `state/omt/sessions/<sid>/scrollback/<gen>.zst` | zstd-compressed styled-line chunks, per [04 §2.4](04-terminal-core.md#24-scrollback-blocks-of-logical-lines) generations | **~120 MB/day** raw, ~15–25 MB/day after zstd (agents are verbose and repetitive, so compression is unusually good) | 0600 | **Yes, the worst offender.** Raw PTY output |
| 3 | Live grid snapshot | `state/omt/sessions/<sid>/grid.bin` | one screen, uncompressed | ≤ 400 KB per live session, overwritten | 0600 | **Yes** |
| 4 | Command blocks (metadata) | `state/omt/store.db` (SQLite, shared — see §1.3) | rows: command, cwd, branch, exit, timing, attribution | ~400 blocks/day ≈ 200 KB/day | 0600 | **Yes** — the command line itself (`export FOO_TOKEN=…`) |
| 5 | Command block output | `state/omt/sessions/<sid>/blocks/<bid>.zst` | zstd styled lines, one file per block over `block_output_min_bytes` | included in (2)'s budget; bounded per block at 2 MiB | 0600 | **Yes** |
| 6 | Command history | `state/omt/store.db` (same file as #4/#11) | [05 §9](05-session-model.md#9-command-history) `HistoryEntry` | ~150 KB/day, ~50 MB/year | 0600 | **Yes** — already redacted at write per 05 §9 |
| 7 | Agent event log | `state/omt/sessions/<sid>/agent.jsonl.zst` | normalized `AgentEvent` stream ([06](06-agent-layer.md)) | ~3–8 MB/day across all sessions | 0600 | **Yes** — tool inputs, file paths, prompt text |
| 8 | Interaction ledger | `state/omt/interactions.log` + `interactions.db` | append-only records + resolved index | ~30 interactions/day ≈ 60 KB/day | 0600 | **Yes** — full tool input, and the user's free-text answers |
| 9 | Agent transcript cache | `cache/omt/transcripts/<agent>/<native-id>.idx` | offsets + digests into the *agent's own* transcript file | ~1 MB/day | 0600 | Pointers only — but see §1.1 |
| 10 | Media blobs | `$XDG_RUNTIME_DIR/omt/<instance>/blobs/` | content-addressed, [09 §2](09-ssh-and-media.md#2-the-blob-store) | bursty; capped at 512 MiB, TTL 24 h / 7 d referenced | 0600 / dir 0700 | **Yes** — a pasted screenshot of a dashboard is a secret |
| 11 | Search index | `state/omt/store.db` — the `doc_fts` FTS5 tables inside the block/history database | FTS5 external-content tables over blocks, output windows and agent turns; **the index design is owned by [20 §3](20-recall-and-usage.md)**, this document owns what may enter it and when it is deleted | ~35 % of the indexed corpus ≈ 8 MB/day | 0600 | **Yes if unredacted** — §2.3 forbids that |
| 12 | Usage/cost ledger | `state/omt/store.db` (same file) | token and cost deltas per session | ~20 KB/day | 0600 | No |
| 13 | Audit log | `state/omt/audit/<yyyy-mm>.log` | [12 §8](12-collaboration.md#8-audit-log), redacted, 90-day retention | ~300 KB/day | 0600 / dir 0700 | Redacted by construction; no PTY bytes |
| 14 | Configuration | `config/omt/config.toml`, `keybindings.toml`, `instances.toml`, `themes/`, `workflows/`, `launch/` | TOML/YAML, [10](10-configuration.md) | static | 0644 (0600 for `instances.toml`) | No — a secret inline is a validation error ([13 §5.1](13-security.md#51-at-rest)) |
| 15 | Secrets and credentials | `config/omt/secrets.toml`, `state/omt/credentials.db`, `instance.key`, `invites.db` | [13 §5.1](13-security.md#51-at-rest) | static | **0600, enforced and corrected at startup** | **Yes, by definition** — §5 |
| 16 | Daemon log | `state/omt/omt.log` (+ `.1`…`.4`) | line-oriented, redacted tracing output ([22 §4.2](22-operations.md#42-structured-logging)) | rotated at 32 MiB × 5 | 0600 | Redacted per [13 §8](13-security.md#8-secret-redaction) |
| 17 | Crash records | `state/omt/crashes/<ts>/` | panic message, backtrace, quarantined input bytes | rare; capped at 20 records / 64 MiB | 0600 / dir 0700 | **Yes** — quarantined bytes are raw PTY input |
| 18 | Quarantine | `state/omt/quarantine/<ts>-<what>` | corrupt store fragments, never deleted automatically ([05 §8.2](05-session-model.md#82-crash-semantics)) | rare | 0600 | **Yes** |
| 19 | **Native session transcript** | `state/omt/sessions/<sid>/native.jsonl.zst` | typed ACP event stream, one JSON object per line, zstd-framed | ~2–5 MB/day per active `native` session | 0600 | **Yes** — prompts, assistant text, tool inputs and results |
| 20 | **omt-managed message queue** | `state/omt/store.db` (same file) | pending queued text per binding, with `created_at` / `valid_until` ([06 §8](06-agent-layer.md#8-ancillary-semantics)) | tiny | 0600 | **Yes** — free text the user typed |
| 21 | **Per-actor continuity state** | `state/omt/store.db` (same file) | drafts, read marks, surface intent, last-seen positions ([`../design/remote-continuity.md` §1.3](../design/remote-continuity.md#13-where-per-actor-state-is-stored-and-why-on-the-instance)) | ~50 KB/day | 0600 | **Yes** — a draft is free text |
| 22 | **Attention log** | `state/omt/store.db` (same file) | per `(identity, session)` index of interactions reaching a terminal state ([20 §12.5](20-recall-and-usage.md#125-attention-and-the-durable-attention-log)) | ~20 KB/day | 0600 | Pointers plus outcome; the response text lives in #8 |
| 23 | Ephemeral runtime | `$XDG_RUNTIME_DIR/omt/<instance>.sock`, `pid.txt`, `ssh-<hash>` control sockets | — | tiny | 0600 | Contains no data; the socket *is* full authority ([22 §2](22-operations.md#2-multi-user-machines)) |

**Row 19 exists because [05 §8.1](05-session-model.md#8-persistence-and-restore)
mandates it.** A `native` session ([D8](decisions.md#d8--two-session-modes-pty-default-and-native-acp))
has no PTY and therefore no scrollback (row 2) and no grid (row 3): its typed
event stream **is** its entire history, and if omt does not persist it, closing
the app loses the conversation. It is distinct from row 7 — row 7 is the merged
observation of a `pty` agent, row 19 is the authoritative record of a session
that exists nowhere else. It is also distinct from row 9: row 9 is a pointer
index into a file the *agent* owns, whereas the native transcript is omt's own
data and `store.purge` does delete it.

**Headline: about 25–40 MB/day compressed for a heavy user, dominated by
scrollback and the search index; roughly 9 GB/year before retention, and about
2.4 GB/year after the §3 defaults apply.** That number is worth stating in the
README, because "where did my disk go" is otherwise discovered rather than
known.

### 1.1 Two things omt writes that are not omt's

Worth calling out because a purge that misses them is a lie:

- **Agent transcripts** belong to the agent CLI (`~/.claude/projects/…`,
  `~/.codex/sessions/…`). omt reads and indexes them; it does not own them and
  **`store.purge` will not delete them**, but it *reports* them in the manifest
  with their paths and sizes, and prints the agent's own command for clearing
  them. Silently deleting another tool's data is not omt's business; leaving the
  user believing they are gone would be worse.
- **Hook configuration** written into agent config files by
  `omt integrate install` ([22 §3](22-operations.md#3-omt-doctor)). Purge removes
  omt's own blocks from those files, marked by sentinel comments, and reports
  any it could not confidently remove rather than rewriting a file it did not
  fully understand.

### 1.2 The one-line answer for backup exclusion

Users need to exclude this from Time Machine, Dropbox and Backblaze, which is
half the point of publishing the inventory:

```
$ omt store paths --for-backup-exclusion
~/.local/state/omt          # session content — exclude
~/.cache/omt                # regenerable — exclude
~/.config/omt               # configuration — KEEP, it is small and precious
~/.config/omt/secrets.toml  # KEEP but ensure the backup is encrypted
```

### 1.3 One database file, and why

Inventory rows #4 (blocks), #6 (history), #11 (the FTS5 search index) and #12
(usage) are **one SQLite file, `state/omt/store.db`**. They were once four, and
that was wrong: [20 §3.1](20-recall-and-usage.md#31-the-decision)'s D-R1 chooses
FTS5 precisely so that the index row is written *in the same transaction* as the
block record it describes, which makes "the block exists but is not findable" an
unreachable state. Two files means two transactions and reconciliation after a
crash becomes a subsystem. One durability domain is the whole argument, so there
is one file.

The two databases that stay separate stay separate for a durability reason, not
a historical one:

- **`interactions.db` + `interactions.log`** (#8) are in the `Critical` fsync
  class (§6.3) — the ledger is `fsync`ed *before the capability returns*, and
  putting it in the same file as the `Bulk`-class index writes would drag every
  scrollback flush into that discipline or the ledger out of it.
- Everything else (#1 tree, #7 agent events, #13 audit) is a log, not a
  database.

Usage (#12) has no such reason — its loss window is a flush cadence, not a
durability class — so it is folded into `store.db` with the rest.

`omt doctor store` warns when `$XDG_STATE_HOME/omt` is inside a directory that
looks synced (`~/Dropbox`, `~/Library/Mobile Documents`, a path containing
`.dropbox` or `.icloud` markers), because a state directory synced to iCloud is
the concrete incident in scenario J46 / gap G9.

---

## 2. Redaction before write

### 2.1 The placement rule

> **Redaction runs once, on the path from the terminal into durable storage, and
> before anything is indexed. It never runs at display time.**

This is the load-bearing sentence in the document. A display-time filter — mask
the secret when rendering the scrollback — is the intuitive design and it is
wrong in the way that matters: the plaintext is already on disk, so the filter
protects the screen of the person who already saw it and protects nothing from
the backup, the sync client, or the stolen laptop. The only redaction worth
having is redaction that changes what is written.

Concretely, in the pipeline from [04](04-terminal-core.md):

```
PTY bytes
   │
   ├──► VT parser ──► grid ──► live render          (NEVER redacted — P4: omt
   │                                                 does not rewrite output;
   │                                                 the secret is on the screen
   │                                                 because the program printed
   │                                                 it there)
   │
   └──► block tracker ──► line accumulator
                              │
                              ▼
                    ┌──────────────────┐
                    │    Redactor      │   ◄── the single choke point
                    └────────┬─────────┘
                             ├──► scrollback chunk writer  (inventory #2)
                             ├──► block output writer      (#5)
                             ├──► block metadata / history (#4, #6)
                             ├──► agent event log          (#7)
                             ├──► interaction ledger       (#8)
                             ├──► omt-managed queue        (#20)
                             └──► search indexer           (#11) ── R56

ACP events (`native` sessions — no PTY, no grid)
   │
   └──► event normalizer ──► Redactor ──► native transcript writer (#19)
                                     └──► search indexer           (#11)
```

**The native transcript passes through the same choke point.** A `native`
session has no PTY, so it does not enter via the pipeline above — but it carries
exactly the same content (prompts, tool inputs, tool results, file contents in
diffs) and must not be a hole in the guarantee. The redactor is applied to the
typed event *before* it is written to `native.jsonl.zst` and before it is
indexed. There is no display-time exception here either: unlike `pty` mode there
is no "the program printed it on the screen" case to respect, because omt owns
the rendering — so for `native` sessions the redacted form is the *only* form,
on screen and on disk alike.

Two consequences follow and both are deliberate:

1. **The live terminal shows the secret and the disk does not.** A user who runs
   `cat .env` sees their `.env`, scrolls up and still sees it *within the live
   grid and the in-memory scrollback ring*, but the chunk that gets flushed to
   disk carries `<redacted:env:len=51>`. Scrolling far enough back after a flush
   shows the redacted form. That transition is visible and it is honest: the UI
   marks a redacted span with a dim `▒` gutter glyph and a hover/tap explanation
   *"redacted before writing to disk — see `omt store explain-redaction`"*.
2. **There is exactly one redactor, and §2.2 below is its specification.** The
   `Redactor`, its classes, its rules and its markers are defined here and
   nowhere else. [13 §8](13-security.md#8-secret-redaction) owns only the
   *integration* — the tracing `Layer` and the event-bus serializer wrapper that
   make it impossible to emit a log line or an event without passing through it —
   so the same detector covers logs, audit entries, events **and all persisted
   terminal content**. Sharing it is not a code-reuse nicety: two redactors would
   drift, and the weaker one would define the product's real guarantee.

### 2.2 The detector

```rust
pub struct Redactor {
    key_rules:     Vec<KeyRule>,      // map/assignment key names
    shape_rules:   Vec<ShapeRule>,    // compiled regex set, single-pass
    entropy:       EntropyRule,
    user_rules:    Vec<UserRule>,     // from config, §2.4
    allow:         Vec<Regex>,        // explicit false-positive suppressions
}

pub enum RedactionClass {
    Env,
    Key,
    /// A credential passed as a command-line argument: `--password X`,
    /// `--token X`, `mysql -pX`. Medium confidence; see (a').
    Flag,
    /// A credential in an HTTP header line: `Authorization: Bearer …`,
    /// `X-Api-Key:`, `Proxy-Authorization:`, `Cookie:`/`Set-Cookie:`.
    Header,
    Shape(&'static str),
    Entropy,
    User(String),
}

pub struct Finding {
    pub span: Range<usize>,
    pub class: RedactionClass,
    pub replacement: String,          // "<redacted:sk:len=51>"
}
```

Layers, in order, first match wins:

**(a) Key rules** — the assignment/keyword forms, applied to structured values
(env maps, JSON tool inputs) and to line text:

```
(?i)\b(pass(word|wd)|secret|token|api[-_ ]?key|access[-_ ]?key|auth(orization)?|
      cookie|private[-_ ]?key|credential|client[-_ ]?secret|refresh[-_ ]?token)\b
      \s*[:=]\s*(?P<value>\S+)
```
plus every key in a `KEY=VALUE` line whose *key* ends in `_TOKEN`, `_SECRET`,
`_KEY`, `_PASSWORD`, `_PASSWD`, `_CREDENTIALS`.

**(a') Flag rules** (`RedactionClass::Flag`) — a credential passed as an
argument rather than an assignment: `--password X`, `--token X`, `--api-key X`,
`--secret X`, `-p X` for a known client, and the attached form `mysql -pX`.
Confidence is **medium**, not high: `-p` means "port" to as many programs as it
means "password", so the attached and known-client forms are matched and a bare
`-p X` is matched only for clients on a small built-in list. The matched span is
the *value*, never the flag.

**(a'') Header rules** (`RedactionClass::Header`) — a credential on an HTTP
header line, matched on the header name: `Authorization: Bearer …`,
`Authorization: Basic …`, `X-Api-Key:`, `Proxy-Authorization:`, `Cookie:` and
`Set-Cookie:`. High confidence, and worth its own class rather than folding into
key rules because the replacement keeps the header name — `Authorization:
<redacted:header:len=44>` retains the fact that an auth header was present,
which is most of the diagnostic value.

**(b) Shape rules** — known credential formats, matched with a single
`regex::RegexSet` pass so cost is one scan regardless of rule count. This table
is the source of truth; 13 §8's shape list is a summary of it:

| Class | Pattern (abbreviated) |
|---|---|
| `sk` | `sk-(ant-|proj-)?[A-Za-z0-9_-]{20,}` |
| `gh` | `gh[pousr]_[A-Za-z0-9]{36}` |
| `aws` | `AKIA[0-9A-Z]{16}`, and `aws_secret_access_key` values |
| `slack` | `xox[baprs]-[A-Za-z0-9-]{10,}` |
| `jwt` | `eyJ[\w-]{10,}\.[\w-]{10,}\.[\w-]{10,}` |
| `pem` | `-----BEGIN [A-Z ]*PRIVATE KEY-----` … `-----END` (whole block) |
| `gcp` | `"private_key_id"` / `"private_key"` inside a service-account JSON |
| `url` | `://[^/\s:@]+:[^/\s@]+@` — credentials in a URL's userinfo |
| `k8s` | base64 values under a `data:` key in `kind: Secret` YAML |
| `omt` | `omt_c_…`, `omt_s_…` — omt's own credentials |

**(c) Entropy heuristic** — the backstop for the ones nobody enumerated. A token
is redacted when **all** of the following hold:

- length 24–512 and drawn from a single plausible alphabet (base64/base64url,
  base32, or lowercase hex);
- Shannon entropy ≥ 3.6 bits/char for base64-class, ≥ 3.0 for hex;
- it is not a match for the allow-set: git object ids (40/64 hex, and
  specifically the SHA on a line starting with `commit `), UUIDs, content hashes
  in `Cargo.lock`/`package-lock.json`-shaped lines, base64 image data URIs
  (those go to the blob store instead), and known-safe prefixes the user
  configured;
- it is *not* inside a block whose command matched the allow list (`git log`,
  `sha256sum`, `openssl dgst`, `docker images`) — command context is cheap and
  removes most of the false positives that would otherwise make the entropy rule
  unusable.

The entropy rule defaults to `on` for **agent event logs, interaction ledger and
the search index**, and to `on` for scrollback as well. It is the rule most
likely to annoy, so `omt store explain-redaction --session <sid>` prints every
finding with its class and a one-key "add an allow rule for this shape" action.

### 2.3 What gets written instead

```
<redacted:sk:len=51>
<redacted:env:AWS_SECRET_ACCESS_KEY:len=40>
<redacted:entropy:len=64>
<redacted:pem:lines=27>
```

Length-preserving markers — `<redacted:CLASS[:detail]:len=N>` is **the** marker
format, and every other document referring to a redaction marker refers to this
one — because a bug report and
a diff remain readable when you can see *that* a 51-character `sk-` token was
there. Column alignment inside a styled line is preserved by padding the
replacement to the original cell width when the original was ≤ 40 cells and
truncating with a marker otherwise; a redaction never changes a line's cell
count, because a scrollback chunk that reflows differently than the live grid is
a rendering bug waiting to happen.

**Indexing rule (R56).** The indexer consumes the *redacted* stream only. There
is no code path from the raw accumulator to the index. This is asserted by a
test that feeds a corpus of fake-but-real-shaped credentials through a full
session and greps the index files for them — a byte-level assertion, not a
mocked one.

### 2.4 The block model interaction: pasted command vs. printed output

The two cases behave differently and the difference is worth being explicit
about, because it is where users get surprised.

| Case | What happens | Why |
|---|---|---|
| A secret **typed or pasted into a command line** — `export API_KEY=sk-…`, `curl -H "Authorization: Bearer …"` | The block's `command` field is redacted before it reaches `store.db`. The **input bytes were never persisted at all** — omt does not log keystrokes ([12 §8](12-collaboration.md#8-audit-log)) — so the only copy is the block record, and it is redacted. | Command text is the durable, searchable, cross-session artifact. It is also the one users re-run by accident. |
| A secret **printed into output** — `cat .env`, a tool result containing a JWT | Redacted on the way into scrollback, block output, agent event log and index. Present in the live grid and the in-memory ring until flushed. | Output is high-volume and unstructured; the guarantee is "not on disk", not "never on screen" (P4). |
| A secret in a **shell prompt** (some prompts embed a session token) | Prompt regions are marked by OSC 133 `A`…`B` and are redacted with the same rules; a prompt is output. | Same as output. |
| A secret in an **agent's tool input** | Redacted in the ledger and audit entry, per 13 §7. **Not** redacted on the way to the agent — omt is not in that path (P4). | omt observes; it does not mediate the agent's own I/O. |
| `HISTCONTROL=ignorespace` / a leading space | The block is recorded with `command: None` and `suppressed: true`, and its output is not persisted at all. | The user used the shell's own documented mechanism for "do not remember this". Honouring it is P4. |

### 2.5 Per-workspace and per-session control

```toml
# config.toml — instance defaults
[store.redaction]
enabled          = true          # cannot be false at instance level; see below
entropy          = true
extra_patterns   = [
  '(?i)acme-internal-[a-z0-9]{22}',
]
allow_patterns   = [
  '\bsha256:[0-9a-f]{64}\b',     # our CI prints these constantly
]

[store.persist]
scrollback       = true          # R55 — the master switch for inventory #2/#3/#5
block_output     = true
agent_events     = true
index            = true
```

```toml
# .omt/config.toml in a project — per workspace
[store.persist]
scrollback = false               # this repo handles customer PII; keep nothing

[store.redaction]
extra_patterns = ['CUST-[0-9]{9}']
```

```sh
$ omt session create --ephemeral            # persist nothing for this session
$ omt session set-persist <sid> --off       # flip a live session; see below
```

Rules:

- **`store.redaction.enabled = false` is refused at the instance level**
  (`OMT-C260`), the same shape as `[log].redact_secrets`
  ([10 §7.12](10-configuration.md#712-log)). It *can* be disabled per workspace,
  in a project-scoped `.omt/config.toml`, which requires the project to be
  trusted (`config.project.trust`, [10](10-configuration.md)) — so disabling it
  is an explicit, auditable, per-directory act by someone who owns that
  directory. Disabling it prints a startup warning and marks every session in
  that workspace with an amber `no-redaction` badge on every surface.
- **`persist.scrollback = false` is visible** (R55): the pane border, the session
  row, the web session header and `omt session list` all show `⦸ ephemeral`.
  Invisible privacy modes get forgotten and then relied on.
- **Flipping persistence off at runtime deletes what was already written for
  that session**, after a confirmation naming the byte count. Flipping it on
  starts persisting from that moment; it cannot recover what was skipped, and
  says so.
- Precedence is the normal config ladder ([10 §3](10-configuration.md)):
  session flag > project `.omt/` > user config > default. A **workspace can only
  make persistence stricter than the instance default, never looser** — a user
  config with `scrollback = false` cannot be overridden back to `true` by a
  project file, because a checked-in project file is not a trustworthy source of
  "please record more".

### 2.6 What this will miss — stated honestly

The redactor is a net with known holes, and pretending otherwise is worse than
the holes:

1. **Secrets with no shape.** A database password that is `hunter2`, a customer
   record, an internal hostname, the contents of a private document printed to
   the terminal. No pattern and no entropy rule will find these. The control for
   them is `persist.scrollback = false`, not redaction.
2. **Split across writes.** A token emitted in two PTY reads, or wrapped across a
   line boundary by the program that printed it. The redactor buffers within a
   logical line and across a soft-wrap continuation, but a token that a program
   deliberately prints in two chunks separated by other output is not
   reconstructed.
3. **Encoded or transformed.** base64 of a base64 token, a URL-encoded secret, a
   secret inside a gzip stream dumped with `xxd`.
4. **The live in-memory grid, always.** Redaction is a write-path transform.
   Anything still in the scrollback ring is plaintext in the daemon's address
   space, and a core dump of the daemon contains it. `omt bug-report`
   ([22 §5](22-operations.md#5-diagnostics-panel-and-omt-bug-report)) therefore
   never includes a core dump.
5. **The agent's own transcript files** (§1.1) are written by the agent, not by
   omt, and are unredacted on disk regardless of what omt does.
6. **False negatives are silent; false positives are loud.** We tune toward
   false positives — a redacted git SHA is an annoyance with a one-key fix, a
   leaked `sk-` key is an incident. `omt store explain-redaction` exists to make
   the annoyance cheap.

A test corpus of ~200 real-shaped-but-fake credentials plus ~200 near-miss
non-secrets is checked in, and the fuzz target from 13 §8 is extended to the
scrollback path. The measured recall and false-positive rate on that corpus are
printed by `omt doctor store --redaction` so the number is a fact, not a claim.

---

## 3. Retention and compaction

### 3.1 The principle: compact, then delete

Old data loses value unevenly. The *output* of a `cargo build` from three months
ago is worthless; the *fact* that you ran it, in that directory, on that branch,
and it took 94 seconds and failed, is still useful and costs 200 bytes. So
retention is a two-stage ladder — compact first, delete second — rather than a
single age cutoff.

| # | Data | Compact after | Compacted to | Delete after | Size cap |
|---|---|---|---|---|---|
| 2/5 | Scrollback + block output | **7 days** | dropped entirely for successful blocks under `keep_output_bytes`; kept for failed blocks and for blocks with an agent attribution | **30 days** | 8 MiB/session, 4 GiB/instance |
| 3 | Live grid snapshot | on session close | folded into the last scrollback chunk | with the session | — |
| 4 | Block metadata | never | — | **1 year** | 2 M rows |
| 6 | History | never | — | **2 years** | 2 M rows |
| 7 | Agent event log | **14 days** | keep turn boundaries, tool names, file paths, exit statuses, usage; drop tool inputs/outputs bodies | **90 days** | 1 GiB/instance |
| 8 | Interaction ledger | **30 days** | keep id, kind, tool name, decision, actor, latency; drop the full input | **1 year** | — |
| 9 | Transcript index | on agent transcript deletion | — | 30 days | 500 MiB |
| 19 | **Native session transcript** | **14 days** | keep turn boundaries, tool names, file paths, statuses and usage; drop message bodies and tool input/output bodies — the same compaction as #7, because it is the same content in the same shape | **90 days** | 1 GiB/instance |
| 20 | **omt-managed message queue** | never | — | entries are removed on flush, on discard, or at `valid_until` + 7 days | 1 MiB/session |
| 21 | **Continuity state** | never | — | drafts at 30 days untouched; read marks and surface intent with the session | — |
| 22 | **Attention log** | never | — | follows the interaction ledger (**1 year**), so a terminal-state record never outlives the interaction it points at | — |
| 10 | Blobs | — | — | 24 h / 7 d referenced ([09 §2](09-ssh-and-media.md#2-the-blob-store)) | 512 MiB |
| 11 | Search index | follows its source | FTS5 rows for deleted docs are removed in the same transaction as the source row and the shadow table is `optimize`d on the full sweep; freed pages are reclaimed by `VACUUM` | with the source data | 25 % of `state/` |
| 12 | Usage | **90 days** | rolled up to daily totals per session | never (it is tiny and it is the cost record) | — |
| 13 | Audit | never | — | **90 days** ([12 §8](12-collaboration.md#8-audit-log)) | — |
| 16 | Daemon log | — | — | 5 files × 32 MiB | 160 MiB |
| 17 | Crash records | — | — | 20 records or 90 days | 64 MiB |
| 18 | Quarantine | — | — | **never automatically** — reported by `store.usage`, removed only by explicit purge | — |

Every one of these is a config key under `[store.retention]`, per-kind, with
per-workspace overrides:

```toml
[store.retention]
scrollback_compact_after = "7d"
scrollback_delete_after  = "30d"
scrollback_max_bytes     = "4GiB"    # per instance
scrollback_max_bytes_per_session = "8MiB"
blocks_delete_after      = "1y"
agent_events_compact_after = "14d"
agent_events_delete_after  = "90d"
native_transcript_compact_after = "14d"
native_transcript_delete_after  = "90d"
interactions_compact_after = "30d"
audit_delete_after       = "90d"     # capped at 1y; a shorter value is allowed

[workspace."~/code/client-x".store.retention]
scrollback_delete_after  = "3d"      # NDA work: keep the minimum that is useful
```

`RetentionPolicy` is the resolved form of that config block — what
`store.retention.get` returns (§8) and what `store.retention.set` takes. It is a
plain record with one entry per swept kind, plus the scope it was resolved for
and where each value came from, so a user can see *why* their scrollback is
being deleted after three days:

```rust
pub struct RetentionPolicy {
    /// Scope this policy was resolved for; workspace policies may only be
    /// stricter than the instance policy, never looser (§2.5's ladder).
    pub scope: RetentionScope,          // Instance | Workspace(WorkspaceId)
    pub rules: BTreeMap<DataKind, RetentionRule>,
}

pub struct RetentionRule {
    /// Age after which the record is compacted to its metadata. `None` = never.
    pub compact_after: Option<Duration>,
    /// Age after which the record is deleted outright. `None` = never.
    pub delete_after: Option<Duration>,
    /// Size cap for this kind, enforced oldest-first after the age rules.
    pub max_bytes: Option<u64>,
    /// Row cap, for the two kinds that are counted in rows rather than bytes.
    pub max_rows: Option<u64>,
    /// Which layer of the config ladder supplied this rule.
    pub source: ConfigSource,
}
```

`DataKind` is the inventory's `#` column, named rather than numbered
(`Scrollback`, `BlockOutput`, `Blocks`, `History`, `AgentEvents`,
`NativeTranscript`, `Interactions`, `TranscriptIndex`, `Blobs`, `SearchIndex`,
`Usage`, `Audit`, `MessageQueue`, `ContinuityState`, `AttentionLog`,
`DaemonLog`, `Crashes`, `Quarantine`), so the table above and the type are the
same list and CI can assert it. That assertion and §6.2's loss-table check are
the two directions of the same rule: every persisted kind has a retention policy
*and* a stated loss window.

**Retention is enforced by the sweeper, not by the writer.** Nothing checks the
clock on the hot path. A record written today with a 30-day policy is deleted by
a sweep, which means changing the policy retroactively applies to existing data
on the next sweep — including *shortening* it, which is what a user expects when
they tighten a setting after the fact.

### 3.2 The sweeper

```rust
pub struct Sweeper {
    schedule: SweepSchedule,
    budget:   SweepBudget,
}

pub struct SweepSchedule {
    /// Cheap pass: expired blobs, log rotation, FTS5 incremental `optimize`.
    light:  Duration,       // 5 min
    /// Full pass: compaction, deletion, size-cap enforcement, vacuum.
    full:   Duration,       // 6 h, plus once at startup after a 60 s settle delay
}

pub struct SweepBudget {
    max_wall:        Duration,   // 30 s per full pass; resume where it stopped
    max_io_bytes:    u64,        // 256 MiB read+write per pass
    /// Yield if the daemon's own event loop lag exceeds this.
    lag_ceiling:     Duration,   // 20 ms
    nice:            bool,       // ionice/`setpriority` where available
}
```

Scheduling rules that matter:

- **The sweeper never runs while any session is `Working`** with output
  throughput above 64 KiB/s. Compaction competing with a live `cargo build` for
  disk is exactly when a user notices omt is slow.
- **It is incremental and resumable.** State is a cursor per data kind in
  `state/omt/sweep.json`; a killed daemon resumes rather than restarting.
- **It yields.** Every 500 records it checks event-loop lag and stops the pass if
  the daemon is behind. A sweep that never finishes is better than a stutter.
- **It is observable.** `store.usage` reports `last_sweep`, `bytes_reclaimed`,
  and `next_sweep`; `omt doctor store` reports a sweeper that has not completed a
  full pass in 24 h as a **failure with a remedy**, because a silently stuck
  sweeper is how a disk fills.

### 3.3 Disk pressure

Retention is a policy; disk pressure is an emergency, and they need different
behaviour. The daemon checks free space on the filesystem holding
`$XDG_STATE_HOME/omt` on every light sweep.

| Free space | State | Behaviour |
|---|---|---|
| > 5 GiB and > 10 % | `Healthy` | normal |
| ≤ 5 GiB or ≤ 10 % | `Tight` | warning event + banner on every surface; full sweep scheduled immediately; `VACUUM` and FTS5 `optimize` on `store.db` deferred (both need up to 2× the file transiently) |
| ≤ 1 GiB or ≤ 3 % | `Pressure` | **aggressive reclaim**: apply the shortest retention that any workspace configures, instance-wide; drop all block output older than 24 h; empty the search index (`DELETE FROM doc_fts` and drop the `doc` text columns' contents, then `VACUUM`) — it is derived data and always rebuildable from the blocks, so it is the cheapest large thing to lose; evict all unreferenced blobs |
| ≤ 200 MiB | `Critical` | **stop persisting**: scrollback, block output, agent events and index writes are dropped in memory with a counter; the tree, interaction ledger, credentials and audit log keep writing (they are small and losing them is unacceptable). Every surface shows a red banner naming the filesystem and the free bytes. Sessions keep running — omt never kills a session because of disk |

The `Critical` rule is the important one: **the terminal keeps working when the
disk is full.** A multiplexer that dies because it could not write scrollback
would take the user's running agents with it, and the data it was trying to save
is worth less than the processes it would kill.

Recovery is automatic — when free space returns above the `Tight` threshold for
60 s, persistence resumes, and a `store.gap` marker is written into the affected
sessions so the scrollback shows an honest `⋯ 4 min 12 s of output was not
recorded (disk full) ⋯` separator rather than a silent discontinuity.

---

## 4. Export and purge

### 4.1 `store.usage` — where the bytes went

```
$ omt store usage
INSTANCE 3f2a91c4  ·  state ~/.local/state/omt  ·  6.2 GiB of 231 GiB free

BY KIND                            bytes    share   oldest
  scrollback                      3.9 GiB    63%    2026-07-04  (30d retention)
  search index                    1.1 GiB    18%    2026-07-04
  agent events                  412.0 MiB     6%    2026-05-06  (90d retention)
  blocks + output               301.4 MiB     5%    2025-08-11  (1y retention)
  history                        44.1 MiB     1%    2024-09-02
  audit                          26.8 MiB     -     2026-05-06  (90d retention)
  interactions                   19.2 MiB     -     2025-08-11
  usage                           2.1 MiB     -     2025-08-11
  quarantine                     18.0 MiB     -     2026-02-14  ← never swept
  blobs (runtime)                88.4 MiB     -     2026-08-03  (24h TTL)

BY WORKSPACE                       bytes  sessions  blocks  last active
  ~/code/omt                      2.6 GiB      141   9,204  4 min ago
  ~/code/client-x                 1.9 GiB       88   5,110  yesterday
  ~/code/infra                  801.2 MiB       37   2,940  6 days ago
  ~/code/scratch                 44.0 MiB       12     301  3 months ago
  (deleted workspaces)          912.0 MiB       61   1,882  —

Last sweep: 3 h 12 m ago, reclaimed 402 MiB in 11.3 s.  Next: in 2 h 48 m.
1 note: 18.0 MiB of quarantine from 2026-02-14 is never swept automatically.
        Inspect with `omt store quarantine list`, remove with `omt store purge --quarantine`.
```

`--json` gives the same as a machine-readable tree
([22 §8](22-operations.md#8-automation-and-ci-g14)). `--by session` and
`--workspace <path>` narrow it.

### 4.2 `store.export` — the archive format

The format is specified here rather than left to the implementation, because an
export nobody can read six months later is not an export.

An omt archive is a **zip** (seekable, universally openable, and a user can
inspect it without omt) whose first entry is uncompressed:

```
omt-export-<instance>-<scope>-<yyyymmddThhmmssZ>.omtz
├── MANIFEST.json           ← first entry, stored uncompressed, self-describing
├── README.txt              ← plain English: what this is, how to read it without omt
├── workspaces.jsonl        ← one WorkspaceRecord per line
├── sessions.jsonl          ← one SessionRecord per line
├── blocks.jsonl            ← one BlockRecord per line (metadata)
├── history.jsonl
├── interactions.jsonl
├── agent-events/<sid>.jsonl
├── native-transcripts/<sid>.jsonl  ← native sessions: the whole session (#19)
├── message-queue.jsonl     ← pending omt-managed queue entries (#20)
├── continuity.jsonl        ← drafts, read marks, surface intent (#21)
├── attention.jsonl         ← the durable attention log (#22)
├── usage.jsonl
├── audit.jsonl             ← only with --include audit (Admin)
├── scrollback/<sid>/<seq>.txt   ← plain UTF-8, ANSI stripped
├── scrollback/<sid>/<seq>.ansi  ← original styling, with --fidelity ansi
├── blocks/<sid>/<bid>.txt
└── blobs/<blake3>.<ext>    ← only with --include blobs
```

```jsonc
// MANIFEST.json
{
  "format": "omt-export",
  "format_version": 1,
  "omt_version": "0.4.1",
  "created_at": "2026-08-03T18:41:02Z",
  "instance": { "id": "3f2a91c4…", "hostname": "workstation" },
  "scope": { "kind": "workspace", "path": "/Users/ada/code/client-x" },
  "counts": { "workspaces": 1, "sessions": 88, "blocks": 5110,
              "interactions": 214, "scrollback_bytes": 2041233408,
              "native_sessions": 6, "native_transcript_bytes": 18220544 },
  "redaction": { "applied_at_write": true, "findings": 3122,
                 "classes": { "sk": 4, "env": 61, "entropy": 3057 } },
  "excluded": [
    { "what": "agent transcripts", "why": "owned by the agent CLI, not by omt",
      "paths": ["~/.claude/projects/-Users-ada-code-client-x"] },
    { "what": "credentials", "why": "never exported; see doc 21 §5" }
  ],
  "entries": [ { "path": "sessions.jsonl", "sha256": "…", "records": 88 }, … ]
}
```

Rules:

- **A `native` session exports its transcript, or it exports nothing.** There is
  no scrollback for it (no PTY), so `scrollback/<sid>/` is absent and
  `native-transcripts/<sid>.jsonl` is the entire content of the session. An
  export that omitted it would silently drop whole conversations while reporting
  success. `--fidelity text` renders it to a readable plain-text transcript
  alongside the JSONL, since that is what a human or an LLM will actually read.
- **Self-describing.** `MANIFEST.json` plus `README.txt` are enough to read the
  archive with `jq` and a text editor. Every JSONL record carries a `"type"`
  discriminator and a `"format_version"`.
- **Never exports credentials.** `secrets.toml`, `credentials.db`,
  `instance.key` and `invites.db` are excluded unconditionally and listed in
  `excluded`. There is no `--include credentials` flag. If you want to move an
  instance's identity to another machine, that is a key rotation, not an export.
- **Redaction is not undone.** An export contains what is on disk, which is
  already redacted. `MANIFEST.redaction` states how many findings were applied so
  the recipient knows the data is incomplete by design.
- **Deterministic ordering** (workspace, then session by `created_at`, then seq)
  so two exports of unchanged data are byte-identical except the timestamp,
  which makes an export diffable and a backup deduplicable.
- **Streaming**, with progress. A 4 GiB export must not buffer.
- `--fidelity text|ansi|full` — `text` (default) strips styling and is what a
  human or an LLM will read; `ansi` keeps the original escape sequences;
  `full` additionally keeps the raw chunk files so the archive can be
  re-imported.
- `store.import` exists and is deliberately narrow: it restores into a
  *quarantined, read-only* workspace named `imported/<original>` so an import can
  never overwrite live state. Reading an old project's history is the use case;
  merging two instances is not.

### 4.3 `store.purge` — destruction with a manifest

```
$ omt store purge --workspace ~/code/client-x

This will PERMANENTLY DESTROY the following. There is no undo.

  scope: workspace  /Users/ada/code/client-x  (ws_9c2e11a4)

  88 sessions           (3 still live — they will be closed first)
   5,110 command blocks
   4,882 history entries
     214 interactions   (0 open)
   1.9 GiB scrollback and block output   (across 88 session directories)
   612 MiB search index entries
    88 MiB agent event logs
    22 MiB native session transcripts   (6 native sessions — this is their
                                         only copy; nothing else holds it)
       9 pending queued messages        (omt-managed queue, unflushed)
      41 drafts and read marks          (continuity state, all actors)
     214 attention-log entries
    31 MiB blobs (14 referenced by live sessions — they will be unlinked)
   2,104 audit entries   (retained: audit is NOT purged by default, see below)
       0 quarantined fragments

  Files and directories to be removed:
    ~/.local/state/omt/sessions/{88 directories}
    ~/.local/state/omt/sessions/<sid>/native.jsonl.zst  (native sessions)
    rows in ~/.local/state/omt/store.db  (blocks, history, usage, doc + doc_fts,
                                          message queue, continuity, attention)
    rows in ~/.local/state/omt/interactions.db

  NOT removed (not omt's data):
    ~/.claude/projects/-Users-ada-code-client-x      412 MiB
      → clear with:  claude  /clear-project-history
    ~/.codex/sessions/2026-0{6,7,8}/…                 88 MiB
      → clear with:  rm -rf ~/.codex/sessions/…

  NOT removed (deliberate):
    audit entries — the record of who did what. Add --include-audit to
    remove them too; this is logged as an audit entry of its own.

Type the workspace name to confirm:  client-x
```

```
Purging… ████████████████████████ 88/88 sessions
Done in 6.2 s. Reclaimed 2.6 GiB.

Manifest written to ~/.local/state/omt/purges/2026-08-03T18-44-02Z.json
(a record of what was destroyed; contains no destroyed content)
```

Design:

| Scope | Selector | What it covers |
|---|---|---|
| `session` | `--session <sid>` | one session's content; its history/block rows; its blobs |
| `workspace` | `--workspace <path\|id>` | the above for every session in it, plus the workspace record and layout |
| `agent-scope` | `--agent <kind>` | every session bound to that agent kind, across workspaces |
| `time` | `--before <date>` | everything older than a date, any scope |
| `instance` | `--instance` | all session content; keeps config and credentials |
| `everything` | `--everything` | all of the above **plus** config, credentials, the instance key and the state directory itself. Leaves nothing but the binary |

Rules, each of which exists because of a specific way this goes wrong:

- **Dry run is the default when stdout is not a TTY.** `omt store purge` in a
  script without `--yes` prints the manifest and exits `2`. Destroying data
  because a script was written optimistically is not acceptable.
- **Confirmation is typed, not `y/n`.** The user types the workspace name (or
  `EVERYTHING` for `--everything`). `--yes` bypasses it for automation and is
  audited with the actor.
- **The manifest is printed before, and written after.** The before-manifest is
  the decision; the after-manifest at `state/omt/purges/<ts>.json` is the
  receipt, listing counts and byte totals but never content. A purge receipt is
  what the user shows their client.
- **Purge is synchronous and complete before it reports success.** SQLite rows
  are deleted in a transaction and the database is `VACUUM`ed (otherwise the
  bytes remain in free pages, and "I deleted it" would be false). The purged
  documents' FTS5 rows are deleted in that same transaction — not merely
  tombstoned — so the postings go with the `VACUUM` rather than surviving it.
  Files are
  removed with `unlink`; omt does **not** claim secure erase — on a
  copy-on-write or flash filesystem that claim is unverifiable, and the README
  says so.
- **Live sessions in scope are closed first**, with the SIGHUP/SIGKILL ladder
  from [05 §1.2](05-session-model.md#12-identity-and-lifetimes), and the manifest
  says so up front.
- **The audit log is excluded by default** and the purge itself is audited. The
  record that a purge happened is the last thing that should be deletable by
  accident. `--include-audit` is available and writes one final audit entry
  recording the deletion, which is the honest end state.
- **`--everything` is what a departing contractor runs.** It removes the state
  directory, the config directory, the runtime directory and the credentials,
  prints the list of things it did not own (agent transcripts, shell rc lines,
  agent hook config blocks — with the exact edits to make), and stops the daemon.

### 4.4 "Delete everything about this project" as a first-class flow

Scenario J47. It must be reachable in one action from all three surfaces (P3):

- **CLI**: `omt store purge --workspace ~/code/client-x`
- **TUI**: workspace row → `⟨leader⟩ ⇧X` → the same manifest in a modal, typed
  confirmation, live progress.
- **Web**: workspace detail → *Danger zone* → *Delete all omt data for this
  project* → the manifest rendered as a list with byte counts → typed
  confirmation. Marked `DESTRUCTIVE` in the catalog, so the mobile client
  requires a confirm gesture per
  [03 §2](03-capability-catalog.md#2-declaring-a-capability).

And the *offer* matters as much as the capability: when a workspace's root
directory has been deleted from disk (the `Missing` state,
[05 §7](05-session-model.md#7-workspace-identity)) and stays missing for 30 days,
omt surfaces one notification — *"~/code/client-x is gone but omt still holds
1.9 GiB about it"* — with purge and export as the two actions. Data that
outlives its subject is the thing users are surprised by.

---

## 5. Encryption at rest

**Decision: omt does not encrypt session content at rest. It encrypts nothing
beyond what the OS keychain holds, relies on full-disk encryption plus `0600`
modes for content, and stores credentials in the OS keychain where one exists
with a `0600` file fallback.**

The argument, because this is the decision reviewers will push on:

1. **The key would have to live next to the data.** omt is a daemon that starts
   at login, restores sessions without a prompt, and answers a phone at 3 a.m.
   Any key it can use unattended is a key stored on the same disk, readable by
   the same uid. That is obfuscation, and shipping it would let users believe
   something false — which is worse than the honest `0600`.
2. **The threat it would defend against is already covered better.** The real
   threats are a stolen laptop (FileVault/LUKS, and omt's job is to *tell* you if
   they are off — `omt doctor store` reports FDE status on macOS via
   `fdesetup status` and on Linux via `lsblk`/`cryptsetup`), a backup sync client
   (§1.2's exclusion list), and another local user (`0700` state directory and
   per-uid separation, [22 §2](22-operations.md#2-multi-user-machines)).
3. **omt is explicitly not a privilege boundary against your own user**
   ([13 §1.3](13-security.md#1-threat-model)). Encrypting content against a
   process running as you is theatre.
4. **A passphrase-gated store is a real design, and it is a different product.**
   It means no autostart, no unattended restore, a prompt on every daemon
   restart, and a phone that cannot reach a locked instance. If a user wants
   that, the answer is `persist.scrollback = false` plus FDE, and that answer is
   written down.

What omt *does* do:

- Enforces `0600`/`0700` at startup, **correcting** modes that are too permissive
  with a warning and refusing to start if the state directory is owned by another
  uid ([13 §5.1](13-security.md#51-at-rest)).
- Refuses to create the state directory on a filesystem mounted world-writable
  without `--i-know` (the shared-`/tmp` hazard,
  [22 §2](22-operations.md#2-multi-user-machines)).
- Reports FDE status in `system.health` and `omt doctor`, with the exact command
  to turn it on.

### 5.1 Credentials: keychain vs. file

Credentials are the one thing worth encrypting, because they are small, they are
touched rarely, and the failure mode is unbounded remote access.

| Platform | Backend | Reality |
|---|---|---|
| macOS | Keychain via `security`/`SecItem` | Works well. The daemon's access is granted once per binary signature; **an unsigned self-built `omt` triggers a prompt on every upgrade** because the binary's identity changed. Documented, with the "Always Allow" instruction |
| Linux (desktop) | Secret Service (`libsecret`, GNOME Keyring / KWallet) over D-Bus | Works when a session bus and an unlocked keyring exist. **On a headless server, over SSH, in a container, or under `systemd --user` before graphical login, it does not** — which is exactly where omt runs most |
| Linux (headless) | file | The only thing that works |
| Windows (WSL2) | file | The Windows Credential Manager is not reachable from WSL2 in a way worth depending on |

So: **the file backend is the default**, because it is the only one that works
in every environment omt targets, and the keychain is opt-in
(`auth.key_storage = "keychain"`), matching
[13 §5.1](13-security.md#51-at-rest). The file backend stores hashes and not
plaintext wherever the protocol allows it (argon2id for passwords, hashed bearer
tokens); the values that genuinely must be recoverable are the instance's
Ed25519 private key and any provider API keys the user configured for STT, and
those live in `secrets.toml` at `0600`.

`omt doctor store` reports which backend is in use and why the other was not
chosen — *"keychain requested but no Secret Service on the session bus; using
file backend at ~/.local/state/omt/credentials.db (0600)"* — because a silent
downgrade from keychain to file is precisely the kind of thing a user must not
discover from a blog post.

---

## 6. Crash consistency

### 6.1 The model

Append-log plus periodic snapshot, per
[05 §8](05-session-model.md#8-persistence-and-restore). **This document owns the
durability policy** — the `Record` frame below, the fsync classes in §6.3, the
repair path in §6.4 and the versioned-store rule in §7.1 — and
[05 §8.2](05-session-model.md#82-crash-semantics) defers to it, keeping only the
session-tree-specific facts (what is snapshotted, what `RestoreOutcome` means for
a restored session).

```rust
pub struct Record {
    len:      u32,        // little-endian, of `payload`
    crc:      u32,        // crc32c of `payload`
    kind:     u8,
    payload:  [u8],       // postcard-encoded
}
```

Every log is a sequence of these. A record whose length runs past EOF, or whose
CRC fails, terminates the replay — the classic torn tail — and the log is
truncated to the last good record. That is **not** corruption and is not
reported as such.

Snapshots are written to `<name>.tmp`, `fsync`ed, then `rename(2)`d over the
target, and the containing directory is `fsync`ed. Never written in place.

### 6.2 What `kill -9` loses

| Data | Loss window | Why |
|---|---|---|
| Session tree, layout | ≤ 500 ms (the structural-change debounce) | Small, and a lost layout tweak is trivially redone |
| Scrollback | **≤ `scrollback_flush`, default 10 s** | The dominant loss, and it is bounded output, not decisions |
| Block metadata | 0 for closed blocks; the currently-open block is lost | One record per close, appended synchronously |
| History | 0 | Same write as the block close |
| Agent events | ≤ 1 s (the event log's own flush) | |
| **Interaction ledger** | **0** | §6.3 — this is the one that gets `fsync` |
| Usage | ≤ 60 s | Rolled up; a lost minute is noise |
| Audit | 0 for `Denied`/auth events, ≤ 1 s for the rest | §6.3 |
| Credentials | 0 | Written rarely, always `fsync`ed |
| **Drafts** | ≤ 2 s (the draft debounce) | LWW free text with a `version` CAS; the loser is visible, so a lost keystroke is recoverable by the user who typed it. Never silently replayed |
| **omt-managed message queue** | **0** | §6.3 — a durable intent log, `fsync`ed on enqueue. It was memory-only in an earlier draft, which lost every non-Claude agent's queued text on `kill -9` without recording that it had. Required by [D15](decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism) consequence 8 |
| **Continuity state** (read marks, surface intent, last-seen positions) | ≤ 5 s | Small and idempotent; a lost read mark re-shows something already seen, which is the safe direction |
| **Notification acks** | ≤ 5 s | Same class as read marks. Note there is no notification *backend* ([D12](decisions.md#d12--no-push-notifications-in-v1-open-and-replay-instead)); this row covers the in-app acknowledgement that dismisses an attention item |
| **Attention log** | **0** | Written in the same transaction as the interaction's terminal-state transition, which is already `fsync`ed. If it were lost, an interaction that opened and went terminal inside an offline gap would be invisible forever — which is the entire reason the log exists |
| **Native session transcript** | ≤ 1 s | Same flush class as the agent event log. It is the *only* record of a `native` session, so it is the one place where a bounded loss window is a real loss of content rather than of re-derivable output |
| Search index | 0 for anything committed with its source row (D-R1's whole point); ≤ 30 s for the batched remainder, and **always recoverable** — the index is derived data and is rebuilt from the blocks with `INSERT INTO doc_fts(doc_fts) VALUES('rebuild')` | |

**This table is CI-enforced.** Every type persisted through `omt-store` must
have a row here, and a test fails the build when one does not — the same trick
[03 §5](03-capability-catalog.md#5-the-parity-test)'s parity test plays, and for
the same reason. Persisted types carry a `#[derive(Persisted)]` (or are
registered in `omt-store`'s type registry); the test enumerates the registry,
parses the row keys out of this section, and asserts the two sets are equal in
both directions — a missing row fails, and so does a row for a type that no
longer exists.

The requirement exists because this table drifted silently: the omt-managed
message queue was added, never given a row, and shipped memory-only for that
reason. A durability table that is maintained by hand is a table that is wrong
by the second release. Four of the rows above
([D15](decisions.md#d15--five-classes-of-pending-intent-each-with-its-own-delivery-mechanism)
consequence 8) were added by exactly this audit; the check is what stops the
next four from being found the same way.

### 6.3 fsync policy, and what it costs

Three durability classes, because a single policy is either too slow or too
lossy:

| Class | Data | Policy | Cost |
|---|---|---|---|
| `Critical` | interaction ledger, credentials, purge receipts, `Denied` audit entries | `write` + `fsync` **before the capability returns** | ~0.1–1 ms on NVMe, up to 10 ms on a spinning disk or a network filesystem. Paid at most tens of times per day |
| `Ordered` | block closes, tree snapshots, the rest of the audit log | `write` immediately, `fsync` on a 1 s timer and on clean shutdown | negligible |
| `Bulk` | scrollback, block output, agent events, index | buffered, flushed every 10 s / 1 MiB, `fsync` only at flush | negligible; this is where the volume is |

The `Critical` class exists for exactly one reason: **an interaction must resolve
exactly once, across a crash.** If a phone approves a tool call and the daemon
dies before the record is durable, a restart must not re-open the interaction and
ask again — the agent may already have run the tool. So the ledger write is
synchronous and precedes the acknowledgement to the client. That is the one place
where omt trades latency for correctness, and it is worth naming as such.

Configurable, with a documented footgun:

```toml
[store]
durability = "normal"    # normal | relaxed | paranoid
```
`relaxed` drops `Critical` to `Ordered` (for a laptop on battery, or a network
home directory), prints a startup warning, and is reported in
`system.health`. `paranoid` promotes `Ordered` to `Critical`.

### 6.4 Recovery and repair

`RestoreOutcome` is [05 §8.2](05-session-model.md#82-crash-semantics)'s enum. What
this document adds is the repair path when it returns `Partial`:

```
$ omt store repair
Scanning ~/.local/state/omt …

  tree.log                    ok       (torn tail truncated: 1 record, 812 B)
  interactions.log            ok
  interactions.db             ok       (integrity_check passed)
  store.db                    DAMAGED  (integrity_check: history row 41,882 malformed)
  store.db doc_fts            REBUILD  ('integrity-check' failed: doc_fts out of
                                        step with doc for 812 rows)
  sessions/s_4b2f/scrollback  DAMAGED  (chunk 91: crc mismatch)

Plan:
  store.db        → recover 41,881 history rows into store.db.repaired, quarantine
                    the original at quarantine/2026-08-03T18-51-10Z-store.db
  s_4b2f chunk 91 → truncate the session's scrollback at chunk 90; 2.1 MiB of
                    output after 2026-08-03T14:02:11Z is unrecoverable
  doc_fts         → INSERT INTO doc_fts(doc_fts) VALUES('rebuild')
                    (est. 3 min, background)

Proceed? [y/N]
```

Rules:

- **Nothing damaged is ever deleted.** It moves to `quarantine/` (inventory #18),
  which is never swept automatically and is reported by `store.usage`.
- **Repair is offered, never automatic**, except for the derived index, which is
  rebuilt without asking because it costs nothing to lose.
- A `Partial` restore surfaces as a warning event and a banner on every surface
  until acknowledged, and `omt doctor store` keeps reporting it.
- **The index is checked, not trusted.** `INSERT INTO doc_fts(doc_fts)
  VALUES('integrity-check')` is what detects an external-content FTS table that
  has drifted from its `doc` rows, and the only remedy offered is
  `VALUES('rebuild')` — there is no partial repair of an index, because there is
  no reason to attempt one.
- **The store is checked on startup, cheaply**: header magic, format version, and
  the tail record of each log. The full `integrity_check` runs only in
  `omt store repair` or after an unclean shutdown was detected.

---

## 7. Migration of on-disk formats

### 7.1 The versioned-store rule

> **Every persisted artifact carries a `format_version` in its first bytes (a
> log header record, a SQLite `user_version`, a manifest field). omt refuses to
> open a version it does not understand, and migrates forward explicitly. There
> is no best-effort partial parse, ever.**

`state/omt/STORE_VERSION` holds the store-wide version as a single integer and is
the first thing read at startup. It is the tripwire: if it is newer than the
binary, omt exits with

```
error[OMT-S310]: this store was written by a newer omt
  store version: 7   (~/.local/state/omt/STORE_VERSION)
  this binary:    5   (omt 0.4.1)

  A newer omt has used this state directory. Downgrading would corrupt it.
  → install omt ≥ 0.6.0, or point this build at a different state directory
    with OMT_STATE_DIR.
```

### 7.2 Migrating forward

```rust
pub trait Migration {
    fn from(&self) -> u32;
    fn to(&self) -> u32;
    fn describe(&self) -> &str;
    /// Must be idempotent and must not delete data it cannot convert —
    /// it moves it to `quarantine/` instead.
    fn run(&self, store: &mut StoreHandle, progress: &mut dyn Progress) -> Result<(), MigrateError>;
}
```

- Migrations run **on daemon start**, before any session is restored, one step at
  a time (5→6→7), each committing its own `STORE_VERSION` bump so an interrupted
  chain resumes rather than restarting.
- **A backup is taken first** when the migration is not trivially reversible:
  `state/omt.pre-<version>.bak` holds the small databases (tree, blocks, history,
  interactions, usage) — not the scrollback, which is too large and whose
  migrations are always append-only. The backup is deleted after 30 days by the
  sweeper, and `store.usage` shows it.
- **Migrations are visible.** They print progress, and take longer than a start
  should: `omt` prints `migrating store 5 → 6 (adding block attribution index):
  9,204 blocks…` rather than appearing hung.
- **Migrating down is not supported.** Exporting (§4.2) and importing into an
  older instance is the escape hatch, and it is documented as such.

### 7.3 Fixture requirement (P5)

Per [P5](01-principles.md#p5--production-grade-from-the-first-commit), and
enforced in CI:

```
tests/fixtures/store/
  v1/  v2/  v3/  …            ← a complete, small, realistic store per version
    STORE_VERSION
    tree.log  store.db  interactions.log  interactions.db
    sessions/s_fixture/scrollback/0000.zst
    EXPECTED.json             ← the post-migration state, asserted field by field
```

The rules the CI test enforces:

1. **Every released format version has a checked-in fixture**, generated by that
   version's binary and then frozen. A fixture is never regenerated.
2. **Every fixture migrates cleanly to `current`**, and the result matches
   `EXPECTED.json`.
3. **Adding a format version without a fixture fails the build.** The test
   enumerates `1..=CURRENT` and asserts a directory exists for each.
4. **A migration is tested for interruption**: the test runs it with a failure
   injected at each I/O call and asserts that a re-run reaches the same result.
5. Fixtures include the ugly cases on purpose: a torn tail, a zero-length log, a
   session directory with no matching tree entry, a `0644` file (which must be
   corrected), and a redaction marker from an older marker format.

---

## 8. Capabilities

Declared per [03 §2](03-capability-catalog.md#2-declaring-a-capability). Roles:
`V`iewer < `O`perator < `A`dmin.

| Capability | Kind | Role | Input | Output | Effects |
|---|---|---|---|---|---|
| `store.usage` | Query | V | `{ scope: StoreScope, group_by: Kind\|Workspace\|Session }` | `{ total_bytes, free_bytes, entries: [UsageEntry], last_sweep, next_sweep, notes: [String] }` | `READS_FS` |
| `store.paths` | Query | V | `{ purpose: All\|BackupExclusion }` | `{ paths: [{ path, kind, mode, advice }] }` | `READS_FS` |
| `store.export` | Command | O | `{ scope, format: Omtz\|Jsonl, fidelity: Text\|Ansi\|Full, include: [Blobs\|Audit], dest: PathBuf }` | `{ path, bytes, counts, manifest_sha256 }` | `READS_FS`, `WRITES_FS` |
| `store.export.progress` | Query | V | `{ job: JobId }` | `{ done, total, phase }` | — |
| `store.import` | Command | O | `{ archive: PathBuf, as_workspace: Option<String> }` | `{ workspace, counts, warnings }` | `WRITES_FS` |
| `store.purge` | Command | O | `{ scope, dry_run: bool, include_audit: bool, include_quarantine: bool, confirm: Option<String> }` | `{ manifest: PurgeManifest, executed: bool, reclaimed_bytes: u64, receipt: Option<PathBuf> }` | `WRITES_FS`, `DESTRUCTIVE` |
| `store.quarantine.list` | Query | A | `{}` | `{ items: [{ path, bytes, created, reason }] }` | `READS_FS` |
| `store.repair` | Command | A | `{ dry_run: bool, only: Option<Vec<Kind>> }` | `{ plan: [RepairStep], executed: bool }` | `WRITES_FS`, `DESTRUCTIVE` |
| `store.sweep` | Command | O | `{ full: bool }` | `{ reclaimed_bytes, duration, resumed: bool }` | `WRITES_FS` |
| `store.retention.get` | Query | V | `{ scope }` | `RetentionPolicy` | — |
| `store.retention.set` | Command | A | `{ scope, policy }` | `RetentionPolicy` | `WRITES_FS` |
| `store.persist.get` | Query | V | `{ session }` | `{ scrollback, block_output, agent_events, index, source: ConfigSource }` | — |
| `store.persist.set` | Command | O | `{ session\|workspace, persist: PersistFlags, delete_existing: bool }` | `{ applied: PersistFlags, deleted_bytes: u64 }` | `WRITES_FS`, `DESTRUCTIVE` when `delete_existing` |
| `store.redaction.explain` | Query | O | `{ session, block?, limit }` | `{ findings: [{ class, span, replacement, block, at }], stats }` | `READS_FS` |
| `store.redaction.test` | Query | V | `{ text: String }` | `{ findings: [Finding], redacted: String }` | — |
| `store.migrate.status` | Query | V | `{}` | `{ store_version, binary_version, pending: [Migration], backups: [{ path, bytes, created }] }` | — |

`store.purge` is the bulk, scope-shaped destruction. Its narrow counterpart is
[20 §12.2](20-recall-and-usage.md#122-history)'s `history.forget`, which removes
individual command rows and their FTS entries and is deliberately *not* folded
into this capability: "I just pasted a token into a command" must be one command,
not a retention-policy change with a typed confirmation.

CLI spellings follow the generated tree: `omt store usage`, `omt store export`,
`omt store purge`, `omt store repair`, `omt store explain-redaction`. All of them
honour `--json` ([22 §8](22-operations.md#8-automation-and-ci-g14)).

`store.redaction.test` deserves its place in the catalog even though it is
trivial: it is how a user validates an `extra_patterns` entry without running a
session, and how a bug report about a false positive becomes reproducible.

Events emitted:

| Event | When |
|---|---|
| `store.sweep.completed` | end of a full pass, with bytes reclaimed |
| `store.pressure.changed` | `Healthy` ↔ `Tight` ↔ `Pressure` ↔ `Critical` (§3.3) |
| `store.gap` | persistence was skipped for a session (disk pressure or a flip to ephemeral) |
| `store.purged` | a purge completed, with the manifest counts |
| `store.repair.needed` | a `Partial` restore or a failed integrity check |
| `store.migrated` | a format migration ran, with from/to |

---

## 9. OPEN QUESTIONS

1. **OPEN QUESTION — should the entropy rule default on for scrollback?**
   §2.2 says yes. It is the rule most likely to redact something a user wanted
   (a long build hash, an inline base64 asset, a JWT they are actively
   debugging). The alternative is on-for-index-and-events, off-for-scrollback,
   which protects the searchable copy — the one that leaves the machine — while
   leaving the raw record intact. Needs the false-positive rate measured on the
   real corpus (§2.6) before it is settled; the number, not an opinion, should
   decide it. Interacts with [20](20-recall-and-usage.md)'s search quality: a
   redacted index is a worse index.

2. **OPEN QUESTION — per-block persistence opt-out after the fact.** A user runs
   `cat .env`, sees it on screen, and wants *that block* gone. §2.5 offers
   session-level flipping only. A `session.blocks.forget { block }` capability is
   easy, but it interacts badly with scrollback chunking: a block's output may
   span chunk boundaries shared with neighbouring blocks, so forgetting one block
   means rewriting chunks. Probably worth it — this is the single most likely
   thing a user will ask for after an accident. Needs a design against
   [04 §2.4](04-terminal-core.md#24-scrollback-blocks-of-logical-lines).

   **It must not duplicate `history.forget`.** [20 §12.2](20-recall-and-usage.md#122-history)
   already owns `history.forget { matching: HistorySelector }`, the narrow "I
   just pasted a token into a command line" remedy: it deletes command rows and
   their FTS entries in one transaction and touches nothing else. If
   `session.blocks.forget` is added it is a *different* operation — it destroys a
   block's persisted **output** and rewrites the scrollback chunks that carry it —
   and the two must be specified against each other, with `store.purge` (§4.3)
   remaining the only bulk, scope-shaped destruction. Three overlapping ways to
   delete the same row is how a purge ends up incomplete.

3. **OPEN QUESTION — retention for a workspace whose sessions are still live.**
   §3.1 deletes scrollback older than 30 days. A session that has been running
   for 60 days (which is a normal `tmux`-user pattern) would lose the first half
   of its own scrollback while it is still open. Options: exempt live sessions,
   or apply the cap per session rather than by age. Lean: age-based deletion
   skips the currently-open scrollback chunk range of a live session, and the
   size cap does the work instead. Needs confirmation against
   [05 §8](05-session-model.md#8-persistence-and-restore)'s chunk generations.

4. **OPEN QUESTION — export of another instance's data.** §4.2 exports the local
   instance. A user with five boxes ([22 §6](22-operations.md#6-upgrade)) wants
   one archive. The federating client could fan out `store.export` and merge, but
   the merge has to reconcile `InstanceId`-scoped ids, and the archive would then
   need an instance dimension in every record. Deferred; the honest v1 answer is
   "run it on each host and keep five archives", stated in the docs.

5. **OPEN QUESTION — does purge need to reach the agent's transcripts?** §1.1
   says no, and reports them instead. But a user running "delete everything about
   this client" reasonably expects the agent's own transcript of that client's
   code to go too, and the omt manifest listing a path they must delete manually
   is a step people will skip. A `--include-agent-data` flag that shells out to
   each adapter's documented deletion mechanism (not `rm -rf` on a guessed path)
   is possible; it needs each adapter to declare one, which is a real addition to
   the `AgentAdapter` trait in [06](06-agent-layer.md). Needs a call with the
   agent-layer owner.

6. **OPEN QUESTION — a store on a network filesystem.** §6.3's `fsync` costs and
   the SQLite locking model both degrade badly on NFS/SMB home directories, which
   are common on corporate Linux fleets. Options: detect the filesystem type at
   startup and refuse (too aggressive), warn and switch to `relaxed` durability
   (probably right), or relocate the store to a local path automatically (magic,
   and it splits state across machines in a way users will not expect). Lean:
   warn loudly, force `durability = "relaxed"`, and put a specific remedy in
   `omt doctor store` pointing at `OMT_STATE_DIR`.
