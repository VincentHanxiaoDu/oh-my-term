// The service worker.
//
// Two jobs, and deliberately no third. It receives push and it opens the right
// screen when one is tapped. It does **not** cache the application: an offline
// omt shows a stale roster, and a stale roster is worse than an honest "cannot
// reach your instance" — somebody would act on it.

self.addEventListener('push', (event) => {
  // Every push must show something visible; iOS enforces this, and a silent
  // push there is a push that gets the subscription revoked.
  const payload = (() => {
    try {
      return event.data ? event.data.json() : {}
    } catch {
      // A payload this build cannot read still means *something* wants
      // attention, and saying so beats saying nothing.
      return {}
    }
  })()

  const title = payload.title || 'omt'
  const body = payload.body || 'An agent needs you'

  event.waitUntil(
    (async () => {
      await self.registration.showNotification(title, {
        body,
        // Collapses repeats about the same session rather than stacking five
        // notifications for one thing.
        tag: payload.session || 'omt',
        renotify: Boolean(payload.renotify),
        data: { url: payload.url || '/' },
        badge: '/icon-192.png',
        icon: '/icon-192.png',
      })
      if (typeof payload.blocked === 'number' && self.navigator.setAppBadge) {
        if (payload.blocked > 0) {
          await self.navigator.setAppBadge(payload.blocked)
        } else {
          await self.navigator.clearAppBadge()
        }
      }
    })(),
  )
})

self.addEventListener('notificationclick', (event) => {
  event.notification.close()
  const url = event.notification.data?.url || '/'

  event.waitUntil(
    (async () => {
      const windows = await self.clients.matchAll({
        type: 'window',
        includeUncontrolled: true,
      })
      // Focus an existing window rather than opening a second one: two copies
      // of the app answering the same card is exactly the race the ledger has
      // to resolve, and it is avoidable here.
      for (const client of windows) {
        if ('focus' in client) {
          await client.focus()
          if ('navigate' in client) {
            await client.navigate(url)
          }
          return
        }
      }
      await self.clients.openWindow(url)
    })(),
  )
})

// Take over immediately on update. A page held by an old worker would keep
// using an old protocol version against a newer instance.
self.addEventListener('install', () => self.skipWaiting())
self.addEventListener('activate', (event) => event.waitUntil(self.clients.claim()))
