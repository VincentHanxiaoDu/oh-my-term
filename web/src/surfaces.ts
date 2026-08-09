/**
 * The three ways a session can be read, and how the client chooses between
 * them.
 *
 * A phone is about forty columns at a readable size, and almost every useful
 * terminal output — a test run, a `git status`, an agent's tool output — is
 * written for eighty to a hundred and twenty. Reflowing the grid to forty is
 * technically correct and practically unreadable, so the grid is the fallback
 * and these are what a session is actually read through.
 */

import { el } from './view.js'

/** A way of reading a session. */
export type Surface = 'blocks' | 'transcript' | 'terminal'

/** Whether a surface can be used for a session, and why not when it cannot. */
export interface SurfaceOffer {
  surface: Surface
  available: boolean
  /** Said out loud. A disabled control with no reason reads as a bug. */
  why?: string
}

/**
 * Which surfaces a session offers.
 *
 * The rule is by session *kind*, not by viewport. An agent produces no command
 * marks at all, so block view for an agent session is not a worse option — it
 * is not an option, and the control says so rather than showing an empty list.
 */
export function surfacesFor(session: {
  threads: number
  shellIntegration: boolean
  native: boolean
}): SurfaceOffer[] {
  const offers: SurfaceOffer[] = []

  offers.push(
    session.threads > 0
      ? { surface: 'blocks', available: false, why: 'an agent emits no command marks' }
      : session.shellIntegration
        ? { surface: 'blocks', available: true }
        : {
            surface: 'blocks',
            available: false,
            why: 'this shell does not mark where commands start',
          },
  )

  offers.push(
    session.threads > 0
      ? { surface: 'transcript', available: true }
      : {
          surface: 'transcript',
          available: false,
          why: 'nothing in this session is an agent',
        },
  )

  // A native session has no pty, so there is no grid to show. Absent rather
  // than disabled: it is not a thing that could become available.
  if (!session.native) {
    offers.push({ surface: 'terminal', available: true })
  }
  return offers
}

/**
 * Which surface to open a session on.
 *
 * The terminal is always one tap away, and is the answer whenever nothing
 * better is available — never the first answer when something better is.
 */
export function defaultSurface(offers: SurfaceOffer[]): Surface {
  const usable = offers.filter((o) => o.available)
  return (
    usable.find((o) => o.surface === 'transcript')?.surface ??
    usable.find((o) => o.surface === 'blocks')?.surface ??
    usable[0]?.surface ??
    'terminal'
  )
}

/** One entry of a transcript, as the instance reports it. */
export interface TranscriptEntry {
  kind: string
  text?: string
  synthetic?: boolean
  call?: string
  name?: string
  input?: unknown
  output?: unknown
  error?: string | null
  duration_ms?: number | null
  steps?: { text?: string; title?: string; state?: string }[]
  path?: string
  change?: string
  tool?: string | null
  category?: string
}

/**
 * How long a tool's arguments may be before they are folded away.
 *
 * A `Write` call carries a whole file. Rendered in full it buries the six
 * entries around it, and the thing somebody is scrolling for is the shape of
 * the conversation rather than the contents of one argument.
 */
export const ARGUMENT_PREVIEW = 120

/** A one-line summary of a tool call, for the collapsed state. */
export function summariseCall(entry: TranscriptEntry): string {
  const input = entry.input
  if (input === undefined || input === null) {
    return entry.name ?? 'tool'
  }
  // The commonest arguments get named treatment, because "Bash(cargo test)"
  // says what happened and `{"command":"cargo test"}` makes somebody read JSON.
  if (typeof input === 'object') {
    const record = input as Record<string, unknown>
    for (const key of ['command', 'path', 'file_path', 'pattern', 'query']) {
      const value = record[key]
      if (typeof value === 'string' && value.length > 0) {
        return clip(value, ARGUMENT_PREVIEW)
      }
    }
  }
  return clip(typeof input === 'string' ? input : JSON.stringify(input), ARGUMENT_PREVIEW)
}

function clip(text: string, limit: number): string {
  const flat = text.replace(/\s+/g, ' ').trim()
  return flat.length <= limit ? flat : `${flat.slice(0, limit - 1)}…`
}

/** Whether a tool result is a failure, from what it says rather than a guess. */
export function failed(entry: TranscriptEntry): boolean {
  return typeof entry.error === 'string' && entry.error.length > 0
}

/**
 * Pair tool calls with their results.
 *
 * Rendered apart, a call and its result are two entries with a wall of output
 * between them, and the question anybody has — did that work — needs scrolling
 * to answer. Paired, a call is one row that carries its own outcome.
 */
export function pairCalls(entries: readonly TranscriptEntry[]): TranscriptEntry[] {
  const results = new Map<string, TranscriptEntry>()
  for (const entry of entries) {
    if (entry.kind === 'tool_result' && entry.call) {
      results.set(entry.call, entry)
    }
  }
  const out: TranscriptEntry[] = []
  for (const entry of entries) {
    if (entry.kind === 'tool_result') {
      continue
    }
    if (entry.kind === 'tool_call' && entry.call && results.has(entry.call)) {
      const result = results.get(entry.call)
      out.push({ ...entry, output: result?.output, error: result?.error ?? null, duration_ms: result?.duration_ms ?? null })
      continue
    }
    out.push(entry)
  }
  return out
}

/** Render one transcript entry. */
export function renderEntry(entry: TranscriptEntry): HTMLElement {
  switch (entry.kind) {
    case 'user': {
      const row = el('div', 'entry entry-user')
      if (entry.synthetic) {
        // Never shown as a human turn. omt answering a card on somebody's
        // behalf and somebody typing are different events, and a transcript
        // that conflates them misattributes a decision.
        row.append(el('span', 'entry-tag', 'sent by omt'))
      }
      row.append(el('p', 'entry-text', entry.text ?? ''))
      return row
    }
    case 'assistant': {
      const row = el('div', 'entry entry-assistant')
      row.append(el('p', 'entry-text', entry.text ?? ''))
      return row
    }
    case 'reasoning': {
      // Collapsed by default: reasoning rendered as speech reads as the agent
      // telling you things it has not decided.
      const box = el('details', 'entry entry-reasoning')
      const summary = el('summary', undefined, 'Thinking')
      box.append(summary, el('p', 'entry-text', entry.text ?? ''))
      return box
    }
    case 'tool_call': {
      const box = el('details', failed(entry) ? 'entry entry-tool failed' : 'entry entry-tool')
      const summary = el('summary')
      summary.append(el('span', 'tool-name', entry.name ?? 'tool'))
      summary.append(el('span', 'tool-arg', summariseCall(entry)))
      if (typeof entry.duration_ms === 'number') {
        summary.append(el('span', 'tool-time', `${Math.round(entry.duration_ms)}ms`))
      }
      box.append(summary)
      if (failed(entry)) {
        box.append(el('pre', 'entry-output', entry.error ?? ''))
      } else if (entry.output !== undefined && entry.output !== null) {
        box.append(
          el(
            'pre',
            'entry-output',
            typeof entry.output === 'string' ? entry.output : JSON.stringify(entry.output, null, 2),
          ),
        )
      }
      return box
    }
    case 'plan': {
      const box = el('div', 'entry entry-plan')
      box.append(el('p', 'entry-tag', 'Plan'))
      const list = el('ol', 'plan')
      for (const step of entry.steps ?? []) {
        const item = el('li', step.state ? `step ${step.state}` : 'step')
        item.textContent = step.title ?? step.text ?? ''
        list.append(item)
      }
      box.append(list)
      return box
    }
    case 'file_changed': {
      const row = el('div', 'entry entry-file')
      row.append(el('span', 'entry-tag', entry.change ?? 'changed'))
      row.append(el('span', 'entry-text', entry.path ?? ''))
      return row
    }
    default: {
      // Reported by its own name rather than dropped: an event omt does not
      // model is still something the agent said.
      const row = el('div', 'entry entry-other')
      row.append(el('span', 'entry-tag', entry.category ?? entry.kind))
      row.append(el('span', 'entry-text', entry.text ?? ''))
      return row
    }
  }
}
