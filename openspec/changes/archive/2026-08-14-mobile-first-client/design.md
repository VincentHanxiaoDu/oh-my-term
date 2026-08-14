# Design

## The keyboard is the device's, reached through a real input

A hidden `<textarea>` inside the terminal screen, focused when the screen opens
and when the terminal is tapped. Not `contenteditable`, and not a synthetic key
grid:

- A real form control is what every mobile browser opens the keyboard for.
- It gives IME, swipe, dictation, autocorrect and paste for nothing, because
  the browser already implements all of them.
- A custom key grid would be a worse keyboard that omt then has to maintain,
  in every language.

It is emptied on every input event. Left to accumulate, the second keystroke
sends both characters — and with autocorrect on, the browser rewrites what is
already in it, so what gets sent is not what was typed. `autocorrect`,
`autocapitalize` and `spellcheck` are all off for the same reason: a terminal
is not prose, and a capitalised `Ls` is a command that does not exist.

## One owner for the terminal's state

The bug was two independent paths calling `render()`: the store's subscription
and the route change. The store's fires on any change — a session list refresh,
a card arriving — and the terminal's `null` snapshot meant "loading", so an
unrelated refresh could repaint a drawn terminal into a spinner.

The terminal screen now keeps its own `ScreenState`, and a store update that is
not about the terminal cannot move it. Coming back to a terminal already seen
shows the last screen immediately: the content is still true, and a spinner over
known-good content is a downgrade, not a loading state.

## Pairing is a QR, not a transcription

The token is 32 characters of base62. Reading that off a laptop into a phone is
where people stop, and it is why the first attempt at using this went to the
wrong port entirely.

`omt web` prints a QR of the full invite URL — fragment and all, so the token
never reaches an access log. Rendered as Unicode half-blocks so it works in any
terminal without an image protocol.

## Instances are a list, not a global

A person has a laptop and a desktop. The client keeps a list with a credential
each, and switching is picking one. Forgetting one deletes its credential here,
which is the only thing this device holds.
