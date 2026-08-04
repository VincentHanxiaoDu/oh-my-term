import { describe, expect, it } from 'vitest'
import handlersJson from '../src/generated/handlers.json' with { type: 'json' }
import { HANDLERS, handledCapabilities, handles } from '../src/index.js'

describe('the handler list means what the parity gate thinks it means', () => {
  it('lists exactly what this client implements', () => {
    // The gate reads the JSON. If it could drift from the code, it would
    // degrade into a list that passes itself — a check that cannot fail.
    expect(handledCapabilities()).toEqual([...(handlersJson as string[])].sort())
  })

  it('every listed capability has a callable function', () => {
    for (const name of handlersJson as string[]) {
      expect(handles(name)).toBe(true)
      expect(typeof HANDLERS[name as keyof typeof HANDLERS]).toBe('function')
    }
  })

  it('builds a call carrying the request id the client minted', () => {
    const request = { device: 'd1', n: 7 }
    const message = HANDLERS['session.close'](request, 'sess_abc')
    expect(message).toEqual({
      t: 'call',
      request,
      capability: 'session.close',
      input: { session: 'sess_abc' },
    })
  })

  it('omits an optional filter rather than sending it undefined', () => {
    // `{workspace: undefined}` is a different message from `{}`, and a server
    // checking for the field's presence sees one.
    expect(HANDLERS['session.list']({ device: 'd', n: 1 })).toMatchObject({ input: {} })
    expect(HANDLERS['session.list']({ device: 'd', n: 1 }, 'wksp_x')).toMatchObject({
      input: { workspace: 'wksp_x' },
    })
  })

  it('does not claim to handle something it has no function for', () => {
    expect(handles('agent.interrupt')).toBe(false)
  })
})
