import { describe, expect, it } from 'vitest'
import { type Environment, badgeCount, canPush, decide } from '../src/index.js'

function env(overrides: Partial<Environment> = {}): Environment {
  return {
    hasServiceWorker: true,
    hasPushManager: true,
    isSecureContext: true,
    isStandalone: true,
    isIos: false,
    permission: 'default',
    ...overrides,
  }
}

describe('whether push is available', () => {
  it('a modern installed app can push', () => {
    expect(canPush(env()).available).toBe(true)
  })

  it('an iOS tab is told to install rather than to grant permission', () => {
    // PushManager is not exposed to a Safari tab, and every iOS browser is
    // WebKit — so no browser escapes it and "allow notifications" is the
    // wrong instruction.
    const c = canPush(env({ isIos: true, isStandalone: false }))
    expect(c.available).toBe(false)
    expect(c.blocker?.reason).toBe('needs-install')
    expect(c.blocker?.detail).toContain('Home Screen')
  })

  it('an insecure origin is reported before anything else', () => {
    // Telling somebody to grant permission when the real problem is an
    // untrusted certificate sends them to the wrong settings screen.
    const c = canPush(env({ isSecureContext: false, permission: 'denied' }))
    expect(c.blocker?.reason).toBe('insecure-origin')
    expect(c.blocker?.detail).toContain('tailscale cert')
  })

  it('a blocked permission says where to unblock it', () => {
    const c = canPush(env({ permission: 'denied' }))
    expect(c.blocker?.reason).toBe('denied')
    expect(c.blocker?.detail).toContain('browser settings')
  })

  it('a browser without push says so plainly', () => {
    const c = canPush(env({ hasPushManager: false }))
    expect(c.blocker?.reason).toBe('unsupported')
  })
})

describe('whether to notify', () => {
  const base = {
    blocked: 1,
    focused: false,
    alreadyTold: [] as string[],
    waiting: ['i1'],
    sessionName: 'api',
  }

  it('buzzes when something is waiting and nobody is watching', () => {
    const d = decide(base)
    expect(d.notify).toBe(true)
    if (d.notify) {
      expect(d.title).toContain('api')
    }
  })

  it('stays silent while the user is looking at it', () => {
    // Being buzzed about something you are actively watching is how
    // notifications get turned off entirely.
    expect(decide({ ...base, focused: true })).toEqual({
      notify: false,
      because: 'watching',
    })
  })

  it('does not repeat itself about the same card', () => {
    // Re-notifying every time state changes is the fastest way to teach
    // somebody to ignore the app.
    expect(decide({ ...base, alreadyTold: ['i1'] })).toEqual({
      notify: false,
      because: 'already-told',
    })
  })

  it('does buzz about a new card even when an old one is outstanding', () => {
    const d = decide({ ...base, waiting: ['i1', 'i2'], alreadyTold: ['i1'], blocked: 2 })
    expect(d.notify).toBe(true)
  })

  it('says how many when several are waiting', () => {
    const d = decide({ ...base, waiting: ['i1', 'i2', 'i3'], blocked: 3 })
    expect(d.notify && d.body).toContain('3')
  })

  it('stays silent when nothing is waiting', () => {
    expect(decide({ ...base, blocked: 0, waiting: [] })).toEqual({
      notify: false,
      because: 'nothing-waiting',
    })
  })
})

describe('the badge', () => {
  it('shows how many need you', () => {
    expect(badgeCount(3)).toBe(3)
  })

  it('is cleared rather than set to zero', () => {
    // A badge of zero says "nothing", which is what no badge already says.
    expect(badgeCount(0)).toBeUndefined()
  })
})
