import { beforeEach, describe, expect, it } from 'vitest'
import { choose, current, forget, list, readInvite, remember, rename } from '../src/index.js'
import type { Instance } from '../src/index.js'

/** A `Storage` that is a Map, because jsdom's is shared between tests. */
function storage(): Storage {
  const map = new Map<string, string>()
  return {
    getItem: (k) => map.get(k) ?? null,
    setItem: (k, v) => void map.set(k, v),
    removeItem: (k) => void map.delete(k),
    clear: () => map.clear(),
    key: (i) => [...map.keys()][i] ?? null,
    get length() {
      return map.size
    },
  }
}

let ids = 0
const nextId = () => `i${++ids}`

const instance = (over: Partial<Instance> = {}): Instance => ({
  id: nextId(),
  label: 'laptop',
  url: 'http://192.168.1.9:7777',
  token: 'omt_c_aaa',
  ...over,
})

beforeEach(() => {
  ids = 0
})

describe('the instance list', () => {
  it('remembers more than one, which is the whole point', () => {
    // One token in one slot meant a second invite link silently replaced the
    // first — and the replaced token is shown once and cannot be recovered.
    const store = storage()
    remember(store, instance({ url: 'http://a:1' }))
    remember(store, instance({ url: 'http://b:2' }))
    expect(list(store)).toHaveLength(2)
  })

  it('replaces the credential when the same instance is paired again', () => {
    // Re-scanning after a token rotation is the normal case. A second row with
    // a dead token in it would make the list say two machines exist.
    const store = storage()
    remember(store, instance({ url: 'http://a:1', token: 'old' }))
    remember(store, instance({ url: 'http://a:1', token: 'new' }))
    expect(list(store)).toHaveLength(1)
    expect(list(store)[0]?.token).toBe('new')
  })

  it('keeps the name somebody gave it when they re-pair', () => {
    const store = storage()
    const first = instance({ url: 'http://a:1' })
    remember(store, first)
    rename(store, first.id, 'work laptop')
    remember(store, instance({ url: 'http://a:1', token: 'new', label: 'a' }))
    expect(list(store)[0]?.label).toBe('work laptop')
  })

  it('survives a record written by some other version', () => {
    // This is parsed from storage, not from the network, and one bad record
    // must not take out every instance somebody has paired.
    const store = storage()
    store.setItem('omt.instances', JSON.stringify([{ nonsense: true }, instance()]))
    expect(list(store)).toHaveLength(1)
  })

  it('is empty rather than broken when the storage is not JSON', () => {
    const store = storage()
    store.setItem('omt.instances', 'not json')
    expect(list(store)).toEqual([])
  })
})

describe('choosing an instance', () => {
  it('uses the only one without being asked', () => {
    // A fresh invite has to Just Work: one instance, nothing selected.
    const store = storage()
    remember(store, instance({ url: 'http://a:1' }))
    expect(current(store)?.url).toBe('http://a:1')
  })

  it('uses the chosen one when there are several', () => {
    const store = storage()
    const a = instance({ url: 'http://a:1' })
    const b = instance({ url: 'http://b:2' })
    remember(store, a)
    remember(store, b)
    choose(store, b.id)
    expect(current(store)?.url).toBe('http://b:2')
  })

  it('does not stay pointed at one that was forgotten', () => {
    // Left selected, the client reconnects with a credential it deleted and
    // reports an auth failure rather than an empty list.
    const store = storage()
    const a = instance({ url: 'http://a:1' })
    remember(store, a)
    choose(store, a.id)
    forget(store, a.id)
    expect(current(store)).toBeNull()
  })

  it('has nothing to connect to before anything is paired', () => {
    expect(current(storage())).toBeNull()
  })
})

describe('reading an invite', () => {
  it('takes the token out of the fragment', () => {
    const read = readInvite('https://box.local:7777/#token=omt_c_abc', nextId)
    expect(read?.token).toBe('omt_c_abc')
    expect(read?.url).toBe('https://box.local:7777')
  })

  it('names it after the host until somebody renames it', () => {
    expect(readInvite('https://box.local:7777/#token=t', nextId)?.label).toBe('box.local:7777')
  })

  it('refuses a link with no token rather than adding an unusable instance', () => {
    expect(readInvite('https://box.local:7777/', nextId)).toBeNull()
  })

  it('refuses a scheme that is not http', () => {
    // A `javascript:` URL in a scanned code is the obvious attack on anything
    // that trusts what the camera saw.
    expect(readInvite('javascript:alert(1)//#token=t', nextId)).toBeNull()
    expect(readInvite('data:text/html,x#token=t', nextId)).toBeNull()
  })

  it('refuses text that is not a URL at all', () => {
    // Most of what a camera reads is not an invite.
    expect(readInvite('hello', nextId)).toBeNull()
    expect(readInvite('', nextId)).toBeNull()
  })

  it('ignores the whitespace a paste brings with it', () => {
    expect(readInvite('  https://box:7777/#token=t\n', nextId)?.token).toBe('t')
  })

  it('does not read a token out of the query string', () => {
    // Deliberate: a token in a query string has already been written to an
    // access log by the time it arrives, so accepting one here would make the
    // insecure form work and therefore get used.
    expect(readInvite('https://box:7777/?token=t', nextId)).toBeNull()
  })
})

describe('the invite the client itself receives', () => {
  it('reads its own address bar the same way it reads a scan', () => {
    // One code path, so the link somebody pastes and the link a camera reads
    // cannot behave differently — which is exactly the kind of divergence that
    // makes a QR "not work" only for some people.
    const fromBar = readInvite('http://192.168.1.9:7777/#token=t', nextId)
    const fromCamera = readInvite('http://192.168.1.9:7777/#token=t', nextId)
    expect(fromBar?.url).toBe(fromCamera?.url)
    expect(fromBar?.token).toBe(fromCamera?.token)
  })
})
