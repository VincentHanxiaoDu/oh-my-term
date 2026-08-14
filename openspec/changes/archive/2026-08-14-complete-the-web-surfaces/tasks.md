# Tasks

## 1. Routes

- [x] 1.1 Add `projects`, `files`, `changes`, `diff` and `settings` to the
      `Route` union, and let the compiler find every switch that has to handle
      them
- [x] 1.2 A title and a back destination for each, so no screen is reachable
      and unnameable
- [x] 1.3 Deep links: each new route survives a reload, since a phone that
      backgrounds the browser and comes back must land where it was

## 2. Open a project

- [x] 2.1 The picker: workspaces from `workspace.list`, worktrees from
      `worktree.list` beneath each
- [x] 2.2 A workspace whose root is gone is shown disabled with the reason,
      never omitted
- [x] 2.3 One tap creates a session with the default kind and mode — no form
- [x] 2.4 No path field, and a line saying that a new directory is opened by
      running `omt` in it
- [x] 2.5 The roster's `+` opens this rather than creating a session in
      whatever workspace happened to be first

## 3. Attachments

- [x] 3.1 The attach sheet: photo, file, workspace file, paste — four visible
      rows, none gesture-only
- [x] 3.2 Staging in the composer with a per-attachment progress state
- [x] 3.3 Transfer through `media.put`, and a workspace file referenced rather
      than uploaded — needed a new `media.materialize`, because `media.put`
      answers with a digest and an agent reads a path. It writes under omt's
      managed root per 09 §2; the client had been using `fs.write`, which put
      an unreviewed file from a phone into the repository root.
- [x] 3.4 A failure names the attachment and leaves the typed text alone
- [x] 3.5 The reference handed to the agent uses that agent's own syntax

## 4. Files and diffs

- [x] 4.1 The files sheet: `fs.list`, one directory per expansion
- [x] 4.2 Reading a file with `fs.read`, with the size cap the instance
      enforces reported rather than hit silently
- [x] 4.3 The changes list from `git.status`
- [x] 4.4 The diff reader from `git.hunks`, with line numbers, following
      15 §7.4
- [x] 4.5 Binary and over-limit files say which they are rather than rendering
      an empty diff
- [x] 4.6 No stage, unstage or commit anywhere, per 15 §1.1

## 5. Worktrees

- [x] 5.1 List a workspace's worktrees with their branches
- [x] 5.2 Open a session in one
- [x] 5.3 A refused removal reports what would be lost — the counts, not
      "it has uncommitted changes", which is what git says about every worktree
      anybody has ever removed on purpose

## 6. Settings

- [x] 6.1 Render every key from `config.schema` with type, default and
      description
- [x] 6.2 Show the layer a non-default value came from, via `config.sources`
- [ ] 6.3 Editing writes through `config.import`, which is the only write path
      — **not done.** Reading is; writing needs a per-type editor (a toggle, a
      number, a choice list) and that is a screen's worth of work, not a row's

## 7. Cost and context

- [x] 7.1 One line under an agent session's title: tokens in and out, context
      used
- [x] 7.2 Cost only where the agent stated one; never computed
- [x] 7.3 Nothing on the roster

## 8. Language

- [x] 8.1 A `Messages` type from the English table, with every other language
      `Partial` of it, so the fallback is total by construction
- [x] 8.2 Simplified Chinese for every string omt owns
- [x] 8.3 A phrase with a count is one template per language, never a
      concatenation
- [x] 8.4 An agent's words are passed through untranslated, and a test pins it
- [x] 8.5 A `columns(text)` helper that counts display width, used everywhere
      text is cut to fit — a CJK glyph is two columns
- [x] 8.6 Chosen from the browser, overridable in settings, stored client-side
      and carried by no capability

## 9. Tests

- [x] 9.1 A Playwright case per new screen, on both device profiles, against a
      real instance
- [x] 9.2 Unit: the picker offers no path field and no unknown directory
- [x] 9.3 Unit: a failed attachment names itself and the composer's text
      survives
- [x] 9.4 Unit: a CJK string truncates to the right *width*, not the right
      length
- [x] 9.5 Unit: every English key has a Chinese entry, or is deliberately
      listed as untranslated
- [x] 9.6 A screenshot pass over every new screen in both themes and both
      shapes, reviewed rather than only diffed. Four defects were visible in
      the renders and in nothing else:
      - the measure was on each row rather than on the container, and `.tap`
        sets `margin: 0`, so every row was measured and then pinned left under
        a centred header
      - the projects screen indented its sub-rows with two leading spaces,
        which HTML collapses, so four projects rendered as twelve unrelated
        things
      - a directory and a file were indistinguishable in a listing, because
        the client read `directory` and the instance sends `is_dir`
      - a workspace with no git history rendered as alarm-red "That did not
        work", for a fact about the place rather than a failure

## 10. Close-out

- [x] 10.1 `npm run check` clean: typecheck, unit, build
- [x] 10.2 The Rust gates unaffected: clippy, tests, codegen-check, parity
- [x] 10.3 Record in this change any architecture-document correction the
      implementation forced
