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


class TestTheTwoHighConsequenceCapabilities:
    """13-security §7."""

    def test_writing_to_a_pty_needs_more_than_a_viewer_credential(
        self, client: Client, catalog: dict
    ) -> None:
        # A remote PTY write is remote code execution as the user, by design.
        for declaration in catalog["capabilities"]:
            if "writes_pty" in [e.lower() for e in declaration.get("effects", [])]:
                assert declaration["role"] != "viewer", declaration["name"]

    def test_omt_ships_no_second_permission_model(self, catalog: dict) -> None:
        # No destructive classifier, no approvable-tool allow-list, no
        # auto-approve. Each would be a second permission model with its own
        # bugs and a false sense that omt is protecting somebody in cases it
        # cannot see.
        forbidden = (
            "auto_approve",
            "approve_all",
            "classify_danger",
            "deny_destructive",
            "approvable_tools",
        )
        for declaration in catalog["capabilities"]:
            for word in forbidden:
                assert word not in declaration["name"], declaration["name"]

    def test_an_agents_own_options_come_back_verbatim(
        self, client: Client, session: str
    ) -> None:
        # If the CLI offered something, omt surfaces it as the agent resolved
        # it. If it offered nothing, omt does not invent anything — a shell
        # session has no commands, and an empty list is the honest answer
        # rather than a set omt made up.
        out = client.call("agent.offers", session=session)
        assert out["commands"] == []
        assert out["tools"] == []
        assert out["models"] == []


class TestWhatTheHandshakeOffers:
    """13-security §3. The methods are advertised, not guessed."""

    def test_the_welcome_says_how_to_authenticate(self, instance) -> None:
        # A client that tried a bearer token against an instance with no bearer
        # backend would get a refusal indistinguishable from a wrong token, and
        # the person holding it would retype a password that never worked.
        import omt_client

        connection = omt_client.connect(instance)
        try:
            assert connection.auth_methods, connection.auth_methods
        finally:
            connection.close()

    def test_tailnet_identity_is_not_offered_on_a_loopback_socket(
        self, instance
    ) -> None:
        # Offering it there would let anything local claim to be a tailnet
        # peer, which is the whole authentication bypassed by asserting it.
        import omt_client

        connection = omt_client.connect(instance)
        try:
            assert "tailnet" not in connection.auth_methods
        finally:
            connection.close()

    def test_every_advertised_method_is_one_omt_knows(self, instance) -> None:
        # A closed set. A server accepting an unknown method name would be
        # accepting whatever the client decided that name meant.
        import omt_client

        connection = omt_client.connect(instance)
        try:
            known = {"bearer", "password", "invite", "tailnet", "device_grant"}
            assert set(connection.auth_methods) <= known
        finally:
            connection.close()
