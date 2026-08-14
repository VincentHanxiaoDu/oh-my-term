# Tasks

- [x] Hidden focused input on the terminal screen; keyboard opens on a phone
- [x] Clear it per input event; autocorrect, autocapitalize, spellcheck off
- [x] Tap the terminal to refocus
- [x] Terminal screen owns its `ScreenState`; unrelated updates cannot move it
- [x] Returning shows the last screen rather than a spinner
- [x] QR of the invite URL from `omt web`
- [x] Instance list: add, switch, forget
- [x] Verify at a real touch viewport, not a desktop window

Found while verifying, and fixed here because each one made a task above
untrue in practice:

- [x] Repaint the terminal in place. Rebuilding the screen every 150ms detached
      the input, and detaching blurs — so the keyboard closed six times a
      second and no IME composition could finish.
- [x] Size the pty when a session is opened. The resize only ran on a window
      resize event, so a phone showed an 80-column grid with half of every line
      off the right-hand edge until it was rotated.
- [x] Leave `Starting` on first output. Nothing ever set `Running`, so every
      live session read "starting" for as long as it existed.
- [x] Apply the instance theme. `applyTheme` was imported by the entry point
      and never called.
- [x] Rewrite a wildcard bind in the invite. `0.0.0.0` in a QR scans perfectly
      and opens a page that cannot load.
- [x] Carry over the token an older client stored under `omt.token`. It is the
      only copy.
- [x] Show a refused write. It was recorded and never rendered, so typing into
      a session another client was driving did nothing and explained nothing.
- [x] Claim the writer token when a session is opened, not only when one is
      created.
