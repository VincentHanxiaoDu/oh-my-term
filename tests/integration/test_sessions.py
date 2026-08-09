"""
Sessions: creating, writing, reading, resizing, restarting, closing.

`docs/architecture/05-session-model.md` and the `session.*` capabilities.
"""

from __future__ import annotations

import time

import pytest

from omt_client import CapabilityError, Client


def wait_for(predicate, timeout: float = 15.0, interval: float = 0.05):
    """Poll until something is true, returning what it produced.

    A pty is asynchronous: the shell starts, prints a prompt and echoes a
    command on its own schedule. Sleeping a fixed amount instead either makes
    the suite slow or makes it flaky, and usually both.
    """
    deadline = time.monotonic() + timeout
    last = None
    while time.monotonic() < deadline:
        last = predicate()
        if last:
            return last
        time.sleep(interval)
    return last


def screen_text(client: Client, session: str) -> str:
    snapshot = client.call("session.snapshot", session=session)
    return "\n".join(
        "".join(run.get("text", "") for run in row) for row in snapshot["rows"]
    )


def type_into(client: Client, session: str, text: str) -> None:
    epoch = client.call("session.acquire", session=session)["epoch"]
    client.call("session.write", session=session, text=text, epoch=epoch)


def test_a_workspace_is_open_where_the_instance_started(client: Client) -> None:
    # A browser has no filesystem of its own to point at, so an instance with
    # no workspace hands it a form it cannot fill in.
    workspaces = client.call("workspace.list")["workspaces"]
    assert len(workspaces) >= 1


def test_a_session_can_be_created_and_appears_in_the_list(client: Client) -> None:
    workspace = client.call("workspace.list")["workspaces"][0]["id"]
    created = client.call("session.create", workspace=workspace)
    listed = client.call("session.list")["sessions"]
    assert created["session"] in [s["id"] for s in listed]


def test_a_session_leaves_starting_once_its_program_speaks(
    client: Client, session: str
) -> None:
    # A state that never changes is a label carrying no information: a roster of
    # live shells all reading "starting" tells nobody anything.
    def running() -> bool:
        return any(
            s["id"] == session and s["state"] != "starting"
            for s in client.call("session.list")["sessions"]
        )

    assert wait_for(running), "the session never stopped calling itself starting"


def test_typing_reaches_the_shell_and_the_echo_comes_back(
    client: Client, session: str
) -> None:
    type_into(client, session, "echo omt-integration\r")
    assert wait_for(
        lambda: "omt-integration" in screen_text(client, session)
    ), "what was typed never appeared on the screen"


def test_a_write_without_the_writer_token_is_refused(
    client: Client, session: str
) -> None:
    # Silently accepting it would let two clients interleave keystrokes into one
    # shell, which is how half a command gets run.
    with pytest.raises(CapabilityError):
        client.call("session.write", session=session, text="x", epoch=999_999)


def test_the_writer_token_can_be_released_and_taken_again(
    client: Client, session: str
) -> None:
    first = client.call("session.acquire", session=session)["epoch"]
    client.call("session.release", session=session)
    second = client.call("session.acquire", session=session)["epoch"]
    assert second >= first


def test_resizing_changes_the_grid_the_program_sees(
    client: Client, session: str
) -> None:
    client.call("session.resize", session=session, cols=40, rows=12)
    snapshot = wait_for(
        lambda: (
            client.call("session.snapshot", session=session)
            if client.call("session.snapshot", session=session)["cols"] == 40
            else None
        )
    )
    assert snapshot["cols"] == 40


def test_a_session_reports_its_own_screen(client: Client, session: str) -> None:
    snapshot = client.call("session.snapshot", session=session)
    assert snapshot["cols"] > 0
    assert "cursor" in snapshot
    # The cursor must be on the screen it is reported with, or a client draws it
    # outside the grid.
    column, row = snapshot["cursor"]
    assert 0 <= column <= snapshot["cols"]
    assert 0 <= row <= snapshot["rows_count"]


def test_a_session_can_be_read_back_as_text(client: Client, session: str) -> None:
    type_into(client, session, "echo readable\r")
    wait_for(lambda: "readable" in screen_text(client, session))
    read = client.call("session.read", session=session)
    assert isinstance(read, dict)


def test_restarting_a_live_session_is_refused_rather_than_killing_it(
    client: Client, session: str
) -> None:
    # `session.restart` respawns a session *restored from a snapshot*. Applying
    # it to a live one would kill a running process to no purpose, so it is
    # refused — the interesting part is that it says so rather than doing it.
    with pytest.raises(CapabilityError) as refusal:
        client.call("session.restart", session=session)
    assert refusal.value.code == "precondition_failed"


def test_closing_a_session_removes_it(client: Client) -> None:
    workspace = client.call("workspace.list")["workspaces"][0]["id"]
    created = client.call("session.create", workspace=workspace)["session"]
    client.call("session.close", session=created)
    listed = [s["id"] for s in client.call("session.list")["sessions"]]
    assert created not in listed


def test_a_session_that_does_not_exist_is_an_error_not_an_empty_answer(
    client: Client,
) -> None:
    # Answering an unknown session with an empty screen would have a client draw
    # a blank terminal for a typo.
    with pytest.raises(CapabilityError):
        client.call("session.snapshot", session="sess_doesnotexist")
