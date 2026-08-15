## 1. The hook ingress

- [x] 1.1 `HookEvent` answered on the socket, before dispatch, without a
      handshake
- [x] 1.2 A socket bound beside the terminal that owns the sessions, owning
      neither the pump nor the snapshot
- [x] 1.3 `$OMT_SOCK` in every pty omt spawns
- [x] 1.4 An unattributed report, and one for another instance's session, are
      dropped
- [x] 1.5 A question raises a ledger card and reports `Blocked` at hook tier
- [x] 1.6 The agent's own session id is remembered
- [x] 1.7 End to end through the real `omt-hook` binary and a real socket
- [x] 1.8 End to end through a real terminal, fired from inside a pane

## 2. The conversation view

- [x] 2.1 `⟨leader⟩ t`, rebuilt every frame
- [x] 2.2 Speakers marked apart; synthetic turns never shown as human ones
- [x] 2.3 Open cards read from the ledger, never copied into the transcript
- [x] 2.4 Empty is a sentence, not an empty list
- [x] 2.5 19 §2.1c

## 3. Attention

- [x] 3.1 `omt-notify` at L2: backends, sequences, tmux wrapping, sanitizing
- [x] 3.2 The policy, in its own module: a report is not a change; being waited
      for always counts; finishing only when nobody watched
- [x] 3.3 Sound from what the machine already has; no audio in the repository
- [x] 3.4 `notifications.when` read at last, plus the sound settings
- [x] 3.5 Wired to the transition, not to the state
- [x] 3.6 A screen test asserting the bytes a terminal actually receives
- [x] 3.7 19 §2.1d

## 4. Copy mode

- [x] 4.1 Absolute-row model, viewport following the cursor
- [x] 4.2 Selection, ordered either way it is dragged
- [x] 4.3 Yank over OSC 52, padding trimmed, aged-out rows omitted
- [x] 4.4 `⟨leader⟩ [`, the keys people already have, a visible way out
- [x] 4.5 Keys do not fall through to the pane
- [x] 4.6 A screen test that scrolls output away and reaches it
- [x] 4.7 `/`, `n` and `N`: case-insensitive, no match moves nothing, an empty
      query is dropped
- [x] 4.8 16 §6.8b, including what is still only specified

## 5. Resume

- [x] 5.1 `AgentAdapter::resume_argv`, per agent, defaulting to none
- [x] 5.2 Four agents' own spellings, and a test that they differ
- [x] 5.3 `agent.resume_plan`, with a reason rather than an empty list
- [x] 5.4 The web handler and the schema snapshot the gates demanded
- [x] 5.5 06 §7.1b

## 6. Answering "what colour are you"

- [x] 6.1 `OSC 10/11 ; ?` answered from what omt was told, and never guessed
- [x] 6.2 Setting a colour is still a set, not a question
- [x] 6.3 The colours learned from configuration or `$COLORFGBG`, never by
      querying the terminal omt is running in
- [x] 6.4 04 §5.4.1

## 7. Close-out

- [x] 7.1 clippy, workspace tests, layering, licensing, codegen, file length
- [x] 7.2 `npm run check`
- [x] 7.3 `cargo xtask install` installs the hook binary too
- [x] 7.4 The screen tests wait for a condition rather than for a duration —
      a fixed sleep is a guess about the machine, and it was wrong under a
      full run
- [x] 7.5 `run.rs` split back under the length the repository sets: the
      reading mode, the first-run summary and the answers-a-question helpers
      are their own modules
