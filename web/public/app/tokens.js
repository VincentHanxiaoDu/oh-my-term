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
  /* ---------------------------------------------------------------------
     Two voices.

     omt speaks in monospace and the agent speaks in prose, and you can tell
     at a glance which one is talking. That is not decoration: this client's
     whole job is to put a machine's output and an agent's question on one
     small screen, and a reader who has to work out whose words these are has
     already lost the thread.

     Everything omt says about itself — a session name, a state, a count, a
     key — is set in the same face as the grid it describes. Everything an
     agent says is set in prose, because it is prose.
     --------------------------------------------------------------------- */
  --face-machine: ui-monospace, SFMono-Regular, 'SF Mono', Menlo, Consolas,
    'Liberation Mono', monospace;
  --face-human: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto,
    system-ui, sans-serif;

  /* One character cell. The chrome measures in the same unit as the terminal
     underneath it, so a row of chrome and a row of output line up rather than
     nearly lining up — which is the difference between a layout that reads as
     one surface and one that reads as a wrapper around a surface. */
  --cell: 1ch;
  --gutter: 3ch;

  /* Type. Four sizes, because a phone has one column and a fifth size reads
     as inconsistency rather than as hierarchy. */
  --text-xs: 0.6875rem;
  --text-sm: 0.8125rem;
  --text-base: 0.9375rem;
  --text-lg: 1.0625rem;
  --text-xl: 1.5rem;

  --space-1: 4px;
  --space-2: 8px;
  --space-3: 12px;
  --space-4: 16px;
  --space-5: 24px;
  --space-6: 32px;
  --space-8: 48px;

  /* Nearly square. A terminal has no rounded corners and neither does a list
     of what is running in one; the radius that is left is the smallest that
     keeps a tap target from looking like a table cell. */
  --radius-sm: 2px;
  --radius-md: 3px;
  --radius-lg: 4px;
  --radius-full: 999px;

  --touch-min: 44px;
  --terminal-font: 14px;

  /* ---------------------------------------------------------------------
     Colour.

     The chrome wears the user's own terminal theme. theme.get already
     ships it and it was used only inside the grid, so the client looked like
     a generic app that happened to contain a terminal. These fall back to
     omt's own ink when a theme has not loaded or cannot carry chrome
     contrast — see applyTheme, which checks rather than hopes.

     Ink rather than black: a terminal is phosphor behind glass, and glass is
     blue. Pure black is a switched-off screen.
     --------------------------------------------------------------------- */
  --ink: #0b0e14;
  --ink-lift: #11151f;
  --ink-sunken: #070910;
  --rule: #1c2130;
  --rule-strong: #2b3244;

  --say: #ccd1de;
  --murmur: #7b8397;
  --whisper: #4d5568;

  /* Three states, three colours, and each means exactly one thing anywhere it
     appears. --halt is never used for emphasis, a heading, or a brand mark —
     if it is on screen, something is waiting for a person. That restraint is
     what lets it be recognised out of the corner of an eye, which is the only
     way this client is ever actually read. */
  --halt: #f2a65a;
  --halt-ink: #0b0e14;
  --halt-wash: #2a1f12;
  --run: #57b6c9;
  --gone: #e05a53;
  --done: #78c07a;

  --accent: var(--halt);
  --accent-fg: var(--halt-ink);
  --accent-dim: var(--halt-wash);
  --needs-you: var(--halt);
  --working: var(--run);
  --danger: var(--gone);
  --ok: var(--done);

  --bg: var(--ink);
  --bg-elevated: var(--ink-lift);
  --bg-sunken: var(--ink-sunken);
  --fg: var(--say);
  --fg-muted: var(--murmur);
  --fg-subtle: var(--whisper);
  --border: var(--rule);
  --border-strong: var(--rule-strong);

  color-scheme: dark light;

  /* The ANSI palette. Fallbacks: theme.get overwrites these with the
     instance's real theme at startup. They must exist anyway — the renderer
     emits var(--omt-ansi-N) for every indexed colour, and a variable with no
     definition renders as *nothing*, which is a terminal with no syntax
     colouring and no obvious cause. */
  --omt-bg: var(--ink-sunken);
  --omt-fg: var(--say);
  --omt-cursor: var(--halt);
  --omt-selection: var(--rule-strong);
  --omt-ansi-0: #12161f; --omt-ansi-1: #e05a53; --omt-ansi-2: #78c07a;
  --omt-ansi-3: #d8b878; --omt-ansi-4: #6a9ce0; --omt-ansi-5: #b884c4;
  --omt-ansi-6: #57b6c9; --omt-ansi-7: #ccd1de; --omt-ansi-8: #626b80;
  --omt-ansi-9: #f2736b; --omt-ansi-10: #92d494; --omt-ansi-11: #f2cf8c;
  --omt-ansi-12: #8ab4ff; --omt-ansi-13: #d3a0dd; --omt-ansi-14: #7fd6e4;
  --omt-ansi-15: #ffffff;

  /* -------------------------------------------------------------------
     Material.

     Flat panels on a flat ground read as a wireframe. What makes a surface
     look like an object is the hairline along its top edge: light falls from
     above, so the top edge catches it and nothing else does. One rule, applied
     once, and the whole thing stops looking like a diagram of an interface.
     ------------------------------------------------------------------- */
  --lit: rgba(255, 255, 255, 0.055);
  --cast: 0 1px 2px rgba(0, 0, 0, 0.4), 0 8px 24px -12px rgba(0, 0, 0, 0.7);
  --cast-lift: 0 2px 4px rgba(0, 0, 0, 0.4), 0 16px 40px -16px rgba(0, 0, 0, 0.8);
  --glow-halt: 0 0 0 1px rgba(242, 166, 90, 0.28), 0 0 24px -6px rgba(242, 166, 90, 0.35);

  /* Motion. One duration and one curve, so nothing in the interface moves at a
     speed nothing else moves at. The curve overshoots slightly — a control that
     settles rather than stops is the difference between responsive and
     mechanical, and it costs nothing. */
  --duration: 140ms;
  --duration-slow: 320ms;
  --ease: cubic-bezier(0.32, 0.72, 0, 1);

  /* How wide a column of text or rows is allowed to get.
     A roster stretched across 1440px puts the label at one edge and the
     chevron at the other, and the eye cannot join them. */
  --measure: 68ch;

  /* The one pool of light on the ground, behind the header. Cool on ink and
     warm on paper: the same gesture reads as a lit screen in the dark and as
     daylight on a page. */
  --pool: rgba(122, 162, 247, 0.07);
}

@media (prefers-color-scheme: light) {
  :root {
    /* Paper, not white. The same palette read in daylight: warm enough that
       the amber still reads as attention rather than as a highlighter, and
       never pure #fff, which on an OLED phone in a dark room is a torch. */
    --ink: #f7f6f3;
    --ink-lift: #ffffff;
    --ink-sunken: #ebe9e4;
    --rule: #ddd9d1;
    --rule-strong: #bdb8ad;

    --say: #1a1d24;
    --murmur: #5c6270;
    --whisper: #8c93a1;

    /* Darkened until each holds 4.5:1 on paper. The hues do not change: a
       light theme that switched to a different accent would be a different
       product wearing the same name. */
    --halt: #9a5500;
    --halt-ink: #ffffff;
    --halt-wash: #fbeeda;
    --run: #0b6b7d;
    --gone: #b3271f;
    --done: #2c7a35;

    --shadow-1: 0 1px 2px rgba(26, 29, 36, 0.12);
    --shadow-2: 0 4px 16px rgba(26, 29, 36, 0.16);

    /* Light falls from above here too, but on paper the top edge is where a
       shadow stops rather than where a highlight begins — a white hairline on
       white is nothing, and the dark theme's values would draw a black line
       across the top of every panel. */
    --lit: rgba(26, 29, 36, 0.06);
    --cast: 0 1px 2px rgba(26, 29, 36, 0.06), 0 8px 24px -12px rgba(26, 29, 36, 0.12);
    --cast-lift: 0 2px 4px rgba(26, 29, 36, 0.08), 0 16px 40px -16px rgba(26, 29, 36, 0.16);
    --glow-halt: 0 0 0 1px rgba(154, 85, 0, 0.24), 0 0 24px -6px rgba(154, 85, 0, 0.2);
    --pool: rgba(154, 85, 0, 0.05);
  }
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
  /* A ground with a light in it. One soft pool behind the header, where the
     only thing that matters is, and nothing anywhere else — a page-wide
     gradient is decoration and this is a direction to look. */
  background:
    radial-gradient(120% 80% at 50% -20%, var(--pool), transparent 60%),
    var(--bg);
}

/* The client was a phone laid out once and stretched. At 1440px that put a
   session name at one edge of the screen and its chevron at the other, with
   1300px of nothing between them — the eye cannot join those, so the list
   stopped being a list. Everything is measured now, and centred. */
.body > * {
  max-width: var(--measure);
  margin-inline: auto;
}

.bar {
  display: flex;
  align-items: center;
  /* Never wraps. It wrapped, so the switcher could be a full-width row inside
     it — and then a long instance name pushed the action button onto a third
     line, where nobody taps because nobody expects it there. */
  flex-wrap: nowrap;
  gap: var(--space-2);
  padding: var(--space-3);
  /* No border. The header is above the panel and lit from the same direction,
     so what separates them is the light on the panel's top edge — a rule as
     well would be saying it twice. */
  border-bottom: 0;
  background: transparent;
}


/* Which machine, which is context rather than news. Smaller than the answer
   below it, because a header whose largest words are the ones that never
   change teaches somebody to stop reading headers. */
.bar h1 {
  margin: 0;
  min-width: 0;
  font-family: var(--face-machine);
  font-size: var(--text-sm);
  font-weight: 400;
  letter-spacing: 0.02em;
  text-transform: lowercase;
  color: var(--fg-muted);
  flex: 1 1 auto;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}


/* Before there is an instance.

   Joining, a spent invite, nothing paired yet. These are the first screens
   anybody sees — the one right after scanning the code is one of them — and
   they were unstyled serif on white because the stylesheet loaded after the
   instance resolved. Centred and quiet: there is one fact and one instruction,
   and nothing to do but read them. */
.notice {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  justify-content: center;
  min-height: 100dvh;
  padding: var(--space-6) var(--space-5);
  max-width: 46ch;
  margin: 0 auto;
}
.notice h1 {
  margin: 0;
  font-family: var(--face-machine);
  font-size: var(--text-lg);
  font-weight: 500;
  letter-spacing: -0.02em;
  color: var(--fg);
}
.notice p {
  margin: 0;
  font-family: var(--face-human);
  font-size: var(--text-base);
  line-height: 1.5;
  color: var(--fg-muted);
}

/* The standing answer.

   One line, pinned under the bar, that says the only thing this client exists
   to say. It used to be a count — "4 sessions" — which is the one fact nobody
   opens their phone to learn. When something is waiting it is the accent and
   it names how many; when nothing is, it says so plainly, because "nothing is
   waiting" is a good answer and an empty screen is not. */
/* The standing answer.

   The one line this client exists to say, so it is set at the size of an
   answer rather than as a caption under a title. It used to be a count —
   "4 sessions" — which is the single fact nobody unlocks their phone to learn.

   Tabular figures, because the number changes while somebody is looking at it
   and a heading that jitters as its digits change width is a heading that
   draws the eye for the wrong reason. */
.bar .subtitle {
  display: block;
  margin-top: 3px;
  font-family: var(--face-machine);
  font-size: var(--text-base);
  font-weight: 400;
  font-variant-numeric: tabular-nums;
  letter-spacing: -0.01em;
  color: var(--fg-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}


/* Set by the roster when any session is blocked. Not a class on a new element:
   the line is the same line, and giving it a second home is how a count and a
   headline come to disagree. */
.bar .subtitle[data-waiting='yes'] {
  color: var(--halt);
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

.body {
  flex: 1 1 auto;
  min-width: 0;
  overflow-y: auto;
  -webkit-overflow-scrolling: touch;
  padding: var(--space-3);
}

/* Room to breathe once there is room. A phone has one column and no choice; a
   desktop has both, and using neither is what made this look like a phone
   somebody had zoomed. */
@media (min-width: 720px) {
  .body { padding: var(--space-5) var(--space-5) var(--space-6); }
  /* The bar is measured too, and by the same rule. Left where it was, the one
     line that matters sat at the far edge of a 1440px screen with the list it
     describes floating in the middle — two things that belong together, put in
     different places. */
  .bar {
    width: 100%;
    max-width: var(--measure);
    margin-inline: auto;
    padding: var(--space-6) 0 var(--space-4);
  }
}

/* Every tappable thing. One rule, so a button in a list and a button in a bar
   are the same size and the same shape. */
/* The roster is one object, not a stack of them.

   Rows inside a single panel: one border, one shadow, one lit top edge. Four
   separately-bordered cards is four objects to look at when there is one list
   to read — and it was also what made the client look like a settings screen
   rather than an instrument. */
.tap {
  display: grid;
  grid-template-columns: var(--gutter) 1fr auto;
  align-items: center;
  gap: 0;
  width: 100%;
  min-height: var(--touch-min);
  padding: var(--space-3) var(--space-4) var(--space-3) 0;
  margin: 0;
  border: 0;
  border-bottom: 1px solid var(--border);
  border-radius: 0;
  background: transparent;
  color: var(--fg);
  font-family: var(--face-machine);
  font-size: var(--text-base);
  text-align: left;
  cursor: pointer;
  position: relative;
  transition:
    background var(--duration) var(--ease),
    transform var(--duration) var(--ease);
}

/* The panel. Applied to the run of rows rather than to each, which is why the
   first and last corners are the only rounded ones on the screen. */
.tap:first-of-type {
  border-top-left-radius: var(--radius-lg);
  border-top-right-radius: var(--radius-lg);
  box-shadow: inset 0 1px 0 0 var(--lit);
}
.tap:last-of-type {
  border-bottom: 0;
  border-bottom-left-radius: var(--radius-lg);
  border-bottom-right-radius: var(--radius-lg);
}
.tap { background: var(--bg-elevated); }

/* A pointer exists here and the design did not know it: this ran in a browser
   with no hover state at all, so a desktop user had no way to tell what was a
   control. */
@media (hover: hover) {
  .tap:hover { background: color-mix(in srgb, var(--bg-elevated) 86%, var(--fg) 14%); }
}
.tap:active { transform: scale(0.995); }
.tap:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: -2px;
  z-index: 1;
}
.tap:disabled { cursor: default; opacity: 0.55; }

/* The row that is waiting, lifted out of the list.

   Not a colour on a word: an amber edge down the left of the row and a wash
   behind it, so it is found by shape before anything is read. This is the one
   place the whole client raises its voice. */
.tap:has(.dot.needs-you) {
  background: linear-gradient(
    90deg,
    var(--halt-wash),
    color-mix(in srgb, var(--halt-wash) 30%, var(--bg-elevated)) 40%,
    var(--bg-elevated)
  );
  box-shadow: inset 2px 0 0 0 var(--halt);
}

/* The attention gutter, and the one idea this client is built on.

   Every interface in this category spins something to say "busy" and goes
   still to say "done". That is backwards for omt. The state that needs a human
   is the state where an agent has *stopped* — so here, motion means working
   and stillness means you are needed.

   A working session breathes, slowly, at the edge of noticing. A waiting one
   is perfectly still and the only bright thing on the screen. You can read the
   whole roster without reading a word of it, which is how anybody actually
   reads this screen: at arm's length, one-handed, while doing something else.

   Two characters wide, and the glyph carries the state as well as the colour.
   Colour alone is invisible to a tenth of the people who will use this, and
   says nothing at all in a screenshot. */
.dot {
  grid-column: 1;
  justify-self: center;
  width: auto;
  height: auto;
  border-radius: 0;
  background: none;
  font-family: var(--face-machine);
  font-size: var(--text-base);
  line-height: 1;
  color: var(--fg-subtle);
}

.dot::before { content: "·"; }

.dot.working { color: var(--run); }
.dot.working::before { content: "●"; }
/* Slow enough to be felt rather than watched. A row of these at a spinner's
   speed is a screen nobody can look away from, which is the opposite of what
   this client is for. */
.dot.working { animation: breathe 2.4s var(--ease) infinite; }

.dot.ok { color: var(--done); }
.dot.ok::before { content: "●"; }

.dot.needs-you { color: var(--halt); }
/* Still, and the only glyph on the screen that is not a point or a circle. */
.dot.needs-you::before { content: "◉"; }

@keyframes breathe {
  0%, 100% { opacity: 0.4; }
  50% { opacity: 1; }
}

/* Somebody who asked for less motion gets none and loses nothing: the glyph
   and the colour carried the state on their own the whole time. */
@media (prefers-reduced-motion: reduce) {
  .dot.working { animation: none; opacity: 1; }
  .tap { transition: none; }
  .tap:active { transform: none; }
}

/* Two lines: what it is, then where it is and what it is doing. A column,
   because they are two facts and running them together gives "shelltmp". */
.tap .label {
  grid-column: 2;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.tap .title {
  display: block;
  font-size: var(--text-base);
  letter-spacing: -0.01em;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
/* The machine talking, so the machine face. Lower-case and unpunctuated: a
   field value, not a sentence. */
.tap .meta {
  display: block;
  font-family: var(--face-machine);
  font-size: var(--text-xs);
  color: var(--fg-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.tap:has(.dot.needs-you) .meta { color: var(--halt); }

.chevron {
  grid-column: 3;
  flex: 0 0 auto;
  color: var(--fg-subtle);
  font-family: var(--face-machine);
  font-size: var(--text-sm);
}
/* One of two controls on the bar, and neither should outrank the line that
   says whether anything needs you. Transparent until touched. */
.icon-button {
  flex: 0 0 auto;
  display: grid;
  place-items: center;
  width: var(--touch-min);
  height: var(--touch-min);
  padding: 0;
  border: 0;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--fg-muted);
  font-family: var(--face-machine);
  font-size: var(--text-lg);
  line-height: 1;
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

`;
