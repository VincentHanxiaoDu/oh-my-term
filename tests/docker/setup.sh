#!/bin/sh
# Prepare the SSH fixture.
#
# Generates the keypair if it is absent. Neither half is tracked: a repository
# that has never held a private key cannot leak one, and the public half costs
# nothing to regenerate alongside it.
set -e
cd "$(dirname "$0")"
if [ ! -f keys/id_test ]; then
    mkdir -p keys
    ssh-keygen -t ed25519 -f keys/id_test -N '' -C omt-test -q
    chmod 600 keys/id_test
    echo "generated keys/id_test"
fi
docker compose up -d --build
