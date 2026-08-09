"""
Presence, the audit log, and how the instance is bearing up.

`docs/architecture/12-collaboration.md` §2 and §8, and `22-operations.md` §4.
Presence is first-class state because "is somebody else looking at this" is a
question people ask before they type; the audit log is what makes a shared
credential accountable; and metrics is the difference between "the process is
up" and knowing it is doing anything.
"""

from __future__ import annotations

import omt_client
from omt_client import Client, Instance


class TestPresence:
    def test_this_client_is_visible_to_itself(self, client: Client) -> None:
        clients = client.call("presence.list")["clients"]
        assert clients
        assert any(c["client"] == "integration-tests" for c in clients), clients

    def test_a_second_client_appears(self, instance: Instance, client: Client) -> None:
        # Two people in one shell interleave keystrokes and each blames the
        # other. Seeing that somebody else is here is what prevents it.
        before = len(client.call("presence.list")["clients"])
        other = omt_client.connect(instance)
        other.call("workspace.list")
        assert len(client.call("presence.list")["clients"]) == before + 1
        other.close()

    def test_each_says_what_it_is_and_what_it_may_do(self, client: Client) -> None:
        for present in client.call("presence.list")["clients"]:
            assert present["id"]
            assert present["client"]
            assert present["role"]
            assert present["since"]


class TestAudit:
    def test_a_call_is_recorded_with_who_made_it(self, client: Client) -> None:
        client.call("workspace.list")
        entries = client.call("audit.list")["entries"]
        assert any(e["capability"] == "workspace.list" for e in entries)
        for entry in entries:
            assert entry["client"]
            assert entry["at"]

    def test_a_refusal_is_recorded_too(self, client: Client) -> None:
        # A log of only what succeeded answers the wrong question: who *tried*
        # to do what is most of what an audit is for.
        client.try_call("session.snapshot", session="sess_00000000000000000000000000000000")
        entries = client.call("audit.list")["entries"]
        refused = [e for e in entries if e["capability"] == "session.snapshot" and not e["ok"]]
        assert refused, "a refused call left no trace"

    def test_a_command_carries_the_intent_it_was_made_with(
        self, client: Client
    ) -> None:
        # Which is what lets a repeat be recognised as the same intent rather
        # than counted as a second one.
        workspace = client.call("workspace.list")["workspaces"][0]["id"]
        created = client.call("session.create", workspace=workspace)
        entries = client.call("audit.list")["entries"]
        entry = next(e for e in entries if e["capability"] == "session.create")
        assert entry["intent"], entry
        client.call("session.close", session=created["session"])

    def test_a_bounded_log_says_what_it_dropped(self, client: Client) -> None:
        # A partial log presented as whole is worse than a short one.
        assert "dropped" in client.call("audit.list")


class TestMetrics:
    def test_it_counts_what_is_happening(self, client: Client) -> None:
        before = client.call("instance.metrics")
        client.call("workspace.list")
        after = client.call("instance.metrics")
        assert after["calls"] > before["calls"]
        assert after["clients"] >= 1
        assert after["started_at"]

    def test_refusals_are_counted_apart_from_calls(self, client: Client) -> None:
        before = client.call("instance.metrics")["failures"]
        client.try_call("session.snapshot", session="sess_00000000000000000000000000000000")
        assert client.call("instance.metrics")["failures"] > before

    def test_sessions_are_counted(self, client: Client) -> None:
        listed = len(client.call("session.list")["sessions"])
        assert client.call("instance.metrics")["sessions"] == listed
