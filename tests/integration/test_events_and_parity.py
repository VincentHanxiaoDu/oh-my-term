"""
The event stream, and the parity rules the whole catalog is supposed to obey.

`docs/architecture/07-remote-protocol.md` on subscribe and resume, and
`03-capability-catalog.md` on what a declaration must state. The parity tests
read the catalog the binary actually registers, not a checked-in copy, because
the question is whether *this build* keeps the promises.
"""

from __future__ import annotations

import pytest

from omt_client import Client, Instance
from test_sessions import type_into, wait_for


class TestSubscription:
    def test_a_client_can_subscribe_and_receives_what_happens(
        self, client: Client, session: str
    ) -> None:
        client.subscribe()
        type_into(client, session, "echo subscribed\r")
        seen = list(client.events(timeout=5.0))
        assert seen, "nothing arrived on the stream after a session was written to"

    def test_every_event_carries_a_position_within_its_own_stream(
        self, client: Client, session: str
    ) -> None:
        # Every durable client position is a (scope, seq) pair, so an event with
        # no position is one a resuming client cannot place — and a gap is
        # indistinguishable from a lost event.
        client.subscribe()
        type_into(client, session, "echo positioned\r")
        for event in client.events(timeout=5.0):
            assert "seq" in event, event
            assert "scope" in event, event

    def test_every_event_says_which_source_it_came_from(
        self, client: Client, session: str
    ) -> None:
        # The tier ladder is the whole honesty mechanism: an event omt inferred
        # and an event an agent told it are different claims, and only the
        # told-not-inferred ones may carry structured content.
        client.subscribe()
        type_into(client, session, "echo sourced\r")
        for event in client.events(timeout=5.0):
            assert event.get("source"), event


class TestCatalogParity:
    def test_every_capability_declares_its_effects(self, catalog: dict) -> None:
        # Effects drive UI policy, not just audit: a client cannot know to
        # confirm before invoking something that spawns a process unless it is
        # told that it does.
        for declaration in catalog["capabilities"]:
            assert "effects" in declaration, declaration["name"]

    def test_a_command_declares_an_intent_class_and_a_query_does_not(
        self, catalog: dict
    ) -> None:
        # Commands are retried by clients that lost an acknowledgement. Without
        # an intent class there is no rule for what a repeat means, and "did my
        # tap go through" gets answered by pressing it again.
        for declaration in catalog["capabilities"]:
            if declaration["kind"] == "command":
                assert declaration["intent"], declaration["name"]
                # The five classes D15 defines, and exactly those. This set
                # previously listed `idempotent` and `exactly_once`, which are
                # not in the enum at all, and omitted two that are — it passed
                # only because no capability had yet used either arm. A closed
                # vocabulary that does not match the type it mirrors checks
                # nothing.
                assert declaration["intent"]["class"] in {
                    "cas",
                    "append",
                    "raw_stream",
                    "externally_confirmed",
                    "lww_free_text",
                }, declaration
            else:
                assert declaration.get("intent") in (None, {}), declaration["name"]

    def test_every_capability_states_the_role_it_needs(self, catalog: dict) -> None:
        for declaration in catalog["capabilities"]:
            # The three roles `Role` actually has. This set previously
            # included `owner`, which is not a variant — the same dead
            # vocabulary as the intent classes above, and it passed for the
            # same reason: nothing ever used the arm.
            assert declaration["role"] in {"viewer", "operator", "admin"}

    def test_every_capability_is_documented(self, catalog: dict) -> None:
        # The doc is what a settings screen, the generated client and the
        # reference all render. An undocumented capability is one nobody can
        # discover from the outside.
        for declaration in catalog["capabilities"]:
            assert declaration["title"], declaration["name"]
            assert len(declaration["doc"]) > 20, declaration["name"]

    def test_every_capability_declares_its_parity(self, catalog: dict) -> None:
        # The parity gate is what stops a capability existing on one surface
        # only. A declaration with no parity claim is one the gate cannot check.
        for declaration in catalog["capabilities"]:
            assert declaration["parity"], declaration["name"]

    def test_a_name_is_its_group_and_verb(self, catalog: dict) -> None:
        # The name is derived, not written twice: a three-level name cannot be
        # expressed, which is the constraint that keeps the routes and the
        # generated client mechanical.
        for declaration in catalog["capabilities"]:
            assert declaration["name"] == f"{declaration['group']}.{declaration['verb']}"

    def test_no_two_capabilities_share_a_name_or_a_route(self, catalog: dict) -> None:
        names = [d["name"] for d in catalog["capabilities"]]
        routes = [d["route"] for d in catalog["capabilities"]]
        assert len(names) == len(set(names))
        assert len(routes) == len(set(routes))

    def test_the_instance_offers_exactly_what_it_registers(
        self, client: Client, catalog: dict
    ) -> None:
        # The handshake's list and the binary's own catalog must be the same
        # set, or a client greys out something that works — or worse, offers
        # something that does not.
        registered = {
            d["name"] for d in catalog["capabilities"] if not d["hidden"]
        }
        assert set(client.capabilities) == registered


class TestGeneratedClientParity:
    def test_the_typescript_client_covers_every_capability(
        self, catalog: dict
    ) -> None:
        # The generated client is one of the parity gate's outputs. A capability
        # missing from it is one the web surface cannot call, however well it
        # works over the wire — which is exactly how a feature ends up
        # implemented, tested and unreachable.
        from omt_client import REPO_ROOT

        source = (REPO_ROOT / "web" / "src" / "capabilities.ts").read_text()
        missing = [
            d["name"]
            for d in catalog["capabilities"]
            if not d["hidden"] and f"'{d['name']}'" not in source
        ]
        assert missing == [], f"not reachable from the web client: {missing}"

    def test_the_documented_capability_reference_is_current(
        self, catalog: dict
    ) -> None:
        # The reference is generated and committed, so a reader can see the
        # surface without building. One that has drifted is worse than none.
        from omt_client import REPO_ROOT

        reference = REPO_ROOT / "docs" / "reference" / "capabilities.md"
        text = reference.read_text()
        missing = [
            d["name"]
            for d in catalog["capabilities"]
            if not d["hidden"] and d["name"] not in text
        ]
        assert missing == [], f"undocumented in the catalog reference: {missing}"


class TestLiveDelivery:
    def test_a_session_created_elsewhere_arrives_on_a_subscribed_socket(
        self, instance
    ) -> None:
        # The whole point of a subscription. A client that has to ask again to
        # find out is a client polling with extra steps.
        import omt_client

        watcher = omt_client.connect(instance)
        watcher.subscribe()

        other = omt_client.connect(instance)
        workspace = other.call("workspace.list")["workspaces"][0]["id"]
        created = other.call("session.create", workspace=workspace)["session"]

        seen = list(watcher.events(timeout=8.0))
        # The scope carries a bare uuid; capability inputs carry the prefixed
        # spelling. Comparing the two directly is comparing two encodings of the
        # same id and finding them different.
        bare = created.split("_", 1)[-1]
        scopes = {
            str(e.get("scope", {}).get("session", "")).replace("-", "") for e in seen
        }
        watcher.close()
        other.close()

        assert seen, "nothing arrived after a session was created on another socket"
        assert bare in scopes, f"no event for the new session; saw {scopes}"


def scope_matches(event: dict, session: str) -> bool:
    """Whether an event belongs to a session.

    The scope carries a bare uuid and a capability input carries the prefixed
    spelling; comparing the two directly is comparing two encodings of the same
    id and finding them different — which is how a filter silently matches
    nothing and every assertion after it passes over an empty list.
    """
    bare = session.split("_", 1)[-1].replace("-", "")
    got = str(event.get("scope", {}).get("session", "")).replace("-", "")
    return got == bare


class TestOrderingAndCausality:
    """12-collaboration §5."""

    def test_one_sessions_events_are_totally_ordered(
        self, client: Client, session: str
    ) -> None:
        # G1. A subscriber that saw them out of order could not tell a lost
        # event from a late one, and every resume rule depends on this.
        client.subscribe()
        for i in range(5):
            type_into(client, session, f"echo ordered-{i}\r")
        seqs = [
            e["seq"] for e in client.events(timeout=5.0) if scope_matches(e, session)
        ]
        assert seqs, "no events for this session, so the ordering check saw nothing"
        assert seqs == sorted(seqs), seqs
        assert len(set(seqs)) == len(seqs), "no sequence number is handed out twice"

    def test_a_sequence_number_is_never_reused_within_one_scope(
        self, client: Client, session: str
    ) -> None:
        # G3 rests on this: a client that has seen N has seen everything up to
        # N for *that scope*, which is only true if N means one thing there.
        # Across scopes the counters are independent and may collide freely.
        client.subscribe()
        type_into(client, session, "echo once\r")
        first = [e["seq"] for e in client.events(timeout=5.0) if scope_matches(e, session)]
        type_into(client, session, "echo twice\r")
        second = [e["seq"] for e in client.events(timeout=5.0) if scope_matches(e, session)]
        assert first and second, "one of the batches was empty"
        assert not (set(first) & set(second)), "a seq came back a second time"

    def test_each_scope_counts_independently(self, client: Client, session: str) -> None:
        # The other half: seq 1 in a session scope and seq 1 in the instance
        # scope are different positions, and a client that pooled them would
        # think it had seen things it had not.
        client.subscribe()
        type_into(client, session, "echo scoped\r")
        by_scope: dict[str, list[int]] = {}
        for event in client.events(timeout=5.0):
            key = repr(event.get("scope"))
            by_scope.setdefault(key, []).append(event["seq"])
        assert by_scope, "nothing arrived"
        for key, seqs in by_scope.items():
            assert seqs == sorted(seqs), f"{key} is out of order: {seqs}"

    def test_two_sessions_share_no_ordering_relationship(
        self, client: Client
    ) -> None:
        # G6, a deliberate non-guarantee. A global total order would make the
        # sequence allocator a contention point across every PTY on the machine
        # for a property no UI needs -- so the two streams may interleave, and
        # each is ordered only within itself.
        workspace = client.call("workspace.list")["workspaces"][0]["id"]
        a = client.call("session.create", workspace=workspace)["session"]
        b = client.call("session.create", workspace=workspace)["session"]
        client.subscribe()
        type_into(client, a, "echo from-a\r")
        type_into(client, b, "echo from-b\r")

        events = list(client.events(timeout=5.0))
        for name, sid in (("a", a), ("b", b)):
            seqs = [e["seq"] for e in events if scope_matches(e, sid)]
            assert seqs == sorted(seqs), f"session {name} is out of order: {seqs}"

    def test_every_event_carries_a_timestamp_for_people_and_a_seq_for_machines(
        self, client: Client, session: str
    ) -> None:
        # §5.2: `ts` is for logs, never for sorting -- two events can share a
        # millisecond, so sorting by it silently reorders them. Both are
        # present, and only one of them is the order.
        client.subscribe()
        for i in range(5):
            type_into(client, session, f"echo stamped-{i}\r")
        events = [e for e in client.events(timeout=5.0) if scope_matches(e, session)]
        assert events, "nothing to check"
        assert all("ts" in e for e in events), events[0]
        assert all("seq" in e for e in events), events[0]
