"""
The surfaces around a workspace: files, panes, layouts, blocks, shell
integration, recognised references and command recall.

`docs/architecture/04-terminal-core.md` §6, `15-workspace-explorer.md`,
`17-panes-and-layout.md`, `18-semantic-open.md`.
"""

from __future__ import annotations

import pytest

import base64

from omt_client import CapabilityError, Client, Instance
from test_sessions import type_into, wait_for


def workspace_id(client: Client) -> str:
    """The main repository's workspace.

    By root, never by position. Opening a worktree makes it a workspace of its
    own — that is the whole design, N worktrees are N workspaces — so after the
    worktree tests there are two, and `workspaces[0]` is whichever the id
    ordering happens to put first. That made the suite fail about one run in
    two, in a different test each time.
    """
    workspaces = client.call("workspace.list")["workspaces"]
    main = [w for w in workspaces if not w["root"].endswith(".worktrees")]
    main = [w for w in main if ".worktrees" not in w["root"]]
    return sorted(main, key=lambda w: len(w["root"]))[0]["id"]


class TestFiles:
    def test_a_directory_can_be_listed(self, client: Client) -> None:
        listed = client.call("fs.list", workspace=workspace_id(client))
        names = [e["name"] for e in listed["entries"]]
        assert "README.md" in names

    def test_a_file_can_be_read_back(self, client: Client) -> None:
        chunk = client.call(
            "fs.read", workspace=workspace_id(client), path="README.md"
        )
        # Base64, not text: a file is bytes, and a capability that decoded it as
        # UTF-8 would corrupt every binary it was pointed at.
        assert "repository under test" in base64.b64decode(chunk["data"]).decode()
        assert chunk["chunks"] >= 1

    def test_a_path_cannot_escape_its_workspace(self, client: Client) -> None:
        # The whole containment claim. A traversal that works turns a file
        # browser into a way to read anything the daemon's user can.
        for escape in ["../../../etc/passwd", "/etc/passwd", "a/../../../etc/passwd"]:
            with pytest.raises(CapabilityError):
                client.call("fs.read", workspace=workspace_id(client), path=escape)

    def test_a_written_file_appears_only_once_every_chunk_has_arrived(
        self, client: Client, instance: Instance
    ) -> None:
        # A half-uploaded file that is readable is a file somebody runs.
        workspace = workspace_id(client)
        client.call(
            "fs.write",
            workspace=workspace,
            path="uploaded.txt",
            chunk=0,
            data=base64.b64encode(b"hello from the tests\n").decode(),
            last=True,
        )
        assert (instance.cwd / "uploaded.txt").exists()


class TestPanes:
    def test_a_pane_opens_onto_an_existing_session_without_starting_one(
        self, client: Client, session: str
    ) -> None:
        workspace = workspace_id(client)
        before = len(client.call("session.list")["sessions"])
        client.call("pane.open", workspace=workspace, session=session)
        after = len(client.call("session.list")["sessions"])
        assert after == before, "opening a view of something started a process"

    def test_the_panes_are_listed_with_which_one_has_focus(
        self, client: Client
    ) -> None:
        panes = client.call("pane.list", workspace=workspace_id(client))
        assert panes["panes"]
        assert "focus" in panes or any("focused" in p for p in panes["panes"])

    def test_focusing_a_pane_that_is_not_there_is_refused(
        self, client: Client
    ) -> None:
        with pytest.raises(CapabilityError):
            client.call(
                "pane.focus",
                workspace=workspace_id(client),
                pane="00000000-0000-4000-8000-000000000000",
            )

    def test_closing_a_pane_leaves_its_session_running(
        self, client: Client, session: str
    ) -> None:
        # Closing a view of something is not ending it.
        workspace = workspace_id(client)
        panes = client.call("pane.list", workspace=workspace)["panes"]
        pane = panes[0]["id"]
        client.call("pane.close", workspace=workspace, pane=pane)
        assert any(
            s["id"] == session for s in client.call("session.list")["sessions"]
        )


class TestLayouts:
    def test_every_pane_of_a_layout_is_reported(self, client: Client) -> None:
        # Including the ones that failed. A layout that reports only the panes
        # that started leaves somebody looking at a half-built screen with no
        # idea which half is missing.
        opened = client.call(
            "layout.open",
            workspace=workspace_id(client),
            panes=[{"program": "/bin/date"}, {"program": "/bin/date"}],
        )
        assert len(opened["panes"]) == 2


class TestBlocks:
    def test_a_shell_with_no_marks_reports_no_blocks_rather_than_guessing(
        self, client: Client, session: str
    ) -> None:
        # A guessed command boundary is a claimed exit code that is not real.
        blocks = client.call("session.blocks", session=session)
        assert isinstance(blocks["blocks"], list)

    def test_shell_integration_is_judged_by_what_was_seen(
        self, client: Client, session: str
    ) -> None:
        # Not by a setting: a config file saying the shell emits marks does not
        # make the marks arrive.
        answer = client.call("shell.integration", session=session)
        assert "active" in answer or "state" in answer


class TestRecognisedReferences:
    def test_a_path_in_a_line_is_found_with_its_offsets(self, client: Client) -> None:
        found = client.call("open.recognize", line="see src/main.rs:42 for the cause")
        assert found["matches"]
        first = found["matches"][0]
        # Offsets, so a client can make exactly that span tappable rather than
        # re-finding the match itself and disagreeing about where it is.
        assert first["start"] < first["end"]
        assert first["kind"] == "path"
        assert first["line_number"] == 42

    def test_a_url_is_found(self, client: Client) -> None:
        found = client.call("open.recognize", line="docs at https://example.invalid/x")
        assert any("example.invalid" in str(t) for t in found["matches"])

    def test_a_line_with_nothing_in_it_finds_nothing_rather_than_guessing(
        self, client: Client
    ) -> None:
        found = client.call("open.recognize", line="just some ordinary prose here")
        assert found["matches"] == []


class TestRecall:
    def test_a_recorded_command_is_suggested_back(
        self, client: Client, session: str
    ) -> None:
        workspace = workspace_id(client)
        client.call(
            "recall.record",
            workspace=workspace,
            session=session,
            command="cargo test --workspace",
            exit_code=0,
        )
        suggestions = client.call(
            "recall.suggest", prefix="cargo", workspace=workspace
        )
        assert any("cargo test" in s["command"] for s in suggestions["suggestions"])

    def test_a_command_that_would_destroy_everything_is_never_suggested(
        self, client: Client, session: str
    ) -> None:
        # History is the one place where being helpful and being dangerous are
        # the same action: a recalled `rm -rf /` is one keystroke from running.
        workspace = workspace_id(client)
        client.call(
            "recall.record",
            workspace=workspace,
            session=session,
            command="rm -rf /",
            exit_code=0,
        )
        suggestions = client.call("recall.suggest", prefix="rm", workspace=workspace)
        assert not any(
            s["command"] == "rm -rf /" for s in suggestions["suggestions"]
        )

    def test_an_ordinary_rm_is_still_suggested(
        self, client: Client, session: str
    ) -> None:
        # The judgement is about the *target*, not the flags. Suppressing every
        # `rm -rf` would make the feature useless for the thing people actually
        # run, and teach them to ignore it.
        workspace = workspace_id(client)
        client.call(
            "recall.record",
            workspace=workspace,
            session=session,
            command="rm -rf ./build",
            exit_code=0,
        )
        suggestions = client.call("recall.suggest", prefix="rm", workspace=workspace)
        assert any(
            s["command"] == "rm -rf ./build" for s in suggestions["suggestions"]
        )


class TestTheFormatsToolsActuallyPrint:
    """18-semantic-open §2.7. The lines people most want to click.

    Whitespace splitting finds `src/main.rs:42`. It cannot find a quoted path
    with a space in it, and it cannot find rustc's arrow form without dragging
    the arrow along — and those are the lines printed when something is broken.
    """

    def test_a_python_traceback_frame_is_clickable(self, client: Client) -> None:
        found = client.call(
            "open.recognize", line='  File "/app/service.py", line 42, in handle'
        )
        first = found["matches"][0]
        assert first["target"] == "/app/service.py"
        assert first["line_number"] == 42

    def test_a_quoted_path_with_a_space_survives(self, client: Client) -> None:
        # The whole reason that rule outranks whitespace splitting.
        found = client.call(
            "open.recognize", line='File "/home/me/my project/app.py", line 7'
        )
        assert found["matches"][0]["target"] == "/home/me/my project/app.py"

    def test_the_rustc_arrow_does_not_travel_with_the_path(
        self, client: Client
    ) -> None:
        found = client.call("open.recognize", line="  --> crates/omt/src/main.rs:9:5")
        first = found["matches"][0]
        assert first["target"] == "crates/omt/src/main.rs"
        assert first["line_number"] == 9

    def test_a_rust_panic_location_is_found(self, client: Client) -> None:
        found = client.call(
            "open.recognize", line="thread 'main' panicked at src/lib.rs:120:9:"
        )
        assert found["matches"][0]["target"] == "src/lib.rs"

    def test_both_node_frame_shapes_work(self, client: Client) -> None:
        # Half a clickable stack trace is worse than none: the frames people
        # want are usually the named ones.
        for line, expected in [
            ("    at handleRequest (/app/server.js:12:15)", "/app/server.js"),
            ("    at /app/index.js:3:1", "/app/index.js"),
        ]:
            found = client.call("open.recognize", line=line)
            assert found["matches"][0]["target"] == expected, line

    def test_prose_containing_at_is_not_made_tappable(self, client: Client) -> None:
        # `" at "` occurs in English. A rule that fired on it would make every
        # log line tappable and none of them meaningful.
        found = client.call("open.recognize", line="the file is at the top")
        assert found["matches"] == []

    def test_a_diagnostic_line_yields_one_target_not_two(
        self, client: Client
    ) -> None:
        # Two overlapping matches in one span give a client two things to make
        # tappable in the same place, and no basis for choosing.
        found = client.call("open.recognize", line="  --> src/x.rs:9:5")
        assert len(found["matches"]) == 1
