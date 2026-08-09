"""
Who holds what.

`docs/architecture/23-identity-and-devices.md`. A credential answers whether a
token may do something. An identity answers whose phone it is — and that second
question is what makes "revoke everything that person has" expressible at all.

`omt-identity` implemented this model and had no callers: it could answer
"tell me about this device", which needs an id somebody already holds, and could
not answer "who is there".
"""

from __future__ import annotations

import pytest

import omt_client
from omt_client import CapabilityError, Client, Instance


class TestIdentities:
    def test_a_device_that_joined_belongs_to_somebody(self, client: Client) -> None:
        identities = client.call("identity.list")["identities"]
        assert identities, "a device joined and belongs to nobody"
        assert any(i["devices"] for i in identities), identities

    def test_a_device_says_whether_a_key_backs_it(self, client: Client) -> None:
        # False for a browser, and recorded as false rather than papered over:
        # a key in local storage is exfiltrated with the token it would protect.
        for identity in client.call("identity.list")["identities"]:
            for device in identity["devices"]:
                assert isinstance(device["bound"], bool)
                assert device["name"]
                assert device["registered_at"]

    def test_every_device_of_one_instance_belongs_to_its_owner(
        self, client: Client, instance: Instance
    ) -> None:
        # One instance has one owner — whoever started it — and every device
        # that joins is theirs.
        made = client.call("invite.create")
        omt_client.exchange_invite(instance, made["invite"], "a second phone")

        identities = client.call("identity.list")["identities"]
        assert len(identities) == 1, identities
        names = [d["name"] for d in identities[0]["devices"]]
        assert "a second phone" in names

    def test_revoking_a_person_takes_every_device_they_hold(
        self, client: Client, instance: Instance
    ) -> None:
        # Device by device means somebody who has left keeps whichever one was
        # missed, and there is no moment at which their access is known to be
        # gone.
        fresh = omt_client.start_instance(instance.cwd)
        try:
            owner_token = omt_client.exchange_invite(fresh, fresh.token, "the first")
            owner = omt_client.connect(fresh, token=owner_token)

            second = omt_client.exchange_invite(
                fresh, owner.call("invite.create")["invite"], "the second"
            )
            # Both work before.
            other = omt_client.connect(fresh, token=second)
            other.close()

            identity = owner.call("identity.list")["identities"][0]
            revoked = owner.call("identity.revoke", identity=identity["id"])
            assert revoked["devices"] >= 2

            # And neither works after.
            with pytest.raises(CapabilityError):
                omt_client.connect(fresh, token=second)
            with pytest.raises(CapabilityError):
                omt_client.connect(fresh, token=owner_token)
            owner.close()
        finally:
            omt_client.stop_instance(fresh)

    def test_revoking_somebody_who_is_not_there_is_an_error(
        self, client: Client
    ) -> None:
        with pytest.raises(CapabilityError):
            client.call(
                "identity.revoke", identity="idty_00000000000000000000000000000000"
            )

    def test_an_identity_that_is_not_an_identity_is_refused(
        self, client: Client
    ) -> None:
        with pytest.raises(CapabilityError):
            client.call("identity.revoke", identity="not-an-id")
