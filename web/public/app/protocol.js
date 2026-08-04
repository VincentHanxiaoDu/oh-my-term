/**
 * The wire types, mirroring `omt-proto`.
 *
 * Hand-written for now and replaced by `cargo xtask codegen`; the shapes here
 * are the ones the parity gate checks, so a drift between this and the Rust
 * definition is a build failure rather than a runtime surprise.
 */
/** The reserved key an instance-scoped stream resumes by. */
export const INSTANCE_SCOPE_KEY = 's_instance';
/** How a scope is keyed in a resume map. */
export function scopeKey(scope) {
    switch (scope.kind) {
        case 'session':
            return scope.session;
        case 'workspace':
            return scope.workspace;
        case 'instance':
            return INSTANCE_SCOPE_KEY;
    }
}
/** Whether an answering affordance may be shown. */
export function isAnswerable(d) {
    return d.kind !== 'none';
}
/** Whether nothing further will happen to this interaction. */
export function isTerminal(s) {
    return (s.state === 'resolved' ||
        s.state === 'undelivered' ||
        s.state === 'cancelled' ||
        s.state === 'abandoned');
}
/** The protocol version this client speaks. */
export const PROTO_VERSION = 1;
