"""The bind policy, enforced in code rather than documented in prose.

`docs/architecture/13-security.md` §2.
"""

from __future__ import annotations

import pytest

from omt_client import Client


class TestBindPolicy:
    def test_the_default_needs_no_configuration_at_all(self, client: Client) -> None:
        # Or the first thing anybody learns about omt is a security error.
        out = client.call("security.bind_check", binds=["unix", "loopback"])
        assert out["may_start"] is True
        assert out["refusals"] == []

    def test_an_open_port_without_authentication_refuses_to_start(
        self, client: Client
    ) -> None:
        # A warning about an open port is printed once, into a log nobody
        # reads, on a machine that is now reachable from the network.
        out = client.call("security.bind_check", binds=["0.0.0.0"])
        assert out["may_start"] is False
        assert any("without authentication" in r["problem"] for r in out["refusals"])

    def test_every_refusal_says_what_to_do_about_it(self, client: Client) -> None:
        # A daemon that exits saying only "insecure configuration" leaves
        # somebody to guess, and the guess is usually the one that turns the
        # check off.
        out = client.call("security.bind_check", binds=["192.168.1.10"])
        assert out["refusals"]
        assert all(r["remedy"] for r in out["refusals"])

    def test_all_the_problems_come_back_not_just_the_first(
        self, client: Client
    ) -> None:
        out = client.call("security.bind_check", binds=["192.168.1.10"])
        assert len(out["refusals"]) == 2

    def test_plaintext_needs_an_explicit_proxy_declaration(
        self, client: Client
    ) -> None:
        # "There is probably a proxy" is how a plaintext port ends up on the
        # internet. Someone has to write it down.
        without = client.call(
            "security.bind_check", binds=["192.168.1.10"], auth_backend=True
        )
        assert without["may_start"] is False
        with_proxy = client.call(
            "security.bind_check",
            binds=["192.168.1.10"],
            auth_backend=True,
            trust_proxy=True,
        )
        assert with_proxy["may_start"] is True

    def test_a_tailnet_bind_needs_identity_from_somewhere(
        self, client: Client
    ) -> None:
        assert client.call("security.bind_check", binds=["tailnet"])["may_start"] is False
        assert (
            client.call("security.bind_check", binds=["tailnet"], tailnet_identity=True)[
                "may_start"
            ]
            is True
        )

    def test_an_interface_that_names_loopback_is_loopback(
        self, client: Client
    ) -> None:
        # Refusing it would be pedantry, and pedantry in a security check
        # teaches people to work around the check.
        for addr in ("127.0.0.1", "::1"):
            assert client.call("security.bind_check", binds=[addr])["may_start"] is True

    def test_binding_everything_with_a_default_role_warns(
        self, client: Client
    ) -> None:
        out = client.call(
            "security.bind_check",
            binds=["0.0.0.0"],
            auth_backend=True,
            tls=True,
            default_role="viewer",
        )
        assert out["may_start"] is True
        assert out["warnings"]

    def test_something_that_is_not_an_address_is_refused(
        self, client: Client
    ) -> None:
        with pytest.raises(Exception) as caught:
            client.call("security.bind_check", binds=["nonsense"])
        assert "loopback" in str(caught.value)
