# The quota and the switch

## Why

The media group out of `UNWIRED`, and two symptoms a user would meet.

`media.quota.max_blob_bytes` and `media.quota.max_total_bytes` are in the
settings reference and the settings screen, and the blob store was built with
`Quota::default()`. Somebody who raised their limit still had a screenshot
refused at thirty-two megabytes — by a number they had changed, and could see
was changed, in a screen that reported where the change came from.

`media.images.enabled` could not turn images off, and
`media.images.protocol = "none"` drew them anyway: the image path was
unconditional, so the switch and the protocol list were both decoration.

## What changes

- The blob store is built from the configured quota — blob cap, total cap and
  ttl — each part falling back on its own, so a file that misspells one keeps
  the other.
- Sizes are read the way they are written: `32MiB`, `512 MB`, `1GiB`. Anything
  unreadable keeps the default rather than being guessed at, because a quota
  guessed wrong either refuses somebody's screenshot or fills their disk.
- `media.images.enabled = false` and `protocol = "none"` stop omt asking the
  terminal about graphics at all, which is the first thing it writes.

## Non-goals

- `max_blobs_per_session`, `ttl_referenced`, `blob_dir`, `max_rows`,
  `thumbnail_max_px`, the clipboard and bridge caps stay declared unwired:
  each needs a place in a type that does not have one yet, and inventing the
  field is a bigger change than reading the key.

## Impact

- `omt`: the store's construction, the graphics probe, one size parser.
- Two screen tests, one of which is the positive control for the other.
