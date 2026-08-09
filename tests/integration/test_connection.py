"""
Connecting to an instance.

`docs/architecture/07-remote-protocol.md` — the handshake, the credential, and
what a client is told up front.
"""

from __future__ import annotations

import pytest

import omt_client
from omt_client import CapabilityError, Client, Instance


def test_the_handshake_states_the_protocol_both_sides_will_use(client: Client) -> None:
    assert client.role in {"operator", "viewer", "owner", "admin"}


def test_a_client_is_told_the_whole_capability_set_up_front(client: Client) -> None:
    # So it presents the intersection rather than discovering a gap by calling
    # into it and failing in front of somebody.
    assert "session.create" in client.capabilities
    assert len(client.capabilities) > 1


def test_the_catalog_hash_lets_a_client_check_its_cache(client: Client) -> None:
    assert client.catalog_hash


def test_a_wrong_credential_is_refused(instance: Instance) -> None:
    with pytest.raises(CapabilityError) as refusal:
        omt_client.connect(instance, token="omt_c_notarealtokenatall")
    assert refusal.value.code == "unauthorized"


def test_health_says_the_process_is_alive_and_nothing_else(instance: Instance) -> None:
    # Deliberately unauthenticated and deliberately uninformative: a load
    # balancer needs to know the process is up, not the version or the session
    # count.
    import urllib.request

    with urllib.request.urlopen(f"{instance.url}/api/health", timeout=5) as answer:
        assert answer.status == 200
        assert answer.read().decode() == "ok"


def test_the_catalog_route_needs_a_credential(instance: Instance) -> None:
    import urllib.error
    import urllib.request

    with pytest.raises(urllib.error.HTTPError) as refused:
        urllib.request.urlopen(f"{instance.url}/api/catalog", timeout=5)
    assert refused.value.code == 401


def test_the_client_itself_is_served_without_one(instance: Instance) -> None:
    # The page that asks for a token cannot require one to load.
    import urllib.request

    with urllib.request.urlopen(instance.url, timeout=5) as answer:
        assert answer.status == 200
        assert b"<div id=\"app\">" in answer.read()
