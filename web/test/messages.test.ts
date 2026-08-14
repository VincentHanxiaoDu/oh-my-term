import { describe, expect, it } from 'vitest'
import {
  chooseLanguage,
  columns,
  en,
  languages,
  setLanguage,
  t,
  truncate,
  zhHans,
} from '../src/messages.js'

describe('what omt says, and in which language', () => {
  it('falls back to English rather than rendering a key', () => {
    // A half-translated screen is usable. A screen of identifiers is not, and
    // it is what every i18n layer that "fails loudly" actually ships.
    setLanguage('zh-Hans')
    const untranslated = Object.keys(en).find((k) => !(k in zhHans))
    if (untranslated !== undefined) {
      const english = (en as unknown as Record<string, string>)[untranslated]
      expect(t(untranslated as never)).toBe(english)
    }
    setLanguage('en')
  })

  it('translates omt words and would never be given an agent word', () => {
    // The rule the whole file exists for: omt's words are localizable and the
    // agent's are never touched. The table is the enforcement — an agent's
    // question has no key here to look up, so there is no path by which one
    // could be translated.
    setLanguage('zh-Hans')
    expect(t('projects.title')).toBe('打开项目')
    setLanguage('en')
    expect(t('projects.title')).toBe('Open a project')
  })

  it('makes a phrase with a number one template rather than a concatenation', () => {
    // `${n} + t('waiting')` is a sentence in English and is not one in
    // Chinese: word order and plural rules differ, and the concatenating
    // version is wrong everywhere except where it was written.
    setLanguage('zh-Hans')
    expect(t('projects.openIn', 'omt')).toBe('在 omt 中新建会话')
    setLanguage('en')
    expect(t('projects.openIn', 'omt')).toBe('Start a session in omt')
  })

  it('follows the browser and lets a setting override it', () => {
    expect(chooseLanguage(null, ['zh-CN', 'en'])).toBe('zh-Hans')
    expect(chooseLanguage(null, ['zh'])).toBe('zh-Hans')
    expect(chooseLanguage(null, ['zh-Hans-CN'])).toBe('zh-Hans')
    expect(chooseLanguage(null, ['en-GB'])).toBe('en')
    // The stored choice wins: a device's language and a person's preference
    // are not always the same thing.
    expect(chooseLanguage('en', ['zh-CN'])).toBe('en')
  })

  it('does not mistake a language that merely starts with the same letters', () => {
    // `zu` is Zulu. Matching a bare prefix would have sent it to Chinese.
    expect(chooseLanguage(null, ['zu'])).toBe('en')
  })

  it('names every language it claims to speak', () => {
    expect(Object.keys(languages).sort()).toEqual(['en', 'zh-Hans'])
  })
})

describe('measuring text the way a terminal does', () => {
  it('counts a CJK glyph as two columns', () => {
    // One code point, two columns. A layout that truncated by `String.length`
    // cut Chinese text to half the width it had reserved.
    expect(columns('abc')).toBe(3)
    expect(columns('打开项目')).toBe(8)
    expect(columns('a打b')).toBe(4)
  })

  it('truncates to a width and never exceeds it', () => {
    // The width is what the caller reserved and what the next thing on the row
    // is drawn at. One column over is one column of overlap.
    for (const text of ['abcdefghij', '打开项目设置文件', 'a打b开c项d']) {
      for (const width of [0, 1, 2, 3, 5, 8, 13]) {
        expect(columns(truncate(text, width))).toBeLessThanOrEqual(width)
      }
    }
  })

  it('leaves a string that already fits alone', () => {
    expect(truncate('打开', 4)).toBe('打开')
    expect(truncate('abc', 3)).toBe('abc')
  })

  it('does not cut a wide glyph in half', () => {
    // Half a glyph is a replacement character, which is worse than one fewer
    // character.
    const cut = truncate('打开项目', 5)
    expect(columns(cut)).toBeLessThanOrEqual(5)
    expect(cut.endsWith('…')).toBe(true)
  })
})

describe('the translation is complete enough to ship', () => {
  it('translates every string omt owns', () => {
    // Not a coverage target for its own sake: a screen half in each language
    // reads as broken rather than as partly translated, and this is the only
    // thing that notices.
    const missing = Object.keys(en).filter((key) => !(key in zhHans))
    expect(missing).toEqual([])
  })

  it('keeps the same arity for a phrase that takes arguments', () => {
    // A translation that dropped a parameter would render a sentence with a
    // hole in it, and nothing else would catch that.
    for (const [key, value] of Object.entries(en)) {
      if (typeof value !== 'function') {
        continue
      }
      const translated = (zhHans as Record<string, unknown>)[key]
      expect(typeof translated, `${key} is not a function in zh-Hans`).toBe('function')
      expect((translated as (...a: never[]) => string).length, `${key} arity`).toBe(value.length)
    }
  })
})
