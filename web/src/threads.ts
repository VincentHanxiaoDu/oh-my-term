/**
 * The subagent grid.
 *
 * Claude Code's desktop client has a subagent switcher; its mobile client shows
 * one session at a time even when five agents are running. Closing that gap on
 * a phone is what this is for.
 *
 * **What steering honestly means here.** A running subagent has no free-text
 * input channel — the parent agent drives it, and nothing exposes a prompt for
 * one. What is actionable is answering a card that subagent raised, and
 * interrupting. Rendering a text box omt cannot deliver into would be the same
 * mistake as offering an answer button for an undeliverable card.
 */

import type { AgentState } from './protocol.js'

/** One thread: the session's own, or a subagent it spawned. */
export interface Thread {
  id: string
  /** The tool call that spawned it, for a subagent. */
  parent?: string
  is_subagent: boolean
  state: AgentState
  /** What it is doing, for a cell that has room for a word. */
  label?: string
  /** Cards it raised that are still waiting. */
  open_interactions: string[]
}

/** How a cell should read at a glance. */
export type CellTone = 'blocked' | 'working' | 'idle' | 'finished' | 'unknown'

/**
 * What a cell looks like.
 *
 * `tone` and `glyph` are separate on purpose: colour alone fails for anybody
 * who cannot distinguish it, and a grid whose only signal is hue is a grid that
 * does not tell some users which agent is stuck.
 */
export interface Cell {
  thread: Thread
  tone: CellTone
  /** A shape, so the grid is readable without colour. */
  glyph: string
  /** What a screen reader says. */
  label: string
  /** Whether tapping it does anything. */
  actionable: boolean
}

/** Which tone a state reads as. */
export function toneOf(state: AgentState): CellTone {
  switch (state.state) {
    case 'blocked':
      return 'blocked'
    case 'working':
      return 'working'
    case 'idle':
      return 'idle'
    case 'exited':
      return 'finished'
    case 'starting':
    case 'unknown':
      return 'unknown'
  }
}

const GLYPHS: Record<CellTone, string> = {
  // Filled and distinct in shape, not only in colour.
  blocked: '●',
  working: '◐',
  idle: '○',
  finished: '✓',
  unknown: '·',
}

/** Build the cell for one thread. */
export function cellFor(thread: Thread): Cell {
  const tone = toneOf(thread.state)
  const what = thread.label ?? (thread.is_subagent ? 'subagent' : 'main thread')
  const suffix =
    tone === 'blocked'
      ? 'needs you'
      : tone === 'working'
        ? 'working'
        : tone === 'finished'
          ? 'finished'
          : tone === 'idle'
            ? 'waiting'
            : 'no signal'
  return {
    thread,
    tone,
    glyph: GLYPHS[tone],
    label: `${what} — ${suffix}`,
    // Only a blocked thread has something to do *now*. A working one is
    // openable, but tapping it must not look like it will change anything.
    actionable: tone === 'blocked',
  }
}

/**
 * The grid, in the order it should be drawn.
 *
 * Blocked threads first. A grid sorted by spawn order buries the one that needs
 * you behind four that do not, which on a phone means scrolling to find the
 * only cell that matters.
 */
export function grid(threads: readonly Thread[]): Cell[] {
  const order: Record<CellTone, number> = {
    blocked: 0,
    working: 1,
    idle: 2,
    unknown: 3,
    finished: 4,
  }
  return [...threads]
    .map(cellFor)
    .sort(
      (a, b) =>
        order[a.tone] - order[b.tone] ||
        // Within a tone, a stable tiebreak: cells that reshuffle between two
        // renders are cells the user mistaps.
        Number(a.thread.is_subagent) - Number(b.thread.is_subagent) ||
        a.thread.id.localeCompare(b.thread.id),
    )
}

/** What the header says above the grid. */
export function summarize(threads: readonly Thread[]): string {
  const cells = threads.map(cellFor)
  const blocked = cells.filter((c) => c.tone === 'blocked').length
  const working = cells.filter((c) => c.tone === 'working').length
  if (blocked > 0) {
    // The one number worth leading with.
    return `${blocked} of ${threads.length} need you`
  }
  if (working > 0) {
    return `${working} of ${threads.length} working`
  }
  return threads.length === 0 ? 'nothing running' : `${threads.length} idle`
}

/** What tapping a cell can do. */
export type ThreadAction =
  | { action: 'answer'; interaction: string }
  | { action: 'open-transcript'; thread: string }

/**
 * What a tap offers, most useful first.
 *
 * Deliberately never includes a prompt: a running subagent has no input channel
 * to deliver one into.
 */
export function actionsFor(thread: Thread): ThreadAction[] {
  const actions: ThreadAction[] = thread.open_interactions.map((interaction) => ({
    action: 'answer' as const,
    interaction,
  }))
  actions.push({ action: 'open-transcript', thread: thread.id })
  return actions
}
