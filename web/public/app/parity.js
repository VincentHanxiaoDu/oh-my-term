/**
 * The parity gate, as a type error rather than a test failure.
 *
 * `docs/architecture/08-web-client.md` §2: the parity mechanism is a
 * **type-level exhaustiveness check**, and it only works if the compiler is not
 * lying to us — which is what `strict`, `noUncheckedIndexedAccess` and
 * `exactOptionalPropertyTypes` are for.
 *
 * There was already a test that greps `capabilities.ts` for each name. That
 * catches the same class of mistake, but later and more weakly: it runs in CI
 * rather than in the editor, it passes on a name that appears in a comment, and
 * it says "missing" without saying what shape the missing thing should be.
 * A type error says it where the code is being written.
 *
 * What this does *not* assert is that a handler sends the right arguments —
 * that is the schema's job and it is checked over the wire. This asserts the
 * one thing a client can get wrong silently: a capability the instance offers
 * and the client cannot call, which is how a feature ends up implemented,
 * tested, and unreachable.
 */
import { CAPABILITIES } from './generated/catalog';
import { HANDLERS } from './capabilities';
/**
 * Fails to compile if either set is non-empty.
 *
 * Assigning `never` is the check: TypeScript accepts the empty case and rejects
 * anything else, naming the offending members.
 */
const _unhandled = true;
const _unknown = true;
void _unhandled;
void _unknown;
/**
 * The same question at runtime, for a client talking to an instance of another
 * version.
 *
 * The type check is about *this* build. A phone attached to several instances
 * meets capabilities its own catalog never had, and 03 §7 says it renders the
 * intersection rather than failing — so this reports rather than throws.
 */
export function unreachable(offered) {
    return offered.filter((name) => !(name in HANDLERS)).sort();
}
/** Capabilities this client knows that an instance does not offer. */
export function unavailable(offered) {
    const has = new Set(offered);
    return CAPABILITIES.filter((name) => !has.has(name) && name in HANDLERS).sort();
}
