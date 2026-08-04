import { describe, expect, it } from 'vitest'
import {
  HOLD_MS,
  KEY_BAR,
  KEY_BAR_SECONDARY,
  applyLatch,
  approvalStillValid,
  confirmation,
  dragToArrows,
  gestureBytes,
  keyToBytes,
  riskOf,
} from '../src/index.js'

describe('the key bar', () => {
  it('leads with the key whose absence breaks everything', () => {
    // Esc missing is literally the open bug that makes VS Code's mobile
    // terminal unusable.
    expect(KEY_BAR[0]?.label).toBe('Esc')
  })

  it('carries the characters a software keyboard buries', () => {
    // `/ - | ~` appear in almost every command and are two layers deep.
    const labels = KEY_BAR.map((k) => k.label)
    for (const ch of ['/', '-', '|', '~']) {
      expect(labels).toContain(ch)
    }
  })

  it('has arrows, because history is the second commonest action', () => {
    const labels = KEY_BAR.map((k) => k.label)
    expect(labels).toEqual(expect.arrayContaining(['↑', '↓', '←', '→']))
  })

  it('latches Ctrl rather than requiring it to be held', () => {
    // A modifier that must be held cannot be used one-handed.
    const ctrl = KEY_BAR.find((k) => k.label === 'Ctrl')
    expect(ctrl?.action).toEqual({ latch: 'ctrl' })
  })

  it('puts the interrupt on the second row where it cannot be mis-tapped', () => {
    expect(KEY_BAR_SECONDARY.some((k) => k.label === '^C')).toBe(true)
    expect(KEY_BAR.some((k) => k.label === '^C')).toBe(false)
  })
})

describe('latched modifiers', () => {
  it('ctrl maps a letter into the control range', () => {
    expect(applyLatch('ctrl', 'c')).toBe('\x03')
    expect(applyLatch('ctrl', 'C')).toBe('\x03')
    expect(applyLatch('ctrl', 'd')).toBe('\x04')
  })

  it('alt is a prefixed escape', () => {
    expect(applyLatch('alt', 'b')).toBe('\x1bb')
  })

  it('no latch leaves the key alone', () => {
    expect(applyLatch(null, 'a')).toBe('a')
  })

  it('a key with no control mapping passes through unchanged', () => {
    // Mangling it would send a byte the user did not type.
    expect(applyLatch('ctrl', '£')).toBe('£')
  })
})

describe('drag to arrows', () => {
  it('a short drag does nothing', () => {
    // Otherwise every tap that moves two pixels sends an arrow key.
    expect(dragToArrows(5, 3)).toBeNull()
  })

  it('a drag becomes the arrow in its dominant direction', () => {
    expect(dragToArrows(0, -100)).toMatchObject({ direction: 'up' })
    expect(dragToArrows(100, 0)).toMatchObject({ direction: 'right' })
  })

  it('a long drag accelerates', () => {
    // Scrolling back forty commands one press at a time is not something
    // anybody does twice.
    const short = dragToArrows(0, -DRAG_STEP * 4)
    const long = dragToArrows(0, -DRAG_STEP * 12)
    expect(long && short && long.kind === 'arrows' && short.kind === 'arrows').toBe(true)
    if (long?.kind === 'arrows' && short?.kind === 'arrows') {
      expect(long.repeat / short.repeat).toBeGreaterThan(3)
    }
  })

  it('an arrow gesture sends that many arrow keys', () => {
    expect(gestureBytes({ kind: 'arrows', direction: 'up', repeat: 3 })).toBe(
      '\x1b[A\x1b[A\x1b[A',
    )
  })

  it('a client-side gesture sends no bytes at all', () => {
    // Sending bytes for a scroll would put input into a program that never
    // asked for it.
    for (const g of [
      { kind: 'scroll' as const, lines: 5 },
      { kind: 'resize' as const, scale: 1.2 },
      { kind: 'switch' as const, delta: 1 },
    ]) {
      expect(gestureBytes(g)).toBe('')
    }
  })
})

const DRAG_STEP = 24

describe('card safety', () => {
  it('only irreversible actions need a hold', () => {
    // Requiring one for everything manufactures the habituation the gate
    // exists to prevent — the most-shown warning has the lowest adherence.
    expect(confirmation('irreversible')).toBe('hold')
    expect(confirmation('reversible')).toBe('tap')
    expect(confirmation('ordinary')).toBe('tap')
  })

  it('a hold is long enough that a mis-tap cannot produce one', () => {
    expect(HOLD_MS).toBeGreaterThanOrEqual(500)
  })

  it('classifies the commands that cannot be undone', () => {
    for (const c of [
      'rm -rf /',
      'rm -rf ~/projects',
      'git push --force origin main',
      'git reset --hard HEAD~5',
      'terraform destroy',
      'kubectl delete namespace prod',
      'DROP TABLE users',
    ]) {
      expect(riskOf(c)).toBe('irreversible')
    }
  })

  it('does not treat ordinary work as dangerous', () => {
    // Gating everything is how the gate stops meaning anything.
    for (const c of ['ls -la', 'cargo test', 'git status', 'npm run build']) {
      expect(riskOf(c)).toBe('ordinary')
    }
  })

  it('a reversible removal is reversible, not irreversible', () => {
    expect(riskOf('rm build/output.o')).toBe('reversible')
    expect(riskOf('git checkout -- src/main.rs')).toBe('reversible')
  })

  it('an approval does not survive the request changing', () => {
    // If what the agent is asking for changed between render and tap, the
    // approval belongs to the old request.
    const approved = { id: 'i1', commandHash: 'abc' }
    expect(approvalStillValid(approved, { id: 'i1', commandHash: 'abc' })).toBe(true)
    expect(approvalStillValid(approved, { id: 'i1', commandHash: 'CHANGED' })).toBe(false)
    expect(approvalStillValid(approved, { id: 'i2', commandHash: 'abc' })).toBe(false)
  })
})

describe('what a keypress sends', () => {
  const press = (key: string, mods: Partial<{ ctrlKey: boolean; altKey: boolean; metaKey: boolean }> = {}) =>
    keyToBytes({ key, ctrlKey: false, altKey: false, metaKey: false, ...mods })

  it('turns Ctrl-C into the byte a shell acts on', () => {
    expect(press('c', { ctrlKey: true })).toBe('\x03')
  })

  it('leaves the browser its own shortcuts', () => {
    // A terminal that swallows Cmd-V has taken paste away to gain nothing.
    expect(press('v', { metaKey: true })).toBeNull()
  })

  it('sends carriage return for Enter, not a newline', () => {
    // A line discipline in canonical mode acts on CR; LF leaves the command
    // sitting there looking submitted.
    expect(press('Enter')).toBe('\r')
  })

  it('sends delete for Backspace', () => {
    expect(press('Backspace')).toBe('\x7f')
  })

  it('sends the arrows as escape sequences readline understands', () => {
    expect(press('ArrowUp')).toBe('\x1b[A')
  })

  it('ignores keys a terminal has no use for', () => {
    // Returning something for these is how F5 stops reloading the page.
    expect(press('F5')).toBeNull()
    expect(press('Shift')).toBeNull()
  })

  it('passes an ordinary character through', () => {
    expect(press('a')).toBe('a')
  })
})
