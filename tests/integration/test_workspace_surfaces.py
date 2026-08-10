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


class TestDirectionalFocus:
    """17-panes-and-layout §6. Geometric, over the placed rectangles."""

    # One tall pane on the left, two stacked on the right.
    PANES = [
        {"id": "a", "left": 0, "top": 0, "right": 40, "bottom": 24, "placement_index": 0},
        {"id": "b", "left": 40, "top": 0, "right": 80, "bottom": 12, "placement_index": 1},
        {"id": "c", "left": 40, "top": 12, "right": 80, "bottom": 24, "placement_index": 2},
    ]

    def test_focus_moves_to_the_pane_that_is_actually_there(
        self, client: Client
    ) -> None:
        out = client.call("pane.navigate", panes=self.PANES, **{"from": "b"}, direction="left")
        assert out["focus"] == "a"

    def test_there_is_nothing_past_the_edge_without_wrap(self, client: Client) -> None:
        # A focus key that leaves the edge of the screen reads as a bug before
        # it reads as a feature.
        out = client.call("pane.navigate", panes=self.PANES, **{"from": "a"}, direction="left")
        assert out["focus"] is None

    def test_wrap_jumps_to_the_far_side_when_asked(self, client: Client) -> None:
        out = client.call(
            "pane.navigate", panes=self.PANES, **{"from": "a"}, direction="left", wrap=True
        )
        assert out["focus"] in {"b", "c"}

    def test_the_answer_does_not_depend_on_history(self, client: Client) -> None:
        # The deliberate divergence from tmux and zellij: recency makes the
        # same keypress in the same visible layout do two different things for
        # reasons the user cannot see.
        answers = {
            client.call(
                "pane.navigate", panes=self.PANES, **{"from": "a"}, direction="right"
            )["focus"]
            for _ in range(5)
        }
        assert len(answers) == 1, answers

    def test_a_hidden_pane_is_skipped(self, client: Client) -> None:
        panes = [
            {"id": "a", "left": 0, "top": 0, "right": 20, "bottom": 24, "placement_index": 0},
            {"id": "b", "left": 20, "top": 0, "right": 40, "bottom": 24, "hidden": True, "placement_index": 1},
            {"id": "c", "left": 40, "top": 0, "right": 60, "bottom": 24, "placement_index": 2},
        ]
        out = client.call("pane.navigate", panes=panes, **{"from": "a"}, direction="right")
        assert out["focus"] == "c"

    def test_an_unknown_direction_is_refused_and_says_what_exists(
        self, client: Client
    ) -> None:
        with pytest.raises(Exception) as caught:
            client.call(
                "pane.navigate", panes=self.PANES, **{"from": "a"}, direction="sideways"
            )
        assert "left" in str(caught.value).lower()


class TestSavedLayouts:
    """17-panes-and-layout §9 and §10."""

    def test_a_layout_saves_and_restores(self, client: Client) -> None:
        client.call(
            "layout.save", name="work", panes=["a", "b", "c"], preset="main_vertical"
        )
        out = client.call(
            "layout.restore", name="work", alive=["a", "b", "c"], width=120, height=40
        )
        assert [p["pane"] for p in out["panes"]] == ["a", "b", "c"]
        assert out["missing"] == []

    def test_a_restore_names_the_panes_that_are_gone(self, client: Client) -> None:
        # A restore that quietly drops two panes leaves somebody wondering what
        # they have forgotten.
        client.call("layout.save", name="w2", panes=["a", "b", "c"], preset="even_vertical")
        out = client.call("layout.restore", name="w2", alive=["a", "c"], width=120, height=40)
        assert out["missing"] == ["b"]
        assert [p["pane"] for p in out["panes"]] == ["a", "c"]

    def test_restoring_onto_a_phone_drops_what_will_not_fit(
        self, client: Client
    ) -> None:
        # The whole reason rects are not saved: a desktop layout restored onto
        # a phone must not come back as a desktop layout.
        client.call("layout.save", name="w3", panes=["a", "b", "c", "d"], preset="even_vertical")
        out = client.call("layout.restore", name="w3", alive=["a", "b", "c", "d"], width=30, height=60)
        assert out["hidden"], out

    def test_a_layout_with_nothing_left_alive_is_refused_with_a_reason(
        self, client: Client
    ) -> None:
        client.call("layout.save", name="w4", panes=["a"], preset="even_vertical")
        with pytest.raises(Exception) as caught:
            client.call("layout.restore", name="w4", alive=[], width=80, height=24)
        assert "running" in str(caught.value).lower()

    def test_an_unknown_layout_is_not_found(self, client: Client) -> None:
        with pytest.raises(Exception):
            client.call("layout.restore", name="nope", alive=["a"], width=80, height=24)

    def test_an_unknown_preset_says_what_exists(self, client: Client) -> None:
        with pytest.raises(Exception) as caught:
            client.call("layout.save", name="w5", panes=["a"], preset="spiral")
        assert "main_vertical" in str(caught.value)

    def test_saving_the_same_name_twice_replaces_rather_than_doubling(
        self, client: Client
    ) -> None:
        first = client.call("layout.save", name="dup", panes=["a"], preset="even_vertical")
        second = client.call("layout.save", name="dup", panes=["a", "b"], preset="even_vertical")
        assert first["saved"] == second["saved"]


class TestWhyAResultRankedThere:
    """20-recall-and-usage §4. A transparent weighted product."""

    def candidate(self, id, **over):
        base = {
            "id": id,
            "kind": "command",
            "bm25": -10.0,
            "age_secs": 0,
            "in_scope": True,
            "exit": 0,
            "session_active": False,
        }
        base.update(over)
        return base

    def test_every_factor_comes_back_with_a_reason(self, client: Client) -> None:
        # The only remedy for a ranker that cannot explain itself is to stop
        # trusting search, which is the same as not having it.
        out = client.call("search.explain", candidates=[self.candidate("a")])
        factors = out["results"][0]["factors"]
        assert len(factors) == 6
        assert all(f["because"] for f in factors)

    def test_the_factors_multiply_to_the_score(self, client: Client) -> None:
        # A printed explanation that does not reconstruct the number looks like
        # an audit and is not one.
        result = client.call("search.explain", candidates=[self.candidate("a")])[
            "results"
        ][0]
        product = 1.0
        for factor in result["factors"]:
            product *= factor["value"]
        assert abs(product - result["score"]) < 1e-9

    def test_something_more_recent_wins_all_else_equal(self, client: Client) -> None:
        out = client.call(
            "search.explain",
            candidates=[
                self.candidate("old", age_secs=30 * 86400),
                self.candidate("new"),
            ],
        )
        assert out["results"][0]["id"] == "new"

    def test_a_huge_text_match_cannot_swamp_everything_else(
        self, client: Client
    ) -> None:
        out = client.call(
            "search.explain",
            candidates=[
                self.candidate(
                    "wordy",
                    bm25=-10000.0,
                    in_scope=False,
                    exit=1,
                    age_secs=30 * 86400,
                    kind="output",
                ),
                self.candidate("nearby"),
            ],
        )
        assert out["results"][0]["id"] == "nearby"

    def test_the_same_query_gives_the_same_order(self, client: Client) -> None:
        # An unstable order makes consecutive runs disagree, which reads as the
        # index being broken.
        candidates = [self.candidate(x) for x in ("b", "a", "c")]
        orders = {
            tuple(r["id"] for r in client.call("search.explain", candidates=candidates)["results"])
            for _ in range(3)
        }
        assert len(orders) == 1


class TestWhatIsIndexed:
    """20-recall-and-usage §2. A value-per-byte argument, made checkable."""

    def treatment(self, out, source):
        return next(s for s in out["sources"] if s["source"] == source)

    def test_the_command_is_indexed_in_full(self, client: Client) -> None:
        out = client.call("recall.policy")
        assert self.treatment(out, "blockcommand")["treatment"] == "full"

    def test_reasoning_is_excluded_until_somebody_opts_in(
        self, client: Client
    ) -> None:
        # The most privacy-sensitive text an agent produces. Somebody searching
        # their own history for "password" should not find musings about one.
        off = client.call("recall.policy")
        assert self.treatment(off, "reasoning")["treatment"] == "excluded"
        on = client.call("recall.policy", index_reasoning=True)
        assert self.treatment(on, "reasoning")["treatment"] == "capped"

    def test_tool_results_are_filters_not_text(self, client: Client) -> None:
        out = client.call("recall.policy")
        assert self.treatment(out, "toolresultoutput")["treatment"] == "structured_only"

    def test_output_is_windowed_rather_than_truncated(self, client: Client) -> None:
        # Errors live at the end, so truncating from the front drops exactly
        # what somebody is searching for.
        out = client.call("recall.policy")
        assert self.treatment(out, "blockoutput")["treatment"] == "windowed"

    def test_the_indexed_tool_fields_are_an_allow_list(self, client: Client) -> None:
        # A new tool with a new payload field would otherwise be indexed in
        # full the day it appears.
        out = client.call("recall.policy")
        assert "command" in out["indexed_tool_fields"]
        assert "content" not in out["indexed_tool_fields"]

    def test_every_source_has_a_stated_treatment(self, client: Client) -> None:
        # A search index nobody can enumerate is one nobody can reason about.
        out = client.call("recall.policy")
        assert len(out["sources"]) >= 9
        assert all(s["treatment"] for s in out["sources"])


class TestRetention:
    """21-data-lifecycle §3. Compact first, delete second."""

    def test_the_fact_outlives_what_it_printed(self, client: Client) -> None:
        # The output of a build from three months ago is worthless; that you
        # ran it, where, and that it failed, is still useful.
        output = client.call("store.retention", kind="output", age_days=60)
        metadata = client.call("store.retention", kind="block_metadata", age_days=60)
        assert output["fate"] == "delete"
        assert metadata["fate"] == "keep"

    def test_a_failed_block_keeps_its_output_past_compaction(
        self, client: Client
    ) -> None:
        # The failures are what people come back to.
        plain = client.call(
            "store.retention", kind="output", age_days=10, output_bytes=1000000
        )
        failed = client.call(
            "store.retention",
            kind="output",
            age_days=10,
            output_bytes=1000000,
            failed=True,
        )
        assert plain["fate"] == "compact"
        assert failed["fate"] == "keep"

    def test_a_block_an_agent_touched_keeps_its_output(self, client: Client) -> None:
        out = client.call(
            "store.retention",
            kind="output",
            age_days=10,
            output_bytes=1000000,
            agent_touched=True,
        )
        assert out["fate"] == "keep"

    def test_a_small_output_is_not_worth_compacting(self, client: Client) -> None:
        out = client.call(
            "store.retention", kind="output", age_days=10, output_bytes=100
        )
        assert out["fate"] == "keep"

    def test_a_command_outlives_almost_everything(self, client: Client) -> None:
        out = client.call("store.retention", kind="history", age_days=365)
        assert out["fate"] == "keep"
        assert out["compact_after_days"] is None

    def test_every_kind_says_when_it_goes(self, client: Client) -> None:
        # Data with no delete date grows without bound, and the machine it
        # grows on is somebody's laptop.
        for kind in (
            "output",
            "block_metadata",
            "history",
            "agent_events",
            "interactions",
            "transcript",
        ):
            out = client.call("store.retention", kind=kind, age_days=0)
            assert out["delete_after_days"] > 0, kind

    def test_an_unknown_kind_is_refused(self, client: Client) -> None:
        with pytest.raises(Exception):
            client.call("store.retention", kind="nonsense", age_days=1)


class TestWhatOmtWritesDown:
    """21-data-lifecycle §8."""

    def test_everything_omt_persists_is_listed(self, client: Client) -> None:
        # The alternative is reading twelve crates, which nobody does.
        out = client.call("store.paths")
        assert len(out["artefacts"]) >= 10
        assert all(a["location"] and a["what"] for a in out["artefacts"])

    def test_nothing_is_readable_by_anybody_else(self, client: Client) -> None:
        out = client.call("store.paths")
        assert all(a["mode"] == "600" for a in out["artefacts"]), out["artefacts"]

    def test_everything_says_when_it_goes(self, client: Client) -> None:
        out = client.call("store.paths")
        assert all(a["delete_after_days"] > 0 for a in out["artefacts"])

    def test_the_encryption_answer_is_no_and_says_so_first(
        self, client: Client
    ) -> None:
        out = client.call("store.paths")
        assert out["at_rest"].startswith("omt does not encrypt")

    def test_a_store_from_the_future_cannot_be_opened(self, client: Client) -> None:
        # A partial read of a newer format silently drops whatever that version
        # added, and writes the store back without it.
        out = client.call("store.migrate_status", marker="999")
        assert out["openable"] is False
        assert out["remedy"]

    def test_a_current_store_needs_no_migration(self, client: Client) -> None:
        out = client.call("store.migrate_status", marker="1")
        assert out["openable"] is True
        assert out["steps"] == []

    def test_the_redactor_can_be_tried_before_it_is_trusted(
        self, client: Client
    ) -> None:
        # So somebody finds out what it catches before trusting it with
        # something, rather than after.
        out = client.call("store.redaction_test", text="export TOKEN=ghp_secret")
        assert "ghp_secret" not in out["redacted"]
        assert out["removed"] >= 1

    def test_ordinary_text_comes_back_untouched(self, client: Client) -> None:
        out = client.call("store.redaction_test", text="Compiling omt v0.1.0")
        assert out["redacted"] == "Compiling omt v0.1.0"
        assert out["removed"] == 0
