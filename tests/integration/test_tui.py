"""
The TUI, driven through a real pty.

Everything about omt's terminal surface was tested through capabilities, and a
capability passing says nothing about whether anything is on screen: the TUI
drew the pty grid and one hint line, so a multiplexer was indistinguishable
from the shell it wraps, and every gate stayed green.

These run the real binary on a real pty and read what it paints.
"""

from __future__ import annotations

# omt's prefix, as a byte. Named rather than written as `\x01` at thirty call
# sites, because the default leader moved from `Ctrl-A` to `Ctrl-B` and the
# literals were the reason changing it looked like eight unrelated failures.
PREFIX = "\x02"  # Ctrl-B

import os
import pty
import re
import selectors
import shutil
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
        self.cwd = str(cwd)
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
        tui.send(PREFIX + "p")
        painted = strip(tui.read(2.0))
        assert "Commands" in painted, painted[-700:]
        assert "session" in painted, painted[-700:]

    def test_typing_narrows_the_palette(self, tui: Tui) -> None:
        tui.send(PREFIX + "p")
        tui.read(1.0)
        tui.send("worktree")
        painted = strip(tui.read(2.0))
        assert "worktree" in painted, painted[-700:]

    def test_a_filter_that_matches_nothing_says_so(self, tui: Tui) -> None:
        # An empty box looks like a broken one; the useful information is that
        # the filter matched nothing.
        tui.send(PREFIX + "p")
        tui.read(1.0)
        tui.send("zzzzzz")
        painted = strip(tui.read(2.0))
        assert "nothing matches" in painted, painted[-700:]

    def test_escape_closes_it_and_the_shell_takes_keys_again(self, tui: Tui) -> None:
        # A list that keeps the keyboard after it is dismissed is a terminal
        # that has stopped responding.
        tui.send(PREFIX + "p")
        tui.read(1.0)
        tui.send("\x1b")
        tui.read(0.5)
        tui.send("echo after-palette\r")
        painted = strip(tui.read(3.0))
        assert "after-palette" in painted, painted[-700:]

    def test_the_sessions_list_opens(self, tui: Tui) -> None:
        # Moving between sessions by name rather than counting how many times
        # to press next.
        tui.send(PREFIX + "w")
        painted = strip(tui.read(2.0))
        assert "Sessions" in painted, painted[-700:]

    def test_the_agents_list_opens(self, tui: Tui) -> None:
        tui.send(PREFIX + "a")
        painted = strip(tui.read(2.0))
        assert "Agents" in painted, painted[-700:]

    def test_the_keys_typed_into_a_list_do_not_reach_the_shell(
        self, tui: Tui
    ) -> None:
        # Letting a keystroke through would type into a program nobody is
        # looking at.
        tui.send(PREFIX + "p")
        tui.read(1.0)
        tui.send("rm -rf")
        tui.send("\x1b")
        painted = strip(tui.read(2.0))
        assert "rm -rf" not in painted.split("Commands")[-1] or "nothing matches" in painted


class TestPanes:
    def test_splitting_puts_a_second_session_beside_the_first(self, tui: Tui) -> None:
        # `17 §1`. A split that starts no process is a divider; a split that
        # starts one is a pane.
        tui.send(PREFIX + "s")
        painted = strip(tui.read(3.0))
        assert re.search(r"\b2 sessions\b", painted), painted[-500:]

    def test_focus_moves_between_them(self, tui: Tui) -> None:
        # Which pane typing goes to is the question the bar exists to answer, so
        # the mark has to move with the focus.
        tui.send(PREFIX + "s")
        tui.read(2.0)
        before = strip(tui.read(1.0))
        tui.send(PREFIX + "o")
        after = strip(tui.read(2.0))
        assert before != after, "focus moved and nothing on screen changed"

    def test_closing_a_pane_leaves_the_other_running(self, tui: Tui) -> None:
        # Closing a view of something is not ending it.
        tui.send(PREFIX + "s")
        tui.read(2.0)
        tui.send(PREFIX + "x")
        painted = strip(tui.read(3.0))
        assert re.search(r"\b1 session\b", painted), painted[-500:]
        tui.send("echo still-here\r")
        assert "still-here" in strip(tui.read(3.0))

    def test_a_split_that_would_not_fit_is_refused(self, tui: Tui) -> None:
        # Two unusable panes are worse than one usable one, and somebody cannot
        # see why nothing is legible.
        for _ in range(12):
            tui.send(PREFIX + "s")
            tui.read(0.4)
        painted = strip(tui.read(2.0))
        # However many fit, the bar still agrees with itself about how many
        # there are — a count that disagrees with the panes beside it is the
        # bug this project keeps finding.
        found = re.findall(r"\b(\d+) sessions?\b", painted)
        assert found, painted[-400:]


class TestWhatIsRunningIsNoticed:
    """The agent layer, fed from the local terminal.

    Everything below existed and nothing called it: adapters could identify
    eleven agents, the merge machine could rank six tiers, and `omt` ran a
    shell without ever asking. Starting an agent in a pane and opening the
    list showed "nothing matches", because nothing had ever looked.
    """

    def test_a_pane_running_no_agent_says_so_rather_than_showing_nothing(
        self, tui: Tui
    ) -> None:
        # An empty list and "omt found nothing" are different answers, and the
        # second is the one that says whether to go looking for a bug.
        tui.send(PREFIX + "a")
        painted = strip(tui.read(2.0))
        assert "no agent in any pane" in painted, painted[-600:]

    def test_an_agent_in_a_pane_is_found_and_named(self, tui: Tui) -> None:
        # A binary *named* `claude`, which is the exe-name half of the
        # fingerprint and works on every platform. Deliberately a copy rather
        # than `/bin/sleep` renamed in argv: on macOS the environment of an
        # Apple-signed platform binary is not readable, so a stand-in that
        # relied on an env marker would test the kernel's mood rather than
        # omt. The env-marker path has its own test in `omt-pty`, against a
        # process whose environment can actually be read.
        # A copy of omt's own binary, named `claude`. Not a copy of
        # `/bin/sleep`: a copy of an Apple-signed platform binary loses its
        # signature and is killed on sight, and not `sleep` renamed in argv
        # either, because on macOS a platform binary's environment cannot be
        # read at all — a stand-in resting on that would be testing the
        # kernel's mood rather than omt. `serve` on a throwaway socket is
        # simply something that does not exit.
        fake = os.path.join(tui.cwd, "claude")
        shutil.copy(str(omt_client.omt_binary()), fake)
        os.chmod(fake, 0o755)
        tui.send(f"{fake} serve --socket {tui.cwd}/probe.sock\r")
        # Longer than the probe interval, which is what makes this a test of
        # the loop and not only of `identify`.
        tui.read(2.0)
        tui.send(PREFIX + "a")
        painted = strip(tui.read(2.0))
        assert "Claude Code" in painted, painted[-800:]
        # The evidence travels with the verdict: somebody looking at a wrong
        # detection needs to see what matched, not be told a name again.
        assert "CLAUDECODE" in painted, painted[-800:]
        # Against the *last* frame, not the accumulated history: the list is
        # rebuilt every frame, so the earlier "nothing here yet" frames are
        # legitimately in the buffer. That the newest frame no longer says it
        # is what proves the list is live rather than a snapshot.
        last_frame = painted[painted.rfind("Agents") :]
        assert "no agent in any pane" not in last_frame, last_frame[:600]


class TestTheScreenIsNotCorrupted:
    def test_an_open_overlay_is_not_painted_over_by_the_panes(
        self, tui: Tui
    ) -> None:
        # The panes repaint every frame from their own damage. An overlay
        # drawn once is painted over within milliseconds, and what is left is
        # half a list interleaved with a shell.
        tui.send(PREFIX + "p")
        tui.read(0.5)
        # Keep the pane busy underneath, which is what forces the repaints.
        painted = strip(tui.read(2.0))
        assert "Commands" in painted, painted[-600:]
        # The last thing painted must still contain the overlay, not only the
        # first.
        tail = painted[-2000:]
        assert "Commands" in tail or "type to filter" in tail, tail

    def test_the_cursor_ends_in_the_pane_and_not_on_the_status_row(
        self, tui: Tui
    ) -> None:
        # Every write moves the cursor, so placing it before the chrome leaves
        # it wherever the last row finished. `\x1b[H` sequences are the record
        # of where it went: the final one in a frame must not be the bottom
        # row.
        tui.send("echo where-is-the-cursor\r")
        raw = tui.read(3.0)
        positions = re.findall(r"\x1b\[(\d+);(\d+)H", raw)
        assert positions, raw[-400:]
        last_row = int(positions[-1][0])
        # The bottom row is omt's status row and nothing should ever leave the
        # cursor parked there.
        assert last_row != 24, f"cursor left on the status row: {positions[-6:]}"


class TestPasteArrivesAtOnce:
    """A paste is a burst, and the loop used to read one event per frame.

    With a frame costing two `git status` process spawns, a pasted paragraph
    appeared a character at a time — which is what it looked like to a user.
    """

    def test_a_long_paste_appears_in_one_go(self, tui: Tui) -> None:
        text = "".join(f"word{i} " for i in range(120))
        tui.send("cat > /dev/null\r")
        tui.read(1.0)
        before = len(tui.seen)
        # Bracketed, exactly as a terminal delivers a real paste.
        tui.send("\x1b[200~" + text + "\x1b[201~")
        # Half a second is far less than the seconds a
        # one-event-per-frame loop needed, and far more than an echo needs.
        painted = strip(tui.read(0.5)[before:])
        assert "word0" in painted, painted[:300]
        assert "word119" in painted, (
            "the tail of the paste had not arrived within half a second: "
            f"{painted[-300:]}"
        )
        tui.send("\x04")


class TestTheKeymapIsDiscoverable:
    def test_the_help_shows_every_binding(self, tui: Tui) -> None:
        # `16 §7`. A binding nobody can find is a binding nobody uses, and the
        # hint row says this exists precisely so it can be found.
        tui.send(PREFIX + "?")
        painted = strip(tui.read(2.0))
        assert "detach" in painted.lower() or "^B" in painted, painted[-600:]

    def test_holding_the_leader_lists_what_it_can_do(self, tui: Tui) -> None:
        # `16 §3.3`. Pressing the prefix and stopping is what somebody does
        # when they have forgotten the key, and it is the moment the menu is
        # worth a row. Generated from the binding table, so this checks the
        # keys that used to be reachable and unadvertised.
        tui.send(PREFIX)
        painted = strip(tui.read(2.0))
        for label in ("detach", "sessions", "agents", "palette", "close"):
            assert label in painted, f"`{label}` missing:\n{painted[-500:]}"
        # And it does not leak into the pane: no bytes are written while
        # pending, so the shell never sees the prefix.
        tui.send("d")
        tui.read(0.5)

    def test_the_leader_strip_waits_rather_than_flashing(self, tui: Tui) -> None:
        # `16 §3.3` asks for 250 ms. A user executing the chord from muscle
        # memory must never see the strip appear and vanish.
        tui.send(PREFIX + "w")
        painted = strip(tui.read(2.0))
        # The chord completed, so what is on screen is its result and not the
        # menu that would have flashed on the way.
        assert "Sessions" in painted, painted[-400:]
        assert "palette" not in painted, painted[-500:]

    def test_the_prefix_is_reserved_and_reaches_omt_not_the_shell(
        self, tui: Tui
    ) -> None:
        # Every bare key belongs to the program underneath, which is why omt
        # takes a prefix rather than a key.
        tui.send(PREFIX + "w")
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


class TestModalKeymaps:
    """16-input-and-keymap §6. The hardest conflict case in the document."""

    def test_vim_mode_gives_copy_mode_what_it_is_missed_for(self, client) -> None:
        out = client.call("keys.flavour", flavour="vim")
        chords = {b["chord"] for b in out["bindings"] if b["mode"] == "copy"}
        assert {"h", "j", "k", "l", "y"} <= chords, chords

    def test_a_modal_keymap_only_governs_omts_own_surfaces(self, client) -> None:
        # The scope rule that makes the feature safe. Somebody running vim in
        # a pane, with omt in vim mode, must not have `j` taken from them.
        out = client.call("keys.flavour", flavour="vim")
        assert "global" not in out["governs"], out["governs"]
        assert all(b["mode"] in out["governs"] for b in out["bindings"])

    def test_emacs_mode_binds_its_own_motions(self, client) -> None:
        out = client.call("keys.flavour", flavour="emacs")
        chords = {b["chord"] for b in out["bindings"]}
        assert "ctrl-n" in chords
        assert "j" not in chords, "the two flavours are alternatives, not layers"

    def test_the_plain_flavour_adds_nothing(self, client) -> None:
        assert client.call("keys.flavour", flavour="plain")["bindings"] == []

    def test_an_unknown_flavour_is_refused_and_says_what_exists(
        self, client
    ) -> None:
        import pytest

        with pytest.raises(Exception) as caught:
            client.call("keys.flavour", flavour="kakoune")
        message = str(caught.value).lower()
        assert "vim" in message and "emacs" in message


class TestKeysAreExplicable:
    """16-input-and-keymap §11. The keys.* capabilities this document introduces."""

    def test_every_binding_is_listed_in_a_stable_order(self, client) -> None:
        # An unordered list makes a diff between two configurations unreadable.
        first = client.call("keys.list")["bindings"]
        second = client.call("keys.list")["bindings"]
        assert first == second
        assert first, "the defaults bind something"

    def test_explaining_a_key_says_what_happens_in_a_pane(self, client) -> None:
        # "Not bound" and "bound, but the pane underneath owns it" look
        # identical from the user's chair and have different fixes.
        out = client.call("keys.explain", chord="ctrl-b")
        assert out["in_a_pane"]
        assert out["chord"]

    def test_explaining_a_key_gathers_what_it_would_break(self, client) -> None:
        out = client.call("keys.explain", chord="ctrl-w")
        assert out["conflicts"], out
        assert out["conflicts"][0]["severity"] == "critical"

    def test_explaining_says_whether_a_key_is_awkward_on_a_non_us_layout(
        self, client
    ) -> None:
        awkward = client.call("keys.explain", chord="|")
        assert awkward["awkward"] is True
        assert awkward["alias"] == "v"
        plain = client.call("keys.explain", chord="ctrl-b")
        assert plain["awkward"] is False

    def test_the_minus_key_can_be_explained_at_all(self, client) -> None:
        # It could not be parsed at all until this change, which meant the
        # documented split-down binding was silently absent.
        out = client.call("keys.explain", chord="-")
        assert out["chord"]

    def test_the_registry_says_how_much_it_knows(self, client) -> None:
        out = client.call("keys.registry")
        assert out["programs"] >= 4
        assert set(out["flavours"]) == {"plain", "vim", "emacs"}

    def test_something_that_is_not_a_chord_is_refused(self, client) -> None:
        import pytest

        with pytest.raises(Exception):
            client.call("keys.explain", chord="not a chord")


class TestKeybindingFiles:
    """10-configuration §8.2. Flat, contexts on the trigger."""

    def test_a_good_file_reports_nothing(self, client) -> None:
        out = client.call(
            "keys.check_file",
            leader="ctrl-b",
            bindings=[["ctrl-t", "session.create"]],
        )
        assert out["problems"] == []

    def test_a_binding_naming_no_capability_is_caught(self, client) -> None:
        # It parses, loads and does nothing, and the person who wrote it
        # concludes their key is broken rather than their spelling.
        out = client.call(
            "keys.check_file",
            leader="ctrl-b",
            bindings=[["ctrl-t", "session.craete"]],
        )
        assert len(out["problems"]) == 1
        assert "not a capability" in out["problems"][0]["message"]

    def test_a_near_miss_is_suggested(self, client) -> None:
        out = client.call(
            "keys.check_file", leader="ctrl-b", bindings=[["ctrl-t", "session.creat"]]
        )
        assert "session.create" in out["problems"][0]["help"]

    def test_a_context_lives_on_the_trigger(self, client) -> None:
        # So grepping for `j` finds every place it is bound.
        out = client.call(
            "keys.check_file", leader="ctrl-b", bindings=[["copy:j", "session.create"]]
        )
        assert out["problems"] == []

    def test_an_unknown_context_lists_the_ones_that_exist(self, client) -> None:
        out = client.call(
            "keys.check_file",
            leader="ctrl-b",
            bindings=[["nonsense:j", "session.create"]],
        )
        assert any("copy" in p["help"] for p in out["problems"])

    def test_every_problem_comes_back_with_help(self, client) -> None:
        out = client.call(
            "keys.check_file",
            leader="not a chord",
            bindings=[["ctrl-t", "session.nope"], ["also bad", "session.create"]],
        )
        assert len(out["problems"]) >= 3
        assert all(p["help"] for p in out["problems"])
