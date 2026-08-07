# Research — running an agent *on* a phone

## The question

Could a phone be the machine rather than a window onto one — Claude Code or
Codex running **locally** on iOS or Android, with omt supplying the runtime?

If yes, the architecture changes materially. It is not yes.

## Verdict

| Platform | Verdict |
|---|---|
| **iOS** | **No.** Not in any shippable form, by a mechanism with no entitlement to request |
| **Android** | **Partly.** It genuinely works, and is not distributable |
| **Browser / WASM** | **No.** Ruled out by memory before anything else |

## iOS: the blocker is subprocesses, not JIT

Everyone assumes JIT is the problem. It is not the first one.

[PEP 730](https://peps.python.org/pep-0730/) states that `fork` and `spawn`
exist in iOS's API but "if they are invoked, the invoking iOS process stops,
and the new process doesn't start." Apple DTS confirms iOS apps may not spawn
child processes and that this is **platform policy, not the App Sandbox** —
which means [there is no entitlement to request](https://developer.apple.com/forums/thread/747499).

That invalidates the fundamental design of every agent CLI, all of which shell
out to tools. An agent that cannot run `git` or `rg` is not an agent.

JIT is closed too: `MAP_JIT` plus `dynamic-codesigning` is held only by
WebKit, and `com.apple.security.cs.allow-jit` is a **macOS** key with no iOS
effect — widely miscited. V8 runs only `--jitless`, which disables WebAssembly
entirely and costs about 40%.

**Guideline 2.5.2**, verbatim: an app "may not download, install, or execute
code which introduces or changes features or functionality of the app." An
agent fetching and executing tool calls is squarely inside that.

**The DMA does not help**, and this is settled by precedent rather than
argument: Apple's notarization criteria apply to alternative marketplaces and
Web Distribution too, [UTM was rejected under 2.5.2 for both the App Store and
EU marketplaces](https://daringfireball.net/linked/2024/06/18/utm-notarization),
and Apple [denied iSH's DMA interoperability request for JIT
APIs](https://ish.app/blog/ish-jit-and-eu).

How the shipping apps survive is instructive. `ios_system` runs commands as
*threads in the host process*, with no isolation — a crashing command takes the
app with it. a-Shell compiles to **wasm32-wasi**, and that is the trick worth
remembering: **the compilation target is the compliance boundary.** Compile to
wasm and user code is data rather than code. iSH is App Store-legal precisely
*because* it interprets rather than generating code.

## Android: it works, and you cannot ship it

Termux works because it is frozen at `targetSdk` 28. Android 10 enforces W^X —
an app targeting 29 or above cannot `execve` a file in its own writable data
directory, and every Termux package is written there at runtime. Play has
required 29+ since 2020, so [the Play listing
froze](https://www.xda-developers.com/termux-terminal-linux-google-play-updates-stopped/)
and F-Droid became the channel. Android 16 still sideloads it — a real runway,
and a closing one.

Two things kill it as a product even sideloaded:

- **The phantom process killer.** Android 12+ kills forked children beyond a
 default of **32**, and any using "excessive CPU". `npm install` exceeds that
 trivially, and it is [unfixable from inside an
 app](https://issuetracker.google.com/issues/205156966) — the workaround needs
 `adb`.
- **AVF, the genuinely good path, is unreachable.** Android 16's Linux Terminal
 is a real Debian VM on KVM with no W^X problem, but
 `android.system.virtualmachine` is `@SystemApi` behind a permission grantable
 only to preinstalled apps or via `adb`.

Claude Code *does* run in Termux via npm — the native installer fails on
Bionic, and OAuth is unreliable enough that API-key auth is recommended.

## Browser: memory ends it before anything else

Measured January 2026 on iOS 26.2: tabs crash at **~100 MB on an iPhone SE 3**,
and the failure is [**uncatchable**](https://bugs.webkit.org/show_bug.cgi?id=221530)
— WebKit kills the tab with no event.

The rest is confirmatory. WebContainers is not Node: it reimplements the API
surface, has no native addons, and [no raw TCP or
UDP](https://github.com/stackblitz/webcontainer-core/issues/721) — outbound is
`fetch` under CORS, and StackBlitz's proxy is paid-tier. CheerpX is a real
x86→wasm JIT but 32-bit only; `pip install numpy` fails. Pyodide has no
`subprocess`, no threading, no sockets. WASIX implements `fork`/`exec` and
sockets — and its own documentation says everything works in a browser *except
sockets and DNS*.

One correction to a common assumption: the File System Access API is no longer
desktop-only (Chrome Android 132), but WebKit opposes it, so **OPFS is the
portable option** — and Safari's ITP deletes it after seven days without
interaction. Cache, never truth.

## The reality check

**Nobody ships a locally-executing coding agent on a phone.** Anthropic's own
docs describe the mobile app as [a client for Claude Code
sessions](https://code.claude.com/docs/en/mobile) rather than a place where
code runs. Cursor's cloud agents run in Cursor's infrastructure "not on your
machine". Codex runs in OpenAI-managed containers. Jules spins a GCP VM per
task. other terminals Mobile is explicitly a companion. other terminals has no mobile client at all.

Three tiers exist — vendor cloud sandbox, remote-control-your-own-machine, and
on-device — and the third is occupied only by unofficial Android Termux hacks.
The gap is structural, not un-attempted.

## Decision

**Not pursued.** iOS is closed by a mechanism with no appeal and recent
precedent confirming it will not open. The browser is closed by physics.

What omt does instead is what every serious player converged on, and the
protocol work already done — resume, answerability, treating "the client
vanished mid-turn" as normal — is exactly that.

Two adjacent things are worth doing, and neither is a bet:

1. **Verify omt cross-compiles for `aarch64-linux-android` and runs under
 Termux**, and publish that as a supported sideload target. Low cost, and it
 is the one place "the phone is the machine" is literally true today. It
 costs the Play channel rather than any engineering direction.
2. **iOS Live Activities** for long-running agent progress. Cursor's iOS app
 tracks up to eight concurrent agents in the Dynamic Island, which maps
 almost exactly onto omt's subagent grid — and it is the one thing genuinely
 worth a native app for later.

Worth noting what the Android path would actually buy: inference still goes to
a remote API, it still needs a foreground service and an `adb` incantation to
survive backgrounding, and OAuth still fails. The value is offline framing, and
the offline part is not real, because the model is remote either way.

## One thing not verified

Apple posted "updated requirements related to interpreted code" for DPLA
§3.3.1(B) on 8 October 2025. The text is behind an account paywall and could
not be retrieved. Anyone reconsidering this should read it from their own
developer account before relying on the conclusions above.
