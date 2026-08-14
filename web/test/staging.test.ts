import { describe, expect, it } from 'vitest'
import { Attachments } from '../src/staging.js'

describe('what is on its way, and what happened to it', () => {
  it('shows an attachment the instant it is chosen', () => {
    // The gap between tapping and anything appearing is where somebody taps
    // again — and the second tap sends the file twice.
    const staged = new Attachments()
    staged.begin('screenshot.png')
    expect(staged.list()).toHaveLength(1)
    expect(staged.list()[0]?.state).toBe('sending')
    expect(staged.busy()).toBe(true)
  })

  it('names the one that failed, with the reason', () => {
    // "Something failed" is not actionable when three things are in flight.
    const staged = new Attachments()
    const a = staged.begin('a.png')
    const b = staged.begin('b.png')
    const c = staged.begin('c.png')
    staged.fail(b, 'the connection dropped')

    const failed = staged.list().filter((s) => s.state === 'failed')
    expect(failed).toHaveLength(1)
    expect(failed[0]?.name).toBe('b.png')
    expect(failed[0]?.reason).toBe('the connection dropped')
    // And the other two are untouched.
    expect(staged.list().filter((s) => s.state === 'sending').map((s) => s.id)).toEqual([a, c])
  })

  it('tells two files with the same name apart', () => {
    // Two screenshots taken a minute apart are both `image.png`, and a list
    // keyed by name would mark the wrong one as failed.
    const staged = new Attachments()
    const first = staged.begin('image.png')
    const second = staged.begin('image.png')
    expect(first).not.toBe(second)
    staged.fail(second, 'too large')
    expect(staged.list().find((s) => s.id === first)?.state).toBe('sending')
    expect(staged.list().find((s) => s.id === second)?.state).toBe('failed')
  })

  it('forgets one that arrived rather than marking it done', () => {
    // It is in the conversation now. A list of things that already worked is a
    // list nobody reads, sitting above the field somebody is typing in.
    const staged = new Attachments()
    const id = staged.begin('notes.md')
    staged.succeed(id)
    expect(staged.list()).toEqual([])
    expect(staged.busy()).toBe(false)
  })

  it('keeps a failure until somebody deals with it', () => {
    // A failure that faded on its own is one nobody saw, and the file is still
    // not there.
    const staged = new Attachments()
    const id = staged.begin('big.zip')
    staged.fail(id, 'too large')
    expect(staged.list()).toHaveLength(1)
    expect(staged.busy()).toBe(false)
    staged.dismiss(id)
    expect(staged.list()).toEqual([])
  })

  it('holds no text at all', () => {
    // The rule this file exists for, as a shape rather than a discipline:
    // there is no code path on which a failed upload can clear what somebody
    // typed, because this does not know what they typed.
    const staged = new Attachments()
    staged.fail(staged.begin('x.png'), 'nope')
    for (const item of staged.list()) {
      expect(Object.keys(item).sort()).toEqual(['id', 'name', 'reason', 'state'])
    }
  })
})
