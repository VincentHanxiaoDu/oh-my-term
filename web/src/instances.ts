/**
 * The instances this browser knows about.
 *
 * The client used to hold exactly one token in `localStorage`, so opening a
 * second instance's invite link overwrote the first — silently, with no way
 * back, because the token it replaced is shown once and cannot be recovered.
 * Somebody with a laptop and a desktop had to choose which one their phone was
 * for.
 *
 * Storage is deliberately dumb: a list of records, read and written whole.
 * There is no sync, no merge and no server copy. The credential for an instance
 * lives only in the browser that was invited, which is what makes "forget"
 * mean something.
 */

/** One instance this browser can reach. */
export interface Instance {
  /** Stable across renames — the list is keyed by this, never by label. */
  id: string
  /** What the person calls it. Editable, and never used for identity. */
  label: string
  /** Origin, with scheme and port. No trailing slash. */
  url: string
  /** The bearer credential. Never leaves this browser. */
  token: string
}

/** The prefix an invite carries, as distinct from a credential's. */
export const INVITE_PREFIX = 'omt_i_'

const KEY = 'omt.instances'
const CURRENT = 'omt.instance'

/** Everything this browser has been invited to. */
export function list(storage: Storage): Instance[] {
  const raw = storage.getItem(KEY)
  if (raw === null) {
    return []
  }
  try {
    const parsed: unknown = JSON.parse(raw)
    // Filtered rather than trusted: this is parsed from storage another version
    // of the client wrote, and one malformed record must not take out the list.
    return Array.isArray(parsed) ? parsed.filter(isInstance) : []
  } catch {
    return []
  }
}

function isInstance(value: unknown): value is Instance {
  if (typeof value !== 'object' || value === null) {
    return false
  }
  const record = value as Record<string, unknown>
  return (
    typeof record['id'] === 'string' &&
    typeof record['label'] === 'string' &&
    typeof record['url'] === 'string' &&
    typeof record['token'] === 'string'
  )
}

/**
 * Add an instance, or update the credential of one already known.
 *
 * Keyed by URL, because that is what identity means here: scanning a fresh
 * invite for an instance you already have is somebody re-pairing after a token
 * was rotated, and it must replace the credential rather than produce a second
 * identical row with one dead token in it.
 */
export function remember(storage: Storage, instance: Instance): Instance[] {
  const existing = list(storage)
  const at = existing.findIndex((known) => known.url === instance.url)
  const next =
    at === -1
      ? [...existing, instance]
      : existing.map((known, i) =>
          // The label is kept: somebody who named it "work laptop" did not
          // rename it by re-scanning.
          i === at ? { ...instance, id: known.id, label: known.label } : known,
        )
  storage.setItem(KEY, JSON.stringify(next))
  return next
}

/** Drop an instance and its credential. */
export function forget(storage: Storage, id: string): Instance[] {
  const next = list(storage).filter((known) => known.id !== id)
  storage.setItem(KEY, JSON.stringify(next))
  // A forgotten instance must not stay selected, or the client reconnects to
  // something it has no credential for and reports an auth failure instead of
  // an empty list.
  if (storage.getItem(CURRENT) === id) {
    storage.removeItem(CURRENT)
  }
  return next
}

/** Rename one, without touching its identity or credential. */
export function rename(storage: Storage, id: string, label: string): Instance[] {
  const next = list(storage).map((known) => (known.id === id ? { ...known, label } : known))
  storage.setItem(KEY, JSON.stringify(next))
  return next
}

/** Which instance the client should connect to. */
export function current(storage: Storage): Instance | null {
  const all = list(storage)
  const chosen = all.find((known) => known.id === storage.getItem(CURRENT))
  // Falling back to the first is what makes a fresh invite Just Work: there is
  // exactly one instance and nothing has been selected yet.
  return chosen ?? all[0] ?? null
}

/** Choose the instance to connect to. */
export function choose(storage: Storage, id: string): void {
  storage.setItem(CURRENT, id)
}

/**
 * Read an invite link into an instance.
 *
 * Accepts the URL the QR carries and the URL a person pastes, which are the
 * same string. Returns null rather than throwing on anything that is not one:
 * the commonest input to this is a mis-scan or a URL from somewhere else, and
 * neither is exceptional.
 */
export function readInvite(text: string, id: () => string): Instance | null {
  let url: URL
  try {
    url = new URL(text.trim())
  } catch {
    return null
  }
  // Only over HTTP(S). A `javascript:` or `data:` URL in a scanned code is the
  // obvious attack on a scanner that trusts what a camera saw.
  if (url.protocol !== 'http:' && url.protocol !== 'https:') {
    return null
  }
  const token = new URLSearchParams(url.hash.replace(/^#/, '')).get('token')
  if (token === null || token === '') {
    return null
  }
  return {
    id: id(),
    // The host is the honest default name, and the only one available without
    // asking the instance — which cannot be done before it is added.
    label: url.host,
    url: url.origin,
    token,
  }
}
