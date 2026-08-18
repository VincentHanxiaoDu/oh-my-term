import { describe, expect, it } from 'vitest'
import { applyAppearance } from '../src/appearance.js'

/**
 * A stub element the way the other suites stub them: no DOM library, just
 * the surface the module touches. Custom properties land in `vars`, so an
 * assertion reads the configuration this client would actually apply.
 */
function root(): { vars: Record<string, string> } {
  const vars: Record<string, string> = {}
  const style = {
    setProperty(name: string, value: string): void {
      vars[name] = value
    },
    getPropertyValue(name: string): string {
      return vars[name] ?? ''
    },
  }
  return { vars, style } as never
}

describe('applyAppearance', () => {
  it('leaves a root untouched when nothing is configured', () => {
    const el = root()
    applyAppearance(el as never, {})
    expect(Object.keys(el.vars).length).toBe(0)
  })

  it('applies the font and its size, preferring the appearance keys', () => {
    const el = root()
    applyAppearance(el as never, {
      'appearance.font.family': 'Iosevka',
      'appearance.font.size': 13,
      'appearance.font.line_height': 1.4,
      'font.family': 'not-used',
      'font.size': 99,
    })
    expect(el.vars['--face-machine']).toBe('Iosevka')
    expect(el.vars['--omt-font-size']).toBe('13px')
    expect(el.vars['--omt-line-height']).toBe('1.4')
  })

  it('falls back to the bare font keys when the appearance keys are absent', () => {
    const el = root()
    applyAppearance(el as never, { 'font.family': 'JetBrains Mono', 'font.size': 15 })
    expect(el.vars['--face-machine']).toBe('JetBrains Mono')
    expect(el.vars['--omt-font-size']).toBe('15px')
  })

  it('shapes the cursor and its blink', () => {
    const el = root()
    applyAppearance(el as never, {
      'appearance.cursor.style': 'underline',
      'appearance.cursor.blink': true,
    })
    expect(el.vars['--omt-cursor-width']).toBe('1ch')
    expect(el.vars['--omt-cursor-height']).toBe('2px')
    expect(el.vars['--omt-cursor-blink']).toContain('infinite')
  })

  it('turns ligatures off when asked, and only then', () => {
    const el = root()
    applyAppearance(el as never, { 'appearance.font.ligatures': false })
    expect(el.vars['--omt-ligatures']).toBe('none')
    const other = root()
    applyAppearance(other as never, {})
    expect(other.vars['--omt-ligatures']).toBeUndefined()
  })

  it('appends fallback families to the face that is set', () => {
    const el = root()
    applyAppearance(el as never, {
      'appearance.font.family': 'Fira Code',
      'appearance.font.fallbacks': ['Menlo', 'monospace'],
    })
    expect(el.vars['--face-machine']).toContain('Fira Code')
    expect(el.vars['--face-machine']).toContain('Menlo')
  })
})
describe('applyAppearance blocks', () => {
  it('styles the block cards the way the setting says', () => {
    const el = root()
    applyAppearance(el as never, { 'appearance.block_style': 'minimal' })
    expect(el.vars['--omt-block-left']).toBe('none')
    expect(el.vars['--omt-block-top']).toContain('1px')
  })
})
