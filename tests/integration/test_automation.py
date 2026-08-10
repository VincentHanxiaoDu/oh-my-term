"""What a script needs from omt.

`docs/architecture/22-operations.md` §8.3. These are the primitives a CI job or
a watcher is built out of, so they are exercised the way one would use them --
over the wire, against a live instance -- rather than only as unit tests of the
logic behind them.
"""

from __future__ import annotations

import time

from omt_client import Client


def type_into(client: Client, session: str, text: str) -> None:
    epoch = client.call("session.acquire", session=session)["epoch"]
    client.call("session.write", session=session, text=text, epoch=epoch)


class TestCapture:
    """`session.capture` -- the thing every migrating script asks for first."""

    def test_a_capture_returns_what_the_session_printed(
        self, client: Client, session: str
    ) -> None:
        type_into(client, session, "echo automation-marker\r")
        deadline = time.time() + 10
        while time.time() < deadline:
            captured = client.call("session.capture", session=session)
            if "automation-marker" in captured["content"]:
                break
            time.sleep(0.1)
        else:
            raise AssertionError("the marker never appeared in a capture")

    def test_a_capture_says_how_much_it_did_not_return(
        self, client: Client, session: str
    ) -> None:
        # A line count alone cannot tell a complete capture from a truncated
        # one, and a script that archives the output should know which it got.
        type_into(client, session, "echo one; echo two; echo three\r")
        time.sleep(0.5)
        captured = client.call("session.capture", session=session, lines=1)
        assert captured["lines"] <= 1
        assert captured["total"] >= captured["lines"]

    def test_jsonl_carries_the_row_numbers(self, client: Client, session: str) -> None:
        # So a watcher can tell where in the scrollback a line came from.
        type_into(client, session, "echo jsonl-marker\r")
        time.sleep(0.5)
        captured = client.call("session.capture", session=session, format="jsonl")
        assert captured["format"] == "jsonl"
        if captured["content"]:
            assert '"row"' in captured["content"]

    def test_a_format_omt_does_not_write_is_refused(
        self, client: Client, session: str
    ) -> None:
        _, refusal = client.try_call("session.capture", session=session, format="yaml")
        assert refusal is not None
        assert refusal.code == "invalid_input"

    def test_capturing_a_session_that_does_not_exist_is_not_found(
        self, client: Client
    ) -> None:
        _, refusal = client.try_call(
            "session.capture", session="sess_00000000000000000000000000000000"
        )
        assert refusal is not None
        assert refusal.code == "not_found"


class TestWait:
    """`agent.wait` -- the primitive that stops every script being a sleep loop."""

    def test_waiting_on_a_session_with_no_agent_refuses_rather_than_hanging(
        self, client: Client, session: str
    ) -> None:
        # Waiting thirty minutes for an agent that was never there is exactly
        # the sleep loop this capability exists to replace.
        started = time.time()
        _, refusal = client.try_call(
            "agent.wait",
            session=session,
            until=["idle"],
            timeout_ms=600_000,
        )
        assert refusal is not None
        assert refusal.code == "precondition_failed"
        assert time.time() - started < 10, "it refused immediately, not after the timeout"

    def test_a_wait_with_no_condition_is_a_usage_error(
        self, client: Client, session: str
    ) -> None:
        # A wait with nothing to wait for is a sleep, and a script that wrote
        # one meant something else.
        _, refusal = client.try_call(
            "agent.wait", session=session, until=[], timeout_ms=1_000
        )
        assert refusal is not None
        assert refusal.code == "invalid_input"

    def test_a_condition_outside_the_closed_set_is_refused(
        self, client: Client, session: str
    ) -> None:
        # Otherwise a typo waits for the full timeout and reports nothing.
        _, refusal = client.try_call(
            "agent.wait", session=session, until=["blockd"], timeout_ms=1_000
        )
        assert refusal is not None
        assert refusal.code == "invalid_input"

    def test_waiting_on_a_session_that_does_not_exist_is_not_found(
        self, client: Client
    ) -> None:
        _, refusal = client.try_call(
            "agent.wait",
            session="sess_00000000000000000000000000000000",
            until=["idle"],
            timeout_ms=1_000,
        )
        assert refusal is not None
        assert refusal.code == "not_found"


class TestTheContract:
    """§8.1 -- the rules that hold for every capability."""

    def test_wait_and_capture_are_queries_that_mutate_nothing(
        self, client: Client
    ) -> None:
        # Both observe. A script granted read-only access must be able to run
        # them, which is what `kind` and `role` decide.
        catalog = client.call("instance.catalog")["entries"]
        by_name = {c["name"]: c for c in catalog}
        for name in ("agent.wait", "session.capture"):
            assert by_name[name]["role"] == "viewer", name
            assert by_name[name]["effects"] == [], name

    def test_the_automation_primitives_are_all_present(self, client: Client) -> None:
        # §8.3 lists what a script needs. A missing one sends somebody back to
        # scraping a screen.
        names = {c["name"] for c in client.call("instance.catalog")["entries"]}
        for required in (
            "session.capture",
            "agent.wait",
            "interaction.list",
            "interaction.resolve",
            "events.subscribe",
        ):
            assert required in names, required


class TestOperations:
    """22-operations §10 -- the capabilities that operate the instance."""

    def test_a_task_that_does_not_exist_is_not_found(self, client: Client) -> None:
        _, refusal = client.try_call("task.get", task="task_nope")
        assert refusal is not None
        assert refusal.code == "not_found"

    def test_the_task_list_is_empty_rather_than_missing(self, client: Client) -> None:
        # A registry with nothing in it is a real answer. A client polling it
        # should not have to tell "no jobs" from "no such capability".
        listed = client.call("task.list")
        assert listed["tasks"] == []

    def test_an_upgrade_check_reports_who_owns_the_binary(self, client: Client) -> None:
        # Overwriting a package manager's file leaves it certain it has a
        # version it does not have.
        out = client.call("upgrade.check")
        assert out["current"]
        assert out["installed_by"]
        # Nothing is downloaded and no feed is contacted, so `available` is
        # honestly unknown rather than invented.
        assert out["available"] is None

    def test_shutdown_reports_what_is_running_rather_than_taking_it_down(
        self, client: Client, session: str
    ) -> None:
        # A shutdown that takes an agent down mid-turn is the same loss an
        # upgrade refuses to cause. With nothing working it proceeds.
        out = client.call("instance.shutdown")
        assert "shutting_down" in out
        assert isinstance(out["blocked_by"], list)

    def test_probing_a_host_that_is_not_there_fails_rather_than_hanging(
        self, client: Client
    ) -> None:
        _, refusal = client.try_call("remote.probe", host="omt-nonexistent-host.invalid")
        assert refusal is not None
        assert refusal.code == "precondition_failed"

    def test_remote_probe_is_operator_because_it_makes_this_machine_connect_out(
        self, client: Client
    ) -> None:
        # A read-only shared link must not be able to make this machine reach a
        # host the caller chose. Viewer + NETWORK has no carve-out.
        by_name = {c["name"]: c for c in client.call("instance.catalog")["entries"]}
        assert by_name["remote.probe"]["role"] == "operator"
        assert "network" in [e.lower() for e in by_name["remote.probe"]["effects"]]
