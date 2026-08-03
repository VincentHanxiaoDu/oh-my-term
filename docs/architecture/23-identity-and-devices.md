# Identity, Devices and the Home Instance

`omt` has no cloud, no account server and no telemetry. Every instance is
authoritative for its own sessions ([07 §1.1](07-remote-protocol.md#11-the-shape)),
and the web client federates over N of them
([08 §3](08-web-client.md#3-multi-instance-federation)). That design is correct
and this document does not change it.

It creates one concrete problem. The phone needs durable answers to two
questions — *which instances do I have?* and *which devices are allowed to reach
them?* — and the browser is the worst possible place to keep those answers.
Clearing site data, switching laptops, or iOS evicting a non-installed PWA's
IndexedDB loses the instance list; and a browser-local device list cannot revoke
a phone that is no longer in your hand.

This document specifies the identity layer that fixes that without adding a
server: **the state lives on the machines already running omt**, one of which
may take on the `home` role, with an exportable encrypted identity file as the
fallback that needs nothing online at all.

Related: [Decision log](decisions.md) ·
[03 — Capability catalog](03-capability-catalog.md) ·
[07 — Remote protocol](07-remote-protocol.md) ·
[08 — Web client](08-web-client.md) ·
[12 — Collaboration](12-collaboration.md) ·
[13 — Security model](13-security.md) ·
[19 — Onboarding](19-onboarding.md) ·
[21 — Data lifecycle](21-data-lifecycle.md) ·
[research/connectivity](../research/connectivity.md)

> **Authority note, binding.** This document owns `Identity`, `Device`,
> `DeviceGrant`, `InstanceRegistration`, the identity file format, and the
> `home` role. It does **not** own the `Interaction` CAS
> ([12 §4](12-collaboration.md#4-interaction-ownership)), the writer token
> ([12 §3](12-collaboration.md#3-the-writer-token)), `AuthBackend`, roles or
> credential scope ([13 §3](13-security.md#3-authentication),
> [13 §4](13-security.md#4-roles-and-their-mapping-onto-the-catalog)). Where it
> touches those, it wires into them and cross-references; it never redefines
> them.
>
> **D2 note, also binding.** Nothing here degrades the owner's own devices. A
> registered device of the identity that owns the instance is `Operator` or
> `Admin` and is exactly equivalent to sitting at the TUI
> ([D2](decisions.md#d2--remote-is-exactly-equivalent-to-local)). Everything
> below is about *who may connect* and *how that is proved and revoked*, never
> about narrowing what a connected owner may do.

---

## 0. The five decisions this document implements

| # | Decision | Consequence |
|---|---|---|
| **I1** | State the user creates from a phone is persisted **server-side on omt instances**. The browser holds a credential and a cache, nothing authoritative. | Signing in from another device yields the same view, because the client never held the truth. |
| **I2** | Any ordinary instance may be designated **`home`** — the holder of the canonical registry. It is a role, never a new component and never a hosted service. Plus an **encrypted identity file** as the always-available fallback. | No new binary, no new deployment. A user with no home still works, with a smaller guarantee. |
| **I3** | Every device gets its **own** key and its own per-instance credential, revocable individually and remotely. | A lost phone is one `device.revoke` away from useless, from any other device. |
| **I4** | **Passkeys/WebAuthn bind the device at registration; they do not gate day-to-day use.** Per-action re-authentication is opt-in, configurable, and off by default. | Registration is strong; opening the app is not a biometric checkpoint. |
| **I5** | Interaction answers stay **first-come-first-served with acknowledgement** — the existing CAS. Device identity is wired into it so every surface shows *which device* answered, and the loser gets an explicit echo. | §8; the CAS itself is unchanged. |

---

## 1. The identity model

### 1.1 Four types

```rust
/// The human. One per person, normally exactly one on a machine.
/// `id` is self-certifying: blake3-256 of the identity root public key, so no
/// issuer is required to mint it and two identities cannot collide.
/// Rendered as `idn_<crockford-base32(first 16 bytes)>`.
pub struct IdentityId([u8; 32]);

pub struct Identity {
    pub id: IdentityId,
    pub display_name: String,              // "Vincent", user-editable
    pub root_pub: Ed25519PublicKey,        // the trust anchor every instance enrolls
    pub created_at: OffsetDateTime,
    /// Which instance currently holds the canonical registry, if any (§3).
    pub home: Option<HomeRef>,
    /// Monotonic; every registry mutation bumps it. Used for merge (§3.6).
    pub registry_epoch: u64,
    /// Identity-scoped preferences that follow the human across devices.
    pub prefs: IdentityPrefs,
}

pub struct HomeRef {
    pub instance: InstanceId,
    pub label: String,
    pub endpoints: Vec<Endpoint>,
    pub designated_at: OffsetDateTime,
    pub designated_by: DeviceId,
}
```

```rust
/// One browser profile / app install on one physical device.
/// `DeviceId` is the type already used by `ActorKind::Remote` and `Presence`
/// (12 §1, §2); this document gives it a durable record.
pub struct Device {
    pub id: DeviceId,                      // uuid v7, minted by the device, `dev_<b32>`
    pub identity: IdentityId,
    pub label: String,                     // "Vincent's iPhone" — shown everywhere
    pub form: DeviceForm,                  // Phone | Tablet | Laptop | Desktop | Headless
    pub platform: DevicePlatform,          // { os, os_version, browser, app_kind, standalone }
    /// The device's own signing key. Non-exportable where the platform allows.
    pub device_key: DeviceKey,
    /// Present when the device registered a passkey (§2). Absent is normal.
    pub webauthn: Option<WebauthnBinding>,
    pub created_at: OffsetDateTime,
    pub last_seen_at: Option<OffsetDateTime>,
    pub last_seen_via: Option<InstanceId>,
    pub status: DeviceStatus,
    /// Per-device step-up policy (§7). Default: `ReauthPolicy::OFF`.
    pub reauth: ReauthPolicy,
    /// Where this device's authority comes from. Drives §10.
    pub auth_source: AuthSource,
}

pub enum DeviceKey {
    /// Native clients (TUI, CLI, Tauri): ed25519, file-backed 0600 or keychain.
    Ed25519(Ed25519PublicKey),
    /// Browsers: a non-extractable WebCrypto `ECDSA P-256` key pair held as a
    /// `CryptoKey` in IndexedDB. Ed25519 in WebCrypto is not yet uniformly
    /// available; P-256 is, everywhere omt targets.
    EcdsaP256(P256PublicKey),
}

pub enum DeviceStatus {
    /// Pairing started, grant not yet issued (§5.1). Expires with the pairing.
    Pending { expires_at: OffsetDateTime },
    Active,
    Revoked { at: OffsetDateTime, by: DeviceId, reason: RevocationReason },
}

pub enum RevocationReason { Lost, Stolen, Retired, Compromised, IdentityRotated, Other(String) }

pub enum AuthSource {
    /// Normal: paired, holds a DeviceGrant and per-instance credentials.
    Paired,
    /// Auto-enrolled behind `tailscale serve`; authority is re-derived from the
    /// tailnet on every connection and there is no long-lived credential (§10).
    Tailnet { login: String, node_stable_id: String },
    /// Local shell on the instance itself; the TUI and the CLI (07 §2.3).
    Local { uid: u32 },
}
```

```rust
/// What a device holds *per instance*. Issued by that instance, stored by that
/// instance (13 §5.1) and cached by the device. This is the existing bearer
/// credential of 13 §3.3 — this document adds only the identity/device link.
pub struct InstanceCredential {
    pub instance: InstanceId,
    pub credential: CredentialId,
    pub identity: IdentityId,
    pub device: DeviceId,
    pub role: Role,                        // Operator or Admin for the owner (D2)
    pub scope: CredentialScope,            // 13 §4.1; `None`-capabilities for own devices
    pub issued_at: OffsetDateTime,
    pub expires_at: Option<OffsetDateTime>,
    /// The token itself is never stored server-side in plaintext (13 §3.3).
    pub device_bound: DeviceKey,
}

/// An identity's knowledge of one instance. This is the record that must
/// survive a browser wipe — it is the answer to "which instances do I have?".
pub struct InstanceRegistration {
    pub instance: InstanceId,
    pub label: String,                     // user-editable, defaults to hostname
    pub endpoints: Vec<Endpoint>,          // ordered by preference; probed in order
    /// TOFU pin: the instance's Ed25519 identity key (13 §5.1). A change is a
    /// hard failure, exactly like a changed SSH host key (§5.4).
    pub instance_pub: Ed25519PublicKey,
    pub added_at: OffsetDateTime,
    pub added_by: DeviceId,
    pub last_reachable_at: Option<OffsetDateTime>,
    pub is_home: bool,
    pub tags: BTreeSet<String>,            // "work", "cloud" — client-side filters
    /// Last known catalog hash, so a cold start can render affordances before
    /// the first handshake completes (08 §3.4). Advisory only.
    pub catalog_hash_hint: Option<[u8; 32]>,
}

pub enum Endpoint {
    /// `<host>.<tailnet>.ts.net` behind `tailscale serve`. The recommended one.
    Tailnet { host: String, port: u16 },
    Https   { url: String },               // reverse proxy with a real DNS name
    Lan     { addr: IpAddr, port: u16 },   // http, LAN only, no WebAuthn (§2.3)
    Local   { socket: PathBuf },           // same machine, unix socket
    Ssh     { target: String },            // the stdio bridge, 07 §2.4
}
```

### 1.2 What lives where

The single most important table in this document. "Registry" means the
`home` instance's registry store; "enrolled instance" means any instance that
has enrolled this identity's root key.

| Datum | Scope | Registry (home) | Every enrolled instance | Identity file | Browser (IndexedDB) |
|---|---|---|---|---|---|
| `Identity` record, `root_pub`, display name | per-identity | **authoritative** | copy of `root_pub` + `id` | copy | cache |
| Identity root **private** key | per-identity | yes, `0600`/keychain | no | **yes, encrypted** | **never** |
| `IdentityPrefs` | per-identity | **authoritative** | no | copy | cache |
| `Device` records (all devices) | per-identity | **authoritative** | the subset that has connected, + revocations | copy | cache |
| This device's private key | per-device | no | no | optional (`--include-credentials`) | **yes, non-extractable** |
| `DeviceGrant` certificate (§1.3) | per-device | issued here | verified, cached | optional | yes |
| `InstanceRegistration` set | per-identity | **authoritative** | own record + peers it was told about | copy | cache |
| `InstanceCredential` (bearer token) | per (device, instance) | hashed | **authoritative**, hashed (13 §3.3) | optional, plaintext | yes, wrapped (§9.2) |
| Revocation list | per-identity | **authoritative** | synced copy, honoured locally | copy | cache (advisory) |
| Recovery code hashes | per-identity | yes | yes (argon2id) | no | never |
| Answered-interaction record | per-instance | no | **authoritative** (12 §8 audit + ledger) | no | small window |
| Session content, scrollback | per-instance | **no** | authoritative (21 §1) | no | small window |

Three rules fall out and are worth stating on their own:

1. **The home instance stores registry, not session data.** It is not a proxy,
   not a mirror, and it does not learn anything about the sessions running on
   other instances. Promoting or demoting home moves a few kilobytes.
2. **Instances trust the identity root key, not the home instance.** Home is a
   convenient place to keep the registry and to *sign* on the root key's behalf;
   it is never a trusted third party in the authentication path. An instance
   that has enrolled `root_pub` can verify a `DeviceGrant` with no network at
   all.
3. **The browser is a cache with one exception**: the device's own private key,
   which exists nowhere else by design (I3). Losing it is losing the device, and
   that is the intended semantics — see §6 and §9.3.

### 1.3 `DeviceGrant` — the certificate that makes this decentralized

```rust
/// Signed by the identity root key. This is what lets a device introduce
/// itself to an instance it has never met, without that instance talking to
/// home, and without home being a trusted party.
pub struct DeviceGrant {
    pub v: u8,                             // format version, 1
    pub identity: IdentityId,
    pub device: DeviceId,
    pub device_key: DeviceKey,
    pub label: String,
    pub form: DeviceForm,
    pub issued_at: OffsetDateTime,
    /// Default 90 days. Bounded lifetime is the offline-revocation backstop
    /// (§6.4). Renewed silently whenever home is reachable.
    pub not_after: OffsetDateTime,
    /// The `registry_epoch` at issue time; an instance rejects a grant whose
    /// epoch is below one it has already seen revoked (§6.3).
    pub epoch: u64,
}
// Wire form: base64url(CBOR(DeviceGrant)) || "." || base64url(Ed25519 sig)
```

The JSON the web client sees (`identity.get` output, abridged):

```json
{
  "identity": {
    "id": "idn_7QK3M9XW2ZB4T1RA",
    "display_name": "Vincent",
    "root_pub": "ed25519:9f3a1c…",
    "registry_epoch": 41,
    "home": { "instance": "3f2a…", "label": "workstation",
              "endpoints": [{ "kind": "tailnet", "host": "workstation.tailc9e96c.ts.net", "port": 443 }] }
  },
  "this_device": {
    "id": "dev_5TR9K2QF",
    "label": "Vincent's iPhone",
    "form": "phone",
    "platform": { "os": "ios", "os_version": "18.4", "browser": "safari", "standalone": true },
    "device_key": { "alg": "ecdsa-p256", "pub": "p256:04a1…" },
    "webauthn": { "rp_id": "workstation.tailc9e96c.ts.net", "cred_id": "AZ9x…", "uv_capable": true },
    "grant_expires_at": "2026-11-01T09:00:00Z",
    "auth_source": "paired",
    "reauth": { "mode": "off" }
  },
  "devices": [
    { "id": "dev_5TR9K2QF", "label": "Vincent's iPhone", "form": "phone",
      "status": "active", "last_seen_at": "2026-08-03T18:22:04Z", "last_seen_via": "3f2a…" },
    { "id": "dev_1AA8C0PL", "label": "MacBook Pro", "form": "laptop",
      "status": "active", "last_seen_at": "2026-08-03T18:41:55Z", "last_seen_via": "3f2a…" },
    { "id": "dev_0ZZ4Q7MN", "label": "old iPad", "form": "tablet",
      "status": { "revoked": { "at": "2026-07-11T10:02:00Z", "by": "dev_1AA8C0PL", "reason": "retired" } } }
  ],
  "instances": [
    { "instance": "3f2a…", "label": "workstation", "is_home": true,
      "endpoints": [{ "kind": "tailnet", "host": "workstation.tailc9e96c.ts.net", "port": 443 }],
      "instance_pub": "ed25519:c41d…", "last_reachable_at": "2026-08-03T18:41:55Z",
      "credential": { "present": true, "role": "admin" } },
    { "instance": "88b1…", "label": "cloud-dev", "is_home": false,
      "endpoints": [{ "kind": "tailnet", "host": "cloud-dev.tailc9e96c.ts.net", "port": 443 }],
      "instance_pub": "ed25519:77ee…", "last_reachable_at": null,
      "credential": { "present": false, "reason": "not_yet_reachable_from_this_device" } }
  ]
}
```

---

## 2. WebAuthn and passkeys, honestly

### 2.1 The relying-party problem, stated exactly

WebAuthn binds a credential to an **RP ID**, which must be either the origin's
effective domain or a *registrable domain suffix* of it. Two consequences, both
fatal to the naive design:

- **An RP ID cannot be an IP address.** `http://192.168.1.20:7878` can never
  register or use a passkey. Not a limitation of some browsers — the spec has no
  syntax for it.
- **A passkey registered for RP ID `workstation.tailc9e96c.ts.net` is unusable
  at `http://localhost:7878` and unusable at the LAN IP.** They are different
  effective domains, and `localhost` is not a suffix of anything.

omt is reachable at *several* origins by design ([07 §1.3](07-remote-protocol.md#13-adding-an-instance),
[13 §5.3](13-security.md#53-tls)): a tailnet name, a LAN address, loopback, and
possibly a reverse-proxied DNS name. So there is no single RP ID that covers an
instance's whole reachable surface, and there is no way to make one.

**The resolution — and it is the load-bearing design choice of this section:**

> **WebAuthn is the identity mechanism for one origin: the origin that serves
> the registry.** In the recommended deployment that is the home instance behind
> `tailscale serve`, and the RP ID is its `.ts.net` hostname. Passkeys
> authenticate a device *to its identity*, once, at pairing. They are **not** the
> per-instance authentication mechanism. Reaching an instance uses the
> `DeviceGrant` plus a device-key signature over the server nonce
> ([07 §3.4](07-remote-protocol.md#34-auth)), which works over any origin,
> including plain HTTP on a LAN, including a unix socket, including the SSH
> stdio bridge.

This is what makes the multi-origin problem tractable: it confines WebAuthn to
the one place where an origin is stable and HTTPS is real, and it lets every
other path use a mechanism that has no origin concept at all.

Two refinements worth taking:

- **RP ID choice.** Prefer the tailnet-wide registrable domain
  (`tailc9e96c.ts.net`) over the per-host name, so a passkey registered against
  the workstation still works if home is promoted to another node on the same
  tailnet. This depends on `ts.net` being a public suffix — see
  [§13 Q1](#13-open-questions); until verified, omt registers against the full
  host name and treats a home promotion as requiring a fresh passkey (§3.4).
- **Related Origin Requests** (WebAuthn L3 `/.well-known/webauthn`) let one RP
  ID be used from a declared set of origins. Useful for pairing a proxied DNS
  name with the tailnet name; useless for IPs and localhost, and not yet
  universally implemented. Treat as an optimization, never a dependency.

### 2.2 Registration and authentication, end to end

Registration (pairing a new device, §5.1 step 4):

1. Client calls `device.pair.complete` over the already-open, pairing-token-
   authenticated WebSocket. Server returns `PublicKeyCredentialCreationOptions`:
   `rp: { id: <rp_id>, name: "omt · workstation" }`,
   `user: { id: <IdentityId bytes>, name: "vincent", displayName: "Vincent" }`,
   `pubKeyCredParams: [-7 (ES256), -8 (EdDSA), -257 (RS256)]`,
   `authenticatorSelection: { residentKey: "preferred", userVerification: "preferred" }`,
   `attestation: "none"`.
2. `navigator.credentials.create()` — **inside the tap handler** of the "Pair
   this device" button. iOS rejects it otherwise.
3. Server verifies the attestation object, stores `WebauthnBinding { rp_id,
   cred_id, cred_pub, sign_count, uv_capable, transports }` on the `Device`.
4. In the **same** call the device also submits its WebCrypto P-256 device key
   public part; the server issues the `DeviceGrant`. The passkey and the device
   key are registered together and neither is optional.

Why both keys: the passkey proves *a human with the authenticator* is present at
registration and is the only thing that can do user-verified step-up (§7); the
device key is what every subsequent connection signs with, on origins where
WebAuthn cannot run at all.

Authentication is deliberately **not** WebAuthn in the steady state. A
connection presents the bearer credential plus `device_sig`
([07 §3.4](07-remote-protocol.md#34-auth)); the passkey is invoked only for
(a) re-pairing after a cache wipe, (b) recovery, (c) an opt-in step-up (§7).
`residentKey: "preferred"` means the re-pair flow can be usernameless — the user
taps "I've been here before", the browser offers the passkey, and the identity
is discovered from it. Where discoverable credentials are unavailable, the
server sends an `allowCredentials` list keyed by the `IdentityId` the client
still has cached, and falls back to §2.4 if it has nothing cached.

### 2.3 Where passkeys will not work

Stated plainly, because each of these is a real configuration omt supports:

| Situation | Passkey? | What happens instead |
|---|---|---|
| `http://127.0.0.1:7878` | **Yes** — localhost is a secure context by exception | Works, but the RP ID is `localhost`, which is shared with every other local app and is useless off-machine. omt does **not** register passkeys here; it uses the local-shell path (§5.2). |
| `http://192.168.1.20:7878` (LAN, no TLS) | **No** — not a secure context, and IPs cannot be RP IDs | Token fallback (§2.4) |
| `https://192.168.1.20:7878` self-signed | **No** — IP cannot be an RP ID; the untrusted cert also degrades WebCrypto/SW | Token fallback |
| `https://workstation.tailc9e96c.ts.net` via `tailscale serve` | **Yes**, real LE cert, stable name | The recommended path |
| Reverse proxy with a real DNS name | **Yes** | Fine; consider Related Origin Requests to share the RP ID with the tailnet name |
| Tailscale Funnel | Yes technically | But see [13 §10](13-security.md#10-checklist--publishing-an-instance-over-tailscale-funnel): do not use Funnel |
| Safari tab on iOS (not installed) | Registration works; **persistence does not** — IndexedDB is evicted after 7 days unused, taking the device key with it | The install-to-home-screen walkthrough is part of pairing (§5.1) |
| Headless: CI box, container, `omt --remote` | **No** browser at all | Token fallback, always |

### 2.4 The token fallback, which must always exist

**Every flow in this document has a passkey-free path, and no capability is
reachable only through WebAuthn.** The fallback is the existing bearer
credential ([13 §3.3](13-security.md#33-bearer-tokens)) plus the device key:

- `omt device add --code` on any instance the user has shell access to prints a
  one-time code and a `DeviceGrant` request URL. The new device generates its
  device key in WebCrypto (no WebAuthn), submits the pubkey with the code, and
  receives the grant. Identity binding is proved by possession of the code,
  which came from a shell on a machine the user controls.
- `omt token create --role operator --device "iPad"` mints a device-bound bearer
  credential directly, no identity involved. This is the escape hatch for
  scripts, for a browser that has no WebAuthn, and for the user who simply does
  not want passkeys. Such a device appears in `device.list` with
  `webauthn: null`, and step-up (§7) is unavailable to it — the policy UI says
  so rather than silently downgrading to "no verification".
- The identity file (§4) can carry credentials, so importing it on a new device
  is itself a passkey-free path to the full registry.

### 2.5 Rust crate options for the server side

| Crate | Assessment |
|---|---|
| `webauthn-rs` | The default choice. Maintained, opinionated toward correct usage, has a well-documented `RequestChallengeResponse`/`PasskeyAuthentication` state model that matches the two-round-trip flow above, and explicitly supports the passkey (discoverable, no-attestation) profile omt wants. Requires the RP ID and allowed origins at builder construction — which is exactly the multi-origin friction of §2.1 and the reason the RP ID must be a single stable value. |
| `passkey-rs` (1Password) | Cleaner layering (`passkey-authenticator` / `passkey-client` / `passkey-types`), useful because it can also drive a *virtual authenticator* in tests, which omt needs for the pairing E2E test ([08 §10](08-web-client.md#10-testing)). Server-side maturity is behind `webauthn-rs`. |
| Hand-rolled over `ciborium` + `p256`/`ed25519-dalek` | Rejected. Attestation and authenticator-data parsing is exactly the class of untrusted-input parsing P5 wants fuzzed and P1 wants out of our crates. |

Decision: **`webauthn-rs` in `omt-auth`, `passkey-rs` as a dev-dependency for
the virtual authenticator in tests.**

### 2.6 The concrete first-pairing UX from a phone

What the user actually sees, on the recommended deployment:

1. On the laptop: `omt device pair`. The TUI shows a QR and, under it, an 8-character code
   `K7QM-3XPA`, a 5-minute countdown, and the line *"Scan this on the device you
   want to add."*
2. Phone camera → `https://workstation.tailc9e96c.ts.net/#/pair?p=…` → Safari opens.
3. **First screen is the install prompt, not the pairing prompt.** "Add omt to
   your Home Screen first — otherwise iOS will forget this device in a week and
   notifications won't work." with the annotated Share → *Add to Home Screen*
   walkthrough ([research/connectivity §2.2](../research/connectivity.md)).
   A "continue anyway" link exists and is honest about the consequence.
4. From the installed PWA, the pairing link is re-opened (the token survives in
   the fragment because the PWA `start_url` carries it once). Screen: *"Pair
   iPhone with Vincent's omt?"* and a large **Pair this device** button.
5. Tap → `navigator.credentials.create()` → Face ID once → the passkey is
   created, the device key is generated, the grant is issued.
6. **Verification screen, both sides.** The phone and the laptop each display
   the same four words derived from the pairing transcript hash —
   `amber · kestrel · viola · fond`. The laptop asks *"Do these match what the
   new device shows?"* with **Yes, pair** / **No, cancel**. This is the
   anti-MITM and anti-shoulder-surf step (§5.3).
7. Done screen: *"iPhone paired. 3 instances found: workstation, cloud-dev,
   mini."* — the registry arrived with the grant, so the phone has the full
   instance list before it has ever contacted two of those instances.
8. Then, and only then, the "Enable notifications" button (user gesture rule,
   [research/connectivity §2.2](../research/connectivity.md)).

Total: two taps, one Face ID, one four-word comparison. No password, no token
paste, and — critically — **no biometric prompt on subsequent opens** (I4).

---

## 3. The `home` instance role

### 3.1 What it is

`home` is a boolean role an ordinary instance takes on. It changes nothing about
how that instance runs sessions. It adds one store and one capability group.

```toml
# config/omt/config.toml on the instance being designated
[identity]
home = true
```

or `omt identity home --take`, or, from any authenticated device,
`instance.registry.set_home { instance }`.

What home stores, in `state/omt/registry/` (a new row in the
[21 §1](21-data-lifecycle.md#1-the-inventory) inventory: append-only signed log
+ snapshot, `0600`, tens of kilobytes, static growth):

- the `Identity` record and the identity **root private key**,
- every `Device` record and the revocation list,
- every `InstanceRegistration`,
- `IdentityPrefs` and any other identity-scoped user state — including
  cross-instance UI preferences and the answered-interaction *pointers* the
  phone uses to render "you answered this" across a cache wipe.

What home does **not** store: session trees, scrollback, blocks, agent events,
interaction *content*. Those stay on the instance that owns them
([21 §1](21-data-lifecycle.md#1-the-inventory)). Home is a registry, not a hub.

### 3.2 How other instances learn about it

They mostly do not need to, and that is the point. The propagation that does
happen:

1. **At enrollment.** When an instance is added to the identity (§5.2) it
   records `identity.root_pub` and `identity.id`. That is the whole trust
   relationship. It also records the current `HomeRef` as a *hint*.
2. **Push on connect.** A device's `hello` carries an `identity` block:

```json
{ "t": "hello", "id": "req_0", "proto": [1],
  "client": { "name": "omt-web", "version": "0.4.1", "kind": "web" },
  "device": { "id": "dev_5TR9K2QF", "name": "iPhone", "platform": "ios/safari" },
  "identity": {
    "id": "idn_7QK3M9XW2ZB4T1RA",
    "grant": "eyJ2IjoxLCJpZGVudGl0eSI6….Xh92…",
    "registry_epoch": 41,
    "home_hint": { "instance": "3f2a…", "endpoints": [ … ] }
  } }
```

   The instance verifies `grant` against its enrolled `root_pub`, checks its
   local revocation list and the grant's `not_after`, and — if it has no
   credential for this device yet — mints one on the spot and returns it in
   `auth_ok`. **No call to home is made.** This is the mechanism that keeps home
   off the critical path.
3. **Revocation sync, best effort.** Each non-home instance polls home's
   `instance.registry.revocations.get { since_epoch }` on start and every 15
   minutes when reachable, and applies the deltas. It also accepts a *push* of
   the same list from any authenticated `Admin` device — so a revocation
   initiated on a phone reaches every instance that phone can reach, immediately,
   without home being involved at all (§6.2).

### 3.3 Bootstrapping a device from home

The new device finishes pairing (§5.1) holding: its device key, its
`DeviceGrant`, and a registry snapshot. For each `InstanceRegistration` it now
does, lazily and in parallel, on first use:

```
connect(endpoint) → hello{identity.grant} → welcome → auth{method:"device_grant"}
  → instance verifies grant against enrolled root_pub
  → instance checks revocation list + not_after + epoch floor
  → instance mints InstanceCredential, returns it in auth_ok
```

An instance that has *not* enrolled this identity refuses with
`unauthorized { code: "identity_not_enrolled" }` and the client shows the exact
fix: *"`mini` doesn't know this identity yet. Run `omt identity enroll` on mini,
or open an invite from it."* Never a generic auth error.

### 3.4 Changing home, and having none

**Promote another instance.** `instance.registry.set_home { instance: "88b1…" }`
from any `Admin` device:

1. The client fetches the full registry from the current home (or from its own
   cache if home is gone) and its signature.
2. It opens an `Admin` connection to the new home and calls
   `instance.registry.import { snapshot, signature }`.
3. The new home requires the **identity root private key** to become
   authoritative, because it must be able to sign new grants. Transfer is
   explicit and is one of exactly two paths: (a) the old home hands it over
   directly over the authenticated connection when both are reachable, or
   (b) the user imports the identity file on the new home
   (`omt identity import ~/identity.omtid`). There is no third path and no
   automatic replication of the root key — a private key that silently spreads
   to every instance is a worse design than an occasional manual step.
4. `registry_epoch` bumps; the old home is demoted to a normal instance and
   **deletes its copy of the root private key**, keeping the registry snapshot
   read-only as a stale backup marked with its epoch.
5. Devices learn the new `HomeRef` from the next registry sync, or from any
   instance's `home_hint`. A device that reaches only the old home gets
   `HomeRef` with a `superseded_by` field and follows it.

If the passkey RP ID was the old home's host name and the tailnet-wide RP ID is
unavailable ([§13 Q1](#13-open-questions)), promotion invalidates passkeys and
the flow says so up front: *"Passkeys were registered against
`workstation.tailc9e96c.ts.net`. After promotion you'll be asked to re-register
one passkey per device. Your instances stay reachable throughout."* Reachability
is unaffected because reachability does not use passkeys (§2.1).

**No home at all.** Fully supported, and the default until the user chooses one.
Then:

- The **identity file is the registry** (§4). It is created at `omt identity
  create` and the user is told, once, in plain language, that it is the only
  durable copy.
- Each instance still enrolls `root_pub` and still verifies grants offline, so
  everything except *centralized registry mutation* works identically.
- New device pairing is done from a **shell on any instance** rather than from
  home (§5.2), and the instance doing the pairing needs the root private key to
  sign the grant — which it gets from the identity file, passed as
  `omt device pair --identity-file ~/identity.omtid` and held in memory for the
  duration of the pairing only.
- What is lost, stated honestly: adding an instance from a phone does not
  persist anywhere but that phone's cache and the next identity-file export; and
  a revocation reaches only the instances the revoking device can currently
  reach. The UI carries a persistent, dismissible-per-week banner: *"No home
  instance. Device changes live only on this device until you export your
  identity file."* with a one-tap **Make `workstation` home**.

### 3.5 When home is unreachable

This is the behaviour that matters most, because home *will* be a laptop in a
bag.

| Operation | Home down |
|---|---|
| Connect to any instance the device has a credential or grant for | **Works.** No home involvement in the auth path at all (§3.2). |
| Render the instance list, labels, last-known state | **Works**, from the IndexedDB cache (§9), marked with the cache age. |
| Resolve an interaction, type into a PTY, everything in the catalog | **Works.** Unchanged. D2 holds. |
| Add a *new* instance | Works locally; the registration is queued as a pending registry mutation and flushed when home returns (§3.6). |
| Pair a *new* device | **Blocked at home**, but not blocked overall: fall back to the local-shell path (§5.2) on any instance, or the identity file. The error names the alternative. |
| Revoke a device | **Works, partially.** Fans out immediately to every instance the revoking device can reach; queued for home and for the rest. The UI shows exactly which instances have acknowledged: *"Revoked on 2 of 3 instances. `mini` unreachable — it will apply this within 15 minutes of coming back, or immediately when you next reach it."* Partial revocation reported as partial is the whole point. |
| Renew an expiring `DeviceGrant` | Blocked. With 90-day grants and renewal attempted from day 60, this needs home to be unreachable for a month; when it happens, the device falls back to its per-instance credentials, which do not expire, and shows a non-blocking warning. |

**The invariant, and it is absolute: an unreachable home never blocks a device
from reaching an instance it already has a credential or an unexpired grant
for.** Home is a convenience for *managing* access, never a gate on *using* it.
A design where the phone cannot reach the workstation because the laptop is
asleep would be a worse product than no home instance at all.

### 3.6 Merging divergent registries

Two devices can mutate the registry while home is away. The merge is
deliberately trivial rather than clever:

- The registry log is a set of **idempotent, commutative** records:
  `DeviceAdded`, `DeviceRevoked`, `DeviceRenamed{at}`, `InstanceAdded`,
  `InstanceRemoved{at}`, `PrefSet{key, at}`, `HomeDesignated{at}`.
- Merge = union, with `Revoked` beating `Added` for the same `DeviceId`
  regardless of timestamps (**revocation is monotonic and always wins**), and
  last-write-wins by wall clock for renames and prefs, which is adequate because
  they are cosmetic.
- `registry_epoch` is `max(a, b) + 1` after a merge.
- Conflicting `HomeDesignated` records are the one case surfaced to the user
  rather than resolved: *"Two devices designated different home instances. Pick
  one."* Silently choosing would strand the root key.

---

## 4. The encrypted identity file

### 4.1 What it is for

Two jobs, and it is important that they are the same artifact: **backup** (the
thing that survives losing every device) and **sneakernet sync** (the thing that
bootstraps a device with no working home and no network path to one).

### 4.2 Format

```
Offset  Content
0       magic         "OMTID1\n"                    7 bytes
7       header_len    u32 LE
11      header        JSON, authenticated as AAD, never encrypted
...     ciphertext    XChaCha20-Poly1305 over CBOR(IdentityFileBody)
...     tag           16 bytes
```

```json
{
  "v": 1,
  "identity": "idn_7QK3M9XW2ZB4T1RA",
  "display_name": "Vincent",
  "created_at": "2026-08-03T18:00:00Z",
  "exported_at": "2026-08-03T19:14:22Z",
  "exported_by": "dev_1AA8C0PL",
  "registry_epoch": 41,
  "contains_credentials": true,
  "kdf": { "alg": "argon2id", "m_kib": 262144, "t": 3, "p": 1, "salt": "b64…" },
  "cipher": "xchacha20poly1305",
  "nonce": "b64…"
}
```

```rust
pub struct IdentityFileBody {
    pub identity: Identity,
    pub root_secret: Ed25519PrivateKey,          // the trust anchor
    pub devices: Vec<Device>,
    pub revocations: Vec<Revocation>,
    pub instances: Vec<InstanceRegistration>,
    pub prefs: IdentityPrefs,
    /// Only when exported with `--include-credentials`. Plaintext tokens.
    pub credentials: Vec<InstanceCredential>,
    /// Unused recovery codes, only when exported with `--include-recovery`.
    pub recovery_codes: Vec<String>,
}
```

Choices and why:

- **Argon2id, m = 256 MiB, t = 3, p = 1.** Deliberately *heavier* than the
  password-login parameters in [13 §3.4](13-security.md#34-username--password)
  (64 MiB), because this is an offline-attackable artifact that will sit in a
  password manager or on a USB stick for years, and it is decrypted at most a
  handful of times ever. Parameters are in the header so they can be raised
  later; import re-derives with whatever the file says.
- **XChaCha20-Poly1305**, 24-byte random nonce — no nonce-reuse risk across
  re-exports, and no AES-GCM 96-bit-nonce counting to get wrong.
- **Header authenticated as AAD, not encrypted**, so a user can run
  `omt identity inspect file.omtid` and see *whose* identity it is, when it was
  exported, and whether it carries credentials — **without the passphrase**.
  That is a deliberate, small metadata disclosure in exchange for not having a
  pile of indistinguishable encrypted blobs.
- **Passphrase policy:** minimum 12 characters, zxcvbn-style strength meter, and
  omt offers to *generate* a six-word diceware phrase, which is the default
  affordance. No passphrase means no export — there is no "unencrypted export"
  flag.

### 4.3 Export and import

```
$ omt identity export ~/vincent.omtid --include-credentials
  This file grants full access to 3 instances, including PTY write and
  interaction approval on all of them. Treat it exactly like an SSH private key.
  Passphrase (or press Enter for a generated one):
  Wrote ~/vincent.omtid (mode 0600, 4.1 KB)

$ omt identity import ~/vincent.omtid
$ omt identity import --merge ~/vincent.omtid       # union per §3.6 rather than replace
```

In the web client, export downloads the file via a `Blob` — **it never touches a
server other than the one generating it**, and the file is generated by the
instance, not assembled in the browser, so the browser never holds the root
private key even transiently. Import is a file picker plus passphrase, and lands
in whichever instance the user is importing *to*; importing into a browser alone
(no instance) is supported and populates the cache plus, if credentials were
included, restores connectivity immediately.

### 4.4 It is a secret, and omt says so every time

- `contains_credentials: true` files are flagged on every subsequent
  `identity inspect`, in the export confirmation, and in the import screen.
- `omt doctor` warns if a `*.omtid` is found under a path that looks synced
  (`~/Dropbox`, `~/Library/Mobile Documents`) — the same detector
  [21 §1.2](21-data-lifecycle.md#12-the-one-line-answer-for-backup-exclusion)
  already ships.
- The file is `0600` and the exporter refuses to write to a world-readable
  directory without `--i-know`.
- It is **not** covered by [13 §8](13-security.md#8-secret-redaction) redaction
  because it never appears in a log or an event; the path may, the contents
  never do.

---

## 5. Pairing flows

### 5.1 Adding a new device, from an authenticated device

The primary flow. `device.pair.begin` on home (or, homeless, on any instance
holding the root key, §3.4):

```json
{ "t": "result", "id": "req_9", "ok": true, "output": {
  "pairing": "pr_8KQ2M",
  "code": "K7QM-3XPA",
  "url": "https://workstation.tailc9e96c.ts.net/#/pair?p=pr_8KQ2M.4f9a2c…",
  "qr_svg_ref": "blob_31",
  "expires_at": "2026-08-03T19:19:00Z",
  "max_uses": 1
} }
```

**What the QR encodes:** exactly the `url` above and nothing else. The fragment
carries `<pairing_id>.<32-byte CSPRNG secret, base64url>`; being in the fragment
it is never sent in a request line, never in an access log, never in a `Referer`
— the same property [13 §3.2](13-security.md#32-invite-links-the-primary-onboarding-path)
gives invite links. The QR is *not* a `DeviceGrant` and carries no key material:
a photographed QR is a 5-minute, single-use, verification-gated window, not a
credential.

- **Expiry: 5 minutes**, not the 24 hours of an invite. A pairing is an
  in-person, in-the-moment act; a long TTL buys nothing and costs everything.
- **Single use, enforced server-side** by consuming `pairing` in the same
  transaction that issues the grant (the `jti` mechanism of
  [13 §3.2](13-security.md#32-invite-links-the-primary-onboarding-path)).
- **Rate limited**: 5 pairing attempts per identity per 15 minutes; the 8-char
  code is 40 bits of base32 with a 5-minute window, and the rate limit is what
  makes typing it safe.
- **Verification phrase** (§2.6 step 6): four words from a 2048-word list
  derived as `blake3(pairing_secret || device_pub || instance_pub)`, displayed on
  both screens, confirmed on the *initiating* device. This is what makes
  shoulder-surfing the QR insufficient: an attacker who photographs the code
  from across the room races the legitimate device, and whoever loses sees a
  phrase mismatch and cancels. Without it, first-to-scan silently wins.
- **The initiating device must confirm**, and the confirmation screen shows the
  new device's self-reported platform and IP: *"iPhone · iOS 18.4 · Safari ·
  from 100.100.1.9. Pair?"*

The three transports for the same pairing are equivalent and produce the same
record: **QR** (default, desktop→phone), **one-time code** (typed, for a device
with no camera, or a QR that will not scan), **link** (shared over an already-
trusted channel like AirDrop or a password manager — with the caveat below).

### 5.2 Adding a new instance to an existing identity

```
# on the new machine, with shell access
$ omt identity enroll --from ~/vincent.omtid          # or
$ omt identity enroll --home workstation.tailc9e96c.ts.net
  Enrolled identity idn_7QK3M9XW2ZB4T1RA (Vincent).
  This instance now accepts device grants signed by that identity.
  Registered with home. Your 3 devices can reach it immediately.
```

Enrollment stores `root_pub` and nothing secret. Two things then happen: the
instance can verify grants offline forever, and it registers itself with home so
the registration propagates to every device.

The **no-shell-access** variant is the existing invite path
([13 §3.2](13-security.md#32-invite-links-the-primary-onboarding-path)):
someone with access mints `omt invite`, the device exchanges it for a
credential, and the client then offers *"also enroll your identity on this
instance so your other devices can reach it?"* — which requires an `Admin`
credential on that instance and a root-key signature the device cannot produce,
so it is forwarded to home. If home is down, the instance is added to the
registry as `enrolled: false`, reachable by *this* device only. Honest, and
visible in the instance row.

### 5.3 Trust on first use

- **Instance keys are pinned on first add** (`InstanceRegistration.instance_pub`).
  A subsequent handshake presenting a different key is a **hard failure** with an
  SSH-style full-screen interstitial naming both fingerprints, the date the old
  one was pinned, and the two legitimate causes (`omt instance rotate-key`, a
  reinstall). There is no "continue anyway" button; the fix is
  `omt instance trust --update <fingerprint>` typed on a device, which is
  deliberately more friction than a tap.
- **The identity root key is pinned at enrollment** on each instance,
  symmetrically. An instance never accepts a second root key for the same
  `IdentityId`.
- **The pairing itself is TOFU** in the strict sense — the new device has no
  prior knowledge of anything. That is what the verification phrase (§5.1) is
  for: it converts TOFU into a human-verified channel binding.

Anti-phishing, explicitly: a pairing link *received* rather than *generated* is
the dangerous case, because a user can be talked into opening one. The pairing
screen therefore states, before the button, whose identity is being joined and
what it grants: *"This will let `idn_7QK3…` (Vincent) — and anyone who controls
it — run commands on 3 machines from this device."* And omt never sends pairing
links itself: there is no email, no SMS, no share sheet integration that
transmits one. The user has to move it, which keeps the human in the loop.

### 5.4 The hard case: pairing a phone to an instance it cannot reach yet

Common and important: the phone is being paired at a café, `cloud-dev` is only
reachable on the tailnet, and Tailscale is not installed on the phone yet.

The design handles this **because registration and reachability are separate
concerns**:

1. Pairing (§5.1) happens against home over whatever origin currently works.
2. The registry snapshot arrives with the grant. The phone now *knows about*
   `cloud-dev`: its label, endpoints, pinned key, and last-known session summary
   from the cache.
3. `cloud-dev` appears in the instance list immediately, in state
   `unreachable { reason: "endpoint_unresolvable" }`, greyed, with its
   last-known sessions still listed and stamped with their age — the same
   treatment [08 §3.3](08-web-client.md#33-the-unified-session-list) gives a
   disconnected instance, because "disappearing rows on a subway is worse than
   stale rows".
4. The row's tap target is a **diagnosis**, not an error: *"`cloud-dev` is on
   your tailnet (`cloud-dev.tailc9e96c.ts.net`) and this device isn't. Install
   Tailscale and sign in →"* with the App Store link. If the endpoint list
   contains a LAN address, it says *"…or join the `home-wifi` network."*
5. **No credential is minted until first successful contact.** When the phone
   later reaches `cloud-dev`, the `hello.identity.grant` path (§3.2) mints one
   silently. The user experiences it as the row simply turning live.

This is why grants are root-signed certificates rather than tokens issued by
each instance at pairing time: a certificate can be issued for an instance that
is not present.

---

## 6. Revocation and recovery

### 6.1 Revoking one device

`device.revoke { device, reason }`, callable from **any** `Admin` device of the
identity — including the phone, which is the point.

Order of operations:

1. The revocation record is written to home if reachable, and to the local cache
   regardless.
2. It is fanned out in parallel to every instance in the registry that the
   revoking device can reach, as an authenticated `instance.registry.revocations.push`.
3. Each receiving instance, atomically: adds the device to its revocation list;
   marks every `InstanceCredential` for that device revoked (13 §5.2 semantics,
   which already push revocation to live connections); closes every connection
   from that device with `Close { code: "revoked" }`; and **releases any writer
   token** the device's actor held ([12 §3.3](12-collaboration.md#33-lifecycle):
   disconnect releases immediately, no grace).
4. `device.pair`-issued grants are not individually revocable *as certificates*
   — they are bearer certificates with a `not_after`. The revocation list is
   what makes them ineffective, and it is consulted on every connect and on the
   existing 60-second liveness check ([13 §3.1](13-security.md#31-the-trait)).
5. The push subscription bound to that device's credential stops delivering
   ([13 §6](13-security.md#6-browser-side-controls)), so a lost phone also stops
   buzzing with the content of your interactions. This matters more than it
   sounds.

**Open interactions and in-flight work on the revoked device:**

| State at revocation | Outcome |
|---|---|
| Device was *viewing* an interaction (`Interaction.viewers`, [12 §4.4](12-collaboration.md#44-advisory-viewer-presence-on-a-card)) | Viewer entry drops. Advisory only, nothing else changes. The interaction stays `Open`. |
| Device had a resolve **in flight, not yet CAS-won** | The connection is gone; the call never lands or lands and is rejected as unauthenticated. Interaction stays `Open`. |
| Device's resolve **already won the CAS** (`Resolving`/`Resolved`) | **It stands.** The decision already reached the agent. Revocation is not a time machine, and pretending otherwise would mean a half-applied approval, which is worse than an applied one. The audit entry records the resolution *and* the subsequent revocation, adjacent in time, which is exactly what a "who approved that?" investigation needs. |
| Device held the writer token | Released immediately; the next writer sees the partial line as-is, per [12 §C6](12-collaboration.md#c6--the-writer-disconnects-mid-command). omt does not inject a `Ctrl-C`. |
| Device had queued offline actions ([08 §8.5](08-web-client.md#85-offline-and-reconnect)) | Never replayed — replay requires a live authenticated connection. |

### 6.2 Revoking everything

`identity.rotate_key` is the nuclear option, and it is genuinely nuclear:

1. A new identity root keypair is generated. `IdentityId` **changes**, because it
   is derived from the key — the old identity is dead rather than modified.
2. Every existing `DeviceGrant` becomes unverifiable everywhere, instantly, with
   no list to distribute. This is the property that makes rotation worth having:
   it does not depend on reaching anything.
3. Every reachable instance is told to enroll the new `root_pub` (signed by the
   *old* key, so the transition is verifiable) and to revoke all credentials.
4. **Unreachable instances keep trusting the old root key** until someone enrolls
   the new one on them. There is no way around this and the UI says it: *"3 of 4
   instances updated. `mini` still trusts your old identity — run
   `omt identity enroll --from <file>` on it, or revoke its credentials from a
   device that can reach it."* An instance that trusts a compromised root key is
   a real exposure, listed as such in §11.
5. The device performing the rotation re-pairs itself immediately and is the
   only surviving device.

`omt identity panic` is the shorthand: rotate, revoke all credentials on every
reachable instance, and print the list of instances that need manual attention.

### 6.3 Epoch floor

Each instance records the highest `epoch` it has seen in a revocation record and
refuses grants with a lower epoch for a device on that list. This closes the
replay of an old grant for a re-added device, and it is cheap: one integer.

### 6.4 Recovery, and its limits

The honest section. There is no account, no support desk, and no one who can
verify you are you. Recovery is therefore *possession-based*, and it can fail.

**Four paths, in order of preference:**

1. **Another registered device.** The common case, and the reason §5.1 is cheap
   enough to do twice. Any `Admin` device pairs a replacement. Cost: zero.
   *The product should push hard for a second device at setup* — [19](19-onboarding.md)'s
   flow ends with "add a second device now?" and does not treat "later" as
   equivalent.
2. **The identity file.** Import it anywhere; it contains the root key, so it
   can mint a new grant. Cost: you must have exported it and remember the
   passphrase.
3. **A recovery code.** Ten single-use 24-character codes generated at
   `omt identity create`, printed once, stored argon2id-hashed on home **and on
   every enrolled instance** — deliberately replicated, because the failure mode
   is "home is the thing I lost". Redeeming one (`identity.recovery.use`) on any
   instance, over any origin, issues exactly one new `DeviceGrant` for the device
   redeeming it, consumes the code everywhere it can reach, and emits a loud
   audit entry plus a notification to every other registered device. Rate-limited
   to 3 attempts per hour per instance.
4. **Local shell on any instance.** `omt device pair` from a terminal on the
   machine. This always works and needs nothing but OS-level access — which is
   the correct floor, because someone with a shell on your workstation already
   has everything omt could protect.

**The limits, stated rather than papered over:**

- If you have lost **every** device, have **no** identity file, have **no**
  recovery code, and have **no** shell access to any machine running omt — you
  are locked out, permanently, and nothing can be done. That is not a bug; it is
  the direct consequence of having no third party, and a system that could
  recover you here could also be socially engineered into recovering an
  attacker. omt says this at setup, in one sentence, next to the recovery codes.
- Recovery codes stored on every instance mean an attacker with **disk access to
  any one instance** gets the hashes. They are argon2id-hashed with the §4.2
  parameters and are 24 random base32 characters (≈115 bits), so offline attack
  is not the concern — but it is a wider blast radius than a single-copy secret,
  and it is a deliberate trade for the "home is what I lost" case.
- The identity file is a **single point of compromise**. Whoever has it and the
  passphrase is you.
- Rotation cannot reach offline instances (§6.2). There is no distributed
  revocation that works without connectivity; anyone claiming otherwise is
  selling something.

---

## 7. Per-action re-authentication

### 7.1 What it is, and what it is emphatically not

Per D1, omt classifies **its own** operations and never the agent's tool danger.
So the policy below is expressed over **capability names and `effects` bits**
from [03 §2](03-capability-catalog.md#2-declaring-a-capability) — never over what
a `Bash` tool call contains, never over which tool an agent asked about, never as
a condition on *whether* an interaction may be resolved.

It is a **proof-of-presence timer**, not an authorization narrowing. Failing
step-up does not reduce what the device may do (D2); it only means the device
must prove a human is holding it before the call proceeds, and then it may do
everything again.

### 7.2 Policy

```rust
pub struct ReauthPolicy {
    pub mode: ReauthMode,
    /// Capability patterns requiring a fresh user-verified assertion.
    /// Same `CapabilityPattern` type as `CredentialScope::capabilities` (13 §4.1).
    pub require: BTreeSet<CapabilityPattern>,
    /// Or, over effect bits — so a *new* destructive capability is covered the
    /// day it is added (the same argument 13 §4 makes for policy over effects).
    pub require_effects: EffectBits,
    /// How long a verification counts for. Default 5 min, min 30 s, max 12 h.
    pub freshness: Duration,
    /// What satisfies it when WebAuthn is unavailable on this device.
    pub fallback: ReauthFallback,          // Passphrase | TokenReentry | Deny
}

pub enum ReauthMode { Off, On }
```

```toml
# Off by default. This is what a user who opts in writes.
[identity.reauth]
mode = "on"
freshness = "5m"
require = ["interaction.resolve", "session.write_bytes", "config.set"]
require_effects = ["DESTRUCTIVE"]
fallback = "passphrase"

# Or per-device, from the phone's settings screen:
[[identity.reauth.device]]
device = "dev_5TR9K2QF"
require = ["interaction.resolve"]
```

**Defaults, and the one exception.** `mode = "off"` for everything the user does
day to day — opening the app, reading, typing, resolving interactions (I4).
The single exception is the identity surface itself: `identity.*` and
`device.revoke`/`device.revoke_all` require a fresh assertion **by default**,
because they are the credential-management surface and an unattended unlocked
phone should not be able to silently revoke your laptop or export your identity.
This is not an agent-policy judgement and stays entirely inside D1. It is
configurable to `off` like everything else, with a one-line warning.

### 7.3 Enforcement is server-side

In dispatch, not in the UI — the same argument
[03 §3](03-capability-catalog.md#3-dispatch) makes for roles.

```rust
pub struct CallContext {
    // … existing fields (03 §3) …
    /// Set from the connection state; `None` for a device that has never
    /// step-upped on this connection.
    pub last_verified_at: Option<OffsetDateTime>,
}
```

A `ReauthGuard` runs in the dispatch chain, after the role/scope check, before
the handler. It matches the capability against the actor's device policy, and if
`last_verified_at` is absent or older than `freshness`, it returns:

```json
{ "t": "result", "id": "req_31", "ok": false,
  "error": { "code": "precondition_failed",
             "message": "This action needs verification on this device",
             "detail": { "reason": "reauth_required",
                         "challenge": "ch_9f2c…",
                         "methods": ["webauthn", "passphrase"],
                         "freshness_s": 300 } } }
```

The client runs `navigator.credentials.get({ challenge, userVerification: "required" })`
— inside the user's tap, always — and calls `device.stepup.verify { challenge,
assertion }`. On success the server stamps `last_verified_at` on the *connection*
and the client **retries the original call automatically**, so the user
experiences one extra Face ID and not an error.

Three properties that keep it from being a nag:

- Freshness is per connection and per device, so a burst of approvals costs one
  verification.
- The retry is automatic and the error is never rendered as an error.
- `interaction.resolve` remains **non-optimistic**
  ([12 §7.1](12-collaboration.md#71-what-may-be-optimistic)), so a step-up
  detour cannot produce a UI that showed an answer applied and then took it back.

A device with `webauthn: null` cannot do WebAuthn step-up; its policy falls back
to `passphrase` (the identity file passphrase, verified against a stored
argon2id hash) or `deny`. The settings UI states which, per device, rather than
silently treating "no authenticator" as "verified".

---

## 8. Device identity in the collaboration model

### 8.1 Presence

`Presence.device: DeviceId` already exists ([12 §2](12-collaboration.md#2-presence-is-first-class-state)).
This document makes it resolvable to a label everywhere, by having the instance
join it against its `Device` records at broadcast time:

```rust
/// Denormalized onto every Presence, Actor and Resolution the client receives,
/// so no surface has to look anything up to render an attribution.
pub struct DeviceRef {
    pub id: DeviceId,
    pub label: String,        // "iPhone"
    pub form: DeviceForm,     // drives the icon
    pub platform_short: String, // "iOS · Safari"
    pub is_this_device: bool,   // computed per recipient
}
```

An instance that has never seen the device (possible: a grant-authenticated
first connect populates it immediately, so this is a sub-second window) renders
`DeviceRef { label: platform_short, .. }` rather than a raw uuid. A user must
never see `dev_5TR9K2QF` in a UI.

### 8.2 The interaction race, with devices

The CAS is [12 §4](12-collaboration.md#4-interaction-ownership) and is
**unchanged**. This section adds attribution and the acknowledgement.

`Resolution` and the `InteractionResolved` event carry the winner's `DeviceRef`:

```json
{ "t": "event", "seq": 91422, "session": "s_4b2f",
  "payload": { "kind": "interaction_resolved",
    "interaction": "int_88",
    "response": { "type": "choices", "answers": [{ "labels": ["Use Postgres"] }] },
    "by": { "actor": "act_12", "role": "operator",
            "device": { "id": "dev_5TR9K2QF", "label": "iPhone",
                        "form": "phone", "platform_short": "iOS · Safari" } },
    "at": "2026-08-03T18:22:04.113Z",
    "latency_ms": 4120 } }
```

Every surface renders the same sentence from that one payload:

| Surface | Rendering |
|---|---|
| Web | Card collapses to `✓ Use Postgres — answered by **iPhone**, 3 s ago` with the device icon |
| TUI | `[✓] Use Postgres  · iPhone · 3s` in the card's place |
| CLI | `omt interaction list` gains a `answered_by` column: `iPhone (Vincent)` |
| Push | The resolved notification is closed by `tag`; no new one is sent |
| Audit | `AuditEntry.actor` already carries the device ([12 §8](12-collaboration.md#8-audit-log)) |

### 8.3 What the losing device renders — the ack/echo

This is the specific thing the design must not get wrong: the loser believed
they made a decision.

The loser receives two things, in either order, and the UI is defined for both
orders:

1. `CallResult { ok: false, error: { code: "conflict", detail: { resolved_by, device, at } } }`
   — its own call's failure, and
2. the broadcast `interaction_resolved` above.

Rendering, precisely:

- The card **immediately** becomes the resolved card showing the winner's answer
  and device (§8.2). No intermediate spinner, no "your answer is being
  submitted".
- Directly beneath it, an **echo strip** — inline, not a toast, because a toast
  can be missed:

  > ⚠︎ **Not applied.** You chose ~~Use SQLite~~. **iPhone** answered
  > *Use Postgres* 0.2 s earlier.

  The user's own attempted answer is shown, struck through, verbatim. Showing
  only "someone else answered" leaves the user unsure what they nearly did.
- **Dismissal rules, deliberately asymmetric:**
  - If the loser's attempted response was **identical** to the winner's, the
    strip reads *"Already answered by iPhone — same answer."* and auto-dismisses
    after 5 s. Nothing was lost.
  - If the responses **differed**, the strip has no auto-dismiss. It persists
    until explicitly dismissed, and it survives a reconnect (it is derived from
    the persisted resolution plus the client's own pending overlay record, both
    of which outlive the socket).
- On the TUI the same strip appears as a one-line banner under the card, plus
  [12 §C2](12-collaboration.md#c2--a-phone-answers-a-question-card-while-the-tui-user-is-arrow-keying-it)'s
  **500 ms Enter swallow** — unchanged, and the reason it is repeated here is
  that it is the difference between "collaboration works" and "my phone answered
  a question and then my terminal ran something".
- The retry-idempotence rule of [12 §4.1](12-collaboration.md#41-the-invariant)
  still governs: *same actor, same response* is not a conflict and produces no
  echo at all. A flaky mobile network must never manufacture this banner.

The winning device shows nothing special beyond the resolved card. Being the
winner is the expected case and does not need an interstitial.

---

## 9. What the browser actually caches

### 9.1 Stores and budget

IndexedDB database `omt`, version-migrated like any on-disk format (P5).

| Store | Contents | Cap | Eviction |
|---|---|---|---|
| `identity` | 1 record: `IdentityId`, display name, this `DeviceId`, the grant blob, `registry_epoch` | 8 KB | never (would un-pair the device) |
| `keys` | the non-extractable `CryptoKey` pair, and an AES-KW wrapping key | — | never |
| `credentials` | per-instance tokens, **wrapped** (§9.2) | 16 KB | never |
| `instances` | `InstanceRegistration` × N, plus `label`, `endpoints`, `catalog_hash_hint`, `available` capability set | 256 KB | LRU by `last_reachable_at` |
| `sessions` | last-known `UnifiedSession` rows ([08 §3.3](08-web-client.md#33-the-unified-session-list)) — titles, workspace, agent state, counts. **No terminal content.** | 512 KB | LRU, 200 rows max |
| `interactions` | open interaction *summaries* + resolutions from the last 24 h, for the echo strip (§8.3) and instant cold start | 256 KB | age |
| `terminal` | last **2000 lines** per session for the 3 most recently viewed sessions only | 4 MB | LRU |
| `prefs` | cached `IdentityPrefs` | 32 KB | never |
| `assets` | app shell (service worker cache, not IDB) | separate | SW policy ([08 §8.6](08-web-client.md#86-pwa-and-web-push)) |

**Total budget: 8 MB**, checked on every write; over budget, the LRU stores are
trimmed in the order `terminal` → `sessions` → `interactions` → `instances`.
Eight megabytes is chosen to sit far under every platform's eviction pressure
threshold while being enough for an instant cold start.

### 9.2 What is never cached

- **The identity root private key.** Never enters a browser, in any form.
- **Any private key in exportable form.** The device key is created with
  `extractable: false`; the browser literally cannot serialize it. This is the
  single strongest thing available in a browser and the reason the design leans
  on it.
- **Bearer tokens in plaintext.** They are stored wrapped with a non-extractable
  AES-KW key in the same `keys` store. Honest assessment: this defends against a
  casual IDB dump and against an exfiltration path that reads structured storage
  without executing script in the origin; it does **not** defend against script
  running in the origin, which can simply use the key. It is a real but modest
  improvement, and this document does not claim more.
- **Recovery codes**, in any form.
- **Session content beyond §9.1's window** — no scrollback archive, no blocks
  bodies, no media blobs, no audit entries. Stale terminal content is worse than
  absent terminal content ([08 §8.6](08-web-client.md#86-pwa-and-web-push) makes
  the same call for the service worker).

### 9.3 Invalidation, and eviction mid-session

Invalidation is by identifier, not by time:

- `instances` and `prefs` are invalidated by `registry_epoch` — a higher epoch
  from any instance triggers a registry refetch.
- `sessions` rows are invalidated by their instance's event `seq`; a `Resync`
  ([07 §5.2](07-remote-protocol.md#52-replay-window)) drops that instance's rows
  wholesale, exactly as it drops optimistic overlays
  ([12 §7.2](12-collaboration.md#72-applying-corrections)).
- `available` capability sets are invalidated by `catalog_hash`
  ([07 §3.3](07-remote-protocol.md#33-handshake-and-capability-negotiation)).
- Cached rows are **always rendered with their age** when the owning instance is
  not connected. A stale list is visibly stale.

**Mid-session eviction** — iOS decides to clear the origin while the app is
open. The connection survives (the token is in memory), so the user's current
work is not interrupted, but the device key is gone and the next cold start
cannot authenticate. Behaviour:

1. The client detects the missing `identity` record on the next IDB write (it
   writes a heartbeat record every 60 s specifically to detect this early).
2. A persistent, non-blocking banner: *"This browser cleared omt's data. You're
   still connected, but this device will need to be paired again next time.
   Pair now →"* — and "Pair now" runs §5.1 against the still-authenticated
   connection, which can mint a fresh grant without any other device.
3. If the user ignores it and the connection drops, the cold start lands on the
   re-pair screen with the passkey path first (`residentKey: preferred` makes it
   usernameless), the identity-file import second, and the one-time code third.
4. The old device record is **not** auto-revoked. Two device records for one
   physical phone is untidy; silently revoking on a storage event would be a
   denial-of-service triggered by the browser. The device list shows both with
   their `last_seen_at`, and offers "these look like the same device — retire the
   old one?".

This is precisely the risk [13 §11 Q2](13-security.md#11-open-questions) raises,
and the mitigation is the same one [research/connectivity §2.2](../research/connectivity.md)
identifies: **installed PWAs are exempt from the 7-day eviction**, which is why
the install walkthrough is step 3 of pairing and not an afterthought.

---

## 10. Tailnet identity as an alternative front door

When omt is served via `tailscale serve`, the tailnet has already authenticated
the user and injects `Tailscale-User-Login` / `Tailscale-User-Name` /
`Tailscale-User-Profile-Pic`
([research/connectivity §1.5](../research/connectivity.md)).

### 10.1 How it composes

It **bypasses device registration for authentication and keeps it for
attribution and revocation.** Concretely:

1. A request arriving with trusted identity headers is granted per
   `[[auth.tailnet.grants]]` ([13 §3.5](13-security.md#35-tailnet-identity)) —
   no `DeviceGrant`, no bearer token, no passkey.
2. The instance **auto-enrolls a `Device` record** keyed by
   `(tailnet login, client-reported device id)` with
   `auth_source: Tailnet { login, node_stable_id }` and a label derived from
   `Tailscale-User-Name` plus the client hint (`"Vincent · iOS · Safari"`). It
   mints **no long-lived credential** — authority is re-derived on every
   connection, which is the property that makes this configuration pleasant.
3. That record participates fully in presence, the writer token, the audit log
   and the interaction attribution (§8) — so "answered by iPhone" works
   identically whether the device is paired or tailnet-authenticated.
4. **Revocation authority moves to Tailscale.** `device.revoke` on a
   tailnet-sourced device closes its connections and refuses it locally, but the
   honest UI text is: *"Revoked on this instance. This device authenticates via
   your tailnet — remove it from Tailscale to revoke it everywhere."* Pretending
   omt is the revocation authority here would be a lie.
5. A device may be **both**: paired (grant + credential) and, on this instance,
   tailnet-authenticated. The instance prefers the tailnet assertion when
   present because it is fresher, and records both in the audit entry.
6. `Node.StableID` is what goes in the audit log, not the IP
   ([research/connectivity §1.5](../research/connectivity.md)).

### 10.2 The trap, and the three conditions

The absolute rule from the research, restated because it is the single most
dangerous line in this document:

> **Identity headers must never be trusted when the request did not arrive from
> the tailnet listener.** A backend on `0.0.0.0:7878` lets anyone on the LAN set
> `Tailscale-User-Login: vincent@example.com` and become `Admin`.

omt requires **all** of the following before honouring a header, and it is a
startup-time check, not a runtime hope:

1. The listener is `UnixSocket` or `Loopback` — never `Interface`, never
   `Tailnet` ([13 §2](13-security.md#2-bind-policy)).
2. `[auth.tailnet].trust_proxy_headers = true` is explicitly written in the
   config file.
3. The peer address of the connection is loopback, taken from the socket,
   **never** from `X-Forwarded-For`.

Plus one cheap extra from the research, adopted here: omt cross-checks the
Tailscale LocalAPI `serve-config` to confirm that a serve mapping actually
points at its own port before honouring headers. This closes "another local
process pretends to be `tailscale serve`", which conditions 1–3 do not.

Failure modes are startup failures: `trust_proxy_headers = true` together with
any non-loopback `BindSpec` refuses to start with the exact reason. And per
[13 §3.5](13-security.md#35-tailnet-identity), tailnet identity is disabled
outright for Funnel-sourced requests.

### 10.3 The recommendation

For a single user with Tailscale, **`tailscale serve` + tailnet identity is the
recommended front door, and passkeys are optional on top of it.** It gives a
real certificate, a stable origin (which is also the only place WebAuthn works,
§2.1), password-free auth, and revocation through the tailnet ACL. The identity
layer in this document then supplies what the tailnet cannot: the durable
registry, the cross-instance device list, and the per-device attribution that
makes §8 work.

---

## 11. Threat model deltas vs. [13](13-security.md)

Additions to [13 §1](13-security.md#1-threat-model). Each states what omt does
and does not do.

| Scenario | omt protects | omt does **not** protect |
|---|---|---|
| **Lost phone, screen locked** | Device key is non-extractable and, on iOS, hardware-backed; `device.revoke` from any other device kills every credential on every reachable instance within seconds and stops push delivery. | Nothing, if the phone was unlocked and in an attacker's hands before you revoked. The window between loss and revocation is real, and step-up (§7) is the only thing that narrows it — which is why it is offered even though it is off by default. |
| **Stolen laptop with a running daemon** | Nothing new: it is the machine. FDE is the control, and `omt doctor` reports whether it is on ([21 §5](21-data-lifecycle.md#5-encryption-at-rest)). | Everything. If the laptop *is* home, the attacker also has the identity root key and can mint devices for every enrolled instance. **This is the strongest argument for a home instance that is not your most-carried machine**, and the UI says so when you designate a laptop as home. |
| **Shared / borrowed browser** | The device key is per-browser-profile, so "signing in" on someone else's browser means *pairing a device*, which is visible in `device.list` forever and revocable. There is no password to type and therefore none to leave behind. | A user who pairs a device on a shared machine and walks away. Mitigations: pairing shows "this device will stay paired until you revoke it", and `identity.prefs.pair_expiry` can set an automatic `not_after` on grants created from a device flagged as public. |
| **Evil maid on the home instance** | Nothing at the OS level — omt is not a privilege boundary against its own uid ([13 §1.3](13-security.md#13-out-of-scope--omt-explicitly-does-not-defend-against)). What omt gives you is *detection*: the audit log records every grant issued and every registry mutation with its device, and unexpected devices appear in every client's device list. | Prevention. An attacker with the root key can mint themselves a device on every enrolled instance. Detection is the whole defence, and `identity.rotate_key` is the response. |
| **A malicious instance the user added** | Instances have **no** authority over the identity: they receive `root_pub` (public), verify grants, and mint credentials scoped to themselves. A hostile instance cannot mint a grant, cannot revoke your devices, cannot read another instance's sessions, and cannot impersonate home (home designation is a signed registry record). | It can lie about *its own* state, phish through its own UI, and — because the web client is served *by an instance* — serve a hostile bundle to a browser that then holds credentials for other instances. **This is the real risk and it is not fully mitigated.** Partial mitigations: CSP and `frame-ancestors 'none'` ([13 §6](13-security.md#6-browser-side-controls)); credentials are per-origin in IndexedDB, so a bundle served by instance B cannot read instance A's IDB; and the client warns when adding an instance that is not on the tailnet. Do not add instances you do not control. |
| **Compromised / photographed QR link** | 5-minute TTL, single use, server-enforced; the four-word verification phrase means a racing attacker is detected and the legitimate user cancels; the initiating device must confirm and sees the new device's platform and address. | A user who confirms without reading the phrase. Nothing defends against that, and the phrase comparison is deliberately two large words on each screen rather than a hex string, to make the comparison actually happen. |
| **Identity file leaked** | Argon2id at 256 MiB and a generated diceware default; `contains_credentials` visible without decrypting so the user can triage. | Full compromise, if the passphrase is weak or known. Equivalent to a leaked SSH key. |
| **Home offline while a device is stolen** | Fan-out revocation reaches every instance the revoking device can reach, immediately, without home (§6.1). | Instances that neither device can reach. They apply the revocation when they next sync, and the grant's 90-day `not_after` is the backstop. |

---

## 12. Capabilities

Declared in the [03 §2](03-capability-catalog.md#2-declaring-a-capability) style.
`effects` uses only the closed set. All of these participate in the parity test.

### `identity.*`

| Name | Kind | Role | Input | Output | Effects |
|---|---|---|---|---|---|
| `identity.get` | Query | Viewer | `{}` | `Identity` + `this_device` + `devices` + `instances` (the §1.3 JSON) | — |
| `identity.create` | Command | Admin | `{ display_name }` | `{ identity, recovery_codes: [String; 10] }` | `WRITES_FS` |
| `identity.enroll` | Command | Admin | `{ identity_id, root_pub, home_hint? }` | `{ enrolled: bool }` | `WRITES_FS` |
| `identity.export` | Command | Admin | `{ passphrase, include_credentials: bool, include_recovery: bool }` | `{ blob: BlobId, bytes, sha256 }` | `WRITES_FS` |
| `identity.import` | Command | Admin | `{ blob: BlobId, passphrase, merge: bool }` | `{ registry_epoch, devices, instances }` | `WRITES_FS` |
| `identity.inspect` | Query | Admin | `{ blob: BlobId }` | the file header, unencrypted (§4.2) | `READS_FS` |
| `identity.rotate_key` | Command | Admin | `{ confirm: "rotate" }` | `{ new_identity, updated: [InstanceId], unreachable: [InstanceId] }` | `DESTRUCTIVE`, `NETWORK` |
| `identity.prefs.get` / `.set` | Query / Command | Viewer / Operator | `{ key? }` / `{ key, value, version }` | `IdentityPrefs` / `{ version }` | — / `WRITES_FS` |
| `identity.recovery.generate` | Command | Admin | `{ confirm: "regenerate" }` | `{ codes: [String; 10] }` — invalidates the previous set | `DESTRUCTIVE` |
| `identity.recovery.use` | Command | Viewer¹ | `{ code, device: DeviceRegistration }` | `{ grant, registry }` | `WRITES_FS` |

¹ `identity.recovery.use` is reachable pre-authentication by construction — it
*is* an authentication path. It is rate-limited (3/hour/instance), audited
loudly, and notifies every registered device.

### `device.*`

| Name | Kind | Role | Input | Output | Effects |
|---|---|---|---|---|---|
| `device.list` | Query | Viewer | `{ include_revoked: bool }` | `[Device]` with `DeviceRef` rendering fields | — |
| `device.get` | Query | Viewer | `{ device }` | `Device` | — |
| `device.rename` | Command | Operator | `{ device, label }` | `{ registry_epoch }` | `WRITES_FS` |
| `device.pair.begin` | Command | Admin | `{ transport: "qr"\|"code"\|"link", ttl_s? }` | `{ pairing, code, url, qr_svg_ref, expires_at }` | `WRITES_FS` |
| `device.pair.complete` | Command | Admin² | `{ pairing, device_pub, label, form, platform, webauthn_attestation? }` | `{ grant, registry, verification_words: [String; 4] }` | `WRITES_FS` |
| `device.pair.confirm` | Command | Admin | `{ pairing, confirmed: bool }` | `{ device }` | `WRITES_FS` |
| `device.pair.cancel` | Command | Admin | `{ pairing }` | `{}` | — |
| `device.revoke` | Command | Admin | `{ device, reason }` | `{ applied_on: [InstanceId], pending_on: [InstanceId] }` | `DESTRUCTIVE`, `NETWORK` |
| `device.revoke_all` | Command | Admin | `{ except: [DeviceId], confirm: "revoke-all" }` | same shape | `DESTRUCTIVE`, `NETWORK` |
| `device.reauth.get` / `.set` | Query / Command | Viewer / Admin | `{ device? }` / `{ device?, policy: ReauthPolicy }` | `ReauthPolicy` | — / `WRITES_FS` |
| `device.stepup.challenge` | Command | Viewer | `{ capability }` | `{ challenge, methods, allow_credentials? }` | — |
| `device.stepup.verify` | Command | Viewer | `{ challenge, assertion }` | `{ verified_at, freshness_s }` | — |

² authorized by the pairing token, which the dispatch layer maps to a
short-lived `Admin` grant scoped by `capabilities = {device.pair.complete}`
([13 §4.1](13-security.md#41-credential-scope)) — a use of the existing scope
mechanism, not a new one.

### `instance.registry.*`

| Name | Kind | Role | Input | Output | Effects |
|---|---|---|---|---|---|
| `instance.registry.get` | Query | Viewer | `{ since_epoch? }` | `{ registry_epoch, devices, instances, prefs }` or `{ unchanged: true }` | — |
| `instance.registry.add` | Command | Admin | `{ label, endpoints, instance_pub }` | `InstanceRegistration` | `WRITES_FS` |
| `instance.registry.remove` | Command | Admin | `{ instance }` | `{ registry_epoch }` | `DESTRUCTIVE` |
| `instance.registry.set_home` | Command | Admin | `{ instance }` | `{ home, registry_epoch, passkey_reregistration_required: bool }` | `WRITES_FS`, `NETWORK` |
| `instance.registry.import` | Command | Admin | `{ snapshot, signature }` | `{ registry_epoch }` | `WRITES_FS` |
| `instance.registry.sync` | Command | Operator | `{}` | `{ registry_epoch, merged: u32, conflicts: [Conflict] }` | `NETWORK`, `WRITES_FS` |
| `instance.registry.revocations.get` | Query | Viewer | `{ since_epoch }` | `[Revocation]` | — |
| `instance.registry.revocations.push` | Command | Admin | `{ revocations, epoch }` | `{ applied: u32 }` | `WRITES_FS` |

Parity notes: `identity.export`/`import` and `device.pair.*` have real web
affordances (§2.6, §4.3) and TUI ones (`omt identity`, `omt device`), so none
needs a `Parity::Exempt`. `instance.registry.revocations.push` is
instance-to-instance and declares `Parity::Exempt { reason: "peer-to-peer
replication; no human surface" }`, which is listed in the generated docs like
every other exemption.

Consistency check: no capability above is `Viewer` with a `WRITES_FS`,
`DESTRUCTIVE` or `NETWORK` bit, which is the CI rule from
[13 §4](13-security.md#4-roles-and-their-mapping-onto-the-catalog). The two
`Viewer` commands (`identity.recovery.use`, `device.stepup.*`) are the
authentication path itself; `recovery.use` carries `WRITES_FS` and is therefore
declared `Admin` with an explicit pre-auth exemption in the dispatch chain rather
than being modelled as a `Viewer` capability — the exemption is a named,
enumerated entry, not a general escape hatch.

---

## 13. OPEN QUESTIONS

1. **Is `ts.net` a public suffix, and can `<tailnet>.ts.net` be an RP ID?**
   Everything in §2.1's "prefer the tailnet-wide RP ID" refinement depends on it,
   and it determines whether promoting home to another tailnet node invalidates
   passkeys. **Unverified here.** Test empirically against Safari, Chrome and
   Firefox before committing; until then, register against the full host name.
2. **Related Origin Requests coverage.** If `/.well-known/webauthn` is honoured
   widely enough, a single RP ID could cover the tailnet name *and* a reverse-
   proxied DNS name, which would materially improve §2.1. Unmeasured across the
   browsers omt targets.
3. **iOS PWA storage eviction for installed apps** is documented as exempt, but
   the failure mode in §9.3 is severe enough that it deserves a real measurement
   over months, not a doc citation. If installed PWAs *do* get evicted in
   practice, the re-pair flow moves from an edge case to a routine one and should
   be optimized accordingly.
4. **Should the home instance also hold a copy of the answered-interaction
   ledger?** §3.1 says no (registry only). But the phone's "you already answered
   this" rendering after a cache wipe currently depends on the owning instance
   being reachable. A pointer-only mirror (interaction id, instance, answer,
   timestamp — no tool input) would fix it, at the cost of the registry knowing
   something about sessions. Leaning no; revisit if the cold-start experience is
   bad in practice.
5. **Grant lifetime of 90 days** is a guess balancing offline-revocation risk
   against a device that has not seen home in a while. It interacts badly with a
   phone that only ever reaches one non-home instance. Should the *per-instance
   credential* carry a shorter TTL instead, since it can be renewed by the
   instance itself with no home involvement?
6. **Multi-user identities on one instance.** §1.2 assumes one identity per
   instance. Two humans sharing a dev box means two enrolled root keys and
   per-identity session ownership, which the session model does not have — the
   same gap [13 §11 Q6](13-security.md#11-open-questions) and
   [12 §9](12-collaboration.md#9-open-questions) record. The registry design
   above is already a `Vec<Identity>` internally so that it is an addition rather
   than a refactor (D4), but nothing exercises it.
7. **Cross-instance presence** ([12 §9 Q6](12-collaboration.md#9-open-questions))
   is now *possible*: `IdentityId` is the shared identity notion that document
   says omt does not have. Whether to use it — so the TUI can say "Vincent is on
   your machine and two others" — is a product question, and it leaks a little
   information between instances that today share nothing. Not decided.
8. **Recovery codes replicated to every instance** (§6.4) widens the blast radius
   of a single-instance disk compromise. The alternative (home-only) fails the
   "home is what I lost" case. A third option — codes stored only on instances
   the user marks as `durable` — adds a concept for a rare event. Leaning to keep
   replication; wants a second opinion.
9. **Step-up on a device with no authenticator** falls back to the identity-file
   passphrase (§7.3), which means typing a long diceware phrase on a phone. That
   is bad enough that users will disable step-up rather than use it. A per-device
   short PIN, rate-limited and stored argon2id-hashed, may be the pragmatic
   answer — but a PIN is a weaker factor and the docs would have to say so.
10. **Does the pairing verification phrase actually get compared?** §11 concedes
    the whole anti-race defence rests on a human reading four words. Worth a
    usability test; if users tap through it, consider making the *initiating*
    device require selecting the correct phrase from three options rather than
    confirming a displayed one.
