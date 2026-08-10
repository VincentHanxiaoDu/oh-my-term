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
