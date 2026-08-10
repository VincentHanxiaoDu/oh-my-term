/**
 * Put a real agent's output into a session, from the test.
 *
 * The client has no button for connecting an agent — that is done by the CLI or
 * by a hook — so a browser test cannot produce an agent session on its own. This
 * speaks the protocol directly, the same way `join-once` does, so what the
 * browser then renders has been through the adapter and the event stream rather
 * than being planted in the client.
 */
import WebSocket from 'ws'

/** Connect the scripted agent to a session, and resolve when it has run. */
export async function attachAgent(
  url: string,
  token: string,
  session: string,
  command: string[],
): Promise<void> {
  const socket = new WebSocket(`${url.replace(/^http/, 'ws')}/api/ws`, {
    headers: { Authorization: `Bearer ${token}` },
  })
  await new Promise<void>((resolve, reject) => {
    const giveUp = setTimeout(() => reject(new Error('the instance never answered')), 20_000)
    socket.on('open', () => {
      socket.send(
        JSON.stringify({ t: 'hello', proto: 1, client: 'e2e-agent', token }),
      )
    })
    socket.on('message', (data: Buffer) => {
      const message = JSON.parse(data.toString()) as { t: string; status?: string }
      if (message.t === 'welcome') {
        socket.send(
          JSON.stringify({
            t: 'call',
            request: { device: crypto.randomUUID(), n: 1 },
            capability: 'agent.connect',
            input: { session, agent: 'codex', command },
            intent: crypto.randomUUID(),
          }),
        )
        return
      }
      if (message.t === 'result') {
        clearTimeout(giveUp)
        socket.close()
        resolve()
      }
    })
    socket.on('error', reject)
  })
}

/** Every session this instance has, oldest first. */
export async function listSessions(url: string, token: string): Promise<string[]> {
  const socket = new WebSocket(`${url.replace(/^http/, 'ws')}/api/ws`, {
    headers: { Authorization: `Bearer ${token}` },
  })
  return await new Promise((resolve, reject) => {
    const giveUp = setTimeout(() => reject(new Error('the instance never answered')), 20_000)
    socket.on('open', () => {
      socket.send(JSON.stringify({ t: 'hello', proto: 1, client: 'e2e-agent', token }))
    })
    socket.on('message', (data: Buffer) => {
      const message = JSON.parse(data.toString()) as {
        t: string
        output?: { sessions?: { id: string }[] }
      }
      if (message.t === 'welcome') {
        socket.send(
          JSON.stringify({
            t: 'call',
            request: { device: crypto.randomUUID(), n: 2 },
            capability: 'session.list',
            input: {},
          }),
        )
        return
      }
      if (message.t === 'result') {
        clearTimeout(giveUp)
        socket.close()
        resolve((message.output?.sessions ?? []).map((s) => s.id))
      }
    })
    socket.on('error', reject)
  })
}
