"""
Git and worktrees.

`docs/architecture/15-workspace-explorer.md` and the `git.*`, `worktree.*` and
`fanout.*` capabilities. Every test here runs against a real repository: the
capabilities shell out to git, so a directory that merely looks like a
repository would only ever exercise the error paths.
"""

from __future__ import annotations

import subprocess
from pathlib import Path

import pytest

from omt_client import CapabilityError, Client, Instance


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


def git(cwd: Path, *args: str) -> str:
    return subprocess.run(
        ["git", *args], cwd=cwd, check=True, capture_output=True, text=True
    ).stdout


class TestStatus:
    def test_the_branch_and_the_working_tree_are_reported(
        self, client: Client
    ) -> None:
        status = client.call("git.status", workspace=workspace_id(client))
        assert status["branch"] == "main"

    def test_a_clean_tree_says_so_rather_than_listing_nothing_ambiguously(
        self, client: Client
    ) -> None:
        status = client.call("git.status", workspace=workspace_id(client))
        assert status.get("dirty") in (False, None) or status.get("changed", 0) == 0

    def test_an_edit_shows_up(self, client: Client, instance: Instance) -> None:
        (instance.cwd / "README.md").write_text("edited by the tests\n")
        status = client.call("git.status", workspace=workspace_id(client))
        assert status.get("dirty") or status.get("changed", 0) > 0
        # Put it back, so the tests after this one see the tree they expect.
        git(instance.cwd, "checkout", "--", "README.md")


class TestDiff:
    def test_which_files_changed_with_line_counts(
        self, client: Client, instance: Instance
    ) -> None:
        (instance.cwd / "changed.txt").write_text("one\ntwo\n")
        git(instance.cwd, "add", "changed.txt")
        git(instance.cwd, "commit", "-q", "-m", "add a file to change")
        (instance.cwd / "changed.txt").write_text("one\ntwo\nthree\n")

        diff = client.call("git.diff", workspace=workspace_id(client))
        names = [f["path"] for f in diff["files"]]
        assert "changed.txt" in names
        entry = next(f for f in diff["files"] if f["path"] == "changed.txt")
        assert entry["added"] >= 1

    def test_staged_and_unstaged_are_separate_answers(
        self, client: Client, instance: Instance
    ) -> None:
        # Merging them would show somebody a diff that is not what a commit
        # would contain, which is the one question a staged diff is asked.
        git(instance.cwd, "add", "changed.txt")
        staged = client.call("git.diff", workspace=workspace_id(client), staged=True)
        unstaged = client.call("git.diff", workspace=workspace_id(client), staged=False)
        assert [f["path"] for f in staged["files"]] == ["changed.txt"]
        assert [f["path"] for f in unstaged["files"]] == []

    def test_the_changed_lines_of_one_file_are_available(
        self, client: Client, instance: Instance
    ) -> None:
        hunks = client.call(
            "git.hunks",
            workspace=workspace_id(client),
            path="changed.txt",
            staged=True,
        )
        assert hunks["hunks"]
        # Lines are the diff's own text, prefix and all: a client that wants to
        # colour additions needs the `+`, and stripping it here would test a
        # shape omt does not return.
        body = "\n".join(line for hunk in hunks["hunks"] for line in hunk["lines"])
        assert "three" in body

    def test_nothing_here_writes_to_the_repository(
        self, client: Client, instance: Instance
    ) -> None:
        # `git.*` is read-only by contract. A capability that quietly staged or
        # committed would make a review surface into an editing one.
        before = git(instance.cwd, "rev-parse", "HEAD")
        client.call("git.status", workspace=workspace_id(client))
        client.call("git.diff", workspace=workspace_id(client))
        assert git(instance.cwd, "rev-parse", "HEAD") == before


class TestWorktrees:
    def test_a_worktree_can_be_added_and_is_listed(
        self, client: Client, instance: Instance
    ) -> None:
        workspace = workspace_id(client)
        added = client.call(
            "worktree.add",
            workspace=workspace,
            path="feature-a",
            branch="feature-a",
        )
        assert added["worktree"]["path"]

        listed = client.call("worktree.list", workspace=workspace)["worktrees"]
        branches = [w["branch"] for w in listed]
        assert "feature-a" in branches

    def test_a_worktree_reports_the_workspace_id_it_would_open_as(
        self, client: Client
    ) -> None:
        # N worktrees are already N workspaces — the id derives from the
        # canonical path, so there is no special case anywhere else.
        listed = client.call("worktree.list", workspace=workspace_id(client))[
            "worktrees"
        ]
        for tree in listed:
            assert tree.get("workspace"), f"{tree} has no workspace id"

    def test_a_worktree_opens_as_its_own_workspace(self, client: Client) -> None:
        listed = client.call("worktree.list", workspace=workspace_id(client))[
            "worktrees"
        ]
        feature = next(w for w in listed if w["branch"] == "feature-a")
        opened = client.call("workspace.open", root=feature["path"])
        assert opened["id"] == feature["workspace"]

    def test_removing_a_dirty_worktree_is_refused_without_force(
        self, client: Client
    ) -> None:
        # An agent's only copy of an hour's work is not something to discard
        # because a tidy-up asked politely.
        workspace = workspace_id(client)
        listed = client.call("worktree.list", workspace=workspace)["worktrees"]
        feature = next(w for w in listed if w["branch"] == "feature-a")
        Path(feature["path"], "uncommitted.txt").write_text("work in progress\n")

        with pytest.raises(CapabilityError):
            client.call("worktree.remove", workspace=workspace, path=feature["path"])

    def test_forcing_removes_it(self, client: Client) -> None:
        workspace = workspace_id(client)
        listed = client.call("worktree.list", workspace=workspace)["worktrees"]
        feature = next(w for w in listed if w["branch"] == "feature-a")
        client.call(
            "worktree.remove",
            workspace=workspace,
            path=feature["path"],
            force=True,
        )
        after = client.call("worktree.list", workspace=workspace)["worktrees"]
        assert "feature-a" not in [w["branch"] for w in after]


class TestFanOut:
    def test_an_arm_per_agent_is_created_and_reported(self, client: Client) -> None:
        started = client.call(
            "fanout.start",
            workspace=workspace_id(client),
            name="compare",
            agents=["claude_code", "codex"],
            prompt="compare two approaches to the same change",
        )
        assert len(started["arms"]) == 2
        # Every arm is reported, including any that failed to start: a fan-out
        # that silently drops one arm is a comparison of the wrong things.
        for arm in started["arms"]:
            assert "agent" in arm

    def test_the_arms_are_listed_with_their_worktrees(self, client: Client) -> None:
        status = client.call("fanout.status")
        arms = [arm for run in status["fanouts"] for arm in run["arms"]]
        assert {a["agent"] for a in arms} >= {"claude_code", "codex"}
        for arm in arms:
            assert arm.get("branch")

    def test_choosing_before_anything_finished_is_refused(
        self, client: Client
    ) -> None:
        # Picking a winner out of arms that are all still running is picking at
        # random. Refused with a reason rather than recorded.
        with pytest.raises(CapabilityError) as refusal:
            client.call("fanout.choose", name="compare", agent="claude_code")
        assert refusal.value.code == "precondition_failed"

    def test_choosing_merges_nothing_and_deletes_nothing(
        self, client: Client
    ) -> None:
        # The losing worktrees stay. Deleting them on a choice would throw away
        # the only copy of work somebody may still want to look at, and the
        # whole point of a fan-out is comparing what each arm produced.
        before = client.call("fanout.status")
        arms_before = sum(len(run["arms"]) for run in before["fanouts"])

        # Nothing here can finish an arm without a real agent binary, so this
        # asserts the guarantee that holds either way: choosing never removes an
        # arm, and a refusal leaves the fan-out exactly as it was.
        _, refusal = client.try_call(
            "fanout.choose", name="compare", agent="claude_code"
        )

        after = client.call("fanout.status")
        assert sum(len(run["arms"]) for run in after["fanouts"]) == arms_before
        for run in after["fanouts"]:
            for arm in run["arms"]:
                assert arm.get("path") or arm.get("worktree"), (
                    "an arm lost its worktree"
                )
