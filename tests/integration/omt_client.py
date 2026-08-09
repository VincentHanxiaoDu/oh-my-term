"""
A client for a running omt instance, speaking the real wire protocol.

Deliberately hand-written rather than generated from the TypeScript client:
these tests exist to check that omt does what the documentation says, and a
client generated from omt's own artefacts would agree with omt about a mistake
they both make. The one shared source is the capability catalog, which is what
the parity tests read on purpose.

Everything here is synchronous. A test that has to reason about an event loop
to find out whether a session started is a test whose failures are about the
test.
"""

from __future__ import annotations

import json
import os
import re
import shutil
import signal
import socket
import subprocess
import threading
import uuid
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Iterator

from websocket import (  # type: ignore[import-untyped]
    WebSocket,
    WebSocketBadStatusException,
    create_connection,
)

REPO_ROOT = Path(__file__).resolve().parents[2]


def new_id() -> str:
    """A fresh id, in the form the wire uses.

    A bare UUID: the id newtypes are `serde(transparent)`, so the prefixed
    `sess_…` spelling is a *display* form and is not what crosses the socket.
    Sending the prefixed one fails to deserialize, and an unparseable message
    closes the connection — which from a client looks like the server crashing.
    """
    return str(uuid.uuid4())

#: How long to wait for anything the instance does. Generous, because these run
#: in CI on shared machines, and a flaky timeout is worse than a slow suite.
TIMEOUT = 30.0


class CapabilityError(RuntimeError):
    """A call the instance refused.

    Carries the machine-readable code as well as the message: tests assert on
    the code, because the message is prose and prose is allowed to improve.
    """

    def __init__(self, code: str, message: str, detail: Any = None) -> None:
        super().__init__(f"{code}: {message}")
        self.code = code
        self.message = message
        self.detail = detail


@dataclass
class Instance:
    """An `omt web` process under test."""

    port: int
    #: The invite it printed. Single-exchange and expiring — it is the right to
    #: obtain a credential, not a credential.
    token: str
    process: subprocess.Popen[str]
    cwd: Path
    stdout: str = ""
    #: The credential this test run exchanged its invite for, minted on first
    #: use. Tests that are not *about* joining want the thing a real client
    #: ends up holding.
    credential: str | None = None

    @property
    def url(self) -> str:
        return f"http://127.0.0.1:{self.port}"

    @property
    def ws_url(self) -> str:
        return f"ws://127.0.0.1:{self.port}/api/ws"


def free_port() -> int:
    """A port nothing is listening on.

    Bound and released rather than picked from a range: two tests picking the
    same "probably free" port is the classic way a suite fails only when it is
    run in parallel.
    """
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return int(probe.getsockname()[1])


def omt_binary() -> Path:
    """The binary under test.

    From the environment when set, so the Docker image can point at its own
    build; otherwise the debug build, which is what a developer has to hand.
    """
    override = os.environ.get("OMT_BINARY")
    if override:
        return Path(override)
    built = REPO_ROOT / "target" / "debug" / "omt"
    if not built.exists():
        raise RuntimeError(
            f"no omt binary at {built}. Run `cargo build -p omt` first, "
            "or set OMT_BINARY."
        )
    return built


_TOKEN = re.compile(r"#token=([A-Za-z0-9_]+)")


def start_instance(cwd: Path) -> Instance:
    """Start `omt web` in a directory and wait until it answers.

    The token is read from stdout because that is the only place it exists —
    it is minted once and never written to disk, which is what makes a copied
    state directory not a copied credential.
    """
    port = free_port()
    process = subprocess.Popen(
        [str(omt_binary()), "web", "--bind", f"127.0.0.1:{port}"],
        cwd=cwd,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        # Its own process group, so killing the instance takes the shells it
        # spawned with it rather than leaving them attached to the test runner.
        start_new_session=True,
    )

    token: str | None = None
    lines: list[str] = []
    deadline = time.monotonic() + TIMEOUT
    assert process.stdout is not None
    while time.monotonic() < deadline:
        line = process.stdout.readline()
        if line == "":
            break
        lines.append(line)
        found = _TOKEN.search(line)
        if found:
            token = found.group(1)
            break

    if token is None:
        process.kill()
        raise RuntimeError("omt web printed no invite link:\n" + "".join(lines))

    # Keep draining, or the pipe fills and the instance blocks on its own
    # println — which looks exactly like a hang in an unrelated capability.
    instance = Instance(port=port, token=token, process=process, cwd=cwd)

    def drain() -> None:
        assert process.stdout is not None
        for line in process.stdout:
            instance.stdout += line

    threading.Thread(target=drain, daemon=True).start()
    instance.stdout = "".join(lines)

    _wait_for_health(port)
    return instance


def _wait_for_health(port: int) -> None:
    import urllib.error
    import urllib.request

    deadline = time.monotonic() + TIMEOUT
    while time.monotonic() < deadline:
        try:
            with urllib.request.urlopen(
                f"http://127.0.0.1:{port}/api/health", timeout=1
            ) as answer:
                if answer.status == 200:
                    return
        except (urllib.error.URLError, OSError):
            time.sleep(0.05)
    raise RuntimeError(f"omt web on {port} never became healthy")


def stop_instance(instance: Instance) -> None:
    """Stop the instance and everything it spawned."""
    try:
        os.killpg(os.getpgid(instance.process.pid), signal.SIGTERM)
    except (ProcessLookupError, PermissionError):
        instance.process.terminate()
    try:
        instance.process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(os.getpgid(instance.process.pid), signal.SIGKILL)
        except (ProcessLookupError, PermissionError):
            instance.process.kill()


@dataclass
class Client:
    """A connected client.

    One socket per test. Sharing one between tests would make an event a test
    subscribed to visible to the next one, and "which test left this here" is
    not a debugging session anybody should have.
    """

    socket: WebSocket
    role: str
    capabilities: list[str]
    catalog_hash: str
    #: This client's device. Every request id carries it, so an instance can
    #: tell one client's numbering from another's.
    device: str = ""
    _next: int = 1
    #: Events that arrived while waiting for a call result. Kept rather than
    #: dropped: a test that subscribes and then calls must still see what the
    #: call produced.
    pending_events: list[dict[str, Any]] = field(default_factory=list)

    def close(self) -> None:
        try:
            self.socket.close()
        except Exception:
            pass

    def _request_id(self) -> dict[str, Any]:
        """A request id: a device, and a number monotonic within it.

        Not a free string. The wire type is a struct, and sending a string
        makes the instance close the connection on a deserialization error —
        which reads as "the server crashed" from here.
        """
        self._next += 1
        return {"device": self.device, "n": self._next}

    def call(
        self,
        capability: str,
        /,
        intent: str | None = None,
        **input_: Any,
    ) -> Any:
        """Invoke a capability and return its output.

        Raises `CapabilityError` on refusal, so a test asserting success does
        not have to check a status field it might forget to check.
        """
        request = self._request_id()
        message: dict[str, Any] = {
            "t": "call",
            "request": request,
            "capability": capability,
            "input": input_,
        }
        # Commands need an intent id and queries must not carry one. Minted
        # here from the request id, which is unique per call.
        if intent is not None:
            message["intent"] = intent
        elif _is_command(capability):
            message["intent"] = new_id()
        self.socket.send(json.dumps(message))
        return self._await_result(request)

    def try_call(self, capability: str, /, **input_: Any) -> tuple[Any, CapabilityError | None]:
        """Like `call`, but hands back a refusal instead of raising."""
        try:
            return self.call(capability, **input_), None
        except CapabilityError as refusal:
            return None, refusal

    def _await_result(self, request: str) -> Any:
        deadline = time.monotonic() + TIMEOUT
        while time.monotonic() < deadline:
            self.socket.settimeout(max(0.1, deadline - time.monotonic()))
            raw = self.socket.recv()
            if isinstance(raw, bytes):
                # A terminal frame. Not what this call is waiting for.
                continue
            message = json.loads(raw)
            kind = message.get("t")
            if kind == "event":
                self.pending_events.append(message)
                continue
            if kind == "result" and message.get("request") == request:
                if message.get("status") == "ok":
                    return message.get("output")
                error = message.get("error", {})
                raise CapabilityError(
                    error.get("code", message.get("status", "unknown")),
                    error.get("message", json.dumps(message)),
                    error.get("detail"),
                )
            if kind == "goodbye":
                raise CapabilityError("goodbye", json.dumps(message))
        raise TimeoutError(f"no result for {request}")

    def subscribe(self, since: dict[str, int] | None = None) -> None:
        """Start receiving events.

        A protocol message, not a capability call: a subscription changes what
        the connection is *sent*, and it is answered by what arrives afterwards
        rather than by a reply. `events.subscribe` mints an id for a client that
        wants to name its subscription; it is not what turns the stream on.
        """
        self.socket.send(
            json.dumps({"t": "subscribe", "since": since or {}})
        )

    def events(self, timeout: float = 5.0) -> Iterator[dict[str, Any]]:
        """Everything that arrives within a window.

        A window rather than "until N events": a test that waits for a fixed
        count hangs forever when the code under test emits one fewer, and a
        hang says far less than a failed assertion.
        """
        yield from self.pending_events
        self.pending_events = []
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                self.socket.settimeout(max(0.05, deadline - time.monotonic()))
                raw = self.socket.recv()
            except Exception:
                return
            if isinstance(raw, bytes) or not raw:
                continue
            message = json.loads(raw)
            if message.get("t") == "event":
                yield message


#: Capability kinds are declared in the catalog; this is the fallback used
#: before it has been read. Commands need an intent id, queries must not have
#: one, and sending the wrong shape is refused rather than ignored.
_COMMAND_KINDS: set[str] = set()


def load_catalog() -> dict[str, Any]:
    """The capability catalog, dumped from the binary under test.

    From the binary rather than from a checked-in copy: the point of the parity
    tests is to compare the documentation against what this build actually
    registers.
    """
    dumped = subprocess.run(
        [str(omt_binary()), "debug", "catalog-dump", "--json"],
        capture_output=True,
        text=True,
        check=True,
    )
    return json.loads(dumped.stdout)


def _is_command(capability: str) -> bool:
    if not _COMMAND_KINDS:
        for declaration in load_catalog()["capabilities"]:
            if declaration["kind"] == "command":
                _COMMAND_KINDS.add(declaration["name"])
    return capability in _COMMAND_KINDS


def device_credential(instance: Instance) -> str:
    """This test run's credential, joining once if it has not already.

    Exactly what a client does: the invite is exchanged for a credential of the
    device's own, and everything afterwards uses that. Reusing the invite would
    fail on the second connection, because an invite is single-exchange.
    """
    if instance.credential is None:
        instance.credential = exchange_invite(instance, instance.token)
    return instance.credential


def exchange_invite(instance: Instance, invite: str, name: str = "the tests") -> str:
    """Swap an invite for a credential over a socket with no credential."""
    socket_ = create_connection(instance.ws_url, timeout=TIMEOUT)
    try:
        socket_.send(
            json.dumps(
                {
                    "t": "join_exchange",
                    "request": {"device": new_id(), "n": 1},
                    "invite": invite,
                    "device": {"name": name, "platform": "tests"},
                }
            )
        )
        answer = json.loads(socket_.recv())
    finally:
        socket_.close()
    if answer.get("t") != "join_credential":
        raise CapabilityError("unauthorized", f"the invite was refused: {answer}")
    return str(answer["token"])


def connect(instance: Instance, token: str | None = None) -> Client:
    """Open a connection and complete the handshake.

    The credential rides the upgrade's `Authorization` header, not the `hello`
    body: it is checked before a socket exists at all, so a bad token is a 401
    rather than a connection that opens and is then told to go away.
    """
    credential = token if token is not None else device_credential(instance)
    try:
        socket_ = create_connection(
            instance.ws_url,
            timeout=TIMEOUT,
            header={"Authorization": f"Bearer {credential}"},
        )
    except WebSocketBadStatusException as refused:
        if refused.status_code == 401:
            raise CapabilityError("unauthorized", "the credential was not accepted") from None
        raise
    socket_.send(
        json.dumps(
            {
                "t": "hello",
                "proto": 1,
                "client": "integration-tests",
                "token": credential,
            }
        )
    )
    raw = socket_.recv()
    message = json.loads(raw)
    if message.get("t") == "goodbye":
        socket_.close()
        raise CapabilityError(
            message.get("reason", "goodbye"), message.get("detail", "")
        )
    if message.get("t") != "welcome":
        socket_.close()
        raise RuntimeError(f"expected a welcome, got {message}")
    return Client(
        socket=socket_,
        device=new_id(),
        role=message["role"],
        capabilities=message["capabilities"],
        catalog_hash=message["catalog_hash"],
    )


def git_repo(path: Path) -> Path:
    """A real git repository with one commit.

    Real rather than faked: the git capabilities shell out, and a directory
    that merely looks like a repository would exercise the error paths.
    """
    path.mkdir(parents=True, exist_ok=True)
    run = lambda *args: subprocess.run(  # noqa: E731
        args, cwd=path, check=True, capture_output=True
    )
    run("git", "init", "-q", "-b", "main")
    run("git", "config", "user.email", "tests@example.invalid")
    run("git", "config", "user.name", "omt integration tests")
    (path / "README.md").write_text("a repository under test\n")
    run("git", "add", "-A")
    run("git", "commit", "-q", "-m", "the first commit")
    return path


def have(program: str) -> bool:
    """Whether an external program exists, for tests that need a real one."""
    return shutil.which(program) is not None
