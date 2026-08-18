# media Specification

## Purpose
TBD - created by archiving change 2026-08-17-the-clipboard-answers-and-the-blobs-move. Update Purpose after archive.
## Requirements
### Requirement: The clipboard follows the settings

A yanked selection SHALL be written through OSC 52 only when
`media.clipboard.osc52_write` is on, at `media.clipboard.osc52_max_bytes`, and
a clipboard read from a program in a pane SHALL be answered with the last
yanked text only when `media.clipboard.osc52_read` is on.

The write settings reached nothing — the selection always went at the built-in
cap — and reads were never answered at all: the daemon emitted no event for
them.

#### Scenario: Reads answered

- **WHEN** a program asks for the clipboard and reads are on
- **THEN** it receives the last yanked text

#### Scenario: Reads refused

- **WHEN** reads are off
- **THEN** the question is ignored, and nothing is written to the pane

### Requirement: An image is drawn within the configured rows

A pane SHALL draw an image at no more than `media.images.max_rows` rows, with
the stored image untouched.

#### Scenario: A capped image

- **WHEN** an image is taller than the cap
- **THEN** it is placed at the cap height, and scrollback keeps the whole
  picture

### Requirement: Blobs go where they are told

Materialised blobs SHALL be written under `media.blob_dir` when it is set.

#### Scenario: A configured directory

- **WHEN** `media.blob_dir` names a directory
- **THEN** attachments land there rather than in the state directory

