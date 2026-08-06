/**
 * The wire types, mirroring `omt-proto`.
 *
 * Hand-written for now and replaced by `cargo xtask codegen`; the shapes here
 * are the ones the parity gate checks, so a drift between this and the Rust
 * definition is a build failure rather than a runtime surprise.
 */

/** A position in one scope's stream. */
export type Seq = number

/** Which stream a position counts in. */
export type SeqScope =
  | { kind: 'session'; session: string }
  | { kind: 'workspace'; workspace: string }
  | { kind: 'instance' }

/** The reserved key an instance-scoped stream resumes by. */
export const INSTANCE_SCOPE_KEY = 's_instance'

/** How a scope is keyed in a resume map. */
export function scopeKey(scope: SeqScope): string {
  switch (scope.kind) {
    case 'session':
      return scope.session
    case 'workspace':
      return scope.workspace
    case 'instance':
      return INSTANCE_SCOPE_KEY
  }
}

/** What an agent is doing. The only vocabulary any surface uses. */
export type AgentState =
  | { state: 'starting' }
  | { state: 'idle' }
  | { state: 'working'; detail?: string }
  | { state: 'blocked'; reason: BlockReason; interaction?: string }
  | { state: 'exited'; code?: number }
  | { state: 'unknown' }

/** Why an agent is blocked. */
export type BlockReason =
  | 'question'
  | 'permission'
  | 'plan_review'
  | 'elicitation'
  | 'input'
  | 'unspecified'

/**
 * Whether a surface may offer a way to answer.
 *
 * Read from the interaction's `deliverable`, never from its state: a card that
 * is open is not necessarily one omt can answer, and offering a button for one
 * omt cannot deliver means the user finds out by the wrong option being chosen.
 */
export type Deliverable =
  | { kind: 'native' }
  | { kind: 'synthetic'; requires_token: boolean }
  | { kind: 'none'; reason: string }

/** Whether an answering affordance may be shown. */
export function isAnswerable(d: Deliverable): boolean {
  return d.kind !== 'none'
}

/** An interaction's lifecycle position. */
export type InteractionState =
  | { state: 'open' }
  | { state: 'resolving'; by: string }
  | { state: 'submitted'; by: string }
  | { state: 'resolved'; by: string }
  | { state: 'undelivered'; by: string; reason: string }
  | { state: 'cancelled'; by: string }
  | { state: 'abandoned'; detail: string }

/** Whether nothing further will happen to this interaction. */
export function isTerminal(s: InteractionState): boolean {
  return (
    s.state === 'resolved' ||
    s.state === 'undelivered' ||
    s.state === 'cancelled' ||
    s.state === 'abandoned'
  )
}

/** An interaction as a client sees it. */
export interface Interaction {
  id: string
  session: string
  binding: string
  deliverable: Deliverable
  state: InteractionState
  opened_at: string
}

/**
 * A card as `interaction.list` reports it.
 *
 * Flattened by the instance rather than sent raw, so a small screen does not
 * have to understand every interaction kind to draw one — and so the rule about
 * what may be answered lives in one place instead of in each client.
 */
export interface InteractionCard {
  id: string
  session: string
  kind: string
  /** Whether omt can deliver an answer at all, and over what channel. */
  deliverable: 'native' | 'synthetic' | 'none'
  /** Why not, when it cannot be answered from here. */
  not_deliverable_because?: string
  state: string
  /** The question, or the command being approved. */
  prompt: string
  /** The agent's own options, verbatim and in its order. */
  options: string[]
}

/** One event off the stream. */
export interface OmtEvent {
  scope: SeqScope
  seq: Seq
  ts: string
  source: string
  payload: unknown
}

/** Anything the server can send. */
export type ServerMessage =
  | { t: 'welcome'; proto: number; role: string; catalog_hash: string; capabilities: string[] }
  | { t: 'goodbye'; reason: string; detail: string }
  | { t: 'result'; request: unknown; status: 'ok'; output: unknown }
  | { t: 'result'; request: unknown; status: 'err'; error: { code: string; message: string } }
  | { t: 'event'; event: OmtEvent }
  | { t: 'resync'; scope_key: string; reason: string; dropped: number; from: Seq }

/** Anything a client can send. */
export type ClientMessage =
  | { t: 'hello'; proto: number; client: string; token?: string }
  | { t: 'call'; request: RequestId; capability: string; input: unknown; intent?: string }
  | { t: 'subscribe'; since: Record<string, Seq> }

/**
 * A request's identity.
 *
 * Minted by the client, at intent time, before any server was reached — a
 * disconnected client cannot ask for one, and that is exactly when it needs to
 * be able to retry safely later.
 */
export interface RequestId {
  device: string
  n: number
}

/** The protocol version this client speaks. */
export const PROTO_VERSION = 1
