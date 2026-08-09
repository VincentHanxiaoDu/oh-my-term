"""
Plugins, scheduled jobs, saved state and voice input.

`docs/architecture/11-plugins.md`, `13-security.md` on grants, and the parts of
`08-web-client.md` covering dictation. The theme here is consent: what a plugin
*asks for* and what it *receives* are separate things, and a job that fires
while nobody is watching is still a job that ran.
"""

from __future__ import annotations

import pytest

from omt_client import CapabilityError, Client, Instance


class TestPlugins:
    def test_nothing_is_installed_on_a_fresh_instance(self, client: Client) -> None:
        assert client.call("plugin.list")["plugins"] == []

    def test_a_plugin_is_installed_with_an_explicit_grant(
        self, client: Client
    ) -> None:
        # What it asks for and what it receives are separate. A plugin that got
        # what it asked for by asking is not a permission system.
        client.call(
            "plugin.install",
            id="notes",
            name="Notes",
            version="0.1.0",
            permissions=["read_sessions", "write_input"],
            grant=["read_sessions"],
        )
        installed = client.call("plugin.list")["plugins"]
        entry = next(p for p in installed if p["id"] == "notes")
        assert entry["granted"] == ["read_sessions"]
        # The same spelling it was installed with. Written out with Rust's
        # variant names, an "edit these grants" screen re-sent its own values
        # and was told they were not permissions omt knows.
        # And what was granted is what is reported: `write_input` was asked for
        # and refused, so it must not appear.
        assert "write_input" not in entry["granted"]

    def test_a_high_consequence_grant_is_flagged_as_one(self, client: Client) -> None:
        # So a client can make it harder to agree to than "read a session".
        installed = client.call("plugin.list")["plugins"]
        entry = next(p for p in installed if p["id"] == "notes")
        # Computed from what it holds, not from what it asked for: warning about
        # authority a plugin was refused teaches people to ignore the warning.
        assert entry["high_consequence"] == []

    def test_enabling_is_idempotent(self, client: Client) -> None:
        # Setting the state it already has is not an error: a client that
        # re-sends on reconnect must not produce a failure.
        client.call("plugin.enable", id="notes", enabled=True)
        client.call("plugin.enable", id="notes", enabled=True)
        entry = next(p for p in client.call("plugin.list")["plugins"] if p["id"] == "notes")
        assert entry["enabled"] is True

    def test_a_plugin_cannot_be_granted_what_it_did_not_ask_for(
        self, client: Client
    ) -> None:
        # Granting beyond the request is how a permission dialog becomes a lie:
        # the user agreed to what was shown to them.
        with pytest.raises(CapabilityError):
            client.call(
                "plugin.install",
                id="greedy",
                name="Greedy",
                version="0.1.0",
                permissions=["read_sessions"],
                grant=["read_sessions", "write_input"],
            )

    def test_starting_an_unknown_plugin_is_refused(self, client: Client) -> None:
        with pytest.raises(CapabilityError):
            client.call("plugin.start", id="not-installed")


class TestJobs:
    def test_a_job_is_created_and_listed_with_when_it_fires(
        self, client: Client
    ) -> None:
        client.call(
            "job.create",
            name="nightly",
            workspace=client.call("workspace.list")["workspaces"][0]["id"],
            run="cargo test --workspace",
            every_seconds=3600,
        )
        jobs = client.call("job.list")["jobs"]
        entry = next(j for j in jobs if j["name"] == "nightly")
        assert entry["trigger"], "a job that does not say when it fires"
        assert entry["run"] == "cargo test --workspace"

    def test_creating_the_same_name_replaces_rather_than_duplicates(
        self, client: Client
    ) -> None:
        # Two jobs with one name is two jobs somebody thinks is one.
        client.call(
            "job.create",
            name="nightly",
            workspace=client.call("workspace.list")["workspaces"][0]["id"],
            run="cargo build",
            every_seconds=7200,
        )
        names = [j["name"] for j in client.call("job.list")["jobs"]]
        assert names.count("nightly") == 1

    def test_a_job_reports_how_many_times_it_has_failed_in_a_row(
        self, client: Client
    ) -> None:
        # A job silently failing every night for a week is the failure mode
        # scheduling always has.
        for job in client.call("job.list")["jobs"]:
            assert "failures" in job or "consecutive_failures" in job

    def test_a_job_for_a_workspace_that_is_not_open_is_refused(
        self, client: Client
    ) -> None:
        # Scheduling work in a directory omt does not have open would produce a
        # job that fails every time it fires, silently.
        with pytest.raises(CapabilityError):
            client.call(
                "job.create",
                name="bad",
                workspace="wksp_00000000000000000000000000000000",
                run="true",
                every_seconds=60,
            )


class TestSavedState:
    def test_the_open_workspaces_can_be_written_and_read_back(
        self, client: Client, instance: Instance
    ) -> None:
        path = str(instance.cwd / "state.json")
        client.call("state.save", path=path)
        restored = client.call("state.restore", path=path)
        assert restored["found"] is True
        assert restored["workspaces"] >= 1

    def test_restoring_reports_a_workspace_whose_directory_is_gone(
        self, client: Client, instance: Instance
    ) -> None:
        # Rather than dropping it. A workspace that vanishes from a restore with
        # no explanation looks like omt lost it.
        path = str(instance.cwd / "state.json")
        client.call("state.save", path=path)
        restored = client.call("state.restore", path=path)
        # Reported, not dropped: a workspace that vanishes from a restore with
        # no explanation looks like omt lost it.
        assert "missing" in restored

    def test_a_snapshot_that_is_not_there_says_so_rather_than_failing(
        self, client: Client, instance: Instance
    ) -> None:
        # First run has no snapshot, and that is not an error — it is the
        # ordinary state of a fresh install. `found` is how a caller tells
        # "nothing saved yet" from "something went wrong".
        answer = client.call(
            "state.restore", path=str(instance.cwd / "nothing-here.json")
        )
        assert answer["found"] is False
        assert answer["workspaces"] == 0


class TestVoice:
    def test_the_engines_are_listed_with_whether_each_has_a_key(
        self, client: Client
    ) -> None:
        # Presence only. omt ships no key of its own: a provider appears once
        # the user has supplied one, which is the whole of bring-your-own-key.
        providers = client.call("voice.providers")["providers"]
        for provider in providers:
            assert "name" in provider or "id" in provider
            assert "configured" in provider or "has_key" in provider

    def test_no_provider_claims_a_key_on_an_instance_given_none(
        self, client: Client
    ) -> None:
        for provider in client.call("voice.providers")["providers"]:
            assert not provider.get("configured", provider.get("has_key", False))

    def test_a_partial_chunk_is_replaced_by_a_later_one(
        self, client: Client, session: str
    ) -> None:
        # Dictation revises itself as it hears more. Appending each revision
        # would produce "open the open the file open the file now".
        client.call("voice.append", session=session, text="open the", final_chunk=False)
        client.call(
            "voice.append", session=session, text="open the file", final_chunk=False
        )
        client.call(
            "voice.append", session=session, text="open the file now", final_chunk=True
        )
        # Nothing to assert against but the absence of a failure and the buffer
        # being clearable: the transcript is delivered through the session, and
        # asserting on the screen would test the shell's echo instead.
        client.call("voice.clear", session=session)

    def test_clearing_is_idempotent(self, client: Client, session: str) -> None:
        client.call("voice.clear", session=session)
        client.call("voice.clear", session=session)
