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

  /* Colour by role, never by name. A surface is a surface whether the theme is
     light or dark, and a component that asks for "grey" gets the wrong one in
     half the themes. */
  --bg: #0b0c0e;
  --bg-elevated: #16181c;
  --bg-sunken: #08090a;
  --fg: #e8eaed;
  --fg-muted: #9aa0a6;
  --fg-subtle: #5f6368;
  --border: #2a2d31;
  --border-strong: #3c4043;

  --accent: #7cb0ff;
  --accent-fg: #0b0c0e;
  --needs-you: #ffb454;
  --working: #7cb0ff;
  --danger: #ff6b60;
  --ok: #6cc070;

  /* Elevation as one shadow per level, because a component choosing its own
     blur radius is how two cards at the same depth end up looking different. */
  --shadow-1: 0 1px 2px rgba(0, 0, 0, 0.4);
  --shadow-2: 0 4px 16px rgba(0, 0, 0, 0.5);

  /* Motion. Short, because this is a tool: an animation somebody waits for is
     an animation that makes the tool feel slow. */
  --duration: 140ms;
  --easing: cubic-bezier(0.2, 0, 0, 1);

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
    --accent: #1a73e8;
    --accent-fg: #ffffff;
    --needs-you: #b06000;
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
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border-bottom: 1px solid var(--border);
  background: var(--bg);
  flex: 0 0 auto;
  min-height: var(--touch-min);
}

.bar h1 {
  margin: 0;
  font-size: var(--text-lg);
  font-weight: 600;
  letter-spacing: -0.01em;
  flex: 1 1 auto;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.bar .subtitle {
  display: block;
  font-size: var(--text-xs);
  font-weight: 400;
  color: var(--fg-muted);
  letter-spacing: 0;
}

.body {
  flex: 1 1 auto;
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
  font-size: var(--text-sm);
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
.terminal .cursor {
  position: absolute;
  top: 0;
  width: 1ch;
  height: 1.25em;
  background: var(--omt-cursor, var(--fg));
  mix-blend-mode: difference;
}

/* The key bar. Fixed to the bottom of the shell rather than the page, so the
   on-screen keyboard pushes it rather than covering it. */
.keybar {
  display: flex;
  gap: var(--space-1);
  padding: var(--space-2);
  border-top: 1px solid var(--border);
  background: var(--bg);
  flex: 0 0 auto;
  overflow-x: auto;
}
.keybar button {
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
.keybar button[aria-pressed="true"] {
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
`
