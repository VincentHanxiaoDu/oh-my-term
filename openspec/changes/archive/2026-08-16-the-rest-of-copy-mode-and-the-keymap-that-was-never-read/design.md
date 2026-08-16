# Design

## Motions that need to read a line

`go` moves within known geometry and needs nothing but the row count. Word
motions need the line's text, so they take the same `FnMut(u64) -> Option<String>`
reader every other reading path here takes — because reading a line may be a
call to another process, and a signature promising otherwise pushes the lie
outward until somebody wraps a socket in a lock to satisfy it.

A word motion stops at the end of its line rather than wrapping. Wrapping moves
the reader to a line they were not looking at, and in a scrollback the next
line is usually unrelated output rather than a continuation.

## Line-wise selection

A flag on the selection rather than a second selection type. `span` already
returns an ordered pair of positions; line-wise selection widens that pair to
the ends of its rows at the point the text is taken, so every consumer that
already understands a span keeps working and nothing else learns a second
shape.

## Search direction and case

The direction the search was made in is remembered, and the repeat keys are
relative to it. Repeating a backward search forwards is what `N` means; a
repeat key that always went forwards would make `?` a one-shot.

Smart case is decided from the query: any upper-case character in it means the
comparison is exact. Both sides are lower-cased otherwise, which is what the
existing search does and the reason it stays the default — a reader hunting
through their own output is looking for a word, not a spelling of it.

## Prompts

Copy mode is handed the prompt rows rather than the blocks. It has no business
knowing what a block is, and the rows are the only part it can act on: they are
absolute, which is the coordinate it already moves in.

Unattributed blocks are filtered out **by the caller**, where the block model
lives and where `attributed` means something. Copy mode receiving a list of
rows cannot know which of them omt guessed at.

## The keymap flavour

One key table per flavour, chosen where the key is read. Not a layer in the
keymap: copy mode is a modal context whose keys are not the pane's, and
threading a whole binding resolution through it would be a much larger change
for a setting that has two values people actually use.

`default` keeps the current table exactly, so nothing changes for anybody who
has not asked for something else.
