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


class TestResolvingWhatWasRecognised:
    """18-semantic-open §3 and §4.

    Recognition says what the text looks like. Resolution answers the two
    questions it deliberately refused: is it there, and what can be done.
    """

    def test_a_relative_path_resolves_and_says_which_root_it_used(
        self, client: Client, tmp_path
    ) -> None:
        # A path resolved against the wrong root opens somebody else's file.
        # Naming the root is what makes a wrong answer visible rather than
        # merely wrong.
        (tmp_path / "src").mkdir()
        (tmp_path / "src" / "main.rs").write_text("fn main() {}")
        out = client.call(
            "open.resolve", path="src/main.rs", roots=[str(tmp_path)], line=42
        )
        assert out["existence"] == "file"
        assert out["root"] == str(tmp_path)
        assert out["line"] == 42

    def test_a_traversal_is_refused_rather_than_clamped(
        self, client: Client, tmp_path
    ) -> None:
        with pytest.raises(Exception) as caught:
            client.call(
                "open.resolve", path="../../../etc/passwd", roots=[str(tmp_path)]
            )
        assert "outside" in str(caught.value).lower()

    def test_an_absolute_path_outside_every_root_is_refused(
        self, client: Client, tmp_path
    ) -> None:
        # Otherwise a build log naming /etc/shadow becomes something tappable.
        with pytest.raises(Exception):
            client.call("open.resolve", path="/etc/shadow", roots=[str(tmp_path)])

    def test_a_missing_file_says_missing_and_still_offers_something(
        self, client: Client, tmp_path
    ) -> None:
        # Copying the path is what somebody does next when a build names a file
        # that is not there.
        out = client.call("open.resolve", path="absent.rs", roots=[str(tmp_path)])
        assert out["existence"] == "missing"
        assert out["actions"][0]["handler"] == "clipboard"

    def test_actions_come_back_ranked_best_first(
        self, client: Client, tmp_path
    ) -> None:
        (tmp_path / "a.rs").write_text("")
        out = client.call("open.resolve", path="a.rs", roots=[str(tmp_path)])
        ranks = [a["rank"] for a in out["actions"]]
        assert ranks == sorted(ranks, reverse=True)
        assert out["actions"][0]["handler"] == "editor"

    def test_the_line_is_carried_into_the_label_that_uses_it(
        self, client: Client, tmp_path
    ) -> None:
        (tmp_path / "a.rs").write_text("")
        out = client.call("open.resolve", path="a.rs", roots=[str(tmp_path)], line=42)
        assert "42" in out["actions"][0]["label"]

    def test_anything_that_changes_something_says_so_without_being_run(
        self, client: Client, tmp_path
    ) -> None:
        # Which is what lets a phone decide to show a confirm sheet.
        (tmp_path / "sub").mkdir()
        out = client.call("open.resolve", path="sub", roots=[str(tmp_path)])
        assert out["existence"] == "directory"
        assert any(a["mutates"] for a in out["actions"])

    def test_a_relative_path_with_no_root_is_an_honest_failure(
        self, client: Client
    ) -> None:
        with pytest.raises(Exception) as caught:
            client.call("open.resolve", path="src/main.rs", roots=[])
        assert "workspace" in str(caught.value).lower()


class TestWhoOwnsAClick:
    """18-semantic-open §5.1. The part that decides whether this is a feature
    or a broken editor."""

    def test_a_plain_click_inside_vim_belongs_to_vim(self, client: Client) -> None:
        # The rule everything else protects. A multiplexer that swallows this
        # has broken every editor.
        out = client.call("open.click", mouse_reporting=True)
        assert out["owner"] == "application"
        assert out["consumed"] is False

    def test_shift_click_is_the_one_omt_claims(self, client: Client) -> None:
        # Applications overwhelmingly do not expect shifted clicks, precisely
        # because the xterm convention exists.
        out = client.call("open.click", mouse_reporting=True, shift=True)
        assert out["owner"] == "activate"
        assert out["consumed"] is True

    def test_alt_click_is_forwarded_even_though_omt_would_like_it(
        self, client: Client
    ) -> None:
        # Many applications already use it. Wanting a binding is not a reason
        # to take one somebody else is using.
        out = client.call("open.click", mouse_reporting=True, alt=True)
        assert out["owner"] == "application"

    def test_with_reporting_off_the_ordinary_selection_bindings_apply(
        self, client: Client
    ) -> None:
        assert client.call("open.click")["owner"] == "selection"
        assert client.call("open.click", shift=True)["owner"] == "extendselection"
        assert client.call("open.click", alt=True)["owner"] == "blockselection"

    def test_super_click_means_the_same_thing_in_both_states(
        self, client: Client
    ) -> None:
        # So somebody who learns it does not also have to learn when it applies.
        for reporting in (True, False):
            out = client.call("open.click", mouse_reporting=reporting, super_key=True)
            assert out["owner"] == "activate"


class TestTheRulesAreInspectable:
    """18-semantic-open §9. `open.rules_list` and `open.rules_test`."""

    def test_every_rule_is_listed_with_what_it_recognises(
        self, client: Client
    ) -> None:
        # A recognition system nobody can enumerate is one where "why did that
        # not become a link" has no answer short of reading the source.
        rules = client.call("open.rules_list")["rules"]
        assert len(rules) >= 4
        assert all(r["about"] and r["precedence"] > 0 for r in rules)
        assert any("python" in r["id"] for r in rules)

    def test_a_preview_names_the_rule_that_fired(self, client: Client) -> None:
        # "It matched" is not an answer somebody can use to fix a rule that
        # did not.
        out = client.call("open.rules_test", text="  --> src/x.rs:9:5")
        assert out["rule"] == "builtin.rustc_arrow"
        assert out["matches"][0]["target"] == "src/x.rs"

    def test_a_line_that_matches_nothing_says_so_without_a_rule(
        self, client: Client
    ) -> None:
        out = client.call("open.rules_test", text="ordinary prose here")
        assert out["rule"] is None
        assert out["matches"] == []

    def test_every_listed_rule_id_can_actually_fire(self, client: Client) -> None:
        # A rule listed but unreachable is worse than an unlisted one: it
        # tells somebody their line should have matched.
        listed = {r["id"] for r in client.call("open.rules_list")["rules"]}
        fired = set()
        for line in [
            'File "/a.py", line 3',
            "  --> src/x.rs:9:5",
            "thread 'main' panicked at src/lib.rs:1:1:",
            "    at run (/app/x.js:1:1)",
        ]:
            rule = client.call("open.rules_test", text=line)["rule"]
            if rule:
                fired.add(rule)
        assert fired == listed, listed - fired


class TestRemoteMirrors:
    """18-semantic-open §6.3. Files fetched from a remote host, held locally."""

    def test_nothing_is_mirrored_until_something_is_fetched(
        self, client: Client
    ) -> None:
        out = client.call("open.remote_list")
        assert isinstance(out["files"], list)

    def test_discarding_something_that_is_not_there_is_reported_not_an_error(
        self, client: Client
    ) -> None:
        # A retry after a lost acknowledgement must not look like a failure.
        out = client.call("open.remote_discard", path="/nowhere/x.rs")
        assert out["discarded"] is False
        assert isinstance(out["remaining"], int)

    def test_discarding_twice_gives_the_same_answer(self, client: Client) -> None:
        first = client.call("open.remote_discard", path="/nowhere/x.rs")
        second = client.call("open.remote_discard", path="/nowhere/x.rs")
        assert first["discarded"] == second["discarded"]
        assert first["remaining"] == second["remaining"]

    def test_every_listed_mirror_says_where_it_came_from(
        self, client: Client
    ) -> None:
        # A mirror nobody can trace is a copy of somebody's file on disk with
        # no way to tell which machine it belongs to.
        for mirror in client.call("open.remote_list")["files"]:
            assert mirror["host"]
            assert mirror["remote_path"]
            assert mirror["digest"]
