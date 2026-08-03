# Research — Connectivity, Tailscale, Push and Mobile Resilience

Background research for the flagship omt deployment: *publish the instance on a
Tailscale tailnet, then drive your agents from your phone*.

Cross-references: [07 — Remote protocol](../architecture/07-remote-protocol.md) ·
[13 — Security model](../architecture/13-security.md) ·
[08 — Web client](../architecture/08-web-client.md)

**Confidence markers used throughout:**

- **VERIFIED** — checked on this machine (macOS 15 / Darwin 25.1, Tailscale
  1.98.9, `go1.26.5`) with the command shown. Output reproduced or paraphrased.
- **DOCUMENTED** — stated by vendor documentation, an RFC, or the upstream
  source repository.
- **INFERRED** — my reasoning from the above. Not tested. Treat as a hypothesis
  that needs a spike.

---

## 1. Tailscale integration

### 1.1 The four options, honestly compared

| Option | Daemon work | TLS | Identity | Failure mode | Verdict for omt |
|---|---|---|---|---|---|
| **(A) Bind the tailnet IP directly** | trivial: resolve the 100.64/10 address, `bind()` | omt must supply a cert (`tailscale cert`, or self-signed) | `WhoIs` via LocalAPI on the peer address | tailscaled down → no address to bind | **ship** — needed anyway for QUIC/WebTransport |
| **(B) `tailscale serve` in front of loopback** | almost none: bind `127.0.0.1:7878`, tell the user one command | free, automatic, real LE cert for `<host>.<tailnet>.ts.net` | identity **headers** injected by serve | user must run/persist the serve config | **ship as the recommended default** (matches 13 §5.3) |
| **(C) `tailscale funnel`** | same as (B) plus a hard gate | same | **none** — request comes from the public internet | public exposure | supported, actively discouraged (13 §10) |
| **(D) Embed tsnet / libtailscale / tailscale-rs** | large: cgo archive, or a sidecar, or an immature pure-Rust stack | own cert story | in-process `WhoIs` | build/maintenance burden, DERP-only in the Rust option | **do not ship in v1** |

The interesting design consequence is that (A) and (B) are not alternatives —
they are complementary. (B) gives the best browser experience (real cert, real
hostname, zero config) but is a **TCP/HTTP reverse proxy**, which means it
cannot carry QUIC. (A) is the only path to WebTransport (§3.3). omt should
support both and let the transport choice follow the deployment.

### 1.2 Relying on the user's existing tailscaled — option (A)

**VERIFIED** — `tailscale status --json` on this machine emits, at top level:

```json
{
  "Version": "1.98.9-t4fb758c39-g200941d74",
  "BackendState": "Running",
  "TailscaleIPs": ["100.100.1.3", "fd7a:115c:a1e0::cc01:8162"],
  "Self": {
    "ID": "nai3qcHtmW11CNTRL",
    "HostName": "A0AL0148",
    "DNSName": "hxd-work-mbp.tailc9e96c.ts.net.",
    "UserID": 3092742766943711,
    "Online": true,
    "Relay": "dbi",
    "CurAddr": "",
    "Capabilities": ["funnel", "https", "https://tailscale.com/cap/funnel-ports?ports=443,8443,10000", …],
    "CapMap": { "funnel": null, "https": null, … }
  },
  "Peer": { "<nodekey>": { …same shape per peer… } }
}
```

Fields omt should read, and what each is for:

- `BackendState` — one of `NoState`, `NeedsLogin`, `NeedsMachineAuth`,
  `Stopped`, `Starting`, `Running` (**DOCUMENTED**, `ipn/backend.go`). Anything
  other than `Running` means "tailnet unavailable"; the daemon must surface this
  in `instance.health` rather than silently falling back to loopback.
- `TailscaleIPs` — the exact addresses to bind for `BindSpec::Tailnet`. Note
  **both** a v4 and a v6 address; bind both or the phone-on-v6-only case breaks.
- `Self.DNSName` — trailing-dot FQDN. Strip the dot; this is the hostname the
  cert will be issued for and the origin the browser will use. It is also what
  `allowed_origins` should default to on a tailnet bind (13 §6).
- `Self.CapMap` — **VERIFIED** contains `"funnel"` and the
  `funnel-ports?ports=443,8443,10000` capability URL on this tailnet. This is
  how omt can tell, without trying, whether Funnel is even permitted and on
  which ports.
- `Peer[*].CurAddr` and `Peer[*].Relay` — **this is the DERP indicator**.
  `CurAddr` empty + a non-empty `Relay` (here `"dbi"`, a DERP region code) means
  traffic is relayed. A populated `CurAddr` (`ip:port`) means a direct
  WireGuard path. 07 §7 item 4 wants to surface relay status to the user; this
  is the field. **VERIFIED** shape, **INFERRED** that this is the right
  discriminator for a *client* connection (it is per-peer; for an inbound
  connection omt must look up the peer that owns the source address).

`tailscale status --json` is a shell-out. Prefer the LocalAPI (§1.4) —
`/localapi/v0/status` returns the same `ipnstate.Status` struct without
spawning a process, and the CLI itself is only a client of that endpoint
(**DOCUMENTED**). Tailscale explicitly warns the JSON shape "has changed
between releases and might change more in the future" (**VERIFIED**, printed in
`tailscale status --help`), so omt must parse defensively: a `serde` struct with
`#[serde(default)]` on everything and no `deny_unknown_fields`.

### 1.3 `tailscale serve` and `tailscale funnel` — exact behaviour

**VERIFIED** from `tailscale serve --help` / `tailscale funnel --help` on 1.98.9.

```
tailscale serve <target>
tailscale serve status [--json]
tailscale serve reset
```

Targets: a port (`3000`), a partial URL (`localhost:3000`), a full URL with a
path (`http://localhost:3000/foo`), a filesystem path, literal text, or — on
Unix — `unix:/tmp/myservice.sock`.

Flags that matter to omt (**VERIFIED**, both commands unless noted):

| Flag | Meaning |
|---|---|
| `--bg` | run as a background/persistent config rather than foreground. Without it the serve dies with the shell. |
| `--https <port>` | expose HTTPS on that port (**the default mode**) |
| `--http <port>` | plain HTTP (serve only; not offered by funnel) |
| `--tcp <port>` | raw TCP forwarder — no TLS termination, no identity headers |
| `--tls-terminated-tcp <port>` | Tailscale terminates TLS, forwards plaintext TCP |
| `--set-path <path>` | mount the backend under a sub-path of the base URL |
| `--proxy-protocol 1\|2` | PROXY protocol for TCP forwarding — how a TCP-mode backend learns the real client address |
| `--accept-app-caps <list>` | forward tailnet ACL *app capabilities* to the backend (serve only) |
| `--service <name>` | serve on a distinct virtual service IP rather than the node |
| `--yes` | funnel only: skip the interactive confirmation |

The canonical omt invocation:

```sh
# tailnet-only, persistent, TLS terminated by Tailscale
tailscale serve --bg --https 443 http://127.0.0.1:7878
# reachable at https://<host>.<tailnet>.ts.net/
```

**TLS termination.** `serve --https` terminates TLS at tailscaled using a
Let's Encrypt certificate for `<host>.<tailnet>.ts.net`, obtained through
Tailscale's ACME DNS-01 flow (**DOCUMENTED**). The backend sees plain HTTP on
loopback. This is why 13 §5.3 ranks it first: no cert management, no browser
warning, and — importantly for omt — no degraded WebCrypto/service-worker
behaviour, which is what kills the self-signed option for a PWA.

**WebSockets through serve.** serve is an HTTP reverse proxy and forwards
`Upgrade: websocket` (**DOCUMENTED** by usage; **INFERRED** that no
buffering/idle-timeout surprises exist — this needs a soak test with omt's 20 s
application ping before it is relied on, and is a concrete spike item).

**Funnel restrictions** (**VERIFIED** from the local CapMap, corroborated by
docs):

- Funnel listens on **only** ports **443, 8443, 10000**. There is no way to use
  another port. The node must hold the `funnel` node attribute, granted by a
  tailnet policy `nodeAttrs` entry with `"attr": ["funnel"]`.
- Funnel traffic ingresses through Tailscale's edge (`ingress` DERP-adjacent
  infrastructure) over TLS-SNI routing. The source address the backend sees is
  **not** the real client's; identity headers are **not** present.
- Consequence, which 13 §3.5 already states correctly: tailnet identity auth
  must be *disabled* for Funnel-sourced requests. omt can detect Funnel by
  polling `tailscale funnel status --json` (or the LocalAPI serve-config
  endpoint) and comparing the configured port to its own listener — this is
  exactly the gate 13 §10 describes.

**`tailscale cert`** (**VERIFIED** from `--help`):

```
tailscale cert [flags] <domain>
  --cert-file value      # or "-" for stdout; defaults to DOMAIN.crt
  --key-file value       # defaults to DOMAIN.key
  --min-validity value   # ensure validity for at least this duration
  --serve-demo           # serve on :443 with the cert instead of writing files
```

This is the path for option (A): omt binds the tailnet IP itself and needs a
real cert for `<host>.<tailnet>.ts.net`. Notes:

- The domain **must** be the node's own FQDN; you cannot mint a cert for a peer.
- HTTPS certs must be enabled for the tailnet in the admin console, and MagicDNS
  must be on (**DOCUMENTED**).
- Renewal is on the caller. `--min-validity 720h` plus a daily check is the
  usual pattern; omt should re-fetch on a timer and hot-reload rustls without
  dropping connections (**INFERRED** design; rustls `ResolvesServerCert` with an
  `ArcSwap` is the standard shape).
- The underlying LocalAPI endpoint is `/localapi/v0/cert/<domain>` and returns
  the PEM pair, so omt can do this without shelling out.

### 1.4 The LocalAPI — how to talk to tailscaled programmatically

**Platform difference, and it is a real trap.**

*Linux/BSD* (**DOCUMENTED**): a unix socket, default
`/var/run/tailscale/tailscaled.sock` (overridable with `TS_SOCKET`). HTTP over
the socket with a dummy `Host: local-tailscaled.sock`:

```sh
curl --unix-socket /var/run/tailscale/tailscaled.sock \
  'http://local-tailscaled.sock/localapi/v0/whois?addr=100.64.0.5:1234'
```

*macOS* — **VERIFIED on this machine**, and it is *not* a unix socket:

```
/Library/Tailscale/ipnport            -> symlink whose target is the TCP port ("49217")
/Library/Tailscale/sameuserproof-49217  -> file containing a shared secret
```

The API is a loopback TCP listener on that port, authenticated with HTTP Basic:
empty username, the sameuserproof token as the password. **VERIFIED** working:

```sh
PORT=$(readlink /Library/Tailscale/ipnport)
TOK=$(cat /Library/Tailscale/sameuserproof-$PORT)
curl -s -u "-:$TOK" "http://127.0.0.1:$PORT/localapi/v0/whois?addr=100.100.1.3:1234"
# -> {"Node":{"ID":…,"Name":"hxd-work-mbp.tailc9e96c.ts.net.",…},"UserProfile":{…},"CapMap":{…}}
```

The sameuserproof file is mode `0640 root:admin` here, so a non-admin user
cannot read it — omt must degrade gracefully (offer bearer/password auth
instead of tailnet identity) rather than failing to start. On the Mac App Store
build the path lives inside the app sandbox container instead, which is a third
variant. **Recommendation:** put all of this behind one
`TailnetProbe::detect()` that returns `Option<LocalApiClient>` and is
exhaustively unit-tested per platform, and never assume the unix socket.

Endpoints omt cares about (**DOCUMENTED**):

| Endpoint | Use |
|---|---|
| `GET /localapi/v0/status` | the `ipnstate.Status` of §1.2 |
| `GET /localapi/v0/whois?addr=<ip:port>[&proto=tcp]` | **the identity call** (§1.5) |
| `GET /localapi/v0/cert/<domain>` | TLS cert + key PEM |
| `GET /localapi/v0/serve-config` / `POST` same | read/modify the serve+funnel config programmatically |
| `GET /localapi/v0/prefs` | node prefs, incl. whether the node is shields-up |

**Rust crates:**

- `tailscale-localapi` (jtdowney) — a small typed client for status/whois/cert.
  Handles the unix socket; **INFERRED** it may not handle the macOS
  port+sameuserproof variant, which is worth checking before adopting.
- Otherwise: `hyper` + `hyper-util`'s `UnixConnector` is ~60 lines and gives omt
  control over the platform detection. Given that this sits on the auth path
  (13 §3.5), owning the code is defensible.

### 1.5 Identity — how the server learns *which tailnet user* is connecting

This is the single most valuable thing Tailscale gives omt: **password-free
authentication with real revocation.** Two mechanisms, and they are not
interchangeable.

**(a) `WhoIs` — for option (A), binding the tailnet IP directly.**

The server takes the accepted connection's peer address *including the port*
and asks the LocalAPI. **VERIFIED** response shape (abridged):

```json
{
  "Node": {
    "ID": 3812933083674659,
    "StableID": "nai3qcHtmW11CNTRL",
    "Name": "hxd-work-mbp.tailc9e96c.ts.net.",
    "User": 3092742766943711,
    "Addresses": ["100.100.1.3/32", "fd7a:115c:a1e0::cc01:8162/128"],
    "Hostinfo": { "Hostname": "A0AL0148", … },
    "MachineAuthorized": true,
    "Capabilities": ["https://tailscale.com/cap/is-admin", …],
    "Tags": null
  },
  "UserProfile": { "ID": …, "LoginName": "…", "DisplayName": "…", "ProfilePicURL": "…" },
  "CapMap": { … }
}
```

Mapping onto 13 §3.5's config:

- `UserProfile.LoginName` → the `user = "vincent@example.com"` grant key.
- `Node.Tags` (a `[]string` of `tag:…`, `null` when untagged) → the
  `tag = "tag:ci"` grant key. **Important:** a tagged node has **no user
  identity** — `UserProfile.LoginName` for a tagged node is a synthetic
  `tagged-devices` principal. omt must branch on `Tags` first and never treat a
  tagged node's login name as a human.
- `Node.StableID` is the right thing to record in the audit log: it survives
  key rotation and re-auth, whereas the IP does not.
- **The port matters.** `whois?addr=100.64.0.5` without a port works on this
  build (**VERIFIED** — `tailscale whois 100.100.1.3` succeeds) but the correct
  call for a connection is `ip:port`, because Tailscale can then distinguish
  which peer actually owns that flow. Always pass the port omt got from
  `accept()`.

Guardrails omt must implement (13 §3.5 already mandates the first two):

1. Only offer the `tailnet` method when the peer address is inside the tailnet
   CIDR (`100.64.0.0/10`, `fd7a:115c:a1e0::/48`) **and** `WhoIs` succeeded.
2. Disable it entirely for Funnel-sourced requests.
3. **Never trust `X-Forwarded-For`** when deriving the address for `WhoIs`.
   The address must come from the socket. This is the class of bug that turns
   identity into an open door.

**(b) Identity headers — for option (B), behind `tailscale serve`.**

When `serve` proxies HTTP to a backend, it injects (**DOCUMENTED**):

```
Tailscale-User-Login:       vincent@example.com
Tailscale-User-Name:        Vincent
Tailscale-User-Profile-Pic: https://…
```

With `--accept-app-caps`, ACL grant *app capabilities* are forwarded too, which
is a clean way to encode `role` in the tailnet policy file rather than in omt's
config.

**The security rule, and it is absolute:** these headers are only trustworthy
if the backend is reachable *exclusively* by tailscaled. A backend on
`0.0.0.0:7878` lets anyone on the LAN set `Tailscale-User-Login:` to whatever
they like and become admin. Tailscale's own docs say to bind localhost only;
there is at least one public CVE-shaped issue in another project from getting
this wrong (`denoland/clawpatrol#316`, "Do not trust `Tailscale-User-Login` from
arbitrary loopback proxies").

Concretely for omt, and this is a **recommendation the architecture docs do not
currently state explicitly**:

- Accept identity headers **only** when the listener is a loopback or unix
  bind, **and** `[auth.tailnet].trust_proxy_headers = true` is explicitly set,
  **and** the peer address is loopback. Three conditions, all required.
- Loopback alone is not sufficient (§1.3 of 13 already concedes a same-uid
  process can connect). So header-based identity should additionally cross-check
  against `serve-config` — i.e. omt verifies that a serve config actually points
  at its own port before honouring the headers. **INFERRED**, but cheap and it
  closes the "another local process pretends to be serve" gap.
- Emit a startup warning if `trust_proxy_headers` is on while any non-loopback
  `BindSpec` exists.

Note also that identity headers are populated for *external users who accepted
a share* of the device (**DOCUMENTED**). That is a real sharing story for
12 — Collaboration: `omt invite` is not the only way to give a colleague
access; a Tailscale node share plus a `[[auth.tailnet.grants]]` entry does it
with the tailnet as the revocation authority.

### 1.6 Embedding tsnet — assessment for a Rust process

Three sub-options, all with real costs.

**(i) `libtailscale` via cgo `c-archive`.** tsnet is Go. `libtailscale`
(tailscale/libtailscale, and badboy/passcod forks) compiles tsnet with
`-buildmode=c-archive` into `libtailscale.a` + a header, exposing a
socket-descriptor API (`tailscale_new`, `tailscale_up`, `tailscale_listen`,
`tailscale_dial`, returning real fds). The `tsnet` Rust crate (docs.rs, 0.1.0)
wraps this. Costs:

- The build now requires a Go toolchain and cgo for every target. Cross-
  compiling omt (a stated goal for a daemon users install on Linux boxes and
  Macs) becomes substantially harder; `cargo build` alone stops working.
- Binary size grows by roughly the whole Go runtime plus tailscale — tens of MB.
- Two runtimes in one process: the Go scheduler and tokio, each with their own
  signal handling, thread pools and GC pauses. Debugging a hang across that
  boundary is unpleasant.
- The Rust `tsnet` crate is 0.1.0 and thinly maintained.

**(ii) `tailscale-rs` — the pure-Rust reimplementation.** Published by
Tailscale in April 2026 as an explicit *preview* (**DOCUMENTED**, Tailscale
blog). Supports TCP/UDP to tailnet peers, interoperates with Go clients, ships
FFI for C/Python/Elixir and an axum helper crate. Blocking limitations, stated
by Tailscale themselves:

- **DERP-only.** No peer-to-peer, no NAT traversal — "all traffic to other
  devices goes through DERP and will have limited throughput." For omt this is
  disqualifying on its own: §5 shows DERP roughly doubles the latency budget,
  and 07 §7 item 4 makes *avoiding* DERP an explicit product goal.
- No DNS resolution (dial by IP), no exit nodes, no Tailscale SSH.
- "Do not use it in production yet." No external security audit.

Worth tracking. Re-evaluate when peer-to-peer lands.

**(iii) A tsnet sidecar process.** Run a small Go binary that holds the tsnet
node and hands omt connections over a unix socket (or proxies to omt's
loopback listener). This is the pragmatic form of embedding: no cgo, no runtime
mixing, and it can be shipped as an optional extra binary. But at that point it
is functionally `tailscale serve` with worse ergonomics and a second identity
to manage.

**Recommendation: do not embed in v1.** The value of embedding is "works
without the user installing Tailscale", and the target user of omt — someone
driving coding agents from a phone — is overwhelmingly likely to already run
Tailscale, since they need the app on the phone regardless. The cost is a
permanently harder build. Option (A)+(B) delivers the same user-visible outcome
for a fraction of the complexity. Revisit only if a "one binary, no
dependencies, ephemeral node" story becomes a product requirement.

### 1.7 ACLs and tags — what a sane omt deployment looks like

A minimal tailnet policy for the single-user case (**DOCUMENTED** syntax):

```jsonc
{
  "acls": [
    // Vincent's devices may reach the omt port on Vincent's devices.
    { "action": "accept", "src": ["autogroup:member"],
      "dst": ["autogroup:self:7878", "autogroup:self:443"] }
  ],
  "nodeAttrs": [
    // only if Funnel is genuinely needed; omit otherwise
    { "target": ["tag:omt-public"], "attr": ["funnel"] }
  ],
  "tagOwners": { "tag:omt": ["autogroup:admin"], "tag:ci": ["autogroup:admin"] },
  "grants": [
    // app capabilities forwarded by `serve --accept-app-caps`
    { "src": ["vincent@example.com"], "dst": ["tag:omt"],
      "app": { "example.com/cap/omt": [{ "role": "admin" }] } },
    { "src": ["tag:ci"], "dst": ["tag:omt"],
      "app": { "example.com/cap/omt": [{ "role": "viewer" }] } }
  ]
}
```

Guidance for the docs:

- **Do not tag the workstation running omt** unless you must. A tagged node has
  no user owner, so `WhoIs` gives you a tag rather than a human — which is fine
  for a CI box and wrong for the laptop you also want `Tailscale-User-Login` to
  identify.
- Use `autogroup:self` for the personal case: it restricts to devices owned by
  the same user, which is exactly "my laptop and my phone".
- Revocation is a Tailscale operation (remove the device, change the ACL, or
  revoke the user), which is the whole benefit: no token to hunt down.
- omt should nonetheless keep `is_revoked` polling (13 §5.2) meaningful for
  tailnet grants by re-running `WhoIs` on the 60 s check, because an ACL change
  will not by itself tear down an established TCP connection. **INFERRED but
  important** — this is a genuine gap: WireGuard-level ACL changes drop *new*
  packets in most cases, but omt should not rely on it.

### 1.8 Alternatives users will ask about

| | What it is | Latency | Identity to the server | Setup on phone | Verdict |
|---|---|---|---|---|---|
| **Plain SSH tunnel** (`ssh -L 7878:localhost:7878`) | port-forward over an existing SSH session | direct TCP, +1 hop through the SSH host | none (server sees loopback) | poor — needs an SSH client app holding a tunnel | fine for a laptop, bad for a phone. omt already has a *better* SSH story: the stdio bridge in 07 §2.4. |
| **Cloudflare Tunnel** (`cloudflared`) | outbound-only tunnel to Cloudflare's edge; public hostname or Zero Trust-gated | +1 CDN hop; typically 20–60 ms added | Cloudflare Access JWT in `Cf-Access-Jwt-Assertion` — verifiable, genuinely good | just a browser | the strongest non-Tailscale option. Worth documenting as a supported reverse-proxy configuration (13 §5.3 case 2) with Access as the auth backend. All traffic transits Cloudflare. |
| **ngrok** | same shape, commercial, ephemeral URLs on the free tier | similar | basic auth / OAuth on paid tiers | browser | fine for a demo, wrong for a persistent workstation. Rotating URLs break saved PWAs. |
| **Raw WireGuard** | what Tailscale is built on | best possible — it *is* the direct path | none; you get an IP allowlist, not identity | manual key exchange, no NAT traversal, no key rotation | you end up rebuilding Tailscale. Only sane if the user already runs a WG hub. |
| **ZeroTier** | L2 overlay, similar product shape | comparable, own relay network | node ID; no per-user identity assertion comparable to `WhoIs` | app exists | works, but the identity story is weaker and there is no `serve` equivalent, so omt would have to solve TLS itself. |

The honest summary for the docs: **Tailscale is recommended not because of the
tunnel but because of `serve` + `WhoIs`.** It is the only option in this table
that hands the daemon a *cryptographically-backed user identity* and a *valid
TLS certificate* for free. Cloudflare Access is the runner-up on both counts.

---

## 2. Push notifications to a closed tab

The flagship scenario — an agent blocks while the phone is in a pocket. No tab,
no socket. This section is where a self-hosted daemon meets a wall built by
Apple and Google, so precision matters.

### 2.1 Web Push (VAPID) — the full flow

**The protocol chain** (**DOCUMENTED**, RFCs 8030 / 8291 / 8292):

1. The page's service worker calls
   `registration.pushManager.subscribe({ userVisibleOnly: true,
   applicationServerKey: <VAPID public key, P-256 uncompressed, 65 bytes> })`.
2. The browser returns a `PushSubscription`:
   ```json
   { "endpoint": "https://web.push.apple.com/QK…",
     "keys": { "p256dh": "<client public key, base64url>",
               "auth":   "<16-byte auth secret, base64url>" } }
   ```
   The **endpoint host is chosen by the browser vendor**: `fcm.googleapis.com`
   for Chrome, `updates.push.services.mozilla.com` for Firefox,
   `web.push.apple.com` for Safari. This is the inescapable fact: *there is no
   self-hosted Web Push.* You self-host the *sender*; the *delivery service*
   belongs to the browser vendor and is reached over the public internet.
3. The server encrypts the payload per **RFC 8291** (`aes128gcm` content
   encoding, ECDH on P-256 between a per-message server key and the client's
   `p256dh`, HKDF salted with `auth`), and signs a **VAPID** (RFC 8292) JWT:
   `{"aud": "<endpoint origin>", "exp": <now+12h, max 24h>, "sub":
   "mailto:…"}`, ES256-signed with the server's VAPID private key.
4. `POST <endpoint>` with `Authorization: vapid t=<jwt>, k=<pubkey>`,
   `Content-Encoding: aes128gcm`, `TTL: <seconds>`, optional
   `Urgency: very-low|low|normal|high` and `Topic: <=32 chars` (which collapses
   undelivered messages — this is the server-side analogue of the client-side
   `tag`, and omt should use it for the per-session coalescing in 07 §8.1).
5. `201 Created` = accepted. `404`/`410 Gone` = **the subscription is dead and
   must be deleted**; a server that does not prune on 410 will eventually be
   rate-limited. `413` = payload too large (**keep under ~3 KB** post-
   encryption; 4 KB is the spec floor and vendors differ).

**Rust crates:**

- `web-push` (pimeys/rust-web-push) — implements VAPID + `aes128gcm`,
  async, executor-agnostic, `hyper`-based. The de facto choice.
  **DOCUMENTED**: "still in active development, will have breaking changes in
  accordance with semver." Pin exactly and vendor the VAPID JWT code path if it
  goes unmaintained; the crypto is ~200 lines over `p256`/`hkdf`/`aes-gcm`.
- Alternatives are thin or stale. `web-push` is the answer.

**Where this lands in omt's threat model.** 13 §9.1 lists Web Push as one of
four egress paths, off by default. That is right, and §11 open question 7 is the
real one: even a pointer-only payload tells Apple/Google *that* a named
instance needs attention and *when*. The payload is encrypted; the metadata
(endpoint, timing, size, `Topic`) is not.

### 2.2 iOS is where it breaks — be precise

**DOCUMENTED**, and this is the part product decisions must be made around:

1. **iOS 16.4+ only** (March 2023). Below that: nothing, in any browser.
2. **The Push API is available only to Home Screen web apps.** A PWA opened in
   a Safari *tab* has no `PushManager` at all. The user must do
   Share → *Add to Home Screen*. There is no programmatic install prompt on iOS
   (no `beforeinstallprompt`), so omt must *teach* this with an in-app
   walkthrough — a screenshot-annotated step in the join flow, not a tooltip.
3. **Every browser on iOS uses WebKit**, so "just use Chrome" is not an
   escape (with the narrow EU/DMA exception below).
4. **Permission requires a user gesture.** `Notification.requestPermission()`
   called on load is rejected. It must be inside a click/tap handler. omt's join
   flow should therefore have an explicit "Enable notifications" button, after
   install, not before.
5. **A manifest is required** (`display: standalone`, valid `start_url`, icons).
6. **EU/DMA wrinkle**: in iOS 17.4 Apple briefly removed standalone home-screen
   PWAs in the EU, which removed push with it. It was reversed for Safari-
   installed PWAs, but reports of EU PWAs opening in a tab (and thus losing
   push) persist. **INFERRED**: treat EU behaviour as unreliable and make the
   fallback path first-class rather than an escape hatch.
7. **Delivery is not guaranteed or prompt.** iOS budgets background wakeups;
   `userVisibleOnly: true` is mandatory, so every push *must* display a
   notification — there is no silent push for data sync. Aggressive
   Low Power Mode and Focus modes delay delivery further.
8. **Storage eviction**: Safari evicts IndexedDB for sites unused for 7 days.
   Installed PWAs are exempt (**DOCUMENTED**) — which is a second, independent
   reason omt should push users to install, and it directly bears on 13 §11
   open question 2 (device-bound key survivability).

The honest read: **iOS Web Push works, but only for an installed PWA, with a
multi-step manual install, best-effort timing.** For the flagship "agent blocked
me, buzz my pocket" scenario that is a shaky foundation to be the *only* path.

### 2.3 The pragmatic self-hosted alternatives

**ntfy** — the strongest fit. Pub/sub over HTTP, self-hostable, native iOS and
Android apps, and the publish API is a bare POST:

```sh
curl -H "Title: claude · api-gateway" \
     -H "Priority: high" \
     -H "Tags: warning" \
     -H "Click: https://workstation.tailnet.ts.net/#/s/s_4b2f" \
     -H "Actions: view, Open session, https://…/#/s/s_4b2f" \
     -H "Authorization: Bearer tk_…" \
     -d "Needs a decision" \
     https://ntfy.example.com/omt-vincent
```

Headers (**DOCUMENTED**): `X-Title`/`Title`/`t`, `X-Priority`/`Priority`/`p`
(1=min … 3=default … 5=max), `Tags` (text or emoji shortcodes),
`Click`, `Actions` (view / http / broadcast buttons), `Attach`, `Icon`, `Email`,
`Delay`. A JSON body form (`POST /` with `{"topic":…,"message":…}`) exists too
and is the cleaner thing for a Rust client to emit.

The **critical caveat**, and omt's docs must say it: a *self-hosted* ntfy server
still needs a push path to reach a backgrounded iOS app, and the iOS ntfy app's
APNs delivery is tied to ntfy.sh's own infrastructure unless you build the app
yourself. On self-hosted servers the iOS app can be configured to poll or to
route through ntfy.sh as a relay. Android is fine (foreground service /
websocket, or UnifiedPush). So "zero public egress on iOS" is **harder than
07 §8.2 implies** — that section says "Zero public egress. This is the
recommended fully-private configuration", which is true on Android and
**overstated on iOS**. Worth correcting.

**Gotify** — self-hosted server, `POST /message?token=<apptoken>` with
`{"title","message","priority","extras"}`. Android app is good; **there is no
official iOS app**. Rules it out as omt's mobile default; keep it as a webhook
target for completeness. Rust client crate `gotify` exists.

**Pushover** — commercial, one-time $5 per platform, `POST
https://api.pushover.net/1/messages.json` with `token`, `user`, `message`,
optional `title`, `priority` (-2..2), `url`, `url_title`, `sound`, `html`,
`ttl`. Priority 2 = emergency, requires acknowledgement and repeats — which is
*exactly* the semantics of "an agent is blocked waiting for you". 10k
messages/month free tier per account. First-class iOS app, extremely reliable.
No self-hosting.

**Telegram bot** — `POST https://api.telegram.org/bot<TOKEN>/sendMessage` with
`{"chat_id","text","parse_mode":"MarkdownV2","reply_markup":{…inline keyboard…}}`.
Zero install friction (the user already has Telegram), excellent delivery, and
inline keyboard buttons open a real possibility: **resolving an interaction from
the notification itself**. That is tempting and should be resisted for v1 —
13 §7.2 makes interaction resolution the highest-consequence capability, and
routing it through a third-party bot with a bot-token-shaped credential is a
materially different threat model. Notification-only.

Comparison for omt's default:

| | Self-host | iOS app | Android | Egress | Ack/priority semantics | Friction |
|---|---|---|---|---|---|---|
| Web Push | sender only | installed PWA only | good | vendor push service | `Urgency`, `Topic` | high (install flow) |
| ntfy | yes | yes | yes | ntfy.sh unless self-built app | priority 1–5, actions | low |
| Gotify | yes | **no** | yes | none | priority | low (Android only) |
| Pushover | no | yes | yes | pushover.net | **priority 2 = repeat until ack** | lowest |
| Telegram | no | yes | yes | telegram.org | none | lowest |

### 2.4 Native wrappers — is APNs/FCM direct worth it?

**Tauri v2 mobile.** The official `tauri-plugin-notification` handles *local*
notifications only and cannot receive server pushes (**DOCUMENTED**). Remote
push exists only via community plugins — `tauri-plugin-mobile-push`,
`Choochmeque/tauri-plugin-notifications` (0.4 as of mid-2026),
`tauri-plugin-remote-push` (which explicitly requires manual native host
modification because of Tauri's plugin sandboxing). All are single-maintainer.

**Capacitor** is the more boring, more reliable wrapper for exactly this shape
(a web app that needs push): `@capacitor/push-notifications` is first-party and
mature, and the omt web client is already a PWA, so the wrapper is thin.

**But going native means going direct to APNs**, and that requires:

- An Apple Developer account ($99/yr) and an App Store listing, or TestFlight
  (90-day builds), or an enterprise/ad-hoc distribution story.
- An APNs auth key (`.p8`) that the *daemon* would hold — meaning either every
  user obtains their own Apple developer credentials (absurd), or omt operates
  a relay that holds the key and forwards. **The latter is a hosted service**,
  which 07 §8.2 explicitly rejects and 00 §8 says does not exist.

That is the crux: **a self-hosted daemon cannot talk to APNs without either the
user being an Apple developer or someone operating a relay.** Web Push exists
precisely to launder this through the browser vendor, and ntfy/Pushover solve it
by being that relay with a published trust story. There is no fourth answer.

**Conclusion: do not build a native wrapper for push.** If a native shell is
ever built, it should be for reasons unrelated to notifications (keyboard
handling, background socket lifetime, file access).

### 2.5 Recommendation for omt

Ship a **`Notifier` trait with three implementations**, and be opinionated about
which one the docs lead with.

```rust
#[async_trait]
pub trait Notifier: Send + Sync {
    fn id(&self) -> &str;                              // "webpush" | "webhook" | "log"
    async fn notify(&self, target: &DeviceId, n: &Notification) -> Result<Delivery>;
    /// 404/410 or equivalent → the daemon prunes the registration.
    fn is_permanently_gone(err: &Self::Error) -> bool;
}

pub struct Notification {
    pub instance: InstanceId,
    pub session: SessionId,
    pub kind: NotifyKind,        // Interaction | TurnEnded | AgentError | SessionDied | TakeoverRequest
    pub title: String,           // "claude · api-gateway"
    pub body: String,            // "Needs a decision" — pointer only, never content (07 §8.1)
    pub deep_link: Url,
    pub coalesce_key: String,    // "<instance>:<session>" → Topic / tag / ntfy dedup
    pub urgency: Urgency,
}
```

1. **v1, default-on-when-configured: the generic webhook sink** (`ntfy` shape,
   plus a raw JSON mode that hits Gotify/Pushover/Telegram/Slack/Discord with a
   templated body). It is 200 lines, has no crypto, no key management, no
   browser-vendor dependency, works identically on both phone platforms, and
   the user's existing ntfy app is already on their phone and already on their
   tailnet. **This is what should ship first.**
2. **v1.1: Web Push**, because it is the only path that requires *no third
   party the user must choose*. Gate it behind the PWA-install walkthrough,
   prune on 410, use `Topic` for coalescing, and be honest in the UI about iOS
   ("notifications require adding omt to your Home Screen").
3. **Always: the foreground path.** When the PWA is open or recently
   backgrounded, use the local Notifications API over the live WebSocket. No
   server, no egress, instant. This covers more of the real-world cases than
   people expect and should be the thing that works before any of the above is
   configured.

Also: **correct 07 §8.2's claim** that the ntfy path means "zero public egress"
— true for the daemon, but the iOS ntfy app's own delivery path usually is not.
And 13 §11 Q7 ("should ntfy be the default rather than the alternative?") should
be answered **yes**, on ship-order grounds if nothing else.

---

## 3. Connection resilience for mobile

### 3.1 What mosh actually does, and what to steal

**DOCUMENTED**, from the Winstein & Balakrishnan USENIX ATC '12 paper
(mosh.org/mosh-paper.pdf) and the implementation.

**SSP — the State Synchronization Protocol.** The insight is that a terminal
session is not a byte stream, it is *an object with a state*, and what the two
ends need to agree on is that state, not the bytes that produced it. SSP runs
over UDP and synchronizes a versioned object by sending *diffs between numbered
states*. Two instantiations: `Terminal` (the screen) server→client, and
`UserStream` (keystrokes) client→server.

Properties that fall out, and each maps onto something omt needs:

| mosh property | Mechanism | omt analogue |
|---|---|---|
| **Roaming across IP change** | client sends monotonically-increasing sequence numbers, ≥1 heartbeat every 3 s; any authentic packet with a higher seq **re-targets the server's outbound address to that packet's source IP**. No handshake, no reconnect. | omt cannot do this over TCP/WebSocket. Its equivalent is the `session_token` + `since_seq` warm reconnect of 07 §5.4 — one RTT instead of zero, but no re-auth. Good enough. |
| **Survives sleep and NAT rebinding** | no connection state to lose; UDP + heartbeat | omt's replay window (07 §5.2) plus `Resync`. Same outcome, more machinery. |
| **Bandwidth adapts to the screen** | it sends *the current state*, so a slow link naturally skips intermediate frames | **omt already has this**, and better: 07 §6.2 step 3 collapses an overfull buffer into a grid snapshot. That is precisely mosh's idea, arrived at independently. Keep it and say so. |
| **Predictive local echo** | see below | **the big one — steal it** |
| Crypto | AES-128-OCB3, per-datagram | omt gets this from TLS/WireGuard |
| Scrollback | **mosh has none** — it synchronizes the visible screen only | omt keeps full scrollback server-side; this is a real advantage over mosh, not a gap |

**Predictive/speculative local echo — the mechanism.** This is the single
biggest perceived-latency win and it is worth describing exactly, because the
naive version is worse than nothing.

1. The client keeps a **prediction queue**: local edits it has applied to the
   displayed screen that the server has not yet confirmed, each tagged with the
   `UserStream` sequence number of the keystroke that caused it.
2. Predictions are only made for keystrokes whose effect is *locally
   knowable*: printable characters at the cursor, and `Backspace`/`Left`/
   `Right` within the current line. Control characters, escape sequences,
   anything in the alternate screen — not predicted.
3. **Epochs.** The client starts in a "tentative" state and only displays
   predictions after it has seen the server confirm a prediction correctly. Any
   mismatch bumps the epoch and clears the queue, so a program that does not
   echo (`sudo` password, a TUI, `vim`) stops producing predictions within one
   round trip and does not flicker thereafter.
4. **Confirmation** arrives as a normal state update carrying the highest
   `UserStream` seq the server has consumed. Predictions at or below that seq
   are validated against the authoritative screen: correct → retire silently;
   incorrect → discard the whole queue and repaint from server state.
5. **The underline.** Unconfirmed predictions are rendered with an underline
   (SGR 4) **only when the measured RTT exceeds a threshold** (mosh uses a
   ~50 ms smoothed RTT estimate). On a fast link predictions are still made but
   not visually marked, because the marking would be noise. On a slow link the
   underline is honest: *this character is my guess; it has not been confirmed.*
   Users report this reads as confidence rather than uncertainty, which is why
   it works.
6. A prediction that goes unconfirmed past a timeout is discarded and the screen
   repainted from server state — so the worst case is a brief wrong character,
   never a wedged display.

**How this relates to 07 §7 item 3.** The architecture doc already specifies
local echo "for plain printable keys when the session is not in alt-screen and
bracketed-paste is off … a mismatch within 250 ms reverts." That is the right
idea but it is missing three things that make mosh's version actually usable:

- **the epoch/confidence state machine** — without it, echoing into a program
  that does not echo produces a flicker on every keystroke;
- **the underline-when-slow rendering rule** — without it, users cannot
  distinguish confirmed from predicted text, and the first wrong prediction
  destroys trust in the display;
- **RTT-adaptive engagement** — don't predict at all when RTT < ~20 ms; there is
  nothing to win and every prediction is a chance to be wrong.

**Recommendation: implement it, in the client, in a self-contained module, and
gate it behind a per-session capability flag.** It is maybe 400–600 lines of
TypeScript against the xterm.js buffer, it is entirely client-side (no protocol
change), and it converts a 120 ms tailnet round trip into an apparently instant
one. On DERP or cellular it is the difference between usable and not. It should
be a *fast follow*, not v1, because it is worthless until the rest of the
transport is solid — but the protocol should reserve room for it now: the
server's terminal frames need to carry **the highest client input sequence the
PTY has consumed**, which the current 8-byte binary header (07 §3.6) does not
have a field for. That is a concrete, cheap change to make before the wire is
frozen: add an `ack` u32 to the `kind=1` header, or send it as a periodic
control message.

### 3.2 Eternal Terminal (et)

**DOCUMENTED** (eternalterminal.dev/howitworks, `docs/protocol.md`).

et takes the opposite approach to mosh: instead of synchronizing state over UDP,
it makes **TCP resumable**. `BackedWriter` keeps an encrypted ring of the last N
bytes sent plus a sequence number; `BackedReader` counts bytes received. On
reconnect the server recognizes an existing `ServerClientConnection`, replies
`ConnectResponse{status: RETURNING_CLIENT}`, both sides exchange
`SequenceHeader` protobufs with their last-received sequence, and each replays
the delta as a `CatchupBuffer`.

Why this matters to omt: **it is almost exactly omt's design already.** 07 §5.2's
replay window is `BackedWriter`; `since_seq` is `SequenceHeader`; the replay is
`CatchupBuffer`. The differences are that et is byte-exact (every byte is
delivered, so scrollback is preserved) while omt allows a lossy collapse to a
snapshot, and that et has no notion of a lossless event class alongside the byte
stream.

Two things worth borrowing:

- et's client keeps the connection **and the terminal** alive across the
  reconnect with no visible artifact — no "reconnecting" screen, no repaint.
  omt should aim for the same: a reconnect inside the replay window must be
  *invisible* except for a status pip, because a user who sees a spinner every
  time their phone switches towers will not trust the tool.
- et pins the session to a **server-side id independent of the connection**
  (`ServerClientConnection`), so a reconnect from a *different IP* resumes.
  omt's `session_token` does this; make sure it is explicitly **not** bound to
  the source address (13 §3.3's device-binding is the right binding).

There is also `l1a/etr`, a Rust reimplementation over QUIC — worth reading for
its resume-over-QUIC handling if omt goes the WebTransport route.

### 3.3 Transport for a browser client: WebSocket vs WebTransport vs long-poll

**The 2026 landscape changed.** **DOCUMENTED**: Safari 26.4 (March 2026) ships
WebTransport on macOS **and iOS/iPadOS**, unflagged, making WebTransport
Baseline across Chrome 97+, Edge 98+, Firefox 114+, Safari 26.4+, Opera 83+ and
Samsung Internet 18+.

| | WebSocket/TCP | WebTransport/QUIC (HTTP/3) | HTTP long-poll / SSE |
|---|---|---|---|
| iOS Safari | universal, forever | **26.4+ only** — excludes every older device | universal |
| Head-of-line blocking | yes, one TCP stream: a 40 KB redraw delays the next keystroke's *ack* | no: independent streams, plus unreliable datagrams | n/a (worse) |
| Connection migration | none — a Wi-Fi→cellular switch kills the TCP connection | **QUIC connection IDs survive an address change** — the mosh roaming property, for free | none |
| Loss recovery on a lossy radio | TCP retransmit stalls everything | per-stream | poor |
| Server-side Rust | `tokio-tungstenite` / `axum::extract::ws` — mature, boring | `wtransport` (quinn-based) or `h3` + `h3-webtransport` — young | trivial |
| Through `tailscale serve` | **yes** (HTTP reverse proxy) | **no** — serve proxies TCP/HTTP, not UDP/QUIC | yes |
| Through a corporate proxy | usually | often blocked (UDP/443) | always |

Two conclusions, and the second is the important one.

**(1) WebSocket is the v1 transport and stays the fallback forever.** It is what
07 §2.2 already specifies, it works on every iPhone in existence rather than
those on Safari 26.4+, and — decisively — **it is the only one of the three that
survives `tailscale serve`.** Making the recommended deployment
(`serve` → loopback) incompatible with the primary transport would be a
self-inflicted wound.

**(2) WebTransport is the right *second* transport and it is worth designing
for.** QUIC connection migration solves, natively, the exact problem §3.1 says
mosh solves by hand: the phone changes networks and the connection *does not
break*. That eliminates the reconnect-and-resync round trip entirely for the
most common mobile disruption. But it forces the deployment to be option (A) —
omt binds the tailnet IP itself, holds a `tailscale cert` certificate, and
serves HTTP/3 directly. Which is precisely why §1.1 argues (A) and (B) are
complementary rather than alternatives.

Suggested shape: `Welcome.features` already carries additive capability flags
(07 §3.3). Add `"transport.webtransport"` advertised only when omt is bound
directly with a real cert; the client upgrades opportunistically after a
successful WebSocket handshake and falls back on any failure. `TransportKind`
gains a variant; `omt-transport`'s `Frame` abstraction is unchanged, since
WebTransport gives bidirectional streams that carry the same two frame kinds.

**Long-polling / SSE**: SSE is worth keeping in mind as a *degraded* mode for
event delivery only (no terminal bytes) on a hostile network that blocks
WebSocket upgrades — it's `text/event-stream` over plain HTTP and survives
almost any proxy. Low priority; note it as a possibility, don't build it.

### 3.4 Browser behaviours to design around

All **DOCUMENTED**; the design consequences are **INFERRED**.

- **Background tab throttling.** Timers in a hidden tab are clamped to ≥1/s
  (Chrome, after 5 min, ≥1/min for "budget-exhausted" tabs). A 20 s
  application-level ping (07 §2.5) is coarse enough to survive this, but a
  client that measures liveness with a fine-grained timer will misfire. Measure
  liveness from *message arrival*, not from a timer.
- **`visibilitychange` / `document.visibilityState`.** The correct place to
  (a) send `policy.background`, (b) reset the backoff counter, (c) force an
  immediate reconnect. 07 §2.5's "foreground override" and §5.4's row for iOS
  are both right. Add: `pagehide`/`pageshow` with `event.persisted` for bfcache.
- **bfcache and the Page Lifecycle API.** iOS Safari aggressively bfcaches. A
  page restored from bfcache has **a dead WebSocket that does not fire a close
  event reliably** — this is a classic bug. The client must, on `pageshow` with
  `persisted === true`, treat the socket as dead unconditionally and reconnect,
  rather than trusting `readyState`. Also: iOS *closes* WebSockets when a page
  enters bfcache, which means holding a socket open across backgrounding is not
  something to plan around.
- **`freeze` / `resume` events** (Page Lifecycle) fire on discardable tabs;
  `freeze` is the right moment to flush any unsent input and persist
  `since_seq` to IndexedDB, since the tab may never resume.
- **Service Worker lifetime.** A SW is killed after ~30 s of idle (and a `push`
  handler must resolve its `event.waitUntil()` promise promptly). A SW **cannot
  hold a WebSocket** as a background transport — that architecture does not
  exist. Its only jobs in omt: serve the app shell offline, receive `push`, and
  `clients.openWindow()`/`focus()` on notification click, passing the deep link.
- **PWA state restore.** The thing that makes a restore feel instant is not
  keeping a connection but **rendering from local state before the network
  answers**. Persist, per session, the last grid snapshot + `since_seq` in
  IndexedDB on `freeze`/`visibilitychange`; on resume, paint the stale grid
  immediately with a "reconnecting" pip, then reconcile against the resync. The
  user sees their terminal, not a spinner, in ~0 ms.
- **iOS storage eviction**: 7 days of non-use evicts IndexedDB for websites;
  **installed PWAs are exempt**. Another argument for the install flow, and
  directly relevant to 13 §11 Q2.

### 3.5 Concrete recommendation for reconnect + resume (cross-ref 07 §5)

07 §5 is largely right. The deltas I would make:

1. **Add an input-ack field to the terminal frame header** so predictive echo
   (§3.1) is possible without a protocol break later. Cheapest change with the
   highest option value. Do it before the wire freezes.
2. **Persist `since_seq` and the last snapshot client-side**, and paint from it
   before reconnecting (§3.4). 07 §5.4 describes the server side of resume well
   but the client-side instant-repaint is what the user actually feels.
3. **Make an in-window reconnect visually silent.** Status pip only. Reserve
   the "catching up" badge for an actual `Resync`. A phone will reconnect many
   times an hour and each one must not be an event.
4. **Treat `pageshow{persisted:true}` as a definite disconnect**, not a
   maybe (§3.4). Add it explicitly to the §5.4 table alongside
   `visibilitychange`.
5. **Reconnect on `online` and on `navigator.connection.change`** — already in
   07 §2.5; also listen for a `visibilitychange`-to-visible *and* a failed ping,
   since iOS often reports `online` while the radio is still dead.
6. **Warm-path budget.** A resume inside the window should be: TCP+TLS (1–2
   RTT, 0-RTT with TLS 1.3 session resumption) + `hello{resume}` + `welcome` +
   replay = ~2 RTT. On a tailnet that is ~100 ms. Aim for that number
   explicitly and instrument it; it is the number that determines whether
   switching apps feels broken.
7. **Longer term, WebTransport removes items 3–6 for the network-switch case**
   entirely via QUIC connection migration (§3.3).

---

## 4. Local discovery and zero-config

### 4.1 mDNS/Bonjour

The mechanics are easy. Advertise `_omt._tcp.local` with a TXT record carrying
the instance id, name, proto version and TLS fingerprint; the client browses and
offers a one-tap add. Rust: `mdns-sd` (pure Rust, register + browse, actively
maintained) or `libmdns` (advertise only). On macOS, the OS's own `mDNSResponder`
can be used via `dns-sd`, but a pure-Rust responder avoids the platform split.

**Is it worth it given Tailscale?** Mostly no, and the reasons are worth stating:

- The tailnet already provides discovery — `tailscale status --json`'s `Peer`
  map is an authoritative, authenticated device list, which 07 §1.3's "Tailnet
  discovery" row already proposes. It is strictly better than mDNS: it works
  off-LAN, it carries identity, and it does not require multicast.
- Multicast is unreliable on exactly the networks users care about: most guest
  and corporate Wi-Fi have client isolation or mDNS filtering; iOS additionally
  requires the **Local Network permission** prompt for a native app (a PWA in a
  browser cannot do mDNS at all — this is the killer).
- **A browser cannot browse mDNS.** So LAN discovery only helps the local TUI,
  the CLI, or a hypothetical native client. The phone — the actual target — is
  a browser, and cannot use it.

**Recommendation: implement the advertise side (cheap, ~50 lines, helps
`omt` CLI find local instances and helps a future native client), do not build
browse-based onboarding in the web client, and lead the docs with tailnet peer
discovery.** Note that the daemon should advertise mDNS only on non-tailnet
interfaces and only when explicitly enabled — an unauthenticated broadcast of
"there is a terminal daemon here" is a small but real information leak, and
13 §1.2 case 1 is about network attackers on a shared LAN.

### 4.2 QR-code pairing

The right flow, given 13 §3.2's invite design:

**What the QR encodes: the invite URL, and nothing else.**

```
https://workstation.tailnet.ts.net/#/join?i=<base64url(CBOR(Invite))>.<base64url(sig)>
```

This is already the 13 §3.2 wire form, and it has the properties you want:

- The token is **in the fragment**, so it never reaches a server log or a
  `Referer` header — this survives the QR path too, which matters because a QR
  scanner app may open the URL in a browser the user does not control.
- Scanning it on the phone opens the PWA/site and the client immediately calls
  `join.exchange`, receiving a device-bound credential. The invite itself is
  consumed (`jti` marked used).

Making it single-use and short-lived, concretely:

- `max_uses = 1` and `expires_at = now + 5 min` for the QR path specifically —
  **shorter than the 24 h default for link invites**, because a QR displayed on
  a screen is being photographed by whoever is in the room. `omt pair` (a
  terminal-UI QR renderer via `qrcode` + half-block Unicode) should default to
  5 minutes and **redraw a fresh invite when it expires**, rather than showing a
  stale code.
- Show a short **verification code** (4 digits, derived from the invite `jti`)
  next to the QR, and have the phone display the same code after scanning. This
  is cheap and it defeats the "someone else's QR is on the screen" confusion in
  a shared-screen or screenshared setting.
- Revoke the invite the instant the exchange succeeds, and close the pairing
  screen — do not leave a consumed QR on a monitor.
- The QR must encode the **externally reachable** URL. On a tailnet that is the
  MagicDNS name from `Self.DNSName`, not `127.0.0.1`. omt should pick this by
  asking the LocalAPI and, if a serve config exists, using the serve hostname.
  Getting this wrong (encoding a loopback URL) is the most likely bug in the
  whole flow.
- If the instance is loopback-only, `omt pair` should **say so and refuse**,
  with the fix (`tailscale serve --bg --https 443 http://127.0.0.1:7878`)
  printed. A QR that produces an unreachable URL is worse than no QR.

---

## 5. Latency budget

Keystroke → visible echo. p50 figures; **INFERRED** from component measurements
and the 07 §7 budget, not measured end-to-end for omt.

| Path | Network RTT | Total keystroke→echo | Feel |
|---|---|---|---|
| **localhost** (unix socket / loopback WS) | ~0.1 ms | **20–35 ms** | indistinguishable from a local terminal (dominated by the 16 ms input frame + 8 ms paint) |
| **LAN, Wi-Fi 6** | 2–8 ms | **30–50 ms** | indistinguishable |
| **Tailnet, direct WireGuard, same city** | 10–30 ms | **50–90 ms** | very good; the 07 §7 target of 120 ms p50 is met comfortably |
| **Tailnet, direct WireGuard, transcontinental** | 80–150 ms | **120–200 ms** | noticeably laggy while typing; still fine for reading agent output |
| **Tailnet via DERP relay** | 60–180 ms (two legs through the relay) | **110–250 ms** | *annoying* — this is where users say "it feels broken" |
| **Cellular LTE, direct** | 40–90 ms, jitter to 300 ms | **90–180 ms typical, 400 ms+ spikes** | the jitter is worse than the mean |
| **Cellular 5G, direct** | 15–40 ms | **60–110 ms** | good |
| **Cellular + DERP** | 100–250 ms | **200–400 ms** | unusable for typing without predictive echo |

**Perception thresholds** (established HCI results, **DOCUMENTED** in the
literature and consistent with mosh's own motivation):

- **< 50 ms** — echo feels instantaneous. Nothing to fix.
- **50–100 ms** — perceptible on close attention; fine for most work.
- **100–200 ms** — consciously noticeable while typing; users slow down and
  begin to distrust the connection.
- **> 200 ms** — actively unpleasant; typing errors rise because the user
  cannot use the display to confirm what they typed.
- **Jitter matters more than mean.** A steady 150 ms is more usable than a
  50 ms mean with 300 ms spikes, because the former is predictable.

**What each technique actually buys:**

| Technique | Buys | Where it matters |
|---|---|---|
| **Flush-immediately-when-buffer-empty** (07 §7.1) | up to 16 ms off *every* keystroke | everywhere. Highest value/effort ratio in the whole list. |
| **No permessage-deflate on terminal** (07 §7.2) | 2–10 ms + CPU | everywhere; more on weak phones |
| **Predictive local echo** (§3.1) | **the entire network RTT, for printable characters** — 100–300 ms | DERP and cellular. Converts "unusable" to "fine". Does nothing on localhost/LAN, which is why it must be RTT-gated. |
| **Direct WireGuard instead of DERP** | 50–150 ms | the single largest *network* win. Justifies surfacing relay status prominently (07 §7.4), because it is often fixable by the user (open UDP/41641, disable a restrictive firewall, or enable a peer relay). |
| **WebTransport/QUIC** (§3.3) | removes head-of-line blocking (matters during a big redraw) and **removes the entire reconnect round trip on a network switch** | cellular, moving between networks. Not a steady-state typing win. |
| **Block view instead of PTY bytes** (07 §4.2) | bandwidth, not latency — but on a congested cellular link bandwidth *is* latency | metered/congested links |
| **Warm resume via `session_token`** (07 §3.4) | ~1 RTT + an argon2 verify (~100 ms!) off every reconnect | every app switch on a phone. Underrated: without it, a password-auth reconnect pays the argon2id cost (13 §3.4: m=64 MiB, t=3) on *every* wake. |

The last row deserves emphasis: **argon2id at 64 MiB is ~100–300 ms of CPU**.
Making sure the phone's frequent reconnects hit the resume-token path and not
the password path is worth as much as several network optimizations.

---

## 6. Open questions this research raises

1. Does `tailscale serve` proxy WebSockets without an idle timeout that kills a
   20 s-ping connection? Needs a soak test. If it has a timeout, omt's keepalive
   interval must be tuned to it, or the deployment recommendation changes.
2. Does the macOS `sameuserproof` path work when omt runs as a launchd daemon
   under a different uid than the logged-in user? If not, tailnet identity is
   unavailable in the most common macOS service configuration.
3. iOS ntfy delivery on a *self-hosted* server: what actually happens when the
   app is backgrounded? This determines whether 07 §8.2's "zero public egress"
   claim is true on iOS. **Believed false**; needs confirmation before the docs
   make the claim.
4. Predictive echo against agent TUIs: most agent CLIs are full-screen, which is
   exactly the case mosh's heuristics refuse to predict in. What fraction of
   omt's real typing happens at a line-oriented prompt? If it is small, the
   value of §3.1 drops sharply and this should be measured before building it.
5. WebTransport over a `tailscale cert`-issued certificate: does Safari accept
   HTTP/3 to a `.ts.net` name with an LE cert, over the tailnet, without a
   preceding HTTP/2 Alt-Svc advertisement? Needs a spike.
6. Whether ACL changes actually tear down established omt connections, or only
   block new flows (§1.7). Determines whether omt must re-`WhoIs` on the
   revocation poll.
