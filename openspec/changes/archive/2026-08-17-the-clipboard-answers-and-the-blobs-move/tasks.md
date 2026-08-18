# Tasks

## 1. The clipboard
- [x] 1.1 `yank` honours `osc52_write` and `osc52_max_bytes` and returns the text
- [x] 1.2 A `ClipboardRead` event from the daemon, answered with the last yank
- [x] 1.3 The write path threads the configuration through the mouse handler

## 2. The image and the blobs
- [x] 2.1 `images.max_rows` caps the placement, not the stored image
- [x] 2.2 `managed_root` honours `blob_dir` under the environment override
- [x] 2.3 The cap is tested in the placement path

## 3. The declared five
- [x] 3.1 thumbnail / osc_bridge / reverse_socket / per-session / second-TTL
      declared with the feature each names

## 4. The gates
- [x] 4.1 `pointer.rs`-style argument grouping keeps clippy quiet
- [x] 4.2 Overlay arms move to `panels.rs`; `run.rs` back under the line limit
- [x] 4.3 fmt, clippy, six gates, workspace and screen suites, web check
