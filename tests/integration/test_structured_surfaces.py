"""
The surfaces a phone is actually usable through.

`docs/architecture/08-web-client.md` §4.2 block view, §4.4 transcript view and
§4.5 search. A phone is about forty columns at a readable size and almost every
useful output is written for eighty to a hundred and twenty, so reflowing the
grid is the option that does not work. These are the ones that do.
"""

from __future__ import annotations

import pytest

from omt_client import CapabilityError, Client
from test_sessions import screen_text, type_into, wait_for


def text_of(lines: list[list[dict]]) -> str:
    """Styled runs back to plain text, for asserting on content."""
    return "\n".join("".join(run.get("text", "") for run in line) for line in lines)


class TestScrollback:
    def test_history_comes_back_as_styled_lines(
        self, client: Client, session: str
    ) -> None:
        # Structured rather than raw bytes: a browser that had to parse escape
        # sequences would need a second terminal emulator, and the two would
        # disagree about something eventually.
        type_into(client, session, "echo scrollback-probe\r")
        wait_for(lambda: "scrollback-probe" in screen_text(client, session))

        answer = client.call("session.scrollback", session=session)
        assert answer["total"] > 0
        assert "scrollback-probe" in text_of(answer["lines"])

    def test_it_is_paged_from_an_absolute_row(
        self, client: Client, session: str
    ) -> None:
        # Absolute, because a row's identity has to outlive the screen it was
        # written on — which is the whole reason a block can name its rows.
        first = client.call("session.scrollback", session=session, lines=2)
        assert len(first["lines"]) <= 2
        assert first["from"] == 0

        later = client.call("session.scrollback", session=session, **{"from": 1}, lines=2)
        assert later["from"] == 1

    def test_a_session_that_does_not_exist_is_an_error(self, client: Client) -> None:
        with pytest.raises(CapabilityError):
            client.call("session.scrollback", session="sess_nope")


class TestBlocks:
    def test_a_block_body_is_readable_as_styled_lines(
        self, client: Client, session: str
    ) -> None:
        # A block reported the *rows* its output occupied and no way to fetch
        # them, so block view could not be built at all.
        type_into(client, session, "echo block-probe\r")
        wait_for(lambda: "block-probe" in screen_text(client, session))

        blocks = client.call("session.blocks", session=session)["blocks"]
        assert blocks, "a session that has run something reported no blocks"

        body = client.call("block.body", session=session, block=blocks[0]["id"])
        assert body["total"] >= 0
        assert isinstance(body["lines"], list)

    def test_a_running_block_still_has_a_body(
        self, client: Client, session: str
    ) -> None:
        wait_for(
            lambda: client.call("session.blocks", session=session)["blocks"]
        )
        # Live tailing is the one thing somebody watches a running command for.
        # Reporting nothing until it finishes makes that impossible.
        blocks = client.call("session.blocks", session=session)["blocks"]
        running = [b for b in blocks if b["outcome"] == "running"]
        for block in running:
            body = client.call("block.body", session=session, block=block["id"])
            assert "lines" in body

    def test_paging_a_body_says_whether_more_remains(
        self, client: Client, session: str
    ) -> None:
        type_into(client, session, "printf 'a\\nb\\nc\\n'\r")
        wait_for(lambda: "c" in screen_text(client, session))
        blocks = client.call("session.blocks", session=session)["blocks"]
        assert blocks
        body = client.call(
            "block.body", session=session, block=blocks[0]["id"], lines=1
        )
        assert "truncated" in body

    def test_a_block_that_does_not_exist_is_an_error(
        self, client: Client, session: str
    ) -> None:
        with pytest.raises(CapabilityError):
            client.call("block.body", session=session, block="blck_nope")

    def test_the_list_says_whether_the_shell_marks_commands_at_all(
        self, client: Client, session: str
    ) -> None:
        # False means no blocks will ever appear, which a client should say
        # rather than showing an empty list that reads as "you have run
        # nothing".
        assert "shell_integration" in client.call("session.blocks", session=session)


class TestSearch:
    def test_a_match_comes_back_with_its_row_and_offsets(
        self, client: Client, session: str
    ) -> None:
        # Offsets, so a client scrolls to the row and highlights exactly that
        # span rather than re-finding the match and disagreeing about where it
        # is.
        type_into(client, session, "echo findable-needle\r")
        wait_for(lambda: "findable-needle" in screen_text(client, session))

        found = client.call("search.session", session=session, query="findable-needle")
        assert found["hits"], "a string on the screen was not found"
        hit = found["hits"][0]
        assert hit["len"] == len("findable-needle")
        assert "findable-needle" in hit["text"]
        assert hit["text"][hit["start"] : hit["start"] + hit["len"]] == "findable-needle"

    def test_search_is_case_insensitive_unless_asked_otherwise(
        self, client: Client, session: str
    ) -> None:
        type_into(client, session, "echo findable-needle\r")
        wait_for(lambda: "findable-needle" in screen_text(client, session))

        insensitive = client.call(
            "search.session", session=session, query="FINDABLE-NEEDLE"
        )
        assert insensitive["hits"]

        sensitive = client.call(
            "search.session",
            session=session,
            query="FINDABLE-NEEDLE",
            case_sensitive=True,
        )
        assert sensitive["hits"] == []

    def test_nothing_found_is_an_empty_answer_rather_than_an_error(
        self, client: Client, session: str
    ) -> None:
        found = client.call(
            "search.session", session=session, query="nothing-here-at-all-xyzzy"
        )
        assert found["hits"] == []
        assert found["truncated"] is False

    def test_an_empty_query_is_refused(self, client: Client, session: str) -> None:
        # It matches every position on every line: the answer is useless and
        # enormous, so it is refused rather than produced.
        with pytest.raises(CapabilityError):
            client.call("search.session", session=session, query="")

    def test_a_capped_result_says_it_was_capped(
        self, client: Client, session: str
    ) -> None:
        # Silently truncating reads as "that is all there is".
        found = client.call(
            "search.session", session=session, query="e", limit=1
        )
        assert len(found["hits"]) <= 1
        assert "truncated" in found


class TestTranscript:
    def test_a_session_with_no_agent_has_an_empty_transcript_rather_than_an_error(
        self, client: Client, session: str
    ) -> None:
        # A shell with no agent in it has nothing to transcribe, and that is a
        # true answer rather than a failure.
        answer = client.call("session.transcript", session=session)
        assert answer["entries"] == []
        assert answer["total"] == 0
        assert answer["dropped"] == 0

    def test_an_unknown_session_is_an_error(self, client: Client) -> None:
        with pytest.raises(CapabilityError):
            client.call(
                "session.transcript",
                session="sess_00000000000000000000000000000000",
            )

    def test_it_says_how_much_was_dropped_off_a_long_session(
        self, client: Client, session: str
    ) -> None:
        # A transcript that silently loses its beginning is one somebody reads
        # as the whole conversation.
        assert "dropped" in client.call("session.transcript", session=session)


class TestWhatAnAgentOffers:
    def test_nothing_is_offered_until_an_agent_says_so(
        self, client: Client, session: str
    ) -> None:
        # Never a list omt maintains: a completion popup offering a command the
        # agent in front of somebody does not have is worse than no popup.
        offers = client.call("agent.offers", session=session)
        assert offers["commands"] == []
        assert offers["tools"] == []
        assert offers["queue"] == []

    def test_an_unknown_session_is_an_error(self, client: Client) -> None:
        with pytest.raises(CapabilityError):
            client.call(
                "agent.offers", session="sess_00000000000000000000000000000000"
            )


class TestRerun:
    def test_a_block_omt_saw_the_command_of_can_be_run_again(
        self, client: Client, session: str
    ) -> None:
        type_into(client, session, "echo rerun-me\r")
        wait_for(lambda: "rerun-me" in screen_text(client, session))
        blocks = wait_for(
            lambda: client.call("session.blocks", session=session)["blocks"]
        )
        attributed = [b for b in blocks if b["attributed"] and b["command"].strip()]
        if not attributed:
            pytest.skip("this shell emits no command marks, so nothing is attributed")
        epoch = client.call("session.acquire", session=session)["epoch"]
        answer = client.call(
            "block.rerun",
            session=session,
            block=attributed[0]["id"],
            epoch=epoch,
        )
        assert answer["command"]

    def test_a_block_with_no_command_behind_it_is_refused(
        self, client: Client, session: str
    ) -> None:
        # A block opened from output alone has no command omt ever saw. Running
        # it would mean scraping the screen for something that looks like a
        # prompt, and guessing wrong there runs something nobody asked for.
        blocks = wait_for(
            lambda: client.call("session.blocks", session=session)["blocks"]
        )
        unattributed = [b for b in blocks if not b["attributed"]]
        if not unattributed:
            pytest.skip("every block here is attributed")
        epoch = client.call("session.acquire", session=session)["epoch"]
        with pytest.raises(CapabilityError) as refusal:
            client.call(
                "block.rerun",
                session=session,
                block=unattributed[0]["id"],
                epoch=epoch,
            )
        assert refusal.value.code == "precondition_failed"


class TestQueueHonesty:
    """`docs/architecture/06-agent-layer.md` §8."""

    BINDING = "bind_000000000000000000000000000000ff"

    def test_an_enqueue_needs_a_binding_not_just_a_session(
        self, client: Client
    ) -> None:
        # The session id alone is not a sufficient target. A replayed or delayed
        # enqueue against a session whose agent has since exited types the text
        # into the shell prompt, where it is executed as a command.
        _, refusal = client.try_call("queue.enqueue", binding="nonsense", state="working", text="x")
        assert refusal is not None
        assert refusal.code == "invalid_input"

    def test_an_enqueue_against_an_idle_agent_is_refused(self, client: Client) -> None:
        # There is no turn to queue behind, and what is at the prompt is a shell.
        _, refusal = client.try_call(
            "queue.enqueue", binding=self.BINDING, state="idle", text="x"
        )
        assert refusal is not None
        assert refusal.code == "precondition_failed"

    def test_a_queue_nobody_has_reported_is_unknown_not_empty(
        self, client: Client
    ) -> None:
        # An empty queue is a claim, and a reader that has never spoken must not
        # be able to make it.
        result, refusal = client.try_call("queue.list", binding=self.BINDING)
        assert refusal is None, refusal
        assert result["kind"] == "unknown"
        assert result["entries"] == []

    def test_the_flagship_agent_is_the_one_that_cannot_edit_its_queue(
        self, client: Client
    ) -> None:
        # omt holds every other agent's queue, so an edit is a row rewrite.
        # Claude Code holds its own and emits enqueue and remove and no third
        # verb, so there is no edit primitive to drive and no receipt for one.
        result, refusal = client.try_call(
            "queue.enqueue",
            binding=self.BINDING,
            state="working",
            ownership="mirrored",
            text="run the tests",
        )
        assert refusal is None, refusal
        entry = result["entry"]
        assert result["state"] == "pending", "it waits for the agent's receipt"

        _, refusal = client.try_call(
            "queue.edit",
            binding=self.BINDING,
            state="working",
            entry=entry,
            expected_version=1,
            text="different",
        )
        assert refusal is not None
        assert refusal.code == "unsupported"

        _, refusal = client.try_call(
            "queue.reorder",
            binding=self.BINDING,
            state="working",
            order=[entry],
            expected_version=1,
        )
        assert refusal is not None
        assert refusal.code == "unsupported"

    def test_a_removal_whose_text_has_moved_writes_nothing(
        self, client: Client
    ) -> None:
        # An id alone is a position, and a position in a draining queue is not a
        # stable target.
        managed = "bind_000000000000000000000000000000fe"
        result, refusal = client.try_call(
            "queue.enqueue",
            binding=managed,
            state="working",
            ownership="managed",
            text="deploy",
        )
        assert refusal is None, refusal
        entry = result["entry"]

        _, refusal = client.try_call(
            "queue.remove",
            binding=managed,
            state="working",
            entry=entry,
            expected_text="something else",
        )
        assert refusal is not None
        assert refusal.code == "precondition_failed"

        listed, refusal = client.try_call("queue.list", binding=managed)
        assert refusal is None, refusal
        assert [e["text"] for e in listed["entries"]] == ["deploy"]
        assert listed["can_rearrange"] is True, "omt owns this one"
