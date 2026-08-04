# The SSH fixture

A real remote host for omt's SSH tests: Alpine on musl, plain `sshd`, and
deliberately **no omt installed**. The tests bring omt to it, which is the part
worth testing — an image with omt baked in would pass whether or not the
bootstrap works.

musl on `aarch64` is not an arbitrary choice. It is the target VS Code
Remote-SSH cannot serve at all (its server needs glibc ≥ 2.28), and it is what
omt's static binary is supposed to make ordinary. Testing against glibc only
would leave the interesting claim unverified.

## Running

```sh
./tests/docker/setup.sh          # generates the keypair, then starts the host
cargo test -p omt --test ssh_remote -- --ignored
docker compose -f tests/docker/compose.yml down
```

The tests are `#[ignore]` by default so `cargo test` stays fast and works with
no Docker. CI runs them explicitly.

## Keys

**Neither half of the keypair is tracked.** `setup.sh` generates it, and CI
generates its own per run. A repository that has never held a private key cannot
leak one, and regenerating the public half costs nothing — tracking it would buy
only the risk that someone later "helpfully" commits its sibling.

The container binds to `127.0.0.1:2222` only. A test fixture listening on every
interface is a test fixture somebody eventually finds.
