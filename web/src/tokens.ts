/**
 * The design system, as one stylesheet.
 *
 * Tokens rather than values at call sites: a screen composed from these cannot
 * disagree with another screen about what a heading is, and changing the scale
 * changes it everywhere rather than in the places somebody remembered.
 *
 * Emitted from TypeScript rather than written in the HTML so that it is one
 * artefact with the code that uses it — a stylesheet in a `<style>` block is a
 * second place for a class name to be, and the two drift.
 */

/** The stylesheet, ready to put in a `<style>` element. */
export const STYLES = `
:root {
  /* Type. A fourth-based scale, capped: a phone has one column and more than
     four sizes on it reads as inconsistency rather than as hierarchy. */
  --text-xs: 0.75rem;
  --text-sm: 0.875rem;
  --text-base: 1rem;
  --text-lg: 1.25rem;
  --text-xl: 1.75rem;

  /* Space. Multiples of 4, because every platform's own grid is, and mixing
     scales is what makes a layout feel slightly wrong without being nameable. */
  --space-1: 4px;
  --space-2: 8px;
  --space-3: 12px;
  --space-4: 16px;
  --space-5: 24px;
  --space-6: 32px;
  --space-8: 48px;

  --radius-sm: 8px;
  --radius-md: 12px;
  --radius-lg: 16px;
  --radius-full: 999px;

  /* The smallest a target may be. Anything less is a mis-tap, and a mis-tap in
     a terminal client can be a keystroke somebody did not mean. */
  --touch-min: 44px;

  /* The terminal's own font size, which a pinch changes. A variable rather
     than a stylesheet edit, so one property drives the grid, the measurement
     and the resize that follows. */
  --terminal-font: 14px;

  /* Colour by role, never by name. A surface is a surface whether the theme is
     light or dark, and a component that asks for "grey" gets the wrong one in
     half the themes.

     Dark is the designed one. This is a terminal: the thing it exists to show
     is a dark grid of monospace, and a light chrome around a dark grid is two
     designs fighting. Light is supported and is the compromise, not the other
     way round. */
  --bg: #0d0e12;
  --bg-elevated: #14161c;
  --bg-sunken: #08090c;
  --fg: #e6e8ee;
  --fg-muted: #8b90a0;
  --fg-subtle: #565b6b;
  --border: #22252e;
  --border-strong: #333846;

  /* One warm accent against a cold grey. Warm because everything that matters
     on this surface — an agent waiting, a command that failed — needs to read
     as *attention*, and a blue accent competes with the ANSI blue in the grid
     right underneath it. */
  --accent: #ffb454;
  --accent-fg: #0d0e12;
  --accent-dim: #4a3520;
  --needs-you: #ffb454;
  --working: #5ccfe6;
  --danger: #ff6b60;
  --ok: #7fd962;

  color-scheme: dark light;

  /* The ANSI palette, and the terminal's own colours.
   *
   * These are fallbacks: theme.get overwrites them with the instance's real
   * theme at startup. They must exist anyway — the renderer emits
   * var(--omt-ansi-N) for every indexed colour, and a variable with no
   * definition renders as *nothing*, which is a terminal with no syntax
   * colouring and no obvious cause. */
  --omt-bg: var(--bg-sunken);
  --omt-fg: var(--fg);
  --omt-cursor: var(--accent);
  --omt-selection: var(--border-strong);
  --omt-ansi-0: #16181c; --omt-ansi-1: #e05252; --omt-ansi-2: #6cc070;
  --omt-ansi-3: #d7ba7d; --omt-ansi-4: #59a5ff; --omt-ansi-5: #c586c0;
  --omt-ansi-6: #4ec9b0; --omt-ansi-7: #d0d0d0; --omt-ansi-8: #6a6a6a;
  --omt-ansi-9: #ff7b72; --omt-ansi-10: #8ade94; --omt-ansi-11: #ffd479;
  --omt-ansi-12: #8ab4ff; --omt-ansi-13: #dda0dd; --omt-ansi-14: #7fdbca;
  --omt-ansi-15: #ffffff;
}

@media (prefers-color-scheme: light) {
  :root {
    --bg: #ffffff;
    --bg-elevated: #ffffff;
    --bg-sunken: #f1f3f4;
    --fg: #1f1f1f;
    --fg-muted: #5f6368;
    --fg-subtle: #80868b;
    --border: #e0e2e5;
    --border-strong: #c7c9cc;
    /* The same warm accent, darkened until it holds contrast on white. A light
       theme that switches to blue would be a different product wearing the same
       name. */
    --accent: #a35a00;
    --accent-fg: #ffffff;
    --accent-dim: #ffeacc;
    --needs-you: #a35a00;
    --working: #00697d;
    --danger: #c5221f;
    --ok: #1e8e3e;
    --shadow-1: 0 1px 2px rgba(60, 64, 67, 0.2);
    --shadow-2: 0 4px 16px rgba(60, 64, 67, 0.24);
  }
}

@media (prefers-reduced-motion: reduce) {
  :root { --duration: 0ms; }
}

* { box-sizing: border-box; }

html, body {
  margin: 0;
  height: 100%;
  background: var(--bg);
  color: var(--fg);
  font: var(--text-base)/1.5 -apple-system, BlinkMacSystemFont, "Segoe UI",
        system-ui, sans-serif;
  /* Tabular figures everywhere in the chrome. A count that changes from 9 to 10
     must not move the text beside it, and on a screen that updates every few
     seconds that jitter is the difference between calm and busy. */
  font-variant-numeric: tabular-nums;
  -webkit-text-size-adjust: 100%;
  -webkit-font-smoothing: antialiased;
  overscroll-behavior-y: none;
}

/* The shell: a fixed bar, a scrolling body, and a key bar that stays put. The
   body scrolls rather than the page, so the bar cannot slide away and leave
   somebody on a screen with no way back. */
#app {
  display: flex;
  flex-direction: column;
  height: 100dvh;
  padding: env(safe-area-inset-top) env(safe-area-inset-right)
           env(safe-area-inset-bottom) env(safe-area-inset-left);
}

.bar {
  display: flex;
  align-items: center;
  /* Never wraps. It wrapped, so the switcher could be a full-width row inside
     it — and then a long instance name pushed the action button onto a third
     line, where nobody taps because nobody expects it there. The switcher is
     its own row now, outside the bar, and this stays one line at any width and
     any font scale. */
  flex-wrap: nowrap;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border-bottom: 1px solid var(--border);
  background: var(--bg);
  flex: 0 0 auto;
  min-height: var(--touch-min);
}

.bar h1 {
  margin: 0;
  /* Allowed to be narrower than its text, or the title refuses to shrink and
     pushes everything to its right off the bar. */
  min-width: 0;
  font-size: var(--text-lg);
  font-weight: 600;
  letter-spacing: -0.01em;
  flex: 1 1 auto;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* The title, when it is also the way to switch instances. Styled as the title
   and not as a button: it is the name of the thing, and a chevron is enough to
   say it can be tapped. */
.title-button {
  display: flex;
  align-items: center;
  gap: var(--space-1);
  /* The same shrink-and-ellipsis the heading has. An inline-flex button does
     not inherit it, so a tailnet name — which is what an instance is called by
     default — grew the bar until the controls fell off it. */
  min-width: 0;
  max-width: 100%;
  overflow: hidden;
  white-space: nowrap;
  min-height: var(--touch-min);
  margin: 0;
  padding: 0;
  border: 0;
  background: none;
  color: inherit;
  font: inherit;
  cursor: pointer;
}
.title-button .name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.title-button .chevron-down {
  flex: 0 0 auto;
  font-size: var(--text-sm);
  color: var(--fg-muted);
}

/* Forgetting an instance destroys the only copy of a credential, so it does not
   look like the other controls. */
.danger {
  display: block;
  width: 100%;
  min-height: var(--touch-min);
  margin-top: var(--space-4);
  padding: 0 var(--space-3);
  border: 1px solid var(--danger);
  border-radius: var(--radius-md);
  background: none;
  color: var(--danger);
  font: inherit;
  font-size: var(--text-sm);
  cursor: pointer;
}

/* The scanner covers everything: a camera preview in a corner of a list is a
   camera nobody realises is on. */
.scanner {
  position: fixed;
  inset: 0;
  z-index: 10;
  display: flex;
  flex-direction: column;
  background: #000;
}
.scanner video {
  flex: 1 1 auto;
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.scanner-cancel {
  flex: 0 0 auto;
  min-height: var(--touch-min);
  margin: var(--space-4);
  margin-bottom: calc(var(--space-4) + env(safe-area-inset-bottom));
  border: 1px solid rgba(255, 255, 255, 0.4);
  border-radius: var(--radius-md);
  background: rgba(0, 0, 0, 0.5);
  color: #fff;
  font: inherit;
  cursor: pointer;
}

/* Why a keystroke did nothing. Above the grid rather than over it: a terminal
   somebody cannot type into is still a terminal they need to read. */
.notice {
  margin-bottom: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border-radius: var(--radius-sm);
  background: var(--bg-elevated);
  border: 1px solid var(--danger);
  color: var(--fg);
  font-size: var(--text-sm);
}
.notice[hidden] { display: none; }

.bar .subtitle {
  display: block;
  font-size: var(--text-xs);
  font-weight: 400;
  color: var(--fg-muted);
  letter-spacing: 0;
}

.body {
  flex: 1 1 auto;
  min-width: 0;
  overflow-y: auto;
  -webkit-overflow-scrolling: touch;
  padding: var(--space-3);
}

/* Every tappable thing. One rule, so a button in a list and a button in a bar
   are the same size and the same shape. */
.tap {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  width: 100%;
  min-height: var(--touch-min);
  padding: var(--space-3);
  margin: 0 0 var(--space-2);
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--bg-elevated);
  color: var(--fg);
  font: inherit;
  text-align: left;
  cursor: pointer;
  transition: background var(--duration) var(--easing),
              border-color var(--duration) var(--easing);
}

.tap:active { background: var(--bg-sunken); }
.tap:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
.tap[disabled] { opacity: 0.5; cursor: default; }

.tap .label { flex: 1 1 auto; min-width: 0; }
.tap .title {
  display: block;
  font-size: var(--text-base);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.tap .meta {
  display: block;
  font-size: var(--text-xs);
  color: var(--fg-muted);
}

/* A dot, plus a word. Colour alone is invisible to a tenth of the people who
   will use this, and a screen reader gets nothing from it at all. */
.dot {
  flex: 0 0 auto;
  width: 10px;
  height: 10px;
  border-radius: var(--radius-full);
  background: var(--fg-subtle);
}
.dot.needs-you { background: var(--needs-you); box-shadow: 0 0 0 4px rgba(255, 180, 84, 0.16); }
.dot.working { background: var(--working); }
.dot.ok { background: var(--ok); }

.chevron { flex: 0 0 auto; color: var(--fg-subtle); font-size: var(--text-sm); }

.icon-button {
  display: flex;
  align-items: center;
  justify-content: center;
  width: var(--touch-min);
  height: var(--touch-min);
  margin-left: calc(var(--space-2) * -1);
  padding: 0;
  border: 0;
  border-radius: var(--radius-full);
  background: transparent;
  color: var(--fg);
  font-size: var(--text-lg);
  cursor: pointer;
}
.icon-button:active { background: var(--bg-sunken); }
.icon-button:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }

.primary {
  justify-content: center;
  border-color: transparent;
  background: var(--accent);
  color: var(--accent-fg);
  font-weight: 600;
}
.primary:active { background: var(--accent); filter: brightness(0.92); }

/* Hold-to-confirm reads as heavier *before* it is pressed. The difference is
   otherwise invisible until somebody has already failed to press it. */
.hold {
  border-width: 2px;
  border-color: var(--needs-you);
}

/* One state at a time. Each of these replaces the content rather than sitting
   above or below it — the bug that started this rebuild was three of them on
   screen together. */
.state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--space-3);
  min-height: 60%;
  padding: var(--space-6) var(--space-4);
  text-align: center;
  color: var(--fg-muted);
}
.state .headline { font-size: var(--text-lg); color: var(--fg); font-weight: 600; }
.state .detail { font-size: var(--text-sm); max-width: 34ch; }
.state.error .headline { color: var(--danger); }

.spinner {
  width: 24px;
  height: 24px;
  border: 2px solid var(--border-strong);
  border-top-color: var(--accent);
  border-radius: var(--radius-full);
  animation: spin 700ms linear infinite;
}
@keyframes spin { to { transform: rotate(360deg); } }
@media (prefers-reduced-motion: reduce) {
  .spinner { animation-duration: 2s; }
}

.prompt {
  margin: 0 0 var(--space-4);
  padding: var(--space-3);
  border-left: 3px solid var(--needs-you);
  border-radius: 0 var(--radius-sm) var(--radius-sm) 0;
  background: var(--bg-elevated);
  font-size: var(--text-base);
  overflow-wrap: anywhere;
}

.section {
  margin: var(--space-5) 0 var(--space-2);
  font-size: var(--text-xs);
  font-weight: 600;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--fg-subtle);
}
.section:first-child { margin-top: 0; }

/* The terminal keeps the user's own theme and their program's own colours.
   Restyling it would be omt deciding what somebody's shell looks like. */
.terminal {
  font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace;
  /* Pinch-driven, so one property decides the glyph size, how many columns fit,
     and the resize the instance is told about. */
  font-size: var(--terminal-font);
  line-height: 1.25;
  white-space: pre;
  overflow-x: auto;
  padding: var(--space-2);
  background: var(--omt-bg, var(--bg-sunken));
  color: var(--omt-fg, var(--fg));
  border-radius: var(--radius-sm);
  min-height: 100%;
}
.terminal .line { position: relative; min-height: 1.25em; }

/* The input the device keyboard types into.
 *
 * Off-screen rather than display:none or hidden — a control that is not
 * rendered cannot take focus, and without focus no mobile browser opens a
 * keyboard. One pixel, transparent, positioned inside the scrolling body so
 * the browser does not scroll the page to reveal it. */
.hidden-input {
  position: absolute;
  width: 1px;
  height: 1px;
  padding: 0;
  border: 0;
  opacity: 0;
  /* Not zero: iOS zooms the page in when a focused control has text under
     16px, and the zoom does not come back out. */
  font-size: 16px;
  resize: none;
  caret-color: transparent;
}

/* The terminal body is the scroll container, so the key bar and app bar stay
   put while the output moves under them. */
.terminal-body {
  position: relative;
  padding: var(--space-2);
}
.terminal .cursor {
  position: absolute;
  top: 0;
  width: 1ch;
  height: 1.25em;
  background: var(--omt-cursor, var(--fg));
  mix-blend-mode: difference;
}

/* Everything below the grid: the keys a phone does not have, and the field a
   command is typed into. Fixed to the bottom of the shell rather than the
   page, so the on-screen keyboard pushes it rather than covering it. */
.terminal-foot {
  flex: 0 0 auto;
  border-top: 1px solid var(--border);
  background: var(--bg);
}
.keybar-wrap {
  display: flex;
  align-items: flex-start;
  gap: var(--space-1);
  padding: var(--space-2);
  padding-bottom: 0;
}
/* One row that scrolls. Wrapped, eleven keys become three rows on a phone with
   a large font scale — half the screen given to punctuation. The order is what
   makes that safe: the keys nobody can live without come first, and the ones
   that scroll off are the ones a keyboard buries rather than omits. */
.keybar {
  display: flex;
  flex-wrap: nowrap;
  gap: var(--space-1);
  flex: 1 1 auto;
  min-width: 0;
  overflow-x: auto;
  scrollbar-width: none;
  /* A fade at the right edge, so it is visible that there is more. A row that
     simply ends looks like a row that has ended. */
  mask-image: linear-gradient(to right, #000 calc(100% - 24px), transparent);
}
.keybar::-webkit-scrollbar { display: none; }
.keybar-controls {
  display: flex;
  gap: var(--space-1);
  flex: 0 0 auto;
}
.keybar-control {
  min-width: 40px;
  height: var(--touch-min);
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  background: none;
  color: var(--fg-muted);
  font: var(--text-sm) ui-monospace, monospace;
  cursor: pointer;
}
.keybar-handle {
  width: 100%;
  min-height: 36px;
  border: 0;
  background: none;
  color: var(--fg-subtle);
  font-size: var(--text-xs);
  cursor: pointer;
}
.keybar > button {
  flex: 0 0 auto;
  min-width: var(--touch-min);
  height: var(--touch-min);
  padding: 0 var(--space-3);
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  background: var(--bg-elevated);
  color: var(--fg);
  font: var(--text-sm) ui-monospace, monospace;
  cursor: pointer;
}
.keybar > button[aria-pressed="true"] {
  background: var(--accent);
  color: var(--accent-fg);
  border-color: transparent;
}

.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(var(--touch-min), 1fr));
  gap: var(--space-2);
}
.cell {
  aspect-ratio: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  background: var(--bg-elevated);
  color: var(--fg-muted);
  font-size: var(--text-lg);
  cursor: pointer;
}
.cell.blocked { border-color: var(--needs-you); color: var(--needs-you); }
.cell.working { border-color: var(--working); color: var(--working); }

/* Present for a screen reader, absent for everyone else.

   Not display:none and not visibility:hidden -- both remove the element
   from the accessibility tree, so a live region styled that way announces
   nothing at all -- the failure mode that looks like it works. */
.sr-only {
  position: absolute;
  width: 1px;
  height: 1px;
  margin: -1px;
  padding: 0;
  overflow: hidden;
  clip-path: inset(50%);
  white-space: nowrap;
  border: 0;
}

.banner {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  margin-bottom: var(--space-3);
  padding: var(--space-3);
  border-radius: var(--radius-sm);
  background: color-mix(in srgb, var(--danger) 12%, transparent);
  color: var(--danger);
  font-size: var(--text-sm);
}

/* --- The structured surfaces ------------------------------------------- */

/* The surface switcher. Its own row under the bar rather than inside it: as a
   full-width flex child it forced the bar to wrap, and a wrapped bar puts the
   action button somewhere nobody taps. */
.surfaces {
  display: flex;
  gap: var(--space-1);
  flex: 0 0 auto;
  padding: var(--space-2) var(--space-3);
  border-bottom: 1px solid var(--border);
  background: var(--bg);
  overflow-x: auto;
  scrollbar-width: none;
}
.surfaces::-webkit-scrollbar { display: none; }
.surface {
  flex: 0 0 auto;
  min-height: 36px;
  padding: 0 var(--space-3);
  border: 1px solid var(--border);
  border-radius: var(--radius-full);
  background: none;
  color: var(--fg-muted);
  font: inherit;
  font-size: var(--text-sm);
  cursor: pointer;
}
.surface[aria-selected="true"] {
  background: var(--accent-dim);
  border-color: var(--accent);
  color: var(--fg);
}
.surface:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

/* A command block. Collapsed by default, its body its own scroll container so
   a long line never makes the page pan sideways. */
.block {
  /* A flex or grid child refuses to shrink below its content by default, so a
     PRE full of a 200-column log line makes its *ancestors* wide and the whole
     page pans sideways — the one thing the block model exists to prevent. The
     scroll container only works if everything above it is allowed to be
     narrower than what is inside it. */
  min-width: 0;
  max-width: 100%;
  margin-bottom: var(--space-2);
  border: 1px solid var(--border);
  border-left: 3px solid var(--border-strong);
  border-radius: var(--radius-md);
  background: var(--bg-elevated);
  overflow: hidden;
}
.block.failed { border-left-color: var(--danger); }
.block > summary {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  min-height: var(--touch-min);
  padding: var(--space-2) var(--space-3);
  cursor: pointer;
  list-style: none;
}
.block > summary::-webkit-details-marker { display: none; }
.block-glyph {
  color: var(--fg-subtle);
  font: var(--text-sm) ui-monospace, monospace;
}
.block-command {
  flex: 1 1 auto;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font: var(--text-sm) ui-monospace, monospace;
}
.chip {
  flex: 0 0 auto;
  padding: 2px var(--space-2);
  border-radius: var(--radius-full);
  font-size: var(--text-xs);
  font-variant-numeric: tabular-nums;
}
.chip.ok { background: color-mix(in srgb, var(--ok) 18%, transparent); color: var(--ok); }
.chip.needs-you { background: color-mix(in srgb, var(--danger) 18%, transparent); color: var(--danger); }
.chip.working { background: color-mix(in srgb, var(--working) 18%, transparent); color: var(--working); }
.chip.muted { background: var(--bg-sunken); color: var(--fg-muted); }
.block-output {
  min-width: 0;
  max-width: 100%;
  margin: 0;
  padding: var(--space-3);
  border-top: 1px solid var(--border);
  background: var(--bg-sunken);
  color: var(--fg);
  font: var(--text-sm) ui-monospace, monospace;
  overflow-x: auto;
  max-height: 50vh;
  overflow-y: auto;
}
.block-action {
  width: 100%;
  min-height: var(--touch-min);
  border: 0;
  border-top: 1px solid var(--border);
  background: none;
  color: var(--accent);
  font: inherit;
  cursor: pointer;
}

/* The transcript. Prose width, not terminal width: this is reading material. */
.transcript { padding-bottom: var(--space-6); }
.entry { margin-bottom: var(--space-3); }
.entry-text { margin: 0; white-space: pre-wrap; overflow-wrap: anywhere; }
.entry-tag {
  display: inline-block;
  margin-right: var(--space-2);
  padding: 1px var(--space-2);
  border-radius: var(--radius-sm);
  background: var(--bg-elevated);
  color: var(--fg-muted);
  font-size: var(--text-xs);
  text-transform: lowercase;
}
.entry-user {
  padding: var(--space-3);
  border-radius: var(--radius-md);
  background: var(--bg-elevated);
  border-left: 3px solid var(--accent);
}
.entry-assistant { padding: 0 var(--space-1); }
.entry-reasoning > summary {
  color: var(--fg-subtle);
  font-size: var(--text-sm);
  cursor: pointer;
}
.entry-reasoning .entry-text { color: var(--fg-muted); font-size: var(--text-sm); }
.entry-tool {
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  background: var(--bg-elevated);
}
.entry-tool.failed { border-color: var(--danger); }
.entry-tool > summary {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  min-height: 40px;
  padding: 0 var(--space-3);
  cursor: pointer;
  list-style: none;
}
.entry-tool > summary::-webkit-details-marker { display: none; }
.tool-name { color: var(--accent); font: var(--text-sm) ui-monospace, monospace; }
.tool-arg {
  flex: 1 1 auto;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--fg-muted);
  font: var(--text-sm) ui-monospace, monospace;
}
.tool-time { color: var(--fg-subtle); font-size: var(--text-xs); }
.entry-output {
  min-width: 0;
  max-width: 100%;
  margin: 0;
  padding: var(--space-3);
  border-top: 1px solid var(--border);
  background: var(--bg-sunken);
  font: var(--text-sm) ui-monospace, monospace;
  overflow-x: auto;
  max-height: 40vh;
  overflow-y: auto;
}
.plan { margin: 0; padding-left: var(--space-5); }
.plan .step { margin-bottom: var(--space-1); }
.plan .step.done { color: var(--fg-subtle); text-decoration: line-through; }

/* The composer. */
.composer {
  position: relative;
  display: flex;
  align-items: flex-end;
  gap: var(--space-2);
  flex: 0 0 auto;
  padding: var(--space-2);
  padding-bottom: calc(var(--space-2) + env(safe-area-inset-bottom));
  background: var(--bg);
  flex-wrap: wrap;
}
.composer-note {
  flex: 1 1 100%;
  padding: var(--space-2) var(--space-1);
  color: var(--fg-muted);
  font-size: var(--text-xs);
}
.composer-field {
  flex: 1 1 auto;
  min-height: var(--touch-min);
  max-height: 160px;
  padding: var(--space-2) var(--space-3);
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  background: var(--bg-elevated);
  color: var(--fg);
  /* Not smaller: iOS zooms into a focused control with text under 16px and
     does not zoom back out.
     Written long-hand: the font shorthand with "inherit" as the family is not
     valid CSS, so the browser dropped the whole declaration and the field
     rendered at 11px. */
  font-family: inherit;
  font-size: 16px;
  line-height: 1.4;
  resize: none;
}
.composer-send {
  min-height: var(--touch-min);
  padding: 0 var(--space-4);
  border: 0;
  border-radius: var(--radius-full);
  background: var(--accent);
  color: var(--accent-fg);
  font: inherit;
  font-weight: 600;
  cursor: pointer;
}
.queue { flex: 0 0 100%; margin-bottom: var(--space-2); }
.queue-item {
  display: flex;
  gap: var(--space-2);
  padding: var(--space-1) 0;
  font-size: var(--text-sm);
}
.commands {
  position: absolute;
  left: var(--space-2);
  right: var(--space-2);
  bottom: 100%;
  max-height: 40vh;
  overflow-y: auto;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--bg-elevated);
  box-shadow: var(--shadow-2);
}
.command {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 2px;
  width: 100%;
  min-height: var(--touch-min);
  padding: var(--space-2) var(--space-3);
  border: 0;
  border-bottom: 1px solid var(--border);
  background: none;
  color: var(--fg);
  font: inherit;
  text-align: left;
  cursor: pointer;
}
.command-name { color: var(--accent); font: var(--text-sm) ui-monospace, monospace; }
.command-doc { color: var(--fg-muted); font-size: var(--text-xs); }

/* Search. */
.search-field {
  width: 100%;
  min-height: var(--touch-min);
  margin-bottom: var(--space-3);
  padding: 0 var(--space-3);
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--bg-elevated);
  color: var(--fg);
  font-family: inherit;
  font-size: 16px;
  line-height: 1.4;
}
.search-hit .meta {
  flex: 0 0 auto;
  color: var(--fg-subtle);
  font: var(--text-xs) ui-monospace, monospace;
}
.search-hit .label {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font: var(--text-sm) ui-monospace, monospace;
}

/* The mark. */
.mark { display: inline-flex; color: var(--accent); }
.mark-agent { color: var(--working); }
.bar .mark {
  flex: 0 0 auto;
  align-self: center;
  margin-right: var(--space-1);
}
.mark svg { display: block; }


/* --- Interaction cards -------------------------------------------------- */

/* A permission card's prompt is a command. Shown as prose it gets skimmed;
   shown as code it gets read. */
.prompt-command {
  margin: 0 0 var(--space-4);
  padding: var(--space-3);
  border: 1px solid var(--border);
  border-left: 3px solid var(--danger);
  border-radius: var(--radius-sm);
  background: var(--bg-sunken);
  color: var(--fg);
  font: var(--text-sm) ui-monospace, monospace;
  overflow-x: auto;
  min-width: 0;
  max-width: 100%;
}

/* Hold-to-approve. The fill is the feedback: without it a hold is a button that
   ignores you for most of a second. */
.tap.hold {
  position: relative;
  overflow: hidden;
  border-color: var(--danger);
}
.tap.hold::after {
  content: "";
  position: absolute;
  inset: 0;
  width: 0;
  background: color-mix(in srgb, var(--danger) 24%, transparent);
  pointer-events: none;
}
.tap.hold.holding::after {
  width: 100%;
  transition: width var(--hold-ms, 800ms) linear;
}
.tap.hold .label::after {
  content: " — hold";
  color: var(--fg-muted);
  font-size: var(--text-xs);
}


/* Connection state. Under the bar on every screen, because a connection that
   has gone is a fact about all of them — and a screen that simply stops
   changing is indistinguishable from an instance where nothing is happening. */
.connection {
  flex: 0 0 auto;
  padding: var(--space-2) var(--space-3);
  background: var(--accent-dim);
  color: var(--fg);
  font-size: var(--text-sm);
  text-align: center;
}
.connection.refused { background: color-mix(in srgb, var(--danger) 22%, transparent); }

.composer-icon {
  flex: 0 0 auto;
  width: var(--touch-min);
  height: var(--touch-min);
  border: 1px solid var(--border);
  border-radius: var(--radius-full);
  background: var(--bg-elevated);
  color: var(--fg-muted);
  font-size: var(--text-base);
  cursor: pointer;
}
.composer-icon:disabled { opacity: 0.4; cursor: not-allowed; }


/* The two ways to add an instance, both visible. */
.add-instance { margin-bottom: var(--space-4); }
.tap.primary {
  border-color: var(--accent);
  color: var(--accent);
}

/* Devices this instance has let in. */
.devices { margin-top: var(--space-5); }
.device {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  min-height: var(--touch-min);
  padding: var(--space-2) 0;
  border-bottom: 1px solid var(--border);
}
.device .label { flex: 1 1 auto; min-width: 0; }
.device.revoked { opacity: 0.5; }
.device-revoke {
  flex: 0 0 auto;
  min-height: 36px;
  padding: 0 var(--space-3);
  border: 1px solid var(--danger);
  border-radius: var(--radius-full);
  background: none;
  color: var(--danger);
  font: inherit;
  font-size: var(--text-sm);
  cursor: pointer;
}

`
