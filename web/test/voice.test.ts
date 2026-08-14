import { describe, expect, it } from 'vitest'
import { en, setLanguage, t, zhHans } from '../src/messages.js'
import { layOutCard } from '../src/app.js'

describe("whose words are on the screen", () => {
  it('never translates an option an agent wrote', () => {
    // The rule the whole message table exists under: omt's words are
    // localizable and the agent's are never touched. A card's options *are*
    // how an answer is delivered — omt neither adds, removes, reorders nor
    // rewrites them — so translating one would be putting words in the
    // agent's mouth on the one screen where exactness is the product.
    setLanguage('zh-Hans')
    const options = ['Yes, run it', 'No', '让我看看 diff']
    const laid = layOutCard(options, 'rm -rf build')
    expect(laid.map((o) => o.label)).toEqual(options)
    setLanguage('en')
  })

  it('has no key for anything an agent says', () => {
    // Structural, not a convention: there is no path by which an agent's
    // words could be translated, because the table has nothing to look them
    // up under. Every key names a thing omt says about itself.
    const agentish = ['yes', 'no', 'allow', 'deny', 'approve', 'continue']
    for (const word of agentish) {
      expect(Object.keys(en)).not.toContain(word)
      expect(Object.keys(zhHans)).not.toContain(word)
    }
  })

  it('translates the controls around an agent question', () => {
    // The other half of the rule: the button that submits an agent's options
    // is omt's, and it is translated.
    setLanguage('zh-Hans')
    expect(t('attach.cancel')).toBe('取消')
    setLanguage('en')
    expect(t('attach.cancel')).toBe('Cancel')
  })

  it('keeps an agent option that happens to match a key omt owns', () => {
    // "Cancel" is a word omt uses and an agent might offer. The option is
    // still the agent's, and a lookup would silently replace it with omt's
    // phrasing in another language.
    setLanguage('zh-Hans')
    expect(layOutCard(['Cancel'], undefined).map((o) => o.label)).toEqual(['Cancel'])
    setLanguage('en')
  })
})
