# The clipboard answers, and the blobs move

## Why

Five wires and five honest declarations, from the media group out of
`UNWIRED`.

- `media.clipboard.osc52_write` and `osc52_max_bytes` reached nothing: a
  selection always went to the terminal's clipboard at the built-in sixty-four
  KiB cap, and the settings that exist to steer both were decoration.
- `media.clipboard.osc52_read` reached nothing too: a program in a pane asking
  for the clipboard was never answered — the daemon emitted no event for it,
  which is the vocabulary-without-a-consumer shape for the second time in one
  change.
- `media.images.max_rows`: an image filled however many rows it had; somebody
  who capped it watched a screenshot take half their pane.
- `media.blob_dir`: materialised blobs went to the state directory whatever
  the setting said, so somebody who pointed them somewhere with space in it
  still filled the default.

Five more are declared rather than wired, each with the reason it deserves:
`thumbnail_max_px` sizes thumbnails that no machinery produces,
`osc_bridge.max_bytes` caps the in-band OSC bridge (09 §5.3) that is not
implemented, `reverse_socket.hosts` names the hosts allowed a reverse socket
(09 §5.2) that is not implemented, `max_blobs_per_session` counts attribution
the store does not record, and `ttl_referenced` wants a second lifecycle that
`sweep()` does not have. Each of those is a design change behind the key, not
a wire.

## What changes

- `yank` takes the configuration: writing is a setting, the cap is a setting,
  and the yanked text comes back so the loop can answer reads with it.
- The daemon emits a `ClipboardRead` event; the TUI answers it with the last
  yanked text when `osc52_read` is on, and with nothing otherwise.
- The image placement is capped at `max_rows`; the stored image is untouched,
  so the scrollback keeps the whole picture.
- `managed_root` honours `media.blob_dir`, with the environment override
  staying above it — it is the tests' escape hatch.
- The mouse handler's argument list became one `Pointer` value, and the
  overlay arms left `run.rs` for `panels.rs`, both because the line limit
  caught a file that had outgrown being read.

## Non-goals

- The five declared keys stay declared; each names a feature behind it.
- The shell, log and appearance groups are their own changes.
