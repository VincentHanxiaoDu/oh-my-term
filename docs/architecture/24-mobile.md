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
┌─ Sessions ──────────────┐ ┌─ Session ───────────────┐ ┌─ Card ─────────┐
│ 2 of 7 need you │ │ ● ● ◐ ● ● 5 subagents │ │ Bash wants to │
│ │ │ ● ⬤ ● ● ◐ │ │ run: │
│ ⬤ api needs you │ │ │ │ │
│ ⬤ web needs you │ │ ┌─ transcript ────────┐ │ │ rm -rf build │
│ ◐ infra working │ │ │ … │ │ │ │
│ ○ docs idle │ │ └──────────────────────┘ │ │ [Allow] [Deny] │
│ ✓ tests finished │ │ │ │ hold to allow │
└─────────────────────────┘ └──────────────────────────┘ └────────────────┘
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
transcript. `actionsFor` never offers a prompt for a running subagent,
because there is no input channel to deliver one into — see [06 §5](06-agent-layer.md).

### 2.3 Card — the reason the app exists

One question, the agent's own options, verbatim and in its order. omt neither
adds, removes nor reorders them: the indices are how the answer is delivered.

**The decision is a footer on the card describing the action, not a modal.**
A modal separates the question from what it is about, and the user then
approves a sentence rather than a command.

**Restate the concrete parameters, never "are you sure?".** Show the literal
command and its blast radius — `rm -rf ~/projects — 1,240 files`. This is the
same rule EU PSD2 dynamic linking imposes on payments (RTS 2018/389 Art. 5:
the confirmation must show amount *and* payee, and any change invalidates the
approval), and the terminal analogue is exact.

**An approval is bound to the card id and a hash of the exact command.** If
what the agent is asking for changed between render and tap, the approval is
rejected rather than applied to the new request.

**A second gate only above a risk threshold** — and this is a correction. The
first draft required a hold for every destructive option. The evidence says
that backfires: Akhawe and Felt (USENIX Security 2013) measured 23–25%
click-through on Chrome malware warnings, and found the *most frequently shown*
warning had the *lowest* adherence. Gating everything manufactures the
habituation the gate exists to prevent. So: hold-to-confirm is reserved for
irreversible **and** wide-blast-radius, everything reversible prefers undo, and
the presentation of the highest-risk class is varied — Anderson et al. (CHI
2015) showed varying a warning's appearance measurably reduces habituation.

Target sizes are 44×44pt (Apple) or 48×48dp (Material), with 16pt+ between
opposing actions. A stock Material dialog puts 8dp between Cancel and Delete;
that is not a model to copy.

Scope is explicit on every allow — "this once" / "this tool, this session" /
"everything, this session" — and **there is no bypass-all from the phone**.

A card that omt **cannot** deliver an answer to is shown read-only, with the
reason and a route to the terminal. Read from `Deliverable`, never from
`state == Open` — an open card is not necessarily one omt can answer, and a
button that silently does nothing is worse than no button.

### 2.4 Terminal — the escape hatch

Not the front door. **Reflowed, not letterboxed** — this is a correction: the
first draft of this document said letterbox, and the evidence says otherwise.
At a 390pt phone width a monospace advance of ~0.60em gives roughly 65 columns
at 10px, 54 at 12px, 46 at 14px, 40 at 16px. Eighty columns portrait needs
~8px, which is below legibility. So the grid is resized and `SIGWINCH` is sent,
and pinch changes the font size — which re-fits columns rather than scaling a
bitmap. Every serious mobile terminal (Blink, Termius) does exactly this and
calls it "resize terminal".

Two implementation traps, both real: measuring cell width before the webfont
loads gives the wrong column count, and iOS Safari shrinks `visualViewport`
rather than the layout viewport when the keyboard opens, so a naive `fit` on
`resize` thrashes.

**xterm.js has no native touch handling at all** ([xtermjs#5377], open,
`help wanted`). Tap-to-position, double-tap-select, long-press menu, pinch and
selection handles are all unimplemented and fall back to browser mouse
emulation. Using xterm.js means writing that layer.

[xtermjs#5377]: https://github.com/xtermjs/xterm.js/issues/5377

## 3. Input

### 3.1 The key bar

A phone keyboard has no `Esc`, no `Ctrl`, no arrows, no `Tab`. Without a key
bar the terminal is read-only in practice.

```
[Esc] [Tab] [Ctrl] [↑] [↓] [←] [→] [/] [-] [|] [~]
```

`Esc` first because it is the most pressed and the most missing — its absence
is literally the open bug that makes VS Code's mobile terminal unusable
([vscode#85254]). `Ctrl` **latches**: tap it, then a letter. A modifier that
must be held cannot be used one-handed, which is how Blink does it.

The last four are the characters a software keyboard buries two layers deep and
that appear in almost every command. Termux ships `-` in its seven-key default
for exactly this reason; Prompt 3 omits punctuation entirely, and that is a gap
rather than a model.

A second row, behind a swipe, is tuned to agents rather than shells:
`Ctrl-C`, `Ctrl-D`, `Ctrl-R`, `PgUp`, `PgDn`, and the user's pinned slash
commands.

The bar is hideable. It eats viewport, and on a 46-column screen that matters.

**A multi-line composer** is separate from the key bar and just as important:
typing a paragraph-long prompt into a PTY one keystroke at a time is miserable,
so drafting happens in a real text field and is sent as one message.

[vscode#85254]: https://github.com/microsoft/vscode/issues/85254

### 3.2 Gestures

| Gesture | Does | Why |
|---|---|---|
| Long press **and drag** | Arrow keys, accelerating with distance | The best gesture in any mobile terminal (Termius). Makes history and cursor movement usable without touching the key bar |
| Double tap | `Tab` | Completion without reaching |
| Pinch | Resize the grid — font size, re-fit columns, `SIGWINCH` | Not a bitmap zoom |
| Vertical drag | Scrollback | Never mapped to arrow keys: doing that in the alternate screen is [xtermjs#1007] |
| Horizontal swipe | Move across the subagent grid | |
| Long press on text | Selection, with handles | The only unambiguous way on touch |
| Swipe from edge | Back | The platform gesture; overriding it is hostile |

Nothing destructive is on a swipe. A gesture that can be made by accident must
not be able to stop an agent. Nothing uses three fingers beyond a three-finger
tap for paste — the rest collide with iOS system gestures.

[xtermjs#1007]: https://github.com/xtermjs/xterm.js/issues/1007

## 4. Notifications

A correction to the first draft, which assumed having no cloud made push hard.
It does not. **Web Push needs no push server of your own**: the daemon
generates a VAPID keypair, and needs only *outbound* HTTPS to
`web.push.apple.com` or the equivalent. The push service never contacts omt, so
the instance can be LAN-only and unreachable from the internet, and payloads
are end-to-end encrypted to the browser's keys (RFC 8291) — Apple relays
ciphertext.

Two things it does require:

- **An HTTPS origin the phone can reach.** `tailscale cert` / `tailscale serve`
 gives a real Let's Encrypt certificate for `host.tailnet.ts.net` over DNS-01,
 with nothing publicly exposed. Self-signed is not a path: Safari refuses to
 register a service worker without a trusted certificate.
- **On iOS, the PWA must be installed to the Home Screen.** `PushManager` is
 not exposed to a Safari tab, and every iOS browser is WebKit, so no browser
 escapes this. Permission must come from a direct tap, and every push must
 show something visible — there is no silent push for web on iOS.

Fallback for people who will not install: **self-hosted ntfy with
`upstream-base-url`**, which relays a *poll request* — a topic hash and a
message id — through ntfy.sh, and the app then fetches the body from the local
instance. Message content never leaves the network. Without that setting, iOS
delivery lags twenty minutes to hours.

omt never runs a notification service on behalf of users.

**Push is the wake-up; the socket is what you use once awake.** A backgrounded
WebSocket dies within seconds — on iOS this is equally true of a native app
without an audio or VoIP background mode. Relying on the socket for delivery is
the mobile failure mode: nothing arrives when the app is closed.

Also: **suppress push while the user is looking at the terminal.** Claude Code
does this with a presence file. Being buzzed about something you are actively
watching is how notifications get turned off.

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
| Application shell, screens, key bar | ✅ `web/src/app.ts`, `main.ts` |
| Push | ✅ `web/src/push.ts`, `public/sw.js` |
| Hold-to-confirm | ✅ the rule in `touch.ts`, applied in `layOutCard` |
| Terminal rendering | ✅ `session.snapshot` → `web/src/terminal.ts`, styled runs from the one emulator |
| Touch gestures and sizing | ✅ `touch.ts`, `screen.ts` |
| Native iOS/Android | ⚠️ ahead of what this document asks for: the Swift app compiles with 16 checks, the Android app builds to an APK with 6 tests. Neither has run on a device — see `mobile/README.md` |

## 6.1 PWA or native

**PWA**, over a Tailscale-served HTTPS origin. It gets service workers, Web
Push with VAPID, notifications, and the Badging API — which is exactly right
for "3 agents waiting on you".

What iOS does not give a PWA: Background Sync, Periodic Background Sync,
Background Fetch, and any background WebSocket. None of those change the
design, because push is the wake-up mechanism regardless.

The one thing genuinely worth going native for later is **Live Activities and
the Dynamic Island** — Cursor's iOS app tracks up to eight concurrent agents
there, which maps almost exactly onto the subagent grid. That is a reason to
add a native app eventually, not a reason to start with one.

Two costs to plan for: iOS has no `beforeinstallprompt`, so installation is a
written instruction; and **deleting the home-screen icon destroys the push
subscription**, so re-pairing has to be cheap — a QR scan
it.

The EU DMA changes nothing here. `BrowserEngineKit` shipped in 2024 and as of
2026 no third-party-engine browser has actually shipped, and it would be
EU-only regardless.

## 7. Settled: a phone is a client, not the machine

Researched and closed — see [research/mobile-native.md](../research/mobile-native.md).

**iOS: no.** `fork` and `spawn` exist in the API and do not work; Apple DTS
confirms this is platform policy rather than the sandbox, so **there is no
entitlement to request**. That invalidates every agent CLI, all of which shell
out to tools. The DMA does not help — UTM was rejected under 2.5.2 for the EU
marketplaces too, and Apple denied iSH's interoperability request for JIT.

**Android: works, not shippable.** Termux is frozen at `targetSdk` 28 because
Android 10's W^X rule forbids executing from an app's writable directory, and
Play has required 29+ since 2020. The phantom process killer then caps forked
children at 32, which `npm install` exceeds, and cannot be raised from inside
an app.

**Browser: ruled out by memory.** Tabs crash at ~100 MB on an iPhone SE 3, and
the failure is uncatchable.

**Nobody ships this.** Claude Code mobile, Cursor, Codex, Jules, Replit and
other terminals are all remote. The gap is structural, not un-attempted.

Two things follow that this document does assume: the client stays a client,
and iOS Live Activities are worth a native app *later* — Cursor tracks eight
concurrent agents in the Dynamic Island, which is this document's subagent grid
in another form.
