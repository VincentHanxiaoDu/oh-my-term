# Security Model

`omt` gives a phone the ability to type into a shell on your workstation and to
approve an agent's tool calls. That is a large amount of authority, and this
document is where it is bounded.

It is the specification behind
[P8](01-principles.md#p8--security-by-default-no-ambient-trust) and the
security-facing half of `omt-auth` and `omt-server`.

Related: [Decision log](decisions.md) ·
[03 — Capability catalog](03-capability-catalog.md) ·
[07 — Remote protocol](07-remote-protocol.md) ·
[12 — Collaboration](12-collaboration.md) ·
[14 — Licensing and provenance](14-licensing.md)

> **Scope note, binding.** Per
> [D1](decisions.md#d1--omt-adds-no-policy-layer-over-an-agents-permission-semantics)
> this document specifies **who may connect and with what authority**. It does
> **not** add a second permission gate over an agent CLI's own tool-permission
> model: omt has no danger classifier, no allow-list of approvable tools, and no
> auto-approve. Per
> [D2](decisions.md#d2--remote-is-exactly-equivalent-to-local) an authenticated
> client is equivalent to the TUI; roles and scopes exist so the owner can
> **share** a narrowed view with someone else, never to degrade the owner's own
> devices.

---

## 1. Threat model

### 1.1 Assets

| Asset | Why it matters |
|---|---|
| PTY write access | arbitrary code execution as the user, on their machine |
| PTY read access | source code, secrets echoed into terminals, `.env` contents, tokens in URLs |
| Interaction resolution | *approves an agent's tool call* — the highest-consequence action in the product |
| Credentials at rest | long-lived access to all of the above |
| Config and hook installation | persistence; a modified hook config runs code on every agent turn |
| Audit log | the record used to detect all of the above |

### 1.2 In scope — omt defends against

1. **Network attackers on a shared LAN or a tailnet.** All remote access is
   authenticated and, when not carried inside WireGuard, encrypted.
2. **A leaked invite link.** Invites are short-lived, scoped, single-exchange,
   and revocable.
3. **A stolen device credential.** Credentials are per-device, individually
   revocable, and bound to a device key so the token alone is not sufficient on
   the transports where binding applies.
4. **A hostile web page in the user's browser.** Origin and CSRF controls
   (§6) prevent a page at `evil.example` from driving the local daemon.
5. **A low-privilege collaborator.** Roles and credential *scope* bound what a
   shared credential can do and which workspaces it can see. This is a sharing
   control, not a second permission model over the agent (D1).
6. **Accidental exposure.** Loopback by default; the daemon refuses to bind
   publicly without an auth backend; the Funnel checklist (§10) is a gate, not a
   suggestion.
7. **Secret leakage through omt's own telemetry surface.** There is none —
   nothing leaves the machine unless configured (§9) — and logs/events are
   redacted (§8).

### 1.3 Out of scope — omt explicitly does not defend against

Stated plainly, because a threat model that claims everything protects nothing.

1. **A hostile local process running as the same uid.** It can read the socket,
   the state directory, and `/proc`. omt is not a sandbox and the daemon is not
   a privilege boundary against its own user.
2. **Root, or a compromised OS.** Same reason.
3. **A malicious agent CLI.** omt runs the binary you asked it to run, in a
   PTY, with your environment. It observes; it does not sandbox. If your agent
   is malicious, omt faithfully shows you what it does.
4. **A malicious plugin you installed.** Out-of-process plugins are isolated
   from omt's memory, not from your account. Installation is the trust decision.
   See [11 — Plugin system](11-plugins.md).
5. **The agent's own model behaviour, and the agent's own permission model.**
   omt does not evaluate whether a tool call is safe, does not classify tool
   calls by danger, and does not add or suppress an approval step the agent CLI
   would not have shown. It surfaces the agent's question exactly as the agent
   posed it and records your decision (D1). If you ran the agent with
   `--dangerously-skip-permissions`, omt honours that; it does not reintroduce a
   gate the CLI was told to drop.
6. **Traffic analysis.** Frame sizes leak coarse activity to an on-path
   observer.
7. **A compromised browser or a device with a hostile keyboard.** A credential
   on a compromised phone is a compromised credential.
8. **Cross-instance blast radius.** Instances do not trust each other, but they
   also do not protect each other; compromising the client compromises every
   instance that client holds credentials for.

---

## 2. Bind policy

The default is loopback and the defaults are enforced in code, not documented in
prose.

```rust
pub struct ListenConfig {
    pub bind: Vec<BindSpec>,
    pub tls: TlsConfig,
    pub auth: AuthConfig,
    pub allowed_origins: Vec<Origin>,
}

pub enum BindSpec {
    /// Default and always present. mode 0600, peer-uid checked.
    UnixSocket(PathBuf),
    /// Default TCP: 127.0.0.1:7878 and [::1]:7878.
    Loopback { port: u16 },
    /// A specific tailnet address; requires an auth backend OR tailnet identity.
    Tailnet { port: u16 },
    /// Any other interface, including 0.0.0.0. Heavily gated.
    Interface { addr: IpAddr, port: u16 },
}
```

Startup validation, which **fails the daemon** rather than warning:

| Condition | Result |
|---|---|
| `Loopback` or `UnixSocket` only | starts, no auth backend required |
| `Tailnet` with no `AuthBackend` and no tailnet identity available | **refuses to start**: `bind.tailnet requires an auth backend or a working Tailscale LocalAPI` |
| `Interface` with no `AuthBackend` | **refuses to start**: `refusing to bind 0.0.0.0:7878 without authentication; configure auth.password or auth.bearer, or bind loopback` |
| `Interface` non-loopback with no TLS and no reverse-proxy declaration | **refuses to start** unless `tls.trust_proxy = true` is explicitly set |
| `Interface` = `0.0.0.0` with a `Viewer`-default role | starts with a prominent warning in the log and in `instance.health` |

There is no `--insecure` flag and no environment variable that bypasses this.
The escape hatch is to write the configuration that says what you mean, which is
auditable in a file.

The **unix socket is not implicitly trusted** either. `SO_PEERCRED` /
`LOCAL_PEERCRED` is checked on every connection; a differing uid is rejected
before the handshake. This is the concrete departure from another tool, whose 0600
socket grants full control to any same-uid process with no scoping or record.
omt cannot prevent a same-uid process from connecting (§1.3), but it *records*
it: every socket connection is audited with its pid, uid and argv.

---

## 3. Authentication

### 3.1 The trait

```rust
#[async_trait]
pub trait AuthBackend: Send + Sync {
    /// Stable id; the closed set is
    /// "bearer", "password", "invite", "tailnet", "device_grant".
    /// `device_grant` presents a root-signed `DeviceGrant` and mints a
    /// per-instance credential on the spot — flow in
    /// [23 §3.2](23-identity-and-devices.md#32-how-other-instances-learn-about-it).
    fn method(&self) -> &'static str;

    /// Advertised in the handshake (07 §3.3). May depend on the transport —
    /// e.g. tailnet identity is only offered on a tailnet-sourced connection.
    fn offered_for(&self, peer: &PeerInfo) -> bool;

    /// Verify a credential presentation. Must be constant-time for secrets and
    /// must not leak which of {unknown principal, bad secret} occurred.
    async fn verify(
        &self,
        peer: &PeerInfo,
        nonce: &Nonce,
        presentation: &Presentation,
    ) -> Result<Grant, AuthError>;

    /// Revocation check on every reconnect and, for long-lived connections,
    /// every 60 s. A revoked credential closes the connection.
    ///
    /// A credential id, or a device whose grants and credentials are all revoked
    /// together ([23 §6.1](23-identity-and-devices.md#61-revoking-one-device)).
    async fn is_revoked(&self, subject: &RevocationSubject) -> Result<bool, AuthError>;
}

pub enum RevocationSubject {
    Credential(CredentialId),
    Device(DeviceId),
}

pub struct Grant {
    pub credential: CredentialId,
    /// The identity this credential belongs to (23 §1.1 owns `IdentityId`), and
    /// the device it was issued to. `device` is `None` for a password or tailnet
    /// grant that is not device-bound.
    pub identity: IdentityId,
    pub device: Option<DeviceId>,
    pub role: Role,
    pub scope: CredentialScope,          // §4.1
    pub expires_at: Option<OffsetDateTime>,
    /// `DeviceKey` is defined in [23 §1.1](23-identity-and-devices.md#11-four-types).
    /// Still `Option` here: device binding is genuinely optional for tailnet and
    /// local-socket grants, which have no long-lived credential to bind.
    pub device_bound: Option<DeviceKey>,
}
```

`is_revoked` takes a `RevocationSubject` rather than a bare `CredentialId`
because the unit a user revokes is a **device**, not a credential: `device.revoke`
([23 §6.1](23-identity-and-devices.md#61-revoking-one-device)) invalidates every
credential *and* the root-signed `DeviceGrant` that device holds, and a
`DeviceGrant` carries no `CredentialId` at all.

**Naming, because three things are called "grant".** `Grant` (here) is the
*result of authenticating* — what a backend returns. `DeviceGrant`
([23 §1.3](23-identity-and-devices.md#13-devicegrant--the-certificate-that-makes-this-decentralized))
is a *root-signed certificate* a device presents in order to obtain a `Grant`.
`[[auth.tailnet.mappings]]` (§3.5) is a *config table* mapping tailnet identities
to roles. They are never interchangeable.

Backends issue and verify. They perform no transport work and no routing (P1),
and authorization itself happens in capability dispatch, not in the transport —
so no surface can bypass it
([03 §3](03-capability-catalog.md#35-the-dispatch-path)).

### 3.2 Invite links (the primary onboarding path)

An invite is a signed, expiring, scoped, single-exchange token. It is *not* a
credential; it is a bearer of the right to *obtain* one.

```rust
pub struct Invite {
    pub v: u8,                        // format version
    pub instance: InstanceId,
    pub jti: InviteId,                // single-use marker, stored until expiry
    pub role: Role,
    pub scope: CredentialScope,       // §4.1; `visibility` is an InviteScope:
                                      // AllSessions | Workspaces(Vec<WorkspaceId>) | Sessions(Vec<SessionId>)
    pub issued_at: OffsetDateTime,
    pub expires_at: OffsetDateTime,   // default 24 h, max 7 d
    pub max_uses: u8,                 // default 1
}
// Wire form: base64url(CBOR(Invite)) || "." || base64url(Ed25519 sig over it)
```

- Signed with the instance's Ed25519 key (generated at first start, stored
  `0600` in the state dir, never transmitted).
- The link is `https://<host>:<port>/#/join?i=<token>` — **the token is in the
  fragment**, so it is never sent to the server in a request line and never
  lands in an access log or a `Referer`.
- Exchange (`join.exchange`,
  [07 §1.3](07-remote-protocol.md#13-adding-an-instance)) consumes `jti`,
  binds the resulting credential to the device's public key, and returns a
  long-lived bearer credential. Replaying the invite fails.
- `omt invite --role viewer --ttl 2h --workspace ~/src/api` mints a scoped,
  read-only, two-hour invite — the "let a colleague watch this agent" case.
- Invites are listed and revocable: `omt invite list`, `omt invite revoke <id>`.

**The capabilities.** Every CLI verb is a capability
([P3](01-principles.md#p3--parity-one-capability-three-surfaces)), so `omt
invite`, `omt invite list` and `omt invite revoke` are declared here rather than
existing only as CLI surface. All three are `Admin`: minting a credential-bearing
link is instance administration, not operation.

```rust
capability! {
    /// Mint an invite (§3.2). The token is returned once and never stored in
    /// recoverable form — only `jti`, `role`, `scope` and the expiry are kept,
    /// which is what makes revocation possible without keeping the secret.
    name  = "invite.create",
    group = "invite", verb = "create",
    kind  = Command, role = Role::Admin,
    input  = InviteCreate { role: Role, ttl: Option<Duration>,      // default 24 h, max 7 d
                            scope: CredentialScope, max_uses: Option<u8> },
    output = InviteCreateOut { id: InviteId, token: String, url: Url,
                               expires_at: OffsetDateTime },
    effects = [Effects::WRITES_FS],   // the `jti` record, retained until expiry
    since = "0.4",
}

capability! {
    /// List outstanding invites. Never returns a token — only the record.
    name  = "invite.list",
    group = "invite", verb = "list",
    kind  = Query, role = Role::Admin,
    input  = InviteList { include_expired: bool, include_consumed: bool },
    output = InviteListOut { invites: Vec<InviteRecord> },
    //        InviteRecord { id, role, scope, issued_at, expires_at,
    //                       max_uses, uses, consumed_by: Vec<DeviceRef> }
    effects = [],
    since = "0.4",
}

capability! {
    /// Revoke an invite by id. Idempotent; revoking a consumed invite does
    /// **not** revoke the credential it produced — that is `device.revoke`
    /// ([23 §12](23-identity-and-devices.md#12-capabilities)).
    name  = "invite.revoke",
    group = "invite", verb = "revoke",
    kind  = Command, role = Role::Admin,
    input  = InviteRevoke { id: InviteId },
    output = InviteRevokeOut { revoked: bool, was_consumed: bool },
    effects = [Effects::WRITES_FS, Effects::DESTRUCTIVE],
    since = "0.4",
}

capability! {
    /// Exchange an invite token for a long-lived credential
    /// ([07 §1.3](07-remote-protocol.md#13-adding-an-instance)). Consumes the
    /// invite's `jti`, binds the credential to `device.public_key`, and returns
    /// it exactly once. Replay fails because `jti` is already spent.
    name  = "join.exchange",
    group = "join", verb = "exchange",
    kind  = Command, role = Role::Admin,      // see the pre-auth note below
    input  = JoinExchange { token: String, device: DeviceRegistration },
    output = JoinExchangeOut { credential: String, role: Role,
                               scope: CredentialScope, instance: InstanceId,
                               registry: RegistrySnapshot },
    effects = [Effects::WRITES_FS],           // the credential record and the spent `jti`
    since = "0.4",
}
```

`join.exchange` is **reachable pre-authentication by construction — it *is* an
authentication path**, and it is the only entry point in the `invite`/`join`
groups that is. It is declared `Admin` for the same reason
`identity.recovery.use` is ([23 §12](23-identity-and-devices.md#12-capabilities)):
it carries `WRITES_FS`, which §4 makes a CI failure on a `Viewer` capability, so
the role declaration follows the effect bits and the pre-auth reachability is a
named, enumerated exemption in the dispatch chain rather than a weakened role.
The caller presents no credential; the *invite signature* is the authorization,
verified against the instance's Ed25519 key before the handler runs. Like every
pre-auth path it is **rate-limited** (per source address and per `jti`, with
exponential backoff on signature failures) and **audited loudly** — every
exchange, successful or not, is an audit record naming the invite, the device and
the outcome ([12 §8](12-collaboration.md#8-audit-log)).

### 3.3 Bearer tokens

Long-lived credentials, the output of an invite exchange or of
`omt token create`. Format `omt_c_<base62(32 bytes CSPRNG)>`. Stored **hashed**
(SHA-256; the token has full entropy so a KDF buys nothing) alongside its
metadata. Presented in the `auth` message, or as `Authorization: Bearer` for the
REST surface. Device-bound tokens additionally require an Ed25519 signature over
the server's `nonce`, so a token exfiltrated from a phone's storage is not
usable from elsewhere.

### 3.4 Username + password

For users who want to type a password into a browser rather than manage tokens.

- **argon2id**, `m = 64 MiB, t = 3, p = 1`, 16-byte salt, parameters stored with
  the hash so they can be raised later and rehashed on next login.
- Verification is constant-time; a wrong username performs a dummy hash so the
  timing does not distinguish.
- Rate limited: 5 failures per principal per 15 min and 20 per source address
  per 15 min, with exponential lockout; every failure is audited.
- Success yields the same `Grant` as any other backend, plus the short-lived
  resume `session_token` from
  [07 §3.4](07-remote-protocol.md#34-auth).
- Passwords are never accepted over a non-TLS, non-loopback connection —
  enforced at the transport, not by policy prose.

### 3.5 Tailnet identity

When the daemon can reach the Tailscale LocalAPI, a connection whose source
address is a tailnet address is resolved to `(node, user, tags)` via
`WhoIs`. This is a real identity assertion from the tailnet, not an IP
allowlist, and it is the most pleasant configuration by a wide margin: no tokens
to manage, revocation is a Tailscale ACL change.

```toml
[auth.tailnet]
enabled = true
# Map tailnet identity to an omt role. Unmatched identities are refused.
[[auth.tailnet.mappings]]
user = "vincent@example.com"
role = "admin"

[[auth.tailnet.mappings]]
tag  = "tag:ci"
role = "viewer"                       # Viewer cannot resolve interactions at all
```

Two guardrails: tailnet identity is only *offered* for connections whose peer
address is in the tailnet CIDR **and** whose `WhoIs` succeeds; and it is
disabled outright when the request arrived through Funnel (§10), because a
Funnel request originates from the public internet regardless of its apparent
source.

---

## 4. Roles and their mapping onto the catalog

```rust
pub enum Role { Viewer, Operator, Admin }   // ordered; defined in `omt-types`
```

Roles answer *who you shared this instance with*. The owner's own devices are
`Operator` or `Admin` and are therefore equivalent to sitting at the TUI
([D2](decisions.md#d2--remote-is-exactly-equivalent-to-local)). `Viewer` exists
so `omt invite --role viewer` can mint a link for a colleague to watch.

| Role | Can | Cannot |
|---|---|---|
| **Viewer** | read session list, terminal output, scrollback, blocks, files and diffs, agent state, presence, open interactions | write to a PTY, resolve interactions, change config, create/close sessions |
| **Operator** | everything Viewer can, plus PTY input, writer token, resolve **any** interaction the agent posed, create/close sessions, run agent commands, enqueue prompts | change instance config, manage credentials, install plugins, shut the instance down |
| **Admin** | everything | — |

There is no sub-`Viewer` role and no role tier between them. A credential that
needs a narrower surface than `Viewer` — the `omt ssh` clipboard bridge in
[09 §5.2](09-ssh-and-media.md#52-tier-1--the-reverse-socket-the-recommended-answer),
for instance — is a `Viewer` credential with an explicit **capability scope**
(§4.1), not a new role.

The mapping is mechanical, which is what makes it trustworthy:

- Every capability declares `role = Role::X`
  ([03 §2](03-capability-catalog.md#2-declaring-a-capability)). Dispatch compares
  the actor's role to the declaration. There is no per-handler authorization
  code to get wrong, and no handler may re-check or relax it.
- Every capability declares `effects`. Policy (§7) is expressed over `effects`
  bits, not over capability names, so a *new* capability that writes a PTY is
  covered by existing policy the day it is added rather than the day someone
  remembers to list it.
- A capability whose `role` is `Viewer` but whose `effects` include
  `WRITES_PTY`, `SPAWNS_PROCESS`, `WRITES_FS`, `NETWORK` or `DESTRUCTIVE` is a
  **CI failure**. The two declarations must be consistent, and the test enforces
  it across the whole catalog.

**The one carve-out: read-only subprocess.** `SPAWNS_PROCESS` *alone* does not
disqualify `Viewer`. The CI check exempts a capability when **all five** of the
following hold, and it fails the build if any one of them is missing:

1. `effects` contains `SPAWNS_PROCESS` and `READS_FS`, and **no** other effect
   bit — in particular none of `WRITES_PTY`, `WRITES_FS`, `NETWORK`,
   `DESTRUCTIVE`.
2. The declaration carries `spawn = Spawn::ReadOnly { program, argv_template }`.
   `program` is a fixed absolute path or a fixed program name resolved once at
   startup; it is never taken from the request.
3. The child is executed with `execve`-style fixed argv. **No shell**, no
   `sh -c`, no user-supplied string that is split into arguments. Request data
   may only fill named holes in `argv_template`, and each hole is one whole
   argument.
4. The child gets no stdin, its stdout/stderr are captured, and it is spawned
   with the instance's own uid and a cwd inside a workspace root — never with
   elevated privileges and never in a directory the caller named directly.
5. `argv_template` appears in the **read-only subprocess allow-list** checked
   into the repository next to the catalog test, so adding one is a reviewed
   diff rather than a property a handler can assert about itself.

The motivating case is the `workspace.vcs.*` family
([15 §6](15-workspace-explorer.md#6-capabilities)): `summary`, `status`,
`diff`, `diff_many` and `worktrees` shell out to `git` to read a repository's
state. Reading a repository is a `Viewer` operation by every other measure, and
forcing it to `Operator` would mean a read-only shared link could not see the
file tree — which is the explorer's entire premise. The bit that matters is
*mutation*, and a fixed-argv read-only `git` invocation performs none.

Anything that fails one of the five conditions is not covered: `remote.probe`
opens an outbound SSH connection and therefore declares `NETWORK`, so it is
`Operator` ([22 §10](22-operations.md#10-capabilities)), not `Viewer`.

### 4.1 Credential scope

Scoping is the only narrowing mechanism besides the role. It is deliberately
coarse, mechanical, and expressed over things omt owns — never over an agent's
tools.

```rust
pub struct CredentialScope {
    /// Which workspaces/sessions this credential can see at all.
    pub visibility: InviteScope,           // AllSessions | Workspaces(..) | Sessions(..)
    /// Optional allow-list of capability names or `group.*` globs. `None` = the
    /// full surface for the credential's role.
    pub capabilities: Option<BTreeSet<CapabilityPattern>>,
}
```

- A credential with `visibility = Workspaces([w1])` sees only `w1`'s sessions in
  every query, and calls targeting anything else return `not_found` (not
  `unauthorized` — an unauthorized answer confirms existence).
- `capabilities` is how a purpose-built credential is minted: the reverse-socket
  media token is `Viewer` + `capabilities = {media.clipboard.*, media.blob.*}`.
- Scope is enforced in dispatch alongside the role, so no surface can bypass it.
- Scope **never** varies by which device the owner is using. A narrowed
  credential is something the owner deliberately minted, usually for someone
  else.

---

## 5. Credential storage, rotation and TLS

### 5.1 At rest

```
$XDG_STATE_HOME/omt/
  instance.key        0600  Ed25519 private key (invite signing, TLS cert identity)
  credentials.db      0600  hashed tokens, argon2id password hashes, device pubkeys
  invites.db          0600  unexpired jti set
  audit/              0700  append-only audit log
  sessions/           0700  session tree, scrollback checkpoints, interaction ledger
```

- Secrets live **outside** the main config file (P8). `config.toml` may
  reference `auth.password_file` / `auth.token_file`; it may not contain a
  secret. A secret found inline is a validation error with a fix suggestion.
- Permissions are checked at startup and *corrected* if too permissive, with a
  warning; a directory owned by another uid is fatal.
- On macOS, the instance key may optionally be stored in the Keychain
  (`auth.key_storage = "keychain"`); the file backend is the default because it
  works headless and over SSH.

### 5.2 Rotation

| Secret | Rotation |
|---|---|
| bearer credential | `omt token rotate <id>` issues a replacement and keeps the old one valid for a 15 min overlap so a connected device rolls over without a re-auth prompt |
| password | `omt passwd`; invalidates all `session_token`s for that principal, keeps bearer credentials (they are separate grants) |
| instance key | `omt instance rotate-key`; invalidates **all outstanding invites** (they are signature-verified) and leaves credentials intact |
| resume `session_token` | automatic, 12 h TTL, re-issued on each successful auth |

Revocation is immediate and pushed: `is_revoked` is checked on reconnect and
every 60 s on live connections, and a revoked credential's connections are
closed with `Close { code: "revoked" }`.

### 5.3 TLS

Three supported configurations, in order of recommendation:

1. **Tailscale `serve`** (recommended). omt binds loopback; `tailscale serve
   https / http://127.0.0.1:7878` terminates TLS with a real, tailnet-valid
   Let's Encrypt certificate for `<host>.<tailnet>.ts.net`. No certificate
   management, no browser warnings, no public exposure. This is the intended
   deployment and the one the documentation leads with.
2. **Reverse proxy** (Caddy/nginx/Traefik). omt binds loopback with
   `tls.trust_proxy = true` plus an explicit `tls.trusted_proxies` CIDR list —
   `X-Forwarded-For` and `X-Forwarded-Proto` are honoured **only** from those
   addresses, and ignored otherwise. Without the explicit list, forwarding
   headers are ignored entirely, which is the safe default and a common
   misconfiguration elsewhere.
3. **Built-in self-signed TLS** (`tls.mode = "self_signed"`). omt generates a
   cert from the instance key and prints its SPKI SHA-256 pin. Usable, but the
   browser warning trains bad habits and WebCrypto/service-worker features
   degrade under an untrusted certificate — hence third place, and the docs say
   so.

Plain HTTP is permitted **only** for loopback and unix-socket binds.

---

## 6. Browser-side controls

The web client is served by the daemon, and the daemon must assume the user's
browser is simultaneously running hostile pages.

- **Origin checks.** Every WebSocket upgrade and every REST request is checked
  against `allowed_origins` (default: the origins the instance is served from).
  A missing or unlisted `Origin` on a state-changing request is rejected `403`.
  This is the primary defence against a page at `evil.example` opening
  `ws://127.0.0.1:7878` — browsers send `Origin` on WebSocket handshakes and
  do not allow it to be forged.
- **No ambient credentials.** Authentication is a bearer token in the protocol,
  held in memory and (for PWA persistence) in IndexedDB — **never a cookie**.
  With no cookie there is no ambient authority, and classic CSRF does not apply
  to the WebSocket API at all. The only cookie-shaped surface is the static
  bundle, which is unauthenticated and inert.
- **CSRF token for the REST surface.** Where a browser form-style request is
  unavoidable (in v1, only the OAuth-less login POST), a double-submit token
  bound to the connection nonce is required. There is no push-subscription
  endpoint: [D12](decisions.md#d12--no-push-notifications-in-v1-open-and-replay-instead)
  ships no notification backend.
- **CSP** on the served bundle: `default-src 'self'; connect-src 'self'
  wss://<self>; img-src 'self' blob: data:; script-src 'self'; frame-ancestors
  'none'; base-uri 'none'; form-action 'self'`. No inline script, no CDN. Plus
  `X-Content-Type-Options: nosniff` and `Referrer-Policy: no-referrer`.
- **`frame-ancestors 'none'`** — omt is never embeddable. Clickjacking a
  "Approve" button on a permission card is a realistic attack and framing has no
  legitimate use here.
- **Service worker scope** is the app root. There is no push subscription to
  bind or revoke: [D12](decisions.md#d12--no-push-notifications-in-v1-open-and-replay-instead)
  ships no notification backend
  ([07 §8](07-remote-protocol.md#8-notifications-to-a-closed-tab--none-in-v1)), so
  a revoked device simply stops being able to connect
  ([23 §6.1](23-identity-and-devices.md#61-revoking-one-device)).

---

## 7. The two high-consequence capabilities

### 7.1 Remote PTY write

`session.write_bytes` on a remote connection is remote code execution as the
user, by design. Controls:

- `Operator` minimum, plus the writer token
  ([12 §3](12-collaboration.md#3-the-writer-token)) — so remote input is always
  visible to anyone at the machine, and always attributable.
- Every acquisition, takeover and forced takeover is audited with the actor,
  device and peer address.
- Idle release (90 s) bounds how long a forgotten device holds input rights.

A credential that should never type can be minted `Viewer`, or `Operator` with a
capability scope excluding `session.send_*`/`session.write_bytes` (§4.1). That is
a **sharing** configuration. It is *not* the default for the owner's own phone:
`omt invite --phone` mints a full `Operator` credential, because a phone the
owner holds is the same authority as the laptop keyboard
([D2](decisions.md#d2--remote-is-exactly-equivalent-to-local)). Offering a
degraded default here would recreate exactly the second-class mobile experience
the product exists to remove.

### 7.2 Remotely resolving an agent interaction

**This is the highest-consequence capability in the product**, and it is the
single feature the product exists for. A tap on a phone can approve the same
thing pressing `y` at the keyboard would approve.

**omt adds no gate here.** Per
[D1](decisions.md#d1--omt-adds-no-policy-layer-over-an-agents-permission-semantics):

1. **omt mirrors the agent's own permission gate exactly.** If the CLI asked,
   omt surfaces the question on every surface with the agent's own options,
   verbatim, including `allow_always` / `deny_always` when the agent offered
   them. If the CLI did not ask, omt does not invent a prompt.
2. **There is no destructive classifier, no `deny_destructive_approval`, no
   `approvable_tools` allow-list, and no auto-approve.** Those would be a second
   permission model with its own configuration, its own bugs, and — worst — a
   false sense that omt is protecting the user in cases it cannot see. Danger
   classification belongs to the agent CLI, which has the tool schema, the
   project settings and the user's own `permissionMode`.
3. **What omt *does* show is the agent's own posture, read-only.** The session
   card names the agent's active permission mode (`default`,
   `acceptEdits`, `bypassPermissions`, `--dangerously-skip-permissions`) on
   every surface, so a user can see that a session will not ask before it stops
   asking. Observing is P4; overriding would not be.

The controls that remain are all about *presentation fidelity and attribution*,
and they apply identically on the TUI, the web client and the CLI:

- **Full command text, never truncated, in the approval UI.** A permission card
  that elides the middle of a command is an attack surface. Long commands
  scroll; they do not ellipsize. See
  [08 §5.3](08-web-client.md#53-permission--approval-cards--kindtype--permission).
- **A deliberate gesture on touch.** The web client's Allow control is a
  press-and-hold rather than a bare tap, because a mistap on a phone is a real
  input-model hazard. This is an ergonomic property of the touch surface, not a
  policy: it does not depend on what the tool does, it is applied uniformly to
  every permission card, and it never changes which options are offered or
  their order.
- **Exactly-once resolution**, by exactly one actor, broadcast everywhere
  ([12 §4](12-collaboration.md#4-interaction-ownership)).
- **Everything is audited**: interaction id, tool, the full tool input (redacted
  per §8), the decision, the actor, the device, the peer address, and the
  latency from open to resolution. Attribution is the control, and it is enough,
  because the authority being exercised is authority the user already has.
- **Nothing is auto-resolved by omt.** The only non-human resolution is
  `Cancelled { Timeout }` when the agent's own deadline expires
  ([12 §4.3](12-collaboration.md#43-timeouts)), which is the agent's default
  behaviour, not an omt decision.

`Viewer` credentials cannot resolve interactions at all (§4) — that is the
sharing boundary, and it is the whole of the interaction-authorization model.
There is no `CredentialPolicy` type.

---

## 8. Secret redaction

**The detector is not specified here.** The `Redactor`, its classes, its key /
flag / header / shape / entropy rules, its thresholds, its `<redacted:…:len=N>`
marker format, its configuration and its test corpus are
[21 §2](21-data-lifecycle.md#2-redaction-before-write)'s, and that section also
defines the *scope* of redaction: all persisted terminal content, not only
telemetry. There is exactly one implementation and one rule set; duplicating it
here is how the two would drift.

What this document owns is the **integration**, which is the part that carries a
security guarantee:

> The redactor is a `Layer` in the tracing stack **and** a serializer wrapper on
> the event bus. Every structured value passes through it before serialization,
> so **there is no path that emits an unredacted value by forgetting to call
> it.** Redaction is not something a call site opts into.

Two consequences of that placement:

- **Env maps** are redacted key-first and are never logged in full at any level
  below `trace`; `trace` refuses to log env at all unless `OMT_LOG_SECRETS=1` is
  set, which is documented as a debugging footgun and prints a warning banner on
  startup.
- The `Layer` and the serializer wrapper ship a shared fuzz target (P5) against
  21 §2's corpus of real-shaped-but-fake credentials, so the integration is
  tested with the same inputs as the detector.

Known limits, stated honestly: a secret in an unusual format inside a tool call's
free-text argument will not be caught, and the *live* terminal stream is never
redacted — a secret echoed into a terminal is on the screen, and hiding it would
corrupt the terminal. The **durable** copy of that stream is redacted, on the
write path, per [21 §2](21-data-lifecycle.md#2-redaction-before-write).

---

## 9. Network egress and supply chain

### 9.1 Egress

omt makes **zero** outbound connections by default. There is no telemetry, no
crash reporting, no analytics, and nothing is contacted unless the user asked for
it ([00 §8](00-overview.md#8-what-omt-is-not)). The table below is the complete
list of *features* that can cause egress once enabled — the mechanical check is
the `NETWORK` effect bit in the capability catalog
([03 §2](03-capability-catalog.md#2-declaring-a-capability)), and CI asserts that
every capability that opens a socket declares it, so this table cannot silently
fall behind the code.

**Third-party destinations.** Everything here talks to a machine that is not the
user's:

| Feature | Capability | Destination | Default |
|---|---|---|---|
| STT provider | — | Deepgram / OpenAI | off (local whisper.cpp available) |
| Update check | `upgrade.check` ([22 §10](22-operations.md)) | the release host | **off** — no automatic check; the capability must be called |
| Update install | `upgrade.apply` ([22 §10](22-operations.md)) | the release host | off; explicit, and the artifact's checksum is verified (§9.2) |
| Self-signed cert ACME (if ever added) | — | Let's Encrypt | not implemented |

**Notification egress does not appear in this table**, because in v1 there is
none. Web Push (a browser vendor's relay) and webhook notifiers (`ntfy`, Gotify,
Telegram) were both analysed here and both are removed by
[D12](decisions.md#d12--no-push-notifications-in-v1-open-and-replay-instead): a
daemon-initiated connection to a third party leaks *"this machine needs its owner,
now"* even under an encrypted payload, which contradicts the no-required-egress
position. That analysis is retained as the rationale a future `Notifier` plugin
must answer (§10.7); it is not a live mechanism, and no `Notifier` backend ships.

**Instance-to-instance traffic within the user's own registry.** This is a
distinct class and is not "phoning home": the destination is another machine the
user owns and enrolled, over the transport they configured, and the traffic
carries the user's own identity and device state
([23](23-identity-and-devices.md)). It is listed because it is still egress and a
reviewer must be able to see it, not because it is the same kind of risk.

| Feature | Capability | Destination | Default |
|---|---|---|---|
| Identity key rotation, pushed to the user's other instances | `identity.rotate_key` ([23 §12](23-identity-and-devices.md)) | every enrolled instance | on demand only |
| Device revocation | `device.revoke`, `device.revoke_all` ([23 §12](23-identity-and-devices.md)) | every enrolled instance | on demand only |
| Registry home election | `instance.registry.set_home` ([23 §12](23-identity-and-devices.md)) | the enrolled instances | on demand only |
| Registry sync | `instance.registry.sync` ([23 §12](23-identity-and-devices.md)) | the enrolled instances | on demand, **plus** a recurring poll — see below |
| Revocation poll | — (daemon-initiated) | the **home instance** only | **on whenever more than one instance is enrolled**: every 15 minutes ([23](23-identity-and-devices.md)) |
| Remote host probe / bootstrap | `remote.probe`, `remote.bootstrap` ([22 §10](22-operations.md)) | the SSH host the user named | on demand only |

The 15-minute revocation poll is the one entry that is **not** strictly
on-demand, and it is called out rather than buried: a revoked device must stop
working on every instance within minutes, and that requires someone to ask. A
single-instance install never makes it. It is the only recurring outbound
connection in the product, and it goes to a machine the user owns.

`instance.health` reports which egress paths are enabled, including whether the
revocation poll is running and to which instance, so "what can this thing phone
home to?" is a query, not an audit of the config file.

### 9.2 Supply chain

- **Dependency policy**: `cargo-deny` in CI for licenses, advisories, bans and
  duplicate versions; `cargo-audit` on a schedule; a `Cargo.lock` committed for
  the binaries.
- **Vetting**: `cargo-vet` with an explicit trusted-publisher set. New
  dependencies in the crates that parse untrusted input (`omt-term`,
  `omt-proto`, `omt-config`) require review and a fuzz target
  ([P5](01-principles.md#p5--production-grade-from-the-first-commit)).
- **Minimal `omt-hook`**: the binary installed into agent hook configs has a
  deliberately tiny dependency set. It runs on every agent turn, and it is the
  most attractive persistence target in the system.
- **Hook installation is merge, not overwrite.** omt never rewrites a user's
  `~/.claude/settings.json` wholesale; it merges its entry, backs up the
  original, and `omt integration status` shows exactly what it added. A tool
  that silently owns your hook config is indistinguishable from malware.
- **Reproducible builds and signed releases**: release artifacts are built in
  CI, checksummed, and signed; `omt --remote` verifies the checksum of any
  binary it installs on a remote host (another tool does not, and that is a real gap).
- **Web bundle integrity**: the bundle is embedded in the binary, not fetched.
  The CSP forbids external script entirely (§6).
- Provenance and license obligations with respect to studied code are
  [14 — Licensing and provenance](14-licensing.md)'s subject.

---

## 10. Checklist — publishing an instance over Tailscale Funnel

Funnel puts your instance on the **public internet**. This checklist is a gate:
`omt serve` detects an active Funnel for its port and refuses to start unless
items 1–5 are satisfied, printing the failing item.

1. **An auth backend is configured** and it is *not* tailnet identity. Funnel
   requests arrive from Tailscale's infrastructure, so tailnet identity is
   meaningless and is automatically disabled for Funnel-sourced connections
   (§3.5). Use password or bearer.
2. **Password auth uses argon2id with a password of ≥ 12 characters**, or bearer
   tokens are device-bound. Checked at config load.
3. **Rate limiting and lockout are enabled** (default on) for the auth endpoint.
4. **`allowed_origins` is set to the Funnel hostname exactly** — not `*`, which
   the config validator rejects outright.
5. **A default role of `Viewer`**, so an unscoped credential cannot type into a
   shell by accident. Raising it is per-credential and deliberate.

Then, recommended and not enforced:

6. Mint per-device credentials; never share one token across devices.
7. Mint every credential you hand to someone else at the narrowest role and
   workspace scope that still does its job (§4.1). Your own devices stay
   `Operator`/`Admin` (D2).
8. Set invite TTLs to hours, not days, and revoke used invites.
9. Review `audit.query` after the first day of exposure; it is the only way to
   notice a credential being used from somewhere unexpected.
10. Prefer `tailscale serve` (tailnet-only) over `funnel` (public) unless you
    genuinely need access from a device that cannot join the tailnet. **The
    honest recommendation is: do not use Funnel for omt.** Every use case we
    know of is served by `serve` plus the Tailscale app on the phone, at strictly
    lower risk. Funnel is supported because someone will need it; it is not
    recommended, and the docs say exactly this sentence.

---

## 11. OPEN QUESTIONS

1. **Surfacing the agent's own permission posture (§7.2 rule 3) needs a
   normalized shape.** Each CLI names its modes differently
   (`permissionMode`, `--dangerously-skip-permissions`, ACP's
   `session/set_mode`, Codex's approval policy). omt must read and display them
   without normalizing away meaning, and must not imply omt is enforcing them.
   Coordinate with [06 — Agent layer](06-agent-layer.md).
   *(Recorded alternative, rejected by D1: an omt-side destructive classifier
   that parses `Bash` command lines and refuses remote approval of matches. It
   was rejected because it measures consequence rather than failure mode, is
   defeated by `$(…)`/`eval`/aliases, and creates false confidence. Not to be
   reintroduced without amending D1.)*
2. **Device binding on the web** requires non-extractable WebCrypto keys in
   IndexedDB. Survivability across iOS Safari storage eviction is unmeasured; if
   eviction is common, binding degrades to "re-auth occasionally", which changes
   the threat model for a stolen token.
3. **Should `Viewer` be able to read scrollback at all?** Terminals contain
   secrets, and "read-only" is not "harmless". A `ViewerRedacted` role that sees
   agent state and interactions but not raw terminal output may be the more
   useful default for shared invites.
4. **Per-workspace credentials and `not_found` masking** (§4) leak existence
   through timing and through aggregate counts in `instance.health`. Probably
   acceptable; not yet analysed.
5. **Audit log integrity.** It is append-only by convention, on a filesystem the
   attacker (§1.3 case 1) can write. A hash chain would make tampering
   detectable but not preventable. Worth it? Cheap to add, so probably yes —
   decide before v1.
6. **Multi-user instances.** Everything here assumes one human owns the machine
   and shares access. A genuinely multi-user instance (a team dev box, several
   uids) needs per-principal session ownership, which the session model does not
   currently have. Coordinate with
   [05 — Session model](05-session-model.md) and
   [12 §9](12-collaboration.md#9-open-questions).
7. **Push payload metadata leak — retired.** The risk was that even a minimal
   pointer-plus-short-title payload would tell a push vendor that a session on a
   named instance needs attention, and when.
   [D12](decisions.md#d12--no-push-notifications-in-v1-open-and-replay-instead)
   removes push from v1 outright
   ([07 §8.1](07-remote-protocol.md#81-the-decision)), so no such payload is ever
   produced and the daemon makes no outbound connection at all. The `Notifier`
   trait remains **specified and reserved with zero implementations**
   ([07 §8.3](07-remote-protocol.md#83-the-reserved-extension-point)) for a future
   native app or a user plugin; if one is ever registered, the metadata argument
   above is the analysis it must answer, and a plugin's outbound connection is the
   user's own choice, disclosed as such.
8. **Interaction resolution by plugins.** A plugin acting as an actor with an
   `Operator` role could resolve interactions programmatically. That is a
   legitimate automation feature and an obvious abuse path. D1 forbids omt
   classifying the *decision*; the open question is whether a plugin should need
   a distinct consent scope (`resolve_interactions`) beyond
   `capabilities = ["interaction.resolve"]`, and whether plugin-resolved cards
   must be visibly attributed to the plugin on every surface (leaning: yes to
   both). Coordinate with [11 — Plugin system](11-plugins.md).
