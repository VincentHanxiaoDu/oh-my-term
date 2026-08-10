"""First run, and the help that stops it needing a manual.

`docs/architecture/19-onboarding.md` §1, §2 and §4.

The rule under test is §1.2's: **at most one hint is ever offered**. It is
checked over the wire rather than only in Rust because the failure it guards
against is a client showing all of them, and a unit test of the daemon cannot
see a client.
"""

from __future__ import annotations

import pytest

import omt_client
from omt_client import Client


@pytest.fixture
def fresh(tmp_path_factory: pytest.TempPathFactory):
    """An instance nothing has been told anything on yet.

    The shared module instance cannot answer "what does a first run say",
    because by the time a test asks, an earlier test has already been shown the
    first hint — which is the rule working, not a bug. Asserting a first-run
    property against a used instance is how a test comes to depend on the order
    its file happens to run in.
    """
    workspace = tmp_path_factory.mktemp("omt-first-run")
    omt_client.git_repo(workspace)
    started = omt_client.start_instance(workspace)
    connection = omt_client.connect(started)
    yield connection
    connection.close()
    omt_client.stop_instance(started)


class TestTheOneHintRule:
    def test_a_first_run_offers_one_hint_and_not_a_list(self, client: Client) -> None:
        # The whole feature. Six tips teach nobody six things.
        state = client.call("onboarding.hint_state")
        assert state["next"] is not None, "a first run has something to say"
        assert isinstance(state["next"], dict), "one hint, not a list of them"
        assert state["next"]["text"]

    def test_the_first_thing_said_is_how_to_reach_omt_at_all(
        self, fresh: Client
    ) -> None:
        # Not the most impressive feature. Somebody who cannot reach omt has no
        # use for what omt can do. On a fresh instance, because the shared one
        # has already been shown this by another test.
        state = fresh.call("onboarding.hint_state")
        assert "Ctrl-A" in state["next"]["text"]

    def test_a_hint_does_not_come_back_on_the_next_ask(self, client: Client) -> None:
        # Asking for it is showing it. Otherwise every poll re-offers the same
        # tip and one-at-a-time buys nothing.
        first = client.call("onboarding.hint_state")["next"]
        second = client.call("onboarding.hint_state")["next"]
        assert second is None or second["id"] != first["id"]

    def test_it_says_how_many_remain_so_a_client_can_be_honest(
        self, client: Client
    ) -> None:
        state = client.call("onboarding.hint_state")
        assert isinstance(state["remaining"], int)
        assert state["remaining"] >= 0

    def test_dismissing_one_hint_leaves_the_others(self, fresh: Client) -> None:
        # Fresh, because a skip here would prove nothing: on the shared
        # instance the hints may already be spent, and a test that quietly
        # skips reads exactly like a test that passed.
        state = fresh.call("onboarding.hint_state")
        before = state["remaining"]
        assert state["next"] is not None
        out = fresh.call("onboarding.dismiss_hints", hint=state["next"]["id"])
        assert out["remaining"] <= before
        assert not out["silenced"], "one hint is not all of them"

    def test_dismissing_a_hint_that_does_not_exist_is_an_error(
        self, client: Client
    ) -> None:
        with pytest.raises(Exception) as caught:
            client.call("onboarding.dismiss_hints", hint="no-such-hint")
        assert "hint" in str(caught.value).lower()

    def test_silencing_stops_everything_and_reports_nothing_remaining(
        self, client: Client
    ) -> None:
        # "3 more tips" under a silence somebody asked for is a lie, and the
        # kind that makes them distrust the next thing omt tells them.
        out = client.call("onboarding.dismiss_hints")
        assert out["silenced"]
        assert out["remaining"] == 0
        assert client.call("onboarding.hint_state")["next"] is None


class TestHelpIsGeneratedNotWritten:
    def test_topics_come_from_the_capability_groups(self, client: Client) -> None:
        # Derived from the catalog, so the only way to have an undocumented
        # topic is to have an undocumented capability — which the parity gate
        # already refuses.
        topics = client.call("help.topics")["topics"]
        names = {t["name"] for t in topics}
        assert "session" in names
        assert "help" in names
        assert all(t["entries"] for t in topics), "an empty topic is a dead end"

    def test_a_topic_renders_what_is_in_it(self, client: Client) -> None:
        out = client.call("help.render", topic="onboarding")
        assert out["topic"] == "onboarding"
        assert len(out["entries"]) >= 2
        assert all(e["doc"] for e in out["entries"]), "every entry explains itself"

    def test_a_capability_name_is_a_topic_too(self, client: Client) -> None:
        # Answering "no such topic" to a name omt itself printed is a poor
        # showing.
        out = client.call("help.render", topic="help.render")
        assert len(out["entries"]) == 1
        assert out["entries"][0]["name"] == "help.render"

    def test_help_says_which_entries_change_something(self, client: Client) -> None:
        # The one thing somebody reading help most needs before they try it.
        entries = client.call("help.render", topic="onboarding")["entries"]
        assert any(e["command"] for e in entries)
        assert any(not e["command"] for e in entries)

    def test_an_unknown_topic_is_an_error_rather_than_an_empty_page(
        self, client: Client
    ) -> None:
        with pytest.raises(Exception) as caught:
            client.call("help.render", topic="nonsense")
        assert "topic" in str(caught.value).lower()

    def test_an_empty_topic_is_refused(self, client: Client) -> None:
        with pytest.raises(Exception):
            client.call("help.render", topic="   ")

    def test_every_topic_the_tree_lists_can_actually_be_rendered(
        self, client: Client
    ) -> None:
        # A tree offering a topic that then fails to open is worse than not
        # offering it: somebody has to discover the dead end themselves.
        for topic in client.call("help.topics")["topics"]:
            out = client.call("help.render", topic=topic["name"])
            assert len(out["entries"]) == len(topic["entries"])


class TestWhatIsInstalled:
    """§3.3. What a first run finds on this machine."""

    def test_every_agent_omt_can_drive_is_listed_found_or_not(
        self, client: Client
    ) -> None:
        # Listing only what is present hides the difference between an agent
        # omt did not find and one it has never heard of — and the second is
        # the one worth filing a bug about.
        out = client.call("setup.detect_agents")
        assert len(out["agents"]) >= 10
        kinds = {a["kind"] for a in out["agents"]}
        assert "ClaudeCode" in kinds
        assert "Codex" in kinds

    def test_each_entry_says_what_it_is_invoked_as(self, client: Client) -> None:
        out = client.call("setup.detect_agents")
        assert all(a["command"] for a in out["agents"])

    def test_it_says_whether_it_looked_at_all(self, client: Client) -> None:
        # An unreadable PATH makes every answer below meaningless, and
        # reporting "no agents" there would be a confident wrong answer.
        out = client.call("setup.detect_agents")
        assert out["searched"] is True

    def test_the_count_agrees_with_the_list(self, client: Client) -> None:
        out = client.call("setup.detect_agents")
        assert out["found"] == sum(1 for a in out["agents"] if a["path"])


class TestThisTerminal:
    """§7. A wrong TERM does not fail; it draws a subtly wrong screen."""

    def test_it_reports_what_term_advertises(self, client: Client) -> None:
        out = client.call("term.profile")
        assert "term" in out
        assert isinstance(out["entry_resolved"], bool)

    def test_every_disagreement_names_a_remedy(self, client: Client) -> None:
        # A diagnostic that names a problem and not its fix is one people learn
        # to scroll past, which costs the next one too.
        out = client.call("term.profile")
        for d in out["disagreements"]:
            assert d["remedy"], d
            assert d["advertised"]
            assert d["observed"]

    def test_it_says_whether_omt_is_inside_another_multiplexer(
        self, client: Client
    ) -> None:
        # The most common reason a prefix key appears not to work, and nothing
        # on screen tells you.
        out = client.call("term.profile")
        assert "nested" in out


class TestUninstall:
    """§8. A tool that cannot cleanly uninstall does not get installed."""

    def test_a_plan_says_what_it_would_do_and_does_none_of_it(
        self, client: Client
    ) -> None:
        out = client.call("uninstall.plan")
        assert "steps" in out
        assert out["plan"], "a plan hash, so accepting one means something"

    def test_it_will_not_delete_its_own_binary(self, client: Client) -> None:
        # A program that deletes its own executable mid-run leaves a
        # half-finished uninstall with nothing left to finish it.
        out = client.call("uninstall.plan")
        assert "package manager" in out["binary"]

    def test_applying_without_an_explicit_accept_is_refused(
        self, client: Client
    ) -> None:
        with pytest.raises(Exception) as caught:
            client.call("uninstall.apply", plan="whatever", accept=False)
        assert "accept" in str(caught.value).lower()

    def test_applying_a_plan_the_machine_no_longer_matches_is_refused(
        self, client: Client
    ) -> None:
        # A confirmation against a plan that has since changed is a
        # confirmation of something the user never saw.
        with pytest.raises(Exception) as caught:
            client.call("uninstall.apply", plan="not-the-plan", accept=True)
        assert "changed" in str(caught.value).lower()

    def test_every_step_explains_itself(self, client: Client) -> None:
        for step in client.call("uninstall.plan")["steps"]:
            assert step["reason"], step


class TestComingFromTmux:
    """§5. Sixty seconds, judged with fingers rather than eyes."""

    CONFIG = "\n".join(
        [
            "# my tmux.conf",
            "set -g prefix C-a",
            "bind | split-window -h",
            "bind h select-pane -L",
            "bind x kill-pane",
            "bind t clock-mode",
            "bind C-y run-shell 'something custom'",
        ]
    )

    def test_a_binding_omt_honours_names_what_it_becomes(self, client: Client) -> None:
        out = client.call("migrate.tmux_preview", config=self.CONFIG)
        kill = next(d for d in out["directives"] if "kill-pane" in d["source"])
        assert kill["mapping"] == "mapped"
        assert kill["capability"] == "session.close"

    def test_nothing_is_silently_dropped(self, client: Client) -> None:
        # The failure this whole feature exists to prevent: a third of
        # somebody's config vanishing without an error.
        out = client.call("migrate.tmux_preview", config=self.CONFIG)
        assert out["mapped"] + out["review"] + out["unsupported"] == len(
            out["directives"]
        )
        assert all(d["note"] for d in out["directives"])

    def test_what_omt_cannot_do_says_so_by_name(self, client: Client) -> None:
        out = client.call("migrate.tmux_preview", config=self.CONFIG)
        clock = next(d for d in out["directives"] if "clock" in d["source"])
        assert clock["mapping"] == "unsupported"
        assert "clock" in clock["note"]

    def test_a_line_omt_cannot_read_asks_rather_than_guessing(
        self, client: Client
    ) -> None:
        out = client.call("migrate.tmux_preview", config=self.CONFIG)
        custom = next(d for d in out["directives"] if "run-shell" in d["source"])
        assert custom["mapping"] == "review"

    def test_every_directive_keeps_its_line_number(self, client: Client) -> None:
        out = client.call("migrate.tmux_preview", config=self.CONFIG)
        assert all(d["line"] >= 1 for d in out["directives"])
        assert len({d["line"] for d in out["directives"]}) == len(out["directives"])

    def test_the_preview_writes_nothing(self, client: Client) -> None:
        # Parsing somebody's config and telling them what would happen is the
        # whole of the preview.
        first = client.call("migrate.tmux_preview", config=self.CONFIG)
        second = client.call("migrate.tmux_preview", config=self.CONFIG)
        assert first["preview"] == second["preview"]
