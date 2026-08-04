#!/bin/sh
# Install the test public key with ownership and modes sshd will accept.
#
# A bind-mounted authorized_keys carries the *host's* uid, and sshd refuses it
# with "bad ownership or modes" — which passes locally whenever the host uid
# happens to match and fails everywhere else. Writing the file inside the
# container makes it correct by construction rather than by coincidence.
set -e

if [ -z "$OMT_TEST_PUBKEY" ]; then
    echo "OMT_TEST_PUBKEY is not set; the fixture would accept no logins" >&2
    exit 1
fi

install -d -m 700 -o omtuser -g omtuser /home/omtuser/.ssh
printf '%s\n' "$OMT_TEST_PUBKEY" > /home/omtuser/.ssh/authorized_keys
chown omtuser:omtuser /home/omtuser/.ssh/authorized_keys
chmod 600 /home/omtuser/.ssh/authorized_keys

exec "$@"
