"""
The agent layer: threads, interactions and interrupting.

`docs/architecture/06-agent-layer.md`. These run without a real agent binary
where they can, because the guarantees that matter most are the ones about what
omt does when it has *not* been told something: it must not guess, and it must
say why it cannot act.
"""

from __future__ import annotations

import pytest

from omt_client import CapabilityError, Client


class TestThreads:
    def test_a_session_with_no_agent_reports_no_threads_rather_than_failing(
        self, client: Client, session: str
    ) -> None:
        # A plain shell is a legitimate answer to "what agents are in here", and
        # it is not an error.
        threads = client.call("agent.threads", session=session)
        assert threads["threads"] == []

    def test_an_unknown_session_is_an_error(self, client: Client) -> None:
        with pytest.raises(CapabilityError):
            client.call("agent.threads", session="sess_00000000000000000000000000000000")


class TestInteractions:
    def test_nothing_is_waiting_on_a_fresh_instance(self, client: Client) -> None:
        assert client.call("interaction.list")["interactions"] == []

    def test_answering_something_that_does_not_exist_is_refused(
        self, client: Client
    ) -> None:
        # Not silently accepted. A client that believes it answered a card, when
        # no card was answered, leaves an agent blocked forever.
        with pytest.raises(CapabilityError):
            client.call(
                "interaction.resolve",
                interaction="intr_00000000000000000000000000000000",
                option="Yes",
            )

    def test_the_list_says_whether_omt_can_answer_each_one(
        self, client: Client, session: str
    ) -> None:
        # Answerability is read from the deliverable, never from the state: a
        # card can be open and still have no channel to answer through, and a
        # button that silently does nothing is worse than no button.
        for card in client.call("interaction.list")["interactions"]:
            assert "deliverable" in card
            if card["deliverable"] == "none":
                assert card.get("not_deliverable_because")


class TestInterrupt:
    def test_interrupting_a_session_with_no_agent_is_refused_with_a_reason(
        self, client: Client, session: str
    ) -> None:
        # For an agent with no protocol of its own, interrupting is the entire
        # remote control surface — so it has to be honest about when it cannot.
        _, refusal = client.try_call("agent.interrupt", session=session)
        if refusal is not None:
            assert refusal.message


class TestConnect:
    def test_connecting_an_agent_that_is_not_installed_fails_by_name(
        self, client: Client, session: str
    ) -> None:
        # The failure names the command that could not be started. "Something
        # went wrong" sends somebody reading omt's source instead of their PATH.
        with pytest.raises(CapabilityError) as refusal:
            client.call(
                "agent.connect",
                session=session,
                agent="codex",
                command=["omt-no-such-agent-binary"],
            )
        assert "omt-no-such-agent-binary" in refusal.value.message or refusal.value.code

    def test_every_agent_omt_claims_to_support_is_a_name_it_accepts(
        self, client: Client, session: str
    ) -> None:
        # The docs list the agents omt supports. A name in that list that the
        # instance rejects as unknown is a claim omt cannot keep.
        supported = [
            "claude_code",
            "codex",
            "opencode",
            "gemini_cli",
            "qwen_code",
            "cursor",
            "copilot",
            "goose",
            "amp",
            "aider",
            "crush",
        ]
        for agent in supported:
            _, refusal = client.try_call(
                "agent.connect",
                session=session,
                agent=agent,
                command=["omt-no-such-agent-binary"],
            )
            # It must fail on the *command*, not on the agent name.
            assert refusal is not None
            assert "is not an agent omt knows" not in refusal.message, (
                f"{agent} is documented as supported and the instance rejects it"
            )


class TestUsage:
    def test_a_session_with_no_agent_reports_no_spend_rather_than_zero_cost(
        self, client: Client, session: str
    ) -> None:
        # Cost is not invented from a token count: omt reports what an agent
        # said it spent, and nothing said anything here.
        report = client.call("usage.report")
        assert "sessions" in report or "total" in report


class TestStuckAgentDetection:
    """20-recall-and-usage §10. Working for 22 minutes looks like working for 20
    seconds, except by elapsed time."""

    def test_a_working_agent_is_not_a_stuck_one(self, client) -> None:
        # The detector has to be quiet almost always, or it is one people mute
        # and then never hear from again.
        out = client.call(
            "agent.attention",
            working_secs=20,
            baseline_working_secs=30,
            silent_secs=2,
            baseline_gap_secs=5,
        )
        assert out["needs_attention"] is False
        assert out["reasons"] == []

    def test_far_beyond_its_own_baseline_fires(self, client) -> None:
        out = client.call(
            "agent.attention", working_secs=1320, baseline_working_secs=30
        )
        assert out["needs_attention"] is True
        assert "long_working" in out["reasons"]

    def test_a_slow_agent_is_not_a_permanent_alarm(self, client) -> None:
        # Thirty seconds is an eternity for a linter and a blink for a test
        # suite, so the baseline is the binding's own.
        out = client.call(
            "agent.attention", working_secs=1500, baseline_working_secs=1200
        )
        assert out["needs_attention"] is False

    def test_every_reason_that_fired_is_reported(self, client) -> None:
        # An agent looping and burning tokens has two problems, and being told
        # about one sends somebody looking in the wrong place.
        out = client.call(
            "agent.attention",
            recent_calls=["Bash(x)"] * 4,
            consecutive_failures=5,
            last_failure="exit 1",
            tokens_since_file_change=90000,
        )
        assert len(out["reasons"]) == 3, out["reasons"]

    def test_an_unambiguous_signal_is_strong_and_a_circumstantial_one_is_weak(
        self, client
    ) -> None:
        weak = client.call("agent.attention", working_secs=1320, baseline_working_secs=30)
        assert weak["confidence"] == "weak"
        strong = client.call("agent.attention", consecutive_failures=3)
        assert strong["confidence"] == "strong"

    def test_a_first_turn_with_no_history_is_not_evidence(self, client) -> None:
        out = client.call("agent.attention", working_secs=3600, silent_secs=3600)
        assert out["needs_attention"] is False


class TestComparingParallelRuns:
    """20-recall-and-usage §9. D-R5: present facts, never judge."""

    def run(self, session, **over):
        base = {
            "session": session,
            "branch": f"work/{session}",
            "base_ref": "abc123",
            "agent": "claude_code",
            "elapsed_secs": 600,
            "files_from_git": ["src/a.rs"],
            "files_from_agent": ["src/a.rs"],
            "insertions": 40,
            "deletions": 10,
            "turns": 8,
            "tokens": 120000,
            "summary": "Added the parser",
        }
        base.update(over)
        return base

    def test_nothing_is_ranked_or_recommended(self, client) -> None:
        # A comparison that says "this one looks best" will be believed, and
        # when it is wrong nobody finds out until the other worktrees are gone.
        out = client.call("agent.compare", runs=[self.run("a"), self.run("b")])
        text = str(out).lower()
        for word in ("score", "rank", "best", "recommend", "winner"):
            assert word not in text, word

    def test_a_file_the_agent_claims_but_git_did_not_see_is_flagged(
        self, client
    ) -> None:
        # The alarming direction: the agent believes it wrote something that is
        # not in the diff.
        out = client.call(
            "agent.compare",
            runs=[self.run("a", files_from_agent=["src/a.rs", "src/imagined.rs"])],
        )
        assert out["rows"][0]["files_disagreement"] == ["src/imagined.rs"]
        assert any("disagree" in c for c in out["caveats"])

    def test_different_bases_are_called_out_as_incomparable(self, client) -> None:
        # Two numbers in the same column measured against different things.
        out = client.call(
            "agent.compare",
            runs=[self.run("a"), self.run("b", base_ref="def456")],
        )
        assert any("different base" in c for c in out["caveats"])

    def test_unreported_usage_is_unreported_not_zero(self, client) -> None:
        # Blank meaning zero would make an agent that reports nothing look like
        # the cheapest one.
        out = client.call("agent.compare", runs=[self.run("a"), self.run("b", tokens=None)])
        assert out["rows"][1]["tokens"] is None
        assert any("not zero" in c for c in out["caveats"])

    def test_the_agents_own_summary_is_verbatim(self, client) -> None:
        said = "I refactored the lexer but the tests still fail"
        out = client.call("agent.compare", runs=[self.run("a", summary=said)])
        assert out["rows"][0]["summary"] == said

    def test_comparing_nothing_is_empty_rather_than_an_error(self, client) -> None:
        out = client.call("agent.compare", runs=[])
        assert out["rows"] == []


class TestSessionTimeline:
    """20-recall-and-usage §8. Assembly, not observation."""

    def events(self, specs):
        return [
            {"seq": i, "at": 100 + i, "kind": kind, "label": label}
            for i, (kind, label) in enumerate(specs)
        ]

    def test_repeated_calls_collapse_with_a_count(self, client) -> None:
        # A raw stream for a twenty-minute turn is hundreds of entries, and
        # reading it is worse than the scrollback it replaced.
        events = self.events([("tool_call", "Bash(cargo test)")] * 7)
        out = client.call("session.timeline", events=events)
        assert len(out["entries"]) == 1
        assert out["entries"][0]["count"] == 7

    def test_two_different_calls_never_collapse(self, client) -> None:
        # A canonicaliser that ignored arguments would collapse `rm -rf build`
        # and `rm -rf /`, and the line would be a lie about what happened.
        events = self.events(
            [("tool_call", "Bash(rm -rf build)"), ("tool_call", "Bash(rm -rf /)")]
        )
        assert len(client.call("session.timeline", events=events)["entries"]) == 2

    def test_only_consecutive_repeats_collapse(self, client) -> None:
        events = self.events(
            [
                ("tool_call", "Bash(cargo test)"),
                ("file_changed", "src/a.rs"),
                ("tool_call", "Bash(cargo test)"),
            ]
        )
        assert len(client.call("session.timeline", events=events)["entries"]) == 3

    def test_the_counts_are_of_events_not_collapsed_lines(self, client) -> None:
        # Otherwise seven runs of a failing command report as one failure,
        # which is the opposite of what somebody needs to know.
        events = self.events([("failed_block", "cargo test")] * 5)
        out = client.call("session.timeline", events=events)
        assert out["failed_blocks"] == 5
        assert len(out["entries"]) == 1

    def test_a_loop_shows_up_in_the_digest(self, client) -> None:
        events = self.events([("tool_call", "Bash(cargo test)")] * 6)
        assert "×6" in client.call("session.timeline", events=events)["digest"]

    def test_nothing_happening_says_so(self, client) -> None:
        assert client.call("session.timeline", events=[])["digest"] == "Nothing happened."
