# Security Model

`omt` gives a phone the ability to type into a shell on your workstation and to
approve an agent's tool calls. That is a large amount of authority, and this
document is where it is bounded.

It is the specification behind
[P8](01-principles.md#p8--security-by-default-no-ambient-trust) and the
security-facing half of `omt-auth` and `omt-server`.

Related: [03 — Capability catalog](03-capability-catalog.md) ·
[07 — Remote protocol](07-remote-protocol.md) ·
[12 — Collaboration](12-collaboration.md) ·
[14 — Licensing and provenance](14-licensing.md)

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
5. **A low-privilege collaborator.** Roles and per-credential policy bound what
   a shared credential can do, including *which kinds of agent decisions it may
   make*.
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
5. **The agent's own model behaviour.** omt does not evaluate whether a tool
   call is safe; it surfaces it and records your decision.
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
    /// Stable id, e.g. "bearer", "password", "invite", "tailnet".
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
    async fn is_revoked(&self, cred: &CredentialId) -> Result<bool, AuthError>;
}

pub struct Grant {
    pub credential: CredentialId,
    pub principal: PrincipalId,          // a human or a device
    pub role: Role,
    pub policy: CredentialPolicy,        // §7
    pub expires_at: Option<OffsetDateTime>,
    pub device_bound: Option<DevicePubKey>,
}
```

Backends issue and verify. They perform no transport work and no routing (P1),
and authorization itself happens in capability dispatch, not in the transport —
so no surface can bypass it
([03 §3](03-capability-catalog.md#3-dispatch)).

### 3.2 Invite links (the primary onboarding path)

An invite is a signed, expiring, scoped, single-exchange token. It is *not* a
credential; it is a bearer of the right to *obtain* one.

```rust
pub struct Invite {
    pub v: u8,                        // format version
    pub instance: InstanceId,
    pub jti: InviteId,                // single-use marker, stored until expiry
    pub role: Role,
    pub policy: CredentialPolicy,
    pub scope: InviteScope,           // AllSessions | Workspaces(Vec<WorkspaceId>) | Sessions(Vec<SessionId>)
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
[[auth.tailnet.grants]]
user = "vincent@example.com"
role = "admin"

[[auth.tailnet.grants]]
tag  = "tag:ci"
role = "viewer"
policy = { interaction_kinds = [] }   # may resolve nothing
```

Two guardrails: tailnet identity is only *offered* for connections whose peer
address is in the tailnet CIDR **and** whose `WhoIs` succeeds; and it is
disabled outright when the request arrived through Funnel (§10), because a
Funnel request originates from the public internet regardless of its apparent
source.

---

## 4. Roles and their mapping onto the catalog

```rust
pub enum Role { Viewer, Operator, Admin }   // ordered
```

| Role | Can | Cannot |
|---|---|---|
| **Viewer** | read session list, terminal output, scrollback, blocks, agent state, presence, open interactions | write to a PTY, resolve interactions, change config, create/close sessions |
| **Operator** | everything Viewer can, plus PTY input, writer token, resolve interactions (subject to policy §7), create/close sessions, run agent commands, enqueue prompts | change instance config, manage credentials, install plugins, shut the instance down |
| **Admin** | everything | — |

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

Scoped credentials narrow further: a credential with
`scope = Workspaces([w1])` sees only `w1`'s sessions in every query, and calls
targeting anything else return `not_found` (not `unauthorized` — an unauthorized
answer confirms existence).

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
  unavoidable (the OAuth-less login POST and the push-subscription endpoint), a
  double-submit token bound to the connection nonce is required.
- **CSP** on the served bundle: `default-src 'self'; connect-src 'self'
  wss://<self>; img-src 'self' blob: data:; script-src 'self'; frame-ancestors
  'none'; base-uri 'none'; form-action 'self'`. No inline script, no CDN. Plus
  `X-Content-Type-Options: nosniff` and `Referrer-Policy: no-referrer`.
- **`frame-ancestors 'none'`** — omt is never embeddable. Clickjacking a
  "Approve" button on a permission card is a realistic attack and framing has no
  legitimate use here.
- **Service worker scope** is the app root, and the push subscription is bound
  to a device credential, so a revoked device stops receiving notifications
  (§7 of [07](07-remote-protocol.md#8-notifications-to-a-closed-tab)).

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
- A credential may be minted with `deny_pty_write: true` while still being an
  `Operator` for interactions — the "let my phone answer questions but never
  type" configuration, which is a very reasonable default for a phone and is
  what `omt invite --phone` produces.
- Idle release (90 s) bounds how long a forgotten device holds input rights.

### 7.2 Remotely approving an agent permission prompt

**This is the highest-consequence capability in the product.** A tap on a phone
can approve `rm -rf`, a `curl | sh`, a force-push, or a write to `~/.ssh`. It is
also the single feature the product exists for, so the answer is not to remove
it but to bound it precisely.

```rust
pub struct CredentialPolicy {
    /// Which interaction kinds this credential may resolve at all.
    /// Empty = may resolve none (read-only for decisions).
    pub interaction_kinds: BTreeSet<InteractionKindTag>,   // Choice | Text | Permission | PlanReview

    /// Refuse to *approve* a Permission interaction whose underlying tool call
    /// is classified destructive. Denying is always allowed.
    pub deny_destructive_approval: bool,

    /// Optional allow-list of tool names this credential may approve.
    /// `None` = any tool (subject to the flag above).
    pub approvable_tools: Option<BTreeSet<String>>,

    /// Require an explicit second confirmation gesture for these classes.
    pub confirm_twice: BTreeSet<DestructiveClass>,

    /// Never applies to interactions in these workspaces.
    pub workspace_scope: InviteScope,

    pub deny_pty_write: bool,
}
```

**The destructive classifier.** A `Permission` interaction carries the tool name
and its input ([`agent-clis` §12.2](../research/agent-clis.md)). The agent layer
classifies it before it is offered remotely:

| Class | Examples |
|---|---|
| `FileDelete` | `rm`, `Bash` with `rm -rf`, `Write` to a path outside the workspace |
| `HistoryRewrite` | `git push --force`, `git reset --hard`, `git clean -fdx` |
| `CredentialAccess` | any path under `~/.ssh`, `~/.aws`, `~/.config/gh`, `*.pem`, `.env` |
| `NetworkExec` | `curl`/`wget` piped to a shell, `npm install` of an unpinned package |
| `Privilege` | `sudo`, `doas`, `chmod +s` |
| `Unclassified` | everything the classifier does not recognise |

Design rules, stated as invariants:

1. **The classifier is conservative and its failure mode is `Unclassified`, not
   `safe`.** `deny_destructive_approval` treats `Unclassified` as destructive
   when `strict_unclassified = true` (the default for phone credentials).
2. **Denying is never restricted.** Any credential that can see an interaction
   may deny it. Policy only ever constrains *approval*. A locked-down credential
   must still be able to stop a runaway agent — that is a safety feature, not a
   privilege.
3. **"Always allow" is never remotely grantable.** `AllowAlways` /
   `DenyAlways`-style suggestions change future behaviour, not just this call. A
   remote credential may answer `Allow` or `Deny` for *this* invocation only;
   persistent rules require `Admin` and are, by configuration default, local-only.
4. **Second confirmation is a distinct gesture.** For `confirm_twice` classes the
   web client requires a hold-to-confirm (600 ms) on a differently-coloured
   control, with the command text shown in full and unwrapped. Not a second tap
   in the same place — that is defeated by the muscle memory it is meant to
   interrupt.
5. **Full command text, never truncated, in the approval UI.** A permission card
   that elides the middle of a command is an attack surface. Long commands
   scroll; they do not ellipsize.
6. **Everything is audited**: interaction id, tool, classified class, the full
   input (redacted per §8), the decision, the actor, the device, the peer
   address, and the latency from open to resolution.
7. **Policy is enforced server-side.** The client receives `policy` at auth
   (07 §3.4) to render the right affordances, but a client that ignores it and
   sends a resolution anyway gets `unauthorized`, and the attempt is audited.

The default policy for a credential minted by `omt invite --phone`:

```toml
interaction_kinds        = ["choice", "text", "permission"]
deny_destructive_approval = true
strict_unclassified       = true
confirm_twice             = ["file_delete", "history_rewrite", "network_exec"]
deny_pty_write            = true
```

That is: answer questions freely, approve ordinary tool calls, never approve
anything destructive or unrecognised from a phone, and never type into a shell.
A user who wants more must say so explicitly, per device.

---

## 8. Secret redaction

Redaction applies to **logs, audit entries and events** — the three places data
leaves its original context. It does not and cannot apply to the terminal
stream itself: a secret echoed into a terminal is on the screen, and hiding it
would corrupt the terminal.

The redactor runs on every structured value before serialization:

1. **Key-name rules** — any map key matching
   `(?i)(pass(word|wd)|secret|token|api[-_ ]?key|authorization|cookie|private[-_ ]?key|credential|session[-_ ]?id)`
   has its value replaced with `"<redacted:key>"`.
2. **Value-shape rules** — high-entropy strings matching known credential
   shapes: `sk-[A-Za-z0-9]{20,}`, `gh[pousr]_[A-Za-z0-9]{36}`, `AKIA[0-9A-Z]{16}`,
   `xox[baprs]-…`, JWTs (`eyJ[\w-]+\.[\w-]+\.[\w-]+`), `-----BEGIN … PRIVATE
   KEY-----` blocks, and omt's own `omt_c_…` / `omt_s_…`.
3. **Environment** — env maps are redacted key-first and never logged in full at
   any level below `trace`; `trace` refuses to log env at all unless
   `OMT_LOG_SECRETS=1` is set, which is documented as a debugging footgun and
   prints a warning banner on startup.
4. **Length-preserving markers** — replacement is `<redacted:sk:len=51>`, so a
   bug report remains diagnosable without carrying the secret.

The redactor is a `Layer` in the tracing stack **and** a serializer wrapper on
the event bus, so there is no path that emits an unredacted value by forgetting
to call it. It ships a fuzz target (P5) and a corpus of real-shaped-but-fake
credentials.

Known limits, stated honestly: a secret in an unusual format inside a tool
call's free-text argument will not be caught, and PTY bytes are never redacted.

---

## 9. Network egress and supply chain

### 9.1 Egress

omt makes **zero** outbound connections by default. There is no telemetry, no
update check, no crash reporting, no analytics
([00 §8](00-overview.md#8-what-omt-is-not)). The complete list of things that can
ever cause egress, each off by default and each requiring explicit
configuration:

| Feature | Destination | Default |
|---|---|---|
| Web Push | browser vendor push endpoint | off |
| Webhook notifications (`ntfy`/Gotify) | user-specified URL | off |
| STT provider | Deepgram / OpenAI | off (local whisper.cpp available) |
| Self-signed cert ACME (if ever added) | Let's Encrypt | not implemented |

`instance.health` reports which egress paths are enabled, so "what can this
thing phone home to?" is a query, not an audit of the config file.

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
7. Set `deny_destructive_approval = true` and `deny_pty_write = true` for every
   credential that does not need them (§7).
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

1. **The destructive classifier (§7.2) is the weakest link.** It must parse
   shell command lines to classify `Bash` tool calls, and shell is adversarial to
   parse (`$(…)`, aliases, `eval`, obfuscated paths). Is a conservative
   deny-by-default on anything containing command substitution acceptable
   usability? Needs a corpus and real use. Coordinate with
   [06 — Agent layer](06-agent-layer.md).
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
7. **Push payload metadata leak.** Even a pointer-only payload
   (07 §8.1) tells the push vendor that a session on a named instance needs
   attention, and when. The `ntfy`-on-tailnet path avoids it entirely; should
   that be the *default* rather than the alternative?
8. **Interaction policy for plugins.** A plugin acting as an actor with an
   `Operator` role could resolve interactions programmatically. That is a
   legitimate automation feature and an obvious abuse path. It probably needs
   its own policy class rather than reusing `CredentialPolicy`. Coordinate with
   [11 — Plugin system](11-plugins.md).
