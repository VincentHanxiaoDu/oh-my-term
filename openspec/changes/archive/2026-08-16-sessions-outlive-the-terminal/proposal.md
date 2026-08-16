## Why

A multiplexer exists to make one promise: **close the terminal, the work keeps
running.** omt has documented that promise since [05 §4](../../docs/architecture/05-session-model.md)
— *"a session is never killed by a client going away"* — and the terminal does
not keep it. It owns every pty in its own process, so quitting hangs up on all
of them.

The previous change stopped omt from *lying* about it: the key says `quit`, and
it lists what it is about to end. This change makes the promise true.

Everything needed already exists and is not joined up:

- `omt serve` owns sessions, pumps them, observes their agents and serves the
  capability catalog over a socket.
- `omt attach` proves a session outlives its terminal — by polling whole text
  snapshots at 30 ms, with no colour, no splits, no panel and no copy mode. It
  is an existence proof, not the product.
- The protocol specifies a binary terminal frame with `seq` and `ack`
  ([07 §4](../../docs/architecture/07-remote-protocol.md)) and nothing produces one.

## What Changes

**The terminal becomes a client of a daemon that owns the ptys**, and keeps
every surface it has today — splits, the agent panel, copy mode, the
conversation view, notifications — because the client goes on running the same
emulator it runs now. What changes is where the bytes come from.

**One channel, bytes only.** A subscriber receives the session's raw output as
binary frames. On attach it receives a **seed**: the daemon renders its own
terminal state back into escape sequences, so re-attaching and live output are
the same kind of thing and there is exactly one path to get wrong. A client that
falls behind the daemon's bounded buffer is re-seeded rather than handed a gap —
a raw byte stream must never be replayed from an arbitrary point, which is why
`ack` exists in the header and why the seed is not "the tail you missed".

**Quitting detaches.** The sessions stay, their agents keep working, and the
next `omt` in that directory finds them and repaints from a seed. The
confirmation added last change stops being needed for the ordinary case and
stays for the one it still describes: ending sessions on purpose.

**The daemon starts itself.** Nobody should have to know that `omt serve`
exists. The terminal discovers a daemon, and starts one when there is none.

## Capabilities

### New Capabilities

- `terminal-streaming`: pushing a session's output to a subscribed client, and
  seeding one that has just arrived.

### Modified Capabilities

- `session-lifetime`: quitting a terminal leaves its sessions running.

## Impact

- `SessionRuntime` keeps a bounded ring of recent output and a byte counter, so
  a subscriber has something to be behind *of*.
- `omt-term` gains a renderer from terminal state back to escape sequences. It
  is the seed, and it is also what `omt attach` should have been using.
- The socket server gains a push side. Today it is strictly
  request-then-response; a subscriber needs frames it did not ask for.
- `omt`'s frame loop gains one seam — where a pane's terminal comes from — and
  keeps everything else.
