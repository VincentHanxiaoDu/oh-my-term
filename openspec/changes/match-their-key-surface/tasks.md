## 1. The inventory

- [x] 1.1 Their 47 actions extracted from `config/model.rs`, counted against
      omt's 14
- [x] 1.2 Each one marked present, missing-and-wanted, or missing-on-purpose
      with the reason

## 2. The layout tree

- [x] 2.1 `zoom`: one pane fills the area, and the arrangement comes back
      exactly, ratios included
- [x] 2.2 `swap` in four directions: the pane moves, not the focus
- [x] 2.3 `last_pane`: back to where focus just was

## 3. Reaching a session

- [x] 3.1 `next_agent` / `previous_agent`: focus jumps to what is waiting,
      without a list
- [x] 3.2 Nothing waiting says so rather than appearing stuck
- [x] 3.3 `goto`: `⟨leader⟩ 1`–`9`, matching the numbers the status bar
      already writes — a key that disagreed with what is on the screen would be
      worse than no key

## 4. Naming

- [x] 4.1 `session.rename` as a capability, so both surfaces get it
- [x] 4.2 An empty name is refused
- [x] 4.3 The name is what the bar, the panel and the web client show — all
      three read the session's title, so renaming reaches them without another
      change

## 5. Keys

- [x] 5.1 Every new action bound and advertised
- [x] 5.2 The strip still fits 100 columns

## 5b. The document

- [x] 5b.1 16 §3.3b: the real binding table, and why a one-row strip cannot
      list everything — the order is a priority order and the marker points at
      `?`

## 6. Close-out

- [x] 6.1 clippy, workspace tests, layering, codegen, test-isolation
- [x] 6.2 `npm run check`
- [x] 6.3 Through omt's own emulator: zoom, swap, jump
