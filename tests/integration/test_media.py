"""The blob store: one landing zone for everything that is not text.

`docs/architecture/09-ssh-and-media.md` §2.
"""

from __future__ import annotations

import base64

import pytest

from omt_client import Client

# A one-pixel PNG header, so the sniffer has something real to recognise.
PNG = bytes([0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, 73, 72, 68, 82])


def b64(data: bytes) -> str:
    return base64.b64encode(data).decode()


class TestLanding:
    def test_bytes_land_and_come_back_unchanged(self, client: Client) -> None:
        put = client.call("media.put", content=b64(PNG), origin="web_upload")
        got = client.call("media.get", digest=put["digest"])
        assert base64.b64decode(got["content"]) == PNG

    def test_the_name_is_the_content(self, client: Client) -> None:
        # Which removes a whole class of path problems before they exist: the
        # sender cannot choose where its blob is filed.
        one = client.call("media.put", content=b64(PNG), origin="web_upload")
        two = client.call("media.put", content=b64(PNG), origin="osc_bridge")
        assert one["digest"] == two["digest"]

    def test_the_type_is_sniffed_not_declared(self, client: Client) -> None:
        # A sender that names its own content type decides how the receiver
        # treats it, which is the whole of several decoder exploits.
        put = client.call("media.put", content=b64(PNG), origin="osc_bridge")
        assert put["kind"] == "image"

    def test_an_empty_blob_is_refused(self, client: Client) -> None:
        with pytest.raises(Exception):
            client.call("media.put", content="", origin="web_upload")

    def test_content_that_is_not_base64_is_refused(self, client: Client) -> None:
        with pytest.raises(Exception) as caught:
            client.call("media.put", content="not base64!", origin="web_upload")
        assert "base64" in str(caught.value).lower()

    def test_fetching_a_blob_that_is_not_there_is_an_error(
        self, client: Client
    ) -> None:
        with pytest.raises(Exception):
            client.call("media.get", digest="0" * 64)


class TestTheLandingZoneIsVisible:
    def test_what_landed_can_be_listed(self, client: Client) -> None:
        # A landing zone nobody can see into is one that has quietly become the
        # place things live.
        client.call("media.put", content=b64(PNG), origin="web_upload")
        out = client.call("media.list")
        assert any(b["digest"] for b in out["blobs"])

    def test_the_total_agrees_with_the_list(self, client: Client) -> None:
        out = client.call("media.list")
        assert out["used"] == sum(b["size"] for b in out["blobs"])

    def test_where_each_blob_came_from_is_recorded(self, client: Client) -> None:
        # A blob that arrived over a bridge from an untrusted terminal is not
        # the same thing as one the local user pasted, however identical the
        # bytes.
        put = client.call("media.put", content=b64(b"hello over the bridge"), origin="osc_bridge")
        listed = client.call("media.list")["blobs"]
        entry = next(b for b in listed if b["digest"] == put["digest"])
        assert entry["origin"] == "oscbridge"

    def test_a_repeated_upload_does_not_consume_the_quota_twice(
        self, client: Client
    ) -> None:
        payload = b64(b"the same bytes, twice")
        client.call("media.put", content=payload, origin="web_upload")
        before = client.call("media.list")["used"]
        client.call("media.put", content=payload, origin="web_upload")
        assert client.call("media.list")["used"] == before


class TestClipboardAcrossSsh:
    """§3. OSC 52, which rides the connection that is already there."""

    def test_a_plain_terminal_gets_a_bare_sequence(self, client: Client) -> None:
        out = client.call("media.clipboard", text="hello", term="xterm-256color")
        assert out["sequence"].startswith("\x1b]52;c;")

    def test_tmux_gets_a_passthrough_wrapper(self, client: Client) -> None:
        # Keyed off TERM, not $TMUX: TERM survives ssh and $TMUX does not, so
        # keying off the variable gives a copy that works locally and silently
        # does nothing remotely.
        out = client.call("media.clipboard", text="hello", term="screen-256color")
        assert out["sequence"].startswith("\x1bPtmux;")
        assert out["sequence"].endswith("\x1b\\")

    def test_omt_does_not_claim_the_copy_succeeded(self, client: Client) -> None:
        # There is no acknowledgement to base that on.
        out = client.call("media.clipboard", text="hello", term="xterm")
        assert "sent" in out["message"].lower()
        assert "copied" not in out["message"].lower()

    def test_something_over_the_limit_is_refused_not_truncated(
        self, client: Client
    ) -> None:
        # A truncated clipboard is worse than a refused one: the paste looks
        # like it worked and the tail is gone.
        with pytest.raises(Exception) as caught:
            client.call("media.clipboard", text="x" * 70000, term="xterm")
        assert "limit" in str(caught.value).lower()

    def test_copying_nothing_is_refused(self, client: Client) -> None:
        with pytest.raises(Exception):
            client.call("media.clipboard", text="", term="xterm")


class TestHandingAFileToAnAgent:
    """§7.1. Per-adapter, because one format for all of them is ignored by most."""

    def test_each_agent_gets_its_own_syntax(self, client: Client) -> None:
        forms = {
            agent: client.call("media.attach", agent=agent, path="/tmp/a.png")[
                "reference"
            ]
            for agent in ("claude-code", "aider", "gemini")
        }
        assert forms["claude-code"] == '@"/tmp/a.png"'
        assert forms["aider"] == "/add /tmp/a.png"
        assert forms["gemini"] == "@/tmp/a.png"

    def test_every_reference_carries_the_path(self, client: Client) -> None:
        # A reference that lost the path is one the agent cannot act on, and
        # the failure is silent.
        for agent in ("claude-code", "aider", "gemini", "codex", "brand-new"):
            out = client.call("media.attach", agent=agent, path="/tmp/needle.png")
            assert "/tmp/needle.png" in out["reference"], agent

    def test_an_unknown_agent_is_asked_in_english_and_says_it_is_a_fallback(
        self, client: Client
    ) -> None:
        out = client.call("media.attach", agent="brand-new", path="/tmp/a.png")
        assert out["confidence"] == "fallback"
        assert "look at" in out["reference"]

    def test_a_relative_path_is_refused(self, client: Client) -> None:
        # It would resolve against whatever directory the agent happens to be
        # in, which is not knowable from here.
        with pytest.raises(Exception) as caught:
            client.call("media.attach", agent="claude-code", path="shot.png")
        assert "absolute" in str(caught.value).lower()


class TestMetadataDoesNotSurvive:
    """§4.1 and §8. Stripped on the way in, at the one place everything passes."""

    def test_gps_does_not_reach_an_agent(self, client: Client) -> None:
        # A screenshot from a phone carries EXIF, and EXIF carries GPS. Handing
        # that to an agent — which may send it to a model API — leaks where
        # somebody was standing, with no sign anything was attached beyond the
        # picture they meant to share.
        payload = b"Exif\x00\x00GPSLatitude=51.5074"
        jpeg = (
            bytes([0xFF, 0xD8, 0xFF, 0xE1])
            + (len(payload) + 2).to_bytes(2, "big")
            + payload
            + bytes([0xFF, 0xDA, 0x00, 0x02])
            + b"IMAGEDATA"
        )
        put = client.call("media.put", content=b64(jpeg), origin="local_clipboard")
        got = base64.b64decode(client.call("media.get", digest=put["digest"])["content"])
        assert b"GPSLatitude" not in got
        assert b"IMAGEDATA" in got, "the picture itself survived"

    def test_the_digest_names_what_was_actually_stored(self, client: Client) -> None:
        # Not what arrived. A digest of the pre-strip bytes would name something
        # that is nowhere on disk, and every later fetch by it would miss.
        put = client.call("media.put", content=b64(PNG), origin="web_upload")
        got = client.call("media.get", digest=put["digest"])
        assert len(base64.b64decode(got["content"])) == put["size"]


class TestHowPasteReachesThisMachine:
    """§5.1. A tier is never claimed without a positive handshake."""

    def test_nothing_confirmed_falls_to_out_of_band(self, client: Client) -> None:
        # Never "probably kitty, so assume its protocol". Probably means a
        # paste that silently does nothing.
        out = client.call("media.paste_tier")
        assert out["tier"] == "outofband"

    def test_there_is_no_state_in_which_paste_is_impossible(
        self, client: Client
    ) -> None:
        # A phone can always send an image. Out-of-band is the floor.
        out = client.call("media.paste_tier")
        assert out["explanation"]

    def test_the_best_confirmed_mechanism_wins(self, client: Client) -> None:
        # A machine that could do the good thing must not settle for the QR
        # code because a cheaper probe answered first.
        out = client.call(
            "media.paste_tier",
            omt_on_both_ends=True,
            terminal_upload_confirmed=True,
        )
        assert out["tier"] == "native"

    def test_each_tier_explains_itself(self, client: Client) -> None:
        # So every surface can say exactly why paste behaves the way it does.
        for probe, expected in [
            ({"reverse_socket_answered": True}, "reversesocket"),
            ({"osc_companion_answered": True}, "oscbridge"),
            ({"terminal_upload_confirmed": True}, "terminalnative"),
        ]:
            out = client.call("media.paste_tier", **probe)
            assert out["tier"] == expected
            assert out["explanation"].endswith(".")

    def test_a_socket_that_answers_nothing_is_not_a_tier(self, client: Client) -> None:
        # The shape of a stale forward from a previous session. Treating its
        # presence as success gives a paste that vanishes.
        out = client.call("media.paste_tier", reverse_socket_answered=False)
        assert out["tier"] == "outofband"


class TestTheThinClient:
    """§6. `omt ssh <target>`, and what it would actually run."""

    def test_omt_on_both_ends_speaks_the_wire_protocol_over_stdio(
        self, client: Client
    ) -> None:
        out = client.call("ssh.plan", target="box", remote_has_omt=True)
        assert out["argv"] == ["ssh", "box", "omt", "serve", "--stdio"]
        assert out["mode"] == "native"
        assert out["direct_media"] is True

    def test_the_command_is_a_vector_with_no_shell_string_in_it(
        self, client: Client
    ) -> None:
        # `ssh box "omt serve --stdio"` puts the remote command through the
        # remote login shell.
        out = client.call("ssh.plan", target="box", remote_has_omt=True)
        assert all(" " not in arg for arg in out["argv"])

    def test_a_target_that_would_be_read_as_an_option_is_refused(
        self, client: Client
    ) -> None:
        # `-oProxyCommand=...` is the well-known way a hostname becomes local
        # execution.
        for target in ["-oProxyCommand=touch /tmp/pwned", "-J evil"]:
            with pytest.raises(Exception):
                client.call("ssh.plan", target=target, remote_has_omt=True)

    def test_a_target_carrying_shell_metacharacters_is_refused(
        self, client: Client
    ) -> None:
        for target in ["box; rm -rf /", "box$(whoami)", "box`id`", "a box"]:
            with pytest.raises(Exception):
                client.call("ssh.plan", target=target, remote_has_omt=True)

    def test_ordinary_targets_are_accepted(self, client: Client) -> None:
        for target in ["box", "user@box", "user@box.example.com", "10.0.0.4"]:
            out = client.call("ssh.plan", target=target, remote_has_omt=True)
            assert target in out["argv"]

    def test_declining_the_install_falls_back_and_says_what_changed(
        self, client: Client
    ) -> None:
        # The user types one thing. What they get is reported, not chosen by
        # which command they remembered.
        out = client.call(
            "ssh.plan", target="box", remote_has_omt=False, bootstrap="never"
        )
        assert out["mode"] == "managedpty"
        assert out["direct_media"] is False
        assert "OSC bridge" in out["reason"]

    def test_assuming_it_is_installed_does_not_probe(self, client: Client) -> None:
        out = client.call(
            "ssh.plan",
            target="box",
            remote_has_omt=False,
            bootstrap="assume_installed",
        )
        assert "--exec" not in out["argv"]
        assert "fail loudly" in out["reason"]
