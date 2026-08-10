"""
Configuration, keymap, theme and the settings surface.

`docs/architecture/10-configuration.md`, `16-input-and-keymap.md`, and the
principle that every feature works before configuration and is also a setting.
"""

from __future__ import annotations

from pathlib import Path

import pytest

import omt_client
from omt_client import Client, Instance


class TestSchema:
    def test_every_setting_has_a_default_a_kind_and_a_description(
        self, client: Client
    ) -> None:
        # So a settings screen can be built without hard-coding a list that goes
        # stale the first time somebody adds a setting.
        schema = client.call("config.schema")
        assert schema["settings"]
        for setting in schema["settings"]:
            assert setting["key"]
            assert setting["kind"], f"{setting['key']} has no kind"
            assert setting.get("doc"), f"{setting['key']} has no description"
            assert "default" in setting, f"{setting['key']} has no default"

    def test_the_settings_cover_the_features_that_claim_to_be_configurable(
        self, client: Client
    ) -> None:
        # "Minimal to use, complete to configure" is only true if the surfaces
        # people actually want to change are settings rather than constants.
        keys = {s["key"] for s in client.call("config.schema")["settings"]}
        for prefix in ("font.", "appearance.", "terminal.", "agents.", "server."):
            assert any(k.startswith(prefix) for k in keys), (
                f"nothing under {prefix} is configurable"
            )


class TestResolved:
    def test_an_unconfigured_instance_reports_nothing_set(
        self, client: Client
    ) -> None:
        # Deliberately not "reports the defaults": this answers *what has been
        # configured*, and the defaults come from the schema. Merging them here
        # would make an untouched setting indistinguishable from one somebody
        # chose to set to the default on purpose.
        assert client.call("config.get")["values"] == []

    def test_a_project_setting_is_reported_with_the_file_it_came_from(
        self, workspace: Path
    ) -> None:
        # Which is what makes a surprising setting diagnosable rather than a
        # mystery somebody works around by changing something else.
        config = workspace / ".omt" / "config.toml"
        config.parent.mkdir(exist_ok=True)
        config.write_text('[font]\nfamily = "IBM Plex Mono"\n')

        instance = omt_client.start_instance(workspace)
        try:
            client = omt_client.connect(instance)
            values = client.call("config.get")["values"]
            entry = next((v for v in values if v["key"] == "font.family"), None)
            assert entry is not None, f"the project config was not picked up: {values}"
            assert entry["value"] == "IBM Plex Mono"
            assert ".omt" in str(entry.get("source", entry.get("file", ""))), entry
            client.close()
        finally:
            omt_client.stop_instance(instance)
            config.unlink()

    def test_one_key_can_be_asked_for_on_its_own(self, client: Client) -> None:
        # Every value, filtered — asking for a key nothing set is an empty
        # answer, not an error: "not configured" is a legitimate state.
        answer = client.call("config.get", key="font.family")
        assert isinstance(answer["values"], list)


class TestKeymap:
    def test_the_cheatsheet_is_generated_from_the_keymap_in_force(
        self, client: Client
    ) -> None:
        # Generated, so it cannot go stale — a cheatsheet maintained by hand is
        # wrong within a release.
        sheet = client.call("keys.cheatsheet")
        assert sheet["bindings"]
        for binding in sheet["bindings"]:
            assert binding["chord"]
            assert binding["action"]
            assert binding["mode"]

    def test_reserved_keys_are_on_the_sheet_and_marked_as_reaching_the_program(
        self, client: Client
    ) -> None:
        # "Ctrl+C interrupts the program" is the single most useful line on the
        # sheet, and it is invisible if only bound keys are shown. One list with
        # a flag, rather than two lists somebody has to cross-reference.
        sheet = client.call("keys.cheatsheet")
        reserved = [b for b in sheet["bindings"] if b["reaches_program"]]
        assert reserved, "no reserved keys are listed"
        assert any(b["chord"] == "ctrl-c" for b in reserved), reserved

    def test_no_chord_is_listed_twice_with_different_meanings(
        self, client: Client
    ) -> None:
        # A sheet that contradicts itself is worse than no sheet.
        seen: dict[tuple[str, str], str] = {}
        for binding in client.call("keys.cheatsheet")["bindings"]:
            key = (binding["mode"], binding["chord"])
            assert key not in seen or seen[key] == binding["action"], key
            seen[key] = binding["action"]


class TestTheme:
    def test_the_colours_in_force_are_reported(self, client: Client) -> None:
        # So a remote client renders a session in the user's own theme rather
        # than in whatever the browser's stylesheet happens to say.
        theme = client.call("theme.get")
        assert theme["foreground"]
        assert theme["background"]

    def test_the_palette_is_all_sixteen(self, client: Client) -> None:
        # Half a palette is worse than none: the renderer emits a variable per
        # indexed colour, and an undefined one renders as nothing at all.
        assert len(client.call("theme.get")["ansi"]) == 16

    def test_an_appearance_is_declared(self, client: Client) -> None:
        assert client.call("theme.get")["appearance"] in {"dark", "light"}


class TestInstanceIdentity:
    def test_the_version_and_protocol_are_reported(self, client: Client) -> None:
        info = client.call("instance.info")
        assert info["version"]
        assert info["proto"] >= 1

    def test_the_catalog_is_available_over_the_wire(self, client: Client) -> None:
        catalog = client.call("instance.catalog")
        assert "session.create" in [c["name"] for c in catalog["entries"]]

    def test_each_catalog_entry_carries_what_a_client_needs_to_decide_ui_policy(
        self, client: Client
    ) -> None:
        # Effects drive UI policy: a capability that spawns a process or writes
        # to a repository is one a client should confirm before invoking, and it
        # cannot know that without being told.
        for entry in client.call("instance.catalog")["entries"]:
            assert "effects" in entry
            assert entry["role"]
            assert entry["name"]


class TestWhenASettingTakesEffect:
    """10-configuration §6.3."""

    def test_appearance_applies_immediately(self, client) -> None:
        out = client.call("config.reload_class", keys=["ui.theme", "keys.leader"])
        assert all(k["class"] == "hot" for k in out["keys"]), out
        assert out["pending"] == []

    def test_listener_settings_rebuild_the_listener(self, client) -> None:
        # Losing a session to change a TLS certificate would be a poor trade,
        # which is why this is its own class.
        out = client.call("config.reload_class", keys=["server.bind", "server.tls.cert"])
        assert all(k["class"] == "restart_server" for k in out["keys"]), out

    def test_the_store_needs_the_process_back(self, client) -> None:
        out = client.call("config.reload_class", keys=["store.backend"])
        assert out["keys"][0]["class"] == "restart"

    def test_what_will_not_take_effect_yet_is_listed_separately(
        self, client
    ) -> None:
        # A setting that quietly waits for a restart is changed, nothing
        # happens, and the conclusion is that the setting does not work.
        out = client.call(
            "config.reload_class", keys=["ui.theme", "store.path", "server.bind"]
        )
        assert set(out["pending"]) == {"store.path", "server.bind"}

    def test_an_unknown_key_is_hot_rather_than_silently_pending(
        self, client
    ) -> None:
        out = client.call("config.reload_class", keys=["some.new.setting"])
        assert out["keys"][0]["class"] == "hot"


class TestTheSettingsReferenceIsGenerated:
    """10-configuration §4. One table; the docs, the capability and the
    validator all read it."""

    def test_the_reference_is_current(self) -> None:
        # A hand-written settings page is wrong the first time a default
        # changes, and nothing fails when it drifts.
        import subprocess

        from omt_client import REPO_ROOT

        result = subprocess.run(
            ["cargo", "run", "-q", "-p", "xtask", "--", "codegen-check"],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            check=False,
        )
        assert result.returncode == 0, result.stderr

    def test_every_setting_in_the_capability_is_in_the_reference(
        self, client
    ) -> None:
        # The two reading the same table is the property worth having; a
        # setting in one and not the other means they are not.
        from omt_client import REPO_ROOT

        page = (REPO_ROOT / "docs" / "reference" / "settings.md").read_text()
        for setting in client.call("config.schema")["settings"]:
            assert f"`{setting['key']}`" in page, setting["key"]

    def test_every_setting_has_a_default_and_a_sentence(self, client) -> None:
        # P11: a feature works before it is configured, so a setting with no
        # default is a setting that must be set — the same failure with extra
        # steps.
        for setting in client.call("config.schema")["settings"]:
            assert setting["default"] is not None, setting["key"]
            assert setting["doc"].strip(), setting["key"]

    def test_the_reference_says_when_each_setting_takes_effect(self) -> None:
        from omt_client import REPO_ROOT

        page = (REPO_ROOT / "docs" / "reference" / "settings.md").read_text()
        assert "immediately" in page
        assert "Reload" in page
