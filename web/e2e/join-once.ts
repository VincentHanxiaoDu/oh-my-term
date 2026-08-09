/**
 * Join once, before any test runs.
 *
 * An invite is single-exchange, which is the whole point of it. Handing the
 * same one to every spec means the first test joins and the rest are refused —
 * so the suite has to do what a real device does: exchange it once, keep the
 * credential, and use that from then on.
 */
import WebSocket from 'ws'

export default async function joinOnce(): Promise<void> {
  const invite = process.env['OMT_TOKEN']
  const url = process.env['OMT_URL'] ?? 'http://127.0.0.1:7788'
  if (!invite || !invite.startsWith('omt_i_')) {
    // Already a credential, or nothing to do. Left alone rather than guessed
    // at: a suite that silently invents a token fails in a way that says
    // nothing about the product.
    process.env['OMT_CREDENTIAL'] = invite ?? ''
    return
  }

  const socket = new WebSocket(`${url.replace(/^http/, 'ws')}/api/ws`)
  const credential = await new Promise<string>((resolve, reject) => {
    const giveUp = setTimeout(() => reject(new Error('the instance never answered')), 15_000)
    socket.on('open', () => {
      socket.send(
        JSON.stringify({
          t: 'join_exchange',
          request: { device: crypto.randomUUID(), n: 1 },
          invite,
          device: { name: 'the touch suite', platform: 'tests' },
        }),
      )
    })
    socket.on('message', (data: Buffer) => {
      clearTimeout(giveUp)
      const message = JSON.parse(data.toString()) as { t: string; token?: string }
      socket.close()
      if (message.t === 'join_credential' && message.token) {
        resolve(message.token)
      } else {
        reject(new Error(`the invite was refused: ${data.toString()}`))
      }
    })
    socket.on('error', reject)
  })
  process.env['OMT_CREDENTIAL'] = credential
}
