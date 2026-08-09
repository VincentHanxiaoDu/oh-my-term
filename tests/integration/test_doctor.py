"""
`omt doctor`.

`docs/architecture/22-operations.md` §3: per-link diagnosis, every failure
carrying a remedy. The rule that shapes it is in the heading — a check that
finds a problem and does not say what to do about it has moved the problem
rather than diagnosed it.
"""

from __future__ import annotations

import pytest

from omt_client import CapabilityError, Client


class TestDoctor:
    def test_it_checks_something(self, client: Client) -> None:
        assert client.call("doctor.run")["checks"]

    def test_every_failure_carries_a_remedy(self, client: Client) -> None:
        # The whole point. A diagnosis with no remedy has moved the problem.
        for check in client.call("doctor.run")["checks"]:
            if check["verdict"] != "ok":
                assert check["remedy"], f"{check['name']} says what is wrong and not what to do"

    def test_nothing_is_reported_ok_without_being_checked(
        self, client: Client
    ) -> None:
        # An unrun check reported green is how a diagnosis stops somebody
        # investigating the thing that is actually wrong. So a check this build
        # cannot perform says so, by name, rather than passing.
        for check in client.call("doctor.run")["checks"]:
            assert check["verdict"] in {"ok", "warn", "fail", "not_checked"}
            assert check["detail"], check
            if check["verdict"] == "not_checked":
                assert check["remedy"]

    def test_the_counts_agree_with_the_checks(self, client: Client) -> None:
        # A header that disagrees with the body is the bug this project keeps
        # finding, and a summary line is exactly where it happens.
        out = client.call("doctor.run")
        assert out["failures"] == len(
            [c for c in out["checks"] if c["verdict"] == "fail"]
        )
        assert out["not_checked"] == len(
            [c for c in out["checks"] if c["verdict"] == "not_checked"]
        )

    def test_one_group_can_be_asked_for_on_its_own(self, client: Client) -> None:
        checks = client.call("doctor.run", group="term")["checks"]
        assert checks
        assert {c["group"] for c in checks} == {"term"}

    def test_a_group_omt_does_not_check_is_refused_rather_than_empty(
        self, client: Client
    ) -> None:
        # An empty answer would read as "everything in that group is fine".
        with pytest.raises(CapabilityError):
            client.call("doctor.run", group="nonsense")

    def test_it_reports_on_the_agents_it_can_look_for(self, client: Client) -> None:
        groups = {c["group"] for c in client.call("doctor.run")["checks"]}
        assert {"term", "shell", "agents", "keys", "net"} <= groups

    def test_the_shell_check_is_honest_about_having_seen_nothing(
        self, client: Client
    ) -> None:
        # Judged by what has been observed, never by a setting: a config file
        # saying the shell emits marks does not make the marks arrive.
        shell = [c for c in client.call("doctor.run", group="shell")["checks"]]
        assert shell
        assert shell[0]["verdict"] in {"ok", "warn", "not_checked"}


class TestEveryGroup:
    def test_the_groups_the_document_names_are_all_checked(
        self, client: Client
    ) -> None:
        # `22 §3` names nine groups. `keys` and `media` are the ones it defers
        # to other documents; the rest are checked here.
        groups = {c["group"] for c in client.call("doctor.run")["checks"]}
        assert {"term", "shell", "agents", "keys", "net", "store", "service", "media"} <= groups

    def test_the_store_check_looks_at_where_state_actually_goes(
        self, client: Client
    ) -> None:
        # A state directory inside a synced folder is the failure that looks
        # like corruption: two machines writing one file through a client that
        # resolves conflicts by picking one.
        names = {c["name"] for c in client.call("doctor.run", group="store")["checks"]}
        assert "not inside a synced folder" in names


class TestBugReport:
    def test_it_is_readable_text_and_carries_the_diagnosis(
        self, client: Client
    ) -> None:
        # Readable rather than an opaque bundle: a diagnostic somebody cannot
        # read is one they have to trust blindly about their own machine.
        report = client.call("doctor.report", what_happened="the terminal froze")["report"]
        assert "omt bug report" in report
        assert "the terminal froze" in report
        assert "Diagnosis" in report
        assert "version:" in report

    def test_it_carries_no_credential(self, client: Client) -> None:
        # A bug report with a token in it is one nobody can safely paste.
        report = client.call("doctor.report")["report"]
        assert "omt_c_" not in report
        assert "omt_i_" not in report

    def test_it_works_without_being_told_what_happened(self, client: Client) -> None:
        assert client.call("doctor.report")["report"]
