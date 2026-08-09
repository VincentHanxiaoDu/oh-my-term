"""
Joining an instance.

`docs/architecture/07-remote-protocol.md` §1.3 and `13-security.md` §3.2. An
invite is a signed, expiring, single-exchange token — it is *not* a credential,
it is the right to obtain one. The distinction is the whole feature: printed as
a credential, which is what the invite link used to carry, the QR on somebody's
screen is permanent access for anybody who ever saw it, with nothing to revoke
short of the token itself.
"""

from __future__ import annotations

import json
import uuid

import pytest
from websocket import create_connection  # type: ignore[import-untyped]

import omt_client
from omt_client import CapabilityError, Instance


def join(instance: Instance, invite: str, name: str = "a phone") -> dict:
    """Exchange an invite on a socket that has no credential."""
    socket = create_connection(instance.ws_url, timeout=15)
    try:
        socket.send(
            json.dumps(
                {
                    "t": "join_exchange",
                    "request": {"device": str(uuid.uuid4()), "n": 1},
                    "invite": invite,
                    "device": {"name": name, "platform": "tests"},
                }
            )
        )
        return json.loads(socket.recv())
    finally:
        socket.close()


def test_the_printed_invite_is_an_invite_and_not_a_credential(
    instance: Instance,
) -> None:
    # Different prefix on purpose: an invite and a credential being
    # indistinguishable in a log, a paste or a screenshot is how one gets
    # treated as the other.
    assert instance.token.startswith("omt_i_")


def test_an_invite_cannot_be_used_as_a_credential(instance: Instance) -> None:
    with pytest.raises(CapabilityError) as refusal:
        omt_client.connect(instance, token=instance.token)
    assert refusal.value.code == "unauthorized"


def test_exchanging_it_produces_a_credential_that_works(instance: Instance) -> None:
    answer = join(instance, instance.token)
    assert answer["t"] == "join_credential"
    assert answer["token"].startswith("omt_c_")
    # Admin: this invite was printed on the machine's own terminal, and whoever
    # read it is standing at it. Minted as an operator — which it was — the
    # first device could never invite a second or revoke a lost one, so device
    # management existed and nothing could reach it.
    assert answer["role"] == "admin"

    client = omt_client.connect(instance, token=answer["token"])
    assert client.call("workspace.list")["workspaces"]
    client.close()


def test_the_answer_says_what_was_joined(instance: Instance) -> None:
    # So a device list shows a name somebody recognises rather than a URL.
    fresh = omt_client.start_instance(instance.cwd)
    try:
        answer = join(fresh, fresh.token)
        assert answer["instance"]["name"]
        assert answer["instance"]["proto"] >= 1
        assert answer["instance"]["version"]
    finally:
        omt_client.stop_instance(fresh)


def test_an_invite_can_only_be_exchanged_once(instance: Instance) -> None:
    # An invite is handed around by being *shown*, so the only safe assumption
    # is that more than one person saw it.
    fresh = omt_client.start_instance(instance.cwd)
    try:
        first = join(fresh, fresh.token)
        assert first["t"] == "join_credential"

        second = join(fresh, fresh.token)
        assert second["t"] == "goodbye", second
    finally:
        omt_client.stop_instance(fresh)


def test_an_invite_nobody_minted_is_refused(instance: Instance) -> None:
    answer = join(instance, "omt_i_notarealinviteatallxxxxxxxxxxxx")
    assert answer["t"] == "goodbye"


def test_a_connection_with_no_credential_can_do_nothing_else(
    instance: Instance,
) -> None:
    # It exists to exchange an invite, and that is all. Anything else must be
    # refused whatever role the connection happens to carry internally.
    socket = create_connection(instance.ws_url, timeout=15)
    try:
        socket.send(
            json.dumps(
                {"t": "hello", "proto": 1, "client": "tests", "token": None}
            )
        )
        answer = json.loads(socket.recv())
        assert answer["t"] == "goodbye"
        assert answer["reason"] == "unauthorized"
    finally:
        socket.close()


def test_two_devices_get_two_credentials(instance: Instance) -> None:
    # Per device, so "this phone was stolen" is something somebody can act on
    # without changing every other device's access.
    first = omt_client.start_instance(instance.cwd)
    second = omt_client.start_instance(instance.cwd)
    try:
        phone = join(first, first.token, "a phone")["token"]
        laptop = join(second, second.token, "a laptop")["token"]
        assert phone != laptop
    finally:
        omt_client.stop_instance(first)
        omt_client.stop_instance(second)
