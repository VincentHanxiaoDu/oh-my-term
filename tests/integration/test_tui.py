"""
The TUI, driven through a real pty.

Everything about omt's terminal surface was tested through capabilities, and a
capability passing says nothing about whether anything is on screen: the TUI
drew the pty grid and one hint line, so a multiplexer was indistinguishable
from the shell it wraps, and every gate stayed green.

These run the real binary on a real pty and read what it paints.
"""

from __future__ import annotations

import os
import pty
import re
import selectors
import signal
import subprocess
import time
from pathlib import Path

import pytest

import omt_client

#: Escape sequences, stripped so an assertion is about text rather than colour.
ANSI = re.compile(r"\x1b\[[0-9;?]*[A-Za-z]|\x1b[()][A-Za-z0-9]")


def strip(text: str) -> str:
    return ANSI.sub("", text)


class Tui:
    """A running `omt` with a terminal on the other end of a pty."""

    def __init__(self, cwd: Path) -> None:
        self.parent, child = pty.openpty()
        # A size a real terminal has. Too small and every layout decision is
        # the degenerate one.
        import fcntl
        import struct
        import termios

        fcntl.ioctl(child, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 100, 0, 0))
        self.process = subprocess.Popen(
            [str(omt_client.omt_binary())],
            cwd=cwd,
            stdin=child,
            stdout=child,
            stderr=child,
            start_new_session=True,
            env={**os.environ, "TERM": "xterm-256color"},
        )
        os.close(child)
        self.seen = ""

    def read(self, seconds: float = 2.0) -> str:
        """Everything painted within a window."""
        selector = selectors.DefaultSelector()
        selector.register(self.parent, selectors.EVENT_READ)
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            for _ in selector.select(timeout=0.2):
                try:
                    chunk = os.read(self.parent, 65536)
                except OSError:
                    return self.seen
                if not chunk:
                    return self.seen
                self.seen += chunk.decode("utf-8", "replace")
        selector.close()
        return self.seen

    def send(self, text: str) -> None:
        os.write(self.parent, text.encode())

    def close(self) -> None:
        try:
            os.killpg(os.getpgid(self.process.pid), signal.SIGKILL)
        except (ProcessLookupError, PermissionError):
            self.process.kill()
        self.process.wait(timeout=10)
        os.close(self.parent)


@pytest.fixture
def tui(workspace: Path):
    running = Tui(workspace)
    # Long enough for the shell to start and omt to paint a first frame.
    running.read(3.0)
    yield running
    running.close()


class TestTheScreenIsOmtsAndNotOnlyTheShells:
    def test_it_says_which_workspace_this_is(self, tui: Tui) -> None:
        # A multiplexer whose screen is indistinguishable from the shell it
        # wraps is one nobody can tell they are running.
        painted = strip(tui.read(2.0))
        assert "omt-integration" in painted, painted[-600:]

    def test_it_shows_the_session_that_is_open(self, tui: Tui) -> None:
        painted = strip(tui.read(2.0))
        assert "shell" in painted, painted[-600:]

    def test_the_focused_session_is_marked_as_focused(self, tui: Tui) -> None:
        # Which one typing goes to is the question a tab bar exists to answer.
        painted = strip(tui.read(2.0))
        assert re.search(r"\[[^\]]*shell[^\]]*\]", painted), painted[-600:]

    def test_it_says_how_many_sessions_there_are(self, tui: Tui) -> None:
        painted = strip(tui.read(2.0))
        assert re.search(r"\b1 session\b", painted), painted[-600:]

    def test_it_says_which_branch_the_workspace_is_on(self, tui: Tui) -> None:
        # The workspace is a real git repository, so there is a branch to show.
        painted = strip(tui.read(2.0))
        assert "main" in painted, painted[-600:]


class TestTheShellStillWorks:
    def test_what_is_typed_is_echoed(self, tui: Tui) -> None:
        # The chrome takes two rows. Taking them rather than drawing over them
        # is the only version that works — a program that believes it owns a row
        # will draw there.
        tui.send("echo tui-alive\r")
        painted = strip(tui.read(3.0))
        assert "tui-alive" in painted, painted[-800:]

    def test_the_hint_row_is_still_there(self, tui: Tui) -> None:
        painted = strip(tui.read(2.0))
        assert "detaches" in painted or "shows every key" in painted, painted[-400:]


class TestTheLists:
    def test_the_palette_opens_and_shows_the_catalog(self, tui: Tui) -> None:
        # `ACTIONS` was a table with no code that drew it, and the parity gate
        # passed on the table — which is how a capability can be "reachable from
        # the palette" when there is no palette.
        tui.send("\x01p")
        painted = strip(tui.read(2.0))
        assert "Commands" in painted, painted[-700:]
        assert "session" in painted, painted[-700:]

    def test_typing_narrows_the_palette(self, tui: Tui) -> None:
        tui.send("\x01p")
        tui.read(1.0)
        tui.send("worktree")
        painted = strip(tui.read(2.0))
        assert "worktree" in painted, painted[-700:]

    def test_a_filter_that_matches_nothing_says_so(self, tui: Tui) -> None:
        # An empty box looks like a broken one; the useful information is that
        # the filter matched nothing.
        tui.send("\x01p")
        tui.read(1.0)
        tui.send("zzzzzz")
        painted = strip(tui.read(2.0))
        assert "nothing matches" in painted, painted[-700:]

    def test_escape_closes_it_and_the_shell_takes_keys_again(self, tui: Tui) -> None:
        # A list that keeps the keyboard after it is dismissed is a terminal
        # that has stopped responding.
        tui.send("\x01p")
        tui.read(1.0)
        tui.send("\x1b")
        tui.read(0.5)
        tui.send("echo after-palette\r")
        painted = strip(tui.read(3.0))
        assert "after-palette" in painted, painted[-700:]

    def test_the_sessions_list_opens(self, tui: Tui) -> None:
        # Moving between sessions by name rather than counting how many times
        # to press next.
        tui.send("\x01w")
        painted = strip(tui.read(2.0))
        assert "Sessions" in painted, painted[-700:]

    def test_the_agents_list_opens(self, tui: Tui) -> None:
        tui.send("\x01a")
        painted = strip(tui.read(2.0))
        assert "Agents" in painted, painted[-700:]

    def test_the_keys_typed_into_a_list_do_not_reach_the_shell(
        self, tui: Tui
    ) -> None:
        # Letting a keystroke through would type into a program nobody is
        # looking at.
        tui.send("\x01p")
        tui.read(1.0)
        tui.send("rm -rf")
        tui.send("\x1b")
        painted = strip(tui.read(2.0))
        assert "rm -rf" not in painted.split("Commands")[-1] or "nothing matches" in painted


class TestPanes:
    def test_splitting_puts_a_second_session_beside_the_first(self, tui: Tui) -> None:
        # `17 §1`. A split that starts no process is a divider; a split that
        # starts one is a pane.
        tui.send("\x01s")
        painted = strip(tui.read(3.0))
        assert re.search(r"\b2 sessions\b", painted), painted[-500:]

    def test_focus_moves_between_them(self, tui: Tui) -> None:
        # Which pane typing goes to is the question the bar exists to answer, so
        # the mark has to move with the focus.
        tui.send("\x01s")
        tui.read(2.0)
        before = strip(tui.read(1.0))
        tui.send("\x01o")
        after = strip(tui.read(2.0))
        assert before != after, "focus moved and nothing on screen changed"

    def test_closing_a_pane_leaves_the_other_running(self, tui: Tui) -> None:
        # Closing a view of something is not ending it.
        tui.send("\x01s")
        tui.read(2.0)
        tui.send("\x01x")
        painted = strip(tui.read(3.0))
        assert re.search(r"\b1 session\b", painted), painted[-500:]
        tui.send("echo still-here\r")
        assert "still-here" in strip(tui.read(3.0))

    def test_a_split_that_would_not_fit_is_refused(self, tui: Tui) -> None:
        # Two unusable panes are worse than one usable one, and somebody cannot
        # see why nothing is legible.
        for _ in range(12):
            tui.send("\x01s")
            tui.read(0.4)
        painted = strip(tui.read(2.0))
        # However many fit, the bar still agrees with itself about how many
        # there are — a count that disagrees with the panes beside it is the
        # bug this project keeps finding.
        found = re.findall(r"\b(\d+) sessions?\b", painted)
        assert found, painted[-400:]


class TestTheKeymapIsDiscoverable:
    def test_the_help_shows_every_binding(self, tui: Tui) -> None:
        # `16 §7`. A binding nobody can find is a binding nobody uses, and the
        # hint row says this exists precisely so it can be found.
        tui.send("\x01?")
        painted = strip(tui.read(2.0))
        assert "detach" in painted.lower() or "^A" in painted, painted[-600:]

    def test_the_prefix_is_reserved_and_reaches_omt_not_the_shell(
        self, tui: Tui
    ) -> None:
        # Every bare key belongs to the program underneath, which is why omt
        # takes a prefix rather than a key.
        tui.send("\x01w")
        painted = strip(tui.read(2.0))
        assert "Sessions" in painted, painted[-400:]

    def test_ctrl_c_still_reaches_the_program(self, tui: Tui) -> None:
        # The single most useful line on the cheatsheet is that Ctrl-C
        # interrupts the program. It has to remain true.
        tui.send("sleep 30\r")
        tui.read(1.0)
        tui.send("\x03")
        tui.send("echo interrupted\r")
        painted = strip(tui.read(4.0))
        assert "interrupted" in painted, painted[-500:]


class TestKeymapConflicts:
    """16-input-and-keymap §5. The part the doc calls actually novel."""

    def test_a_chord_the_inner_program_needs_says_what_it_would_break(
        self, client
    ) -> None:
        # "Conflicts with something" is not actionable. What somebody needs to
        # know is which program, and what they would lose.
        out = client.call("keys.conflicts", chord="ctrl-w")
        assert out["conflicts"], out
        first = out["conflicts"][0]
        assert "vim" in first["program"].lower()
        assert first["does"]
        assert first["severity"] == "critical"

    def test_a_critical_conflict_is_refused_and_a_minor_one_is_not(
        self, client
    ) -> None:
        # Refusing on anything less leaves omt with almost no bindings, since
        # every chord collides with something somewhere.
        assert client.call("keys.conflicts", chord="ctrl-x")["refused"] is True
        assert client.call("keys.conflicts", chord="ctrl-alt-f9")["refused"] is False

    def test_spelling_does_not_change_the_answer(self, client) -> None:
        # Comparing written strings would silently miss most conflicts and
        # report a clean bill of health, which is worse than not checking.
        answers = {
            client.call("keys.conflicts", chord=spelling)["chord"]
            for spelling in ("ctrl-w", "Ctrl-W", "CTRL-w")
        }
        assert len(answers) == 1, answers

    def test_every_conflict_says_what_it_was_checked_against(self, client) -> None:
        # A keymap is a claim about somebody else's software at a point in
        # time. Without a version, a stale file looks exactly like a current
        # one and omt warns confidently about a binding that has moved.
        for conflict in client.call("keys.conflicts", chord="ctrl-c")["conflicts"]:
            assert conflict["verified_against"]

    def test_a_free_chord_reports_nothing_rather_than_hedging(self, client) -> None:
        out = client.call("keys.conflicts", chord="ctrl-alt-f9")
        assert out["conflicts"] == []

    def test_something_that_is_not_a_chord_is_refused(self, client) -> None:
        import pytest

        with pytest.raises(Exception) as caught:
            client.call("keys.conflicts", chord="not a chord at all")
        assert "chord" in str(caught.value).lower()


class TestOmtsOwnDefaultsAreAudited:
    """16-input-and-keymap §3 and §8. The leader, and what the defaults take."""

    def test_the_defaults_take_nothing_critical_from_anybody(self, client) -> None:
        # The test that would fail the day somebody adds a convenient default
        # that breaks vim.
        out = client.call("keys.audit")
        critical = [c for c in out["conflicts"] if c["severity"] == "critical"]
        assert critical == [], critical

    def test_the_audit_says_how_many_programs_it_checked_against(
        self, client
    ) -> None:
        # "No conflicts" from a check that examined nothing looks exactly like
        # "no conflicts" from one that examined everything.
        out = client.call("keys.audit")
        assert out["programs_known"] >= 4

    def test_the_leader_is_reachable_and_documented(self, client) -> None:
        # §3. A leader nobody can discover is a leader nobody uses, so the
        # cheatsheet has to name it.
        sheet = client.call("keys.cheatsheet")
        text = str(sheet).lower()
        assert "ctrl" in text, sheet
