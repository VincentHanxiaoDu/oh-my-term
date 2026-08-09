"""
The machinery that existed and nothing could reach.

Three crates were written, tested and unreachable: `omt-identity` knew about
devices and could revoke them, `omt-media` could identify a blob, and
`omt-config`'s importer could read settings out of the tools people already
use. A feature no client can call is a feature nobody has, and this is the
recurring failure this project keeps finding.
"""

from __future__ import annotations

import base64

import pytest

import omt_client
from omt_client import CapabilityError, Client, Instance


class TestDevices:
    def test_a_paired_device_is_listed(self, client: Client) -> None:
        # Pairing mints a credential per device. One nobody can see is one
        # nobody revokes, which makes "individually revocable" a claim rather
        # than a feature.
        devices = client.call("device.list")["devices"]
        assert devices, "a device joined and nothing lists it"
        assert any(d["label"] == "the tests" for d in devices), devices

    def test_each_says_what_it_is_and_whether_it_is_bound(
        self, client: Client
    ) -> None:
        for device in client.call("device.list")["devices"]:
            assert device["id"]
            assert device["role"]
            # False for a browser, honestly: a key in local storage is
            # exfiltrated with the token it would protect.
            assert isinstance(device["bound"], bool)
            assert device["revoked"] is False

    def test_another_device_can_be_invited_without_restarting(
        self, client: Client, instance: Instance
    ) -> None:
        # The invite printed at startup is single-exchange, so without this the
        # only way to add a phone is restarting the instance everybody else is
        # connected to.
        made = client.call("invite.create")
        assert made["invite"].startswith("omt_i_")
        assert made["expires_in_secs"] > 0

        second = omt_client.exchange_invite(instance, made["invite"], "a second phone")
        assert second.startswith("omt_c_")

        other = omt_client.connect(instance, token=second)
        assert other.call("workspace.list")["workspaces"]
        other.close()

    def test_revoking_one_leaves_the_others_alone(
        self, client: Client, instance: Instance
    ) -> None:
        # The whole reason credentials are per device: "this phone was stolen"
        # must not mean everybody signs in again.
        made = client.call("invite.create")
        doomed = omt_client.exchange_invite(instance, made["invite"], "a lost phone")
        lost = omt_client.connect(instance, token=doomed)
        assert lost.call("workspace.list")["workspaces"]
        lost.close()

        target = next(
            d for d in client.call("device.list")["devices"] if d["label"] == "a lost phone"
        )
        assert client.call("device.revoke", device=target["id"])["revoked"] is True

        with pytest.raises(CapabilityError):
            omt_client.connect(instance, token=doomed)
        # And this connection, which is a different device, still works.
        assert client.call("workspace.list")["workspaces"]

    def test_revoking_twice_is_not_an_error(self, client: Client) -> None:
        # A second tap on a row somebody already dealt with should not look
        # like a failure.
        target = next(
            (d for d in client.call("device.list")["devices"] if d["revoked"]), None
        )
        if target is None:
            pytest.skip("nothing has been revoked here")
        assert client.call("device.revoke", device=target["id"])["revoked"] is False

    def test_revoking_something_that_is_not_there_is_an_error(
        self, client: Client
    ) -> None:
        _, refusal = client.try_call("device.revoke", device="cred_nothinghere")
        assert refusal is not None


class TestMedia:
    def test_a_blob_is_identified_from_its_bytes(self, client: Client) -> None:
        # A filename is a claim; the bytes are the evidence. Writing an
        # executable as a `.png` is what trusting the claim allows.
        png = bytes.fromhex("89504e470d0a1a0a") + b"\x00" * 32
        described = client.call(
            "media.describe", data=base64.b64encode(png).decode()
        )
        assert described["kind"] == "image"
        assert described["media_type"] == "image/png"
        assert described["filename"].endswith(".png")
        assert described["digest"]
        assert described["bytes"] == len(png)

    def test_something_that_is_not_an_image_is_not_called_one(
        self, client: Client
    ) -> None:
        described = client.call(
            "media.describe", data=base64.b64encode(b"just some text\n").decode()
        )
        assert described["kind"] != "image"

    def test_an_empty_blob_is_refused(self, client: Client) -> None:
        with pytest.raises(CapabilityError):
            client.call("media.describe", data="")

    def test_bytes_that_are_not_base64_are_refused(self, client: Client) -> None:
        with pytest.raises(CapabilityError):
            client.call("media.describe", data="!!!not base64!!!")


class TestImportingSettings:
    def test_settings_from_another_tool_are_translated(self, client: Client) -> None:
        imported = client.call(
            "config.import",
            source="vscode",
            settings={"terminal.integrated.fontSize": 15},
        )
        assert imported["values"], imported

    def test_what_did_not_map_is_reported_rather_than_dropped(
        self, client: Client
    ) -> None:
        # An import that silently ignores half a config leaves somebody
        # wondering why their font is wrong, looking in the wrong place.
        #
        # Only keys the source is *about*: VS Code's settings file has hundreds
        # of editor keys, and listing every one would bury the handful somebody
        # cares about. So this uses a `terminal.` key omt has nowhere to put.
        imported = client.call(
            "config.import",
            source="vscode",
            settings={"terminal.integrated.gpuAcceleration": "on"},
        )
        assert imported["unmapped"], imported
        assert imported["unmapped"][0]["why"]

    def test_it_says_where_that_tool_keeps_its_settings(self, client: Client) -> None:
        # So a client can offer to read one rather than asking somebody to go
        # and find it.
        assert client.call("config.import", source="vscode", settings={})["usual_paths"]

    def test_a_source_omt_cannot_read_is_refused_rather_than_guessed(
        self, client: Client
    ) -> None:
        # Importing iTerm2's settings as VS Code's would produce a config that
        # is wrong in ways nobody can trace.
        with pytest.raises(CapabilityError):
            client.call("config.import", source="notatool", settings={})

    def test_importing_changes_nothing_by_itself(self, client: Client) -> None:
        # A query: it says what the import *would* be, and writing it is a
        # separate decision somebody makes.
        before = client.call("config.get")["values"]
        client.call(
            "config.import", source="vscode", settings={"terminal.integrated.fontSize": 20}
        )
        assert client.call("config.get")["values"] == before
