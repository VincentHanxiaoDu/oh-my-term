# 24 — The mobile surface

> Status: design. The protocol, the thread roster and the grid exist; the
> application shell does not.

## 1. What a phone is for

Not "a terminal, smaller". A phone is used standing up, one-handed, for ninety
seconds, usually because something buzzed. The whole surface follows from that:

> **The mobile client answers one question — *does anything need me, and can I
> deal with it from here?* — and only then offers everything else.**

A terminal that opens to a wall of text has already failed that. So the first
screen is the roster, not a terminal.

## 2. Screens

```
┌─ Sessions ──────────────┐   ┌─ Session ───────────────┐   ┌─ Card ─────────┐
│ 2 of 7 need you         │   │ ● ● ◐ ● ●   5 subagents │   │ Bash wants to  │
│                         │   │ ● ⬤ ● ● ◐                │   │ run:           │
│ ⬤ api    needs you      │   │                          │   │                │
│ ⬤ web    needs you      │   │ ┌─ transcript ────────┐  │   │  rm -rf build  │
│ ◐ infra  working        │   │ │ …                    │  │   │                │
│ ○ docs   idle           │   │ └──────────────────────┘  │   │ [Allow] [Deny] │
│ ✓ tests  finished       │   │                          │   │  hold to allow │
└─────────────────────────┘   └──────────────────────────┘   └────────────────┘
```

### 2.1 Roster — the home screen

Every session as a row, sorted **blocked first**. Not by name, not by recency:
spawn order buries the one row that matters behind four that do not, which on a
phone means scrolling to find the only thing you opened the app for.

The header is one number: `2 of 7 need you`. If nothing needs you, it says what
is working instead.

### 2.2 Session — the subagent grid

The thing Claude Code's own mobile client does not do: with five agents running
it shows one at a time. omt shows all of them as a dense grid, each cell
coloured by `AgentState` **and carrying a distinct glyph** — a grid whose only
signal is hue does not tell some users which agent is stuck.

Tapping a blocked cell opens its card. Tapping a working cell opens its
transcript. `actionsFor()` never offers a prompt for a running subagent,
because there is no input channel to deliver one into — see [06 §5](06-agent-layer.md).

### 2.3 Card — the reason the app exists

One question, the agent's own options, verbatim and in its order. omt neither
adds, removes nor reorders them: the indices are how the answer is delivered.

**Destructive options require a hold, not a tap.** A phone in a pocket, a
mis-scroll, a fat finger — a single tap that runs `rm -rf` is the one failure
this surface cannot have. Anything omt classifies as destructive gets a
press-and-hold with visible progress; everything else is a tap.

A card that omt **cannot** deliver an answer to is shown read-only, with the
reason and a route to the terminal. Read from `Deliverable`, never from
`state == Open` — an open card is not necessarily one omt can answer, and a
button that silently does nothing is worse than no button.

### 2.4 Terminal — the escape hatch

Not the front door. Letterboxed rather than reflowed: a phone is ~40 columns
and a program that drew an 80-column frame becomes unreadable when re-wrapped,
where letterboxing keeps it legible under a pinch-zoom.

## 3. Input

### 3.1 The key bar

A phone keyboard has no `Esc`, no `Ctrl`, no arrows, no `Tab`. Without a key
bar the terminal is read-only in practice.

```
[Esc] [Tab] [Ctrl] [↑] [↓] [←] [→] [/] [-] [|]
```

`Ctrl` is sticky: tap it, then a letter. A modifier that had to be held is a
modifier that cannot be used one-handed.

### 3.2 Gestures

| Gesture | Does | Why |
|---|---|---|
| Two-finger drag | Scroll the terminal | One finger is text selection |
| Long press | Start a selection | The only unambiguous way on touch |
| Pinch | Font size | Reflow is worse than zoom at 40 columns |
| Swipe from edge | Back | The platform gesture; overriding it is hostile |

Nothing destructive is on a swipe. A gesture that can be made by accident must
not be able to stop an agent.

## 4. Notifications

The constraint that shapes this: **omt has no cloud.** A self-hosted instance
on somebody's laptop cannot send an APNs push, because APNs requires a server
with a certificate and a stable address.

Three mechanisms, in the order they are tried:

1. **Web Push (VAPID)** where the platform supports it — no third party, the
   instance signs its own pushes.
2. **A user's own relay** — ntfy, Pushover, Gotify. The user supplies the
   topic; omt posts to it. Their infrastructure, their choice, their privacy.
3. **Foreground only** — the app is open, the WebSocket is live, and it buzzes
   itself.

omt never runs a notification service on behalf of users. A terminal
multiplexer that routed its users' agent activity through its author's server
would be a different product with a different threat model.

## 5. Connection

- **Same network / VPN** — Tailscale is the common case, and the instance's
  tailnet address needs nothing else.
- **Through a bastion** — `omt ssh` from a machine that can reach both.
- **Never a public port by default.** `omt web` binds loopback.

Reconnect backs off with full jitter and **does not retry a rejected
credential** — retrying produces the same rejection forever and hides the one
thing the user needs to be told.

## 6. What is built and what is not

| | |
|---|---|
| Protocol, resume, gap detection | ✅ `web/src/` |
| Thread roster and grid ordering | ✅ `omt-agent`, `web/src/threads.ts` |
| Answerability from `Deliverable` | ✅ |
| Capability calls over WebSocket | ✅ |
| Application shell, screens, key bar | ❌ |
| Push | ❌ |
| Hold-to-confirm | ❌ (the rule is decided; the widget is not written) |

## 7. Open question: running an agent *on* the phone

Whether a phone could be the machine rather than a window onto one is under
research — see [research/mobile-native.md](../research/mobile-native.md). The
answer changes the architecture materially if it is yes, so nothing here
assumes it either way.
