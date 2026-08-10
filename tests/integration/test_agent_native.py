"""
The agent-native path, end to end.

Everything about the transcript and the cards has been tested against events omt
made up. That checks the renderer against omt's own idea of what an agent says,
which is exactly the thing that can be wrong. Here a process speaks the protocol
the adapters parse, and what arrives has been through the adapter, the event
stream and the transcript store.
"""

from __future__ import annotations

import sys
from pathlib import Path

import pytest

from omt_client import Client

FAKE = str(Path(__file__).with_name("fake_agent.py"))


@pytest.fixture
def agent_session(client: Client, session: str) -> str:
    """A session with an agent's output folded into it."""
    client.call(
        "agent.connect",
        session=session,
        agent="codex",
        command=[sys.executable, FAKE],
    )
    return session


class TestWhatTheAgentSaid:
    def test_its_messages_reach_the_transcript(
        self, client: Client, agent_session: str
    ) -> None:
        entries = client.call("session.transcript", session=agent_session)["entries"]
        said = [e["text"] for e in entries if e["kind"] == "assistant"]
        assert "Reading the file." in said
        assert "Done." in said

    def test_a_tool_call_arrives_with_what_it_ran(
        self, client: Client, agent_session: str
    ) -> None:
        entries = client.call("session.transcript", session=agent_session)["entries"]
        calls = [e for e in entries if e["kind"] == "tool_call"]
        assert calls, entries
        assert any("cat README.md" in str(c.get("input")) for c in calls), calls

    def test_the_result_is_correlated_with_the_call_that_produced_it(
        self, client: Client, agent_session: str
    ) -> None:
        # Rendered apart, a call and its outcome have output between them and
        # the one question anybody has — did that work — needs scrolling.
        entries = client.call("session.transcript", session=agent_session)["entries"]
        calls = {e["call"] for e in entries if e["kind"] == "tool_call"}
        results = {e["call"] for e in entries if e["kind"] == "tool_result"}
        assert calls & results, (calls, results)

    def test_a_file_the_agent_changed_is_reported(
        self, client: Client, agent_session: str
    ) -> None:
        entries = client.call("session.transcript", session=agent_session)["entries"]
        changed = [e for e in entries if e["kind"] == "file_changed"]
        assert any(e["path"].endswith("README.md") for e in changed), entries

    def test_the_transcript_is_in_the_order_it_happened(
        self, client: Client, agent_session: str
    ) -> None:
        # A transcript out of order is a conversation nobody can follow.
        entries = client.call("session.transcript", session=agent_session)["entries"]
        said = [i for i, e in enumerate(entries) if e.get("text") == "Reading the file."]
        done = [i for i, e in enumerate(entries) if e.get("text") == "Done."]
        assert said and done and said[0] < done[0]


class TestWhatOmtDoesNotUnderstand:
    def test_an_unmapped_message_is_reported_by_name_rather_than_dropped(
        self, client: Client, session: str
    ) -> None:
        # An adapter that silently drops what it cannot model makes omt look
        # like it is observing when it is guessing.
        out = client.call(
            "agent.connect",
            session=session,
            agent="codex",
            command=[
                sys.executable,
                "-c",
                'import json,sys; sys.stdout.write(json.dumps('
                '{"jsonrpc":"2.0","method":"nothing/omt/models","params":{}}) + "\\n")',
            ],
        )
        assert out["unmapped"], out


class TestTheSessionAfterwards:
    def test_the_session_now_has_a_thread(
        self, client: Client, agent_session: str
    ) -> None:
        # Which is what routes somebody to the conversation rather than the grid.
        threads = client.call("agent.threads", session=agent_session)
        assert "threads" in threads

    def test_what_the_agent_offers_is_read_from_the_agent(
        self, client: Client, agent_session: str
    ) -> None:
        offers = client.call("agent.offers", session=agent_session)
        assert "commands" in offers


class TestTheSessionIsRecognisedAsAnAgentSession:
    def test_it_reports_a_thread_once_an_agent_has_spoken(
        self, client: Client, agent_session: str
    ) -> None:
        # Every surface reads this to decide whether it is looking at an agent.
        # Without it the conversation stays disabled saying "nothing in this
        # session is an agent" while the agent's own words sit in the transcript.
        threads = client.call("agent.threads", session=agent_session)["threads"]
        assert threads, "an agent ran a whole turn and the session reports no thread"

    def test_the_thread_says_what_it_is_doing(
        self, client: Client, agent_session: str
    ) -> None:
        for thread in client.call("agent.threads", session=agent_session)["threads"]:
            assert thread["state"]
            assert "is_subagent" in thread
