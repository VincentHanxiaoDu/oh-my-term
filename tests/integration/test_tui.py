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
