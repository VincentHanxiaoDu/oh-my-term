"""
Fixtures for the integration suite.

An instance per test module rather than per test: starting `omt web` costs
about a second, and a suite that takes five minutes is a suite nobody runs
before pushing. Tests that need isolation from other tests' sessions create
their own workspace instead, which is cheap.
"""

from __future__ import annotations

import shutil
import tempfile
from pathlib import Path
from typing import Iterator

import pytest

import omt_client
from omt_client import Client, Instance


@pytest.fixture(scope="session")
def catalog() -> dict:
    """What this build actually registers."""
    return omt_client.load_catalog()


@pytest.fixture(scope="module")
def workspace() -> Iterator[Path]:
    """A directory the instance is started in, and a git repository."""
    path = Path(tempfile.mkdtemp(prefix="omt-integration-"))
    omt_client.git_repo(path)
    yield path
    shutil.rmtree(path, ignore_errors=True)


@pytest.fixture(scope="module")
def instance(workspace: Path) -> Iterator[Instance]:
    started = omt_client.start_instance(workspace)
    yield started
    omt_client.stop_instance(started)


@pytest.fixture
def client(instance: Instance) -> Iterator[Client]:
    connection = omt_client.connect(instance)
    yield connection
    connection.close()


@pytest.fixture
def session(client: Client) -> str:
    """A shell session, ready to be written to.

    Acquired as well as created: every write is checked against the writer
    epoch, so a session created and not acquired is one nothing can type into.
    """
    workspaces = client.call("workspace.list")["workspaces"]
    created = client.call("session.create", workspace=workspaces[0]["id"])
    return created["session"]
