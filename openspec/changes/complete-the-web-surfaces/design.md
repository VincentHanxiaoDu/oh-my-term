# How

## The shape of the problem

Nothing here needs a new capability, a new event, or a change to the instance.
Every surface is a route to something already answered. So the whole change
lives in `web/src`, and the risk is not "can omt do this" but "does the client
stay one coherent thing while seven screens are added to it".

Three decisions carry that risk.

## 1. Screens are routes, and routes are exhaustive

`nav.ts` has a `Route` union and `titleOf` switches on it. Adding a screen means
adding a variant, and TypeScript's exhaustiveness makes every switch a compile
error until it is handled. That is the mechanism that keeps a seven-screen
addition from leaving a screen with no title, no back button, or no entry in
the deep-link table — all three of which are what "we added a screen" usually
costs.

New routes:

| Route | Reached from |
|---|---|
| `projects` | the roster's `+` |
| `files` (workspace, path) | a session's overflow, and `projects` |
| `changes` (workspace) | the same |
| `diff` (workspace, path) | a row in `changes` |
| `settings` | the instance switcher |

`terminal`, `threads`, `blocks`, `transcript`, `search` and `card` are
unchanged.

## 2. Attachments reuse the composer, not a new screen

08 §6.3 puts attach on the composer's left, mic on the right, send at the
corner. A separate "attachment screen" would be a second place to type a
message from, and the two would drift.

The sheet is four rows — photo, file, workspace file, paste — and each is a
visible control, because 08 §8.4 forbids a capability reachable by gesture
alone and the parity test cannot drive a `paste` event.

Staging is client-side and optimistic: the attachment appears in the composer
immediately with a progress state, and a failure names *that* attachment
without touching the text. The text somebody typed is the expensive thing on a
phone and it is never the thing that is lost.

## 3. Language is a build-time table and a runtime lookup

A `Messages` record per language, keyed by a union of string literals, so a
missing key is a type error rather than a runtime fallback nobody sees. English
is the type; every other language is `Partial<typeof en>`, which makes the
fallback total by construction and makes "translated" a thing the compiler can
count.

No interpolation helper takes a fragment. `waiting(n)` is a function per
language, not `${n} + t('waiting')` — because a sentence with a number in it
has different word order and different plural rules per language, and the
concatenating version is the one that is wrong everywhere except where it was
written.

**Width, not length.** The status bar, the roster's truncation and the
terminal's own grid already measure in columns; the client's truncation
measured `String.length`, which counts a CJK glyph as one and draws it as two.
One `columns(text)` helper, used everywhere something is cut to fit.

## What could go wrong

- **Bundle size.** Six screens and two message tables on LTE. The message
  tables are small; the screens are code-split behind their routes, which the
  existing dynamic-import boundary already supports.
- **The files sheet on a large repository.** 15 §4.2 makes expansion lazy and
  per-directory, and 15 §4.3 caps what one directory may return. Both are the
  instance's, already enforced, and the client's job is only not to ask for the
  whole tree.
- **Diff rendering cost.** 08's open question 6 flags `shiki`'s WASM as ~1 MB.
  This change ships plain-text hunks with line numbers and no syntax
  highlighting; the highlighting question stays open and is not on this change's
  critical path.

## Testing

Every screen gets a Playwright case on both device profiles, because 08 §10
already runs that suite against a real instance and a mocked one would pass
whatever the screens did. The three that carry a decision — the project picker
refusing a path, an attachment failing without eating the text, and a CJK
string truncating to the right width — get unit tests as well, since those are
properties rather than renderings.

## What the implementation forced back into the documents

Three corrections, each recorded here and applied where it belonged.

**08 §6.3's capability table named a surface that does not exist.** It lists
`media.image.upload`, `media.picker.open` and `media.blob.begin`/`.commit`/
`.abort`. The catalog has `media.put`, `media.get`, `media.attach`,
`media.list` and `media.clipboard`. The client had been quietly using
`fs.write` instead — which is not a smaller version of the documented design
but a different one, because it put an unreviewed file from a phone into the
repository root, where it shows up in `git status` and eventually in a commit.
09 §2 already says the managed root. So the change added `media.materialize`,
which writes a stored blob out under omt's own root and answers with the path,
and 08 §6.3 now carries a table mapping what it wrote against what is built.
The sheet's rows are unchanged: which capability sits behind a row is the
row's business.

**`session.list` knew which agent was running and did not say.** A session's
kind is fixed at creation and `SessionKind::Agent { kind }` was right there;
the summary reported only the title. Every client that needed the agent
therefore guessed from the title, and the guess is wrong for most of the
covered set — each agent has its own attachment syntax, and a reference in the
wrong one is silently ignored, which is indistinguishable from an agent
choosing not to read the file. `agent` is now a field, and `media.attach`
answers the syntax question from the instance's own per-adapter table rather
than from a regex in the client.

**A workspace id is a hash of a path that nothing canonicalized.** The
`workspace.open` input is documented as canonical and callers send what they
have. On macOS `/tmp` is a symlink to `/private/tmp`, so the project list
showed the same directory twice, each with its own Files and Changes over
identical contents. Canonicalization happens at the three edges in `omt` where
a path first arrives; it cannot happen in `omt-session`, which is state with
no filesystem under it by design.
