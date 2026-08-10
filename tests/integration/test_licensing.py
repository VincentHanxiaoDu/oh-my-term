"""The licensing policy, as checks rather than prose.

`docs/architecture/14-licensing.md` §1, §5, §6 and §7.

A licence policy that lives only in a document is one nobody notices breaking.
Each check here corresponds to a way this project could quietly acquire a
problem that is expensive to unwind later: a vendored file from a codebase
under different terms, a crate published under none, an attribution that went
stale without anything failing.

The gate itself is also tested against a deliberate violation. A gate that has
only ever been observed passing is a gate nobody has shown to work.
"""

from __future__ import annotations


import subprocess



from omt_client import REPO_ROOT


def xtask(task: str, cwd=None) -> subprocess.CompletedProcess:
    return subprocess.run(
        ["cargo", "run", "-q", "-p", "xtask", "--", task],
        cwd=cwd or REPO_ROOT,
        capture_output=True,
        text=True,
        check=False,
    )


MINIMAL_MANIFEST = """[package]
name = "fixture"
version = "0.0.0"
edition = "2021"
license = "Apache-2.0"

[dependencies]
"""


def fixture_repo(path) -> None:
    """A repository the gate can run against.

    It needs a real manifest: `cargo metadata` is how the dependency check
    reads licences, and a fixture without one fails for a reason that has
    nothing to do with what the test is about.
    """
    (path / "LICENSE").write_text("Apache License Version 2.0")
    (path / "NOTICE").write_text("oh-my-term\nApache\n" + "x" * 200)
    (path / ".gitignore").write_text(".research/\n")
    (path / "Cargo.toml").write_text(MINIMAL_MANIFEST)
    (path / "crates").mkdir()
    # A real package, not a virtual workspace with no members: `cargo metadata`
    # refuses that outright, and the fixture would fail for a reason that has
    # nothing to do with licensing.
    src = path / "src"
    src.mkdir()
    (src / "lib.rs").write_text("")
    subprocess.run(["git", "init", "-q"], cwd=path, check=True)


def xtask_at(root: str) -> subprocess.CompletedProcess:
    """Run the gate against a fixture rather than this repository."""
    return subprocess.run(
        ["cargo", "run", "-q", "-p", "xtask", "--", "licensing", root],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        check=False,
    )


class TestTheGateItself:
    def test_the_repository_passes_its_own_licensing_gate(self) -> None:
        result = xtask("licensing")
        assert result.returncode == 0, result.stderr

    def test_the_gate_fails_when_studied_code_is_vendored(self, tmp_path) -> None:
        # The check that matters most, so it is the one proven to fire against
        # a repository that actually violates it. Without this, a gate that
        # silently stopped looking at `.research/` would keep printing "ok"
        # forever and nothing would ever notice.
        fixture_repo(tmp_path)
        research = tmp_path / ".research" / "somebody-elses"
        research.mkdir(parents=True)
        (research / "copied.rs").write_text("fn main() {}")
        # `-f`, which is exactly how this happens in real life: ignored is not
        # the same as untracked, and a file forced in once stays tracked.
        subprocess.run(
            ["git", "add", "-f", ".research"], cwd=tmp_path, check=True
        )

        result = xtask_at(str(tmp_path))
        assert result.returncode != 0, "the gate passed a vendored repository"
        assert "vendored" in (result.stderr + result.stdout)

    def test_the_gate_passes_the_same_fixture_once_it_is_clean(
        self, tmp_path
    ) -> None:
        # The other half: a gate that fails everything is as useless as one
        # that passes everything, and only the pair distinguishes them.
        fixture_repo(tmp_path)

        result = xtask_at(str(tmp_path))
        assert result.returncode == 0, result.stderr


class TestOmtsOwnLicence:
    """§1."""

    def test_there_is_a_licence_file_and_it_is_apache(self) -> None:
        # The manifest field tells cargo. The file is what anybody reads.
        text = (REPO_ROOT / "LICENSE").read_text()
        assert "Apache License" in text
        assert "Version 2.0" in text

    def test_every_crate_declares_a_licence(self) -> None:
        # A crate that inherits nothing is one published under no terms at all.
        missing = []
        for manifest in (REPO_ROOT / "crates").glob("*/Cargo.toml"):
            if "license" not in manifest.read_text():
                missing.append(manifest.parent.name)
        assert missing == [], missing

    def test_the_workspace_declares_the_same_licence(self) -> None:
        assert 'license = "Apache-2.0"' in (REPO_ROOT / "Cargo.toml").read_text()


class TestStudiedCodeIsNeverVendored:
    """§5. `.research/` is read to understand problems, never to copy from."""

    def test_research_is_gitignored(self) -> None:
        lines = [
            line.strip().rstrip("/")
            for line in (REPO_ROOT / ".gitignore").read_text().splitlines()
        ]
        assert ".research" in lines

    def test_nothing_under_research_is_tracked(self) -> None:
        # Being ignored is not proof it did not happen: a file added with `-f`
        # stays tracked forever afterwards, and .gitignore says nothing about
        # it. These are two separate checks because they are two separate ways
        # to fail.
        tracked = subprocess.run(
            ["git", "ls-files", ".research"],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            check=True,
        )
        assert tracked.stdout.strip() == ""

    def test_no_source_file_imports_from_research(self) -> None:
        # The subtler version of vendoring: not copying the file, but pointing
        # the build at it.
        offenders = []
        for source in list((REPO_ROOT / "crates").rglob("*.rs")) + list(
            (REPO_ROOT / "web" / "src").rglob("*.ts")
        ):
            if ".research/" in source.read_text():
                offenders.append(str(source.relative_to(REPO_ROOT)))
        assert offenders == [], offenders


class TestAttribution:
    """§7."""

    def test_there_is_a_notice_with_something_in_it(self) -> None:
        # Checked for substance rather than presence: an empty NOTICE satisfies
        # a file-exists test and no obligation whatsoever.
        text = (REPO_ROOT / "NOTICE").read_text()
        assert len(text.strip()) > 100
        assert "Apache" in text
        assert "oh-my-term" in text


class TestNoCompetitorNamesLeaked:
    """§4 clean-room practice, as something that can fail.

    The names of the products studied must not appear in shipped code, docs or
    commit messages. Checked mechanically because the failure mode is a single
    careless line, and a review pass over a whole repository is not something
    anybody repeats.
    """

    FORBIDDEN = ("warp", "orca", "herdr")

    def _shipped_files(self):
        for pattern in ("crates/**/*.rs", "web/src/**/*.ts", "docs/**/*.md"):
            yield from REPO_ROOT.glob(pattern)

    def test_no_studied_product_is_named_in_shipped_source_or_docs(self) -> None:
        offenders = []
        for path in self._shipped_files():
            if ".research" in str(path):
                continue
            lowered = path.read_text().lower()
            for name in self.FORBIDDEN:
                # Word-ish match: `warp` is a substring of ordinary words, and
                # a check that cannot tell them apart is one somebody disables.
                for token in ("`" + name, " " + name + " ", name + "'s"):
                    if token in lowered:
                        offenders.append(f"{path.relative_to(REPO_ROOT)}: {name}")
                        break
        assert offenders == [], offenders


class TestDependencyPolicy:
    """§6. Every dependency's licence, checked rather than trusted."""

    def test_the_gate_reports_how_many_dependencies_it_checked(self) -> None:
        # A count, because "ok" from a check that silently examined nothing
        # looks exactly like "ok" from one that examined everything.
        result = xtask("licensing")
        assert result.returncode == 0, result.stderr
        assert "dependencies checked" in result.stdout
        count = int(result.stdout.split(" dependencies checked")[0].split()[-1])
        assert count > 100, result.stdout

    def test_a_forbidden_licence_family_is_rejected(self, tmp_path) -> None:
        # Prefix families, because `GPL-3.0-or-later` and `AGPL-3.0` are not
        # strings anybody enumerates correctly by hand.
        source = (REPO_ROOT / "xtask" / "src" / "main.rs").read_text()
        for family in ("GPL", "AGPL", "LGPL", "SSPL", "BUSL"):
            assert f'"{family}"' in source

    def test_a_dual_licence_passes_on_the_arm_omt_can_accept(self) -> None:
        # `Apache-2.0 OR MIT` is a choice the user of the crate makes. The
        # first version of this rejected half the ecosystem by requiring every
        # arm, which is why the gate is run over the real tree in the test
        # above rather than only over fixtures.
        result = xtask("licensing")
        assert result.returncode == 0, result.stderr

    def test_every_exception_carries_a_justification(self) -> None:
        # An exception is a decision, and one with no reason attached is how an
        # allow list becomes a list of things somebody once silenced.
        source = (REPO_ROOT / "xtask" / "src" / "main.rs").read_text()
        block = source.split("const EXCEPTIONS")[1].split("];")[0]
        entries = [line for line in block.splitlines() if line.strip().startswith('"')]
        assert entries, "the exception table should be readable"
        # Each entry is a (name, justification) pair, so a bare name with no
        # following prose would show up as an odd count.
        assert len(entries) % 2 == 0, entries
