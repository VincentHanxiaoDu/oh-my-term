#!/usr/bin/env python3
"""Structural checks over the documentation set.

These exist because every one of them caught a real defect during the design
phase, and several caught defects *introduced by fixing other defects*: a
renamed section left eight cross-references dangling, a stray code fence
swallowed a document's headings, and a field rename left a type referenced but
undefined. Manual verification found them; CI keeps them found.

Run: python3 scripts/check-docs.py [--verbose]
Exit: 0 clean, 1 on any failure.
"""

from __future__ import annotations

import re
import sys
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DOCS = ROOT / "docs"

# A link's target file, and optionally its #anchor.
LINK = re.compile(r"\[[^\]]*\]\((?!https?:|mailto:)([^)#\s]*)(?:#([^)\s]+))?\)")
FENCE = re.compile(r"^\s*```")
HEADING = re.compile(r"^(#{1,6})\s+(.*?)\s*$")


def slug(text: str) -> str:
    """GitHub's heading-anchor algorithm.

    Note that GitHub replaces spaces *individually* rather than collapsing
    runs, so a heading like `D9 — Positioning` becomes `d9--positioning`: the
    em-dash is stripped and both surrounding spaces survive as hyphens.
    Collapsing them here would report every such link as broken.
    """
    text = re.sub(r"`", "", text)
    text = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", text)  # links -> their text
    text = text.strip().lower()
    text = re.sub(r"[^\w\s-]", "", text)
    return text.replace(" ", "-")


def anchors_of(path: Path) -> set[str]:
    """Headings outside code fences, as anchors. Deduplicated GitHub-style."""
    found: set[str] = set()
    counts: defaultdict[str, int] = defaultdict(int)
    in_fence = False
    for line in path.read_text(encoding="utf-8").splitlines():
        if FENCE.match(line):
            in_fence = not in_fence
            continue
        if in_fence:
            continue
        m = HEADING.match(line)
        if not m:
            continue
        base = slug(m.group(2))
        n = counts[base]
        counts[base] += 1
        found.add(base if n == 0 else f"{base}-{n}")
    return found


def markdown_files() -> list[Path]:
    return sorted(p for p in DOCS.rglob("*.md") if ".research" not in p.parts)


def check_fences(files: list[Path]) -> list[str]:
    """An odd fence count means a document is silently half code block."""
    problems = []
    for f in files:
        n = sum(1 for line in f.read_text(encoding="utf-8").splitlines() if FENCE.match(line))
        if n % 2:
            problems.append(
                f"{f.relative_to(ROOT)}: {n} code fences — odd, so one is unclosed. "
                f"Everything after it renders as code, and its headings stop "
                f"being link targets."
            )
    return problems


def check_links(files: list[Path]) -> tuple[list[str], list[str]]:
    """Relative links resolve to a real file, and named anchors exist in it."""
    cache: dict[Path, set[str]] = {}
    bad_files, bad_anchors = [], []
    for f in files:
        here = f.parent
        for m in LINK.finditer(f.read_text(encoding="utf-8")):
            target, anchor = m.group(1), m.group(2)
            path = (here / target).resolve() if target else f
            if not path.exists():
                bad_files.append(f"{f.relative_to(ROOT)} -> {target}")
                continue
            if anchor is None or path.suffix != ".md":
                continue
            if path not in cache:
                cache[path] = anchors_of(path)
            if anchor.lower() not in cache[path]:
                bad_anchors.append(
                    f"{f.relative_to(ROOT)} -> {path.name}#{anchor}"
                )
    return bad_files, bad_anchors


def check_decisions_referenced(files: list[Path]) -> list[str]:
    """A decision nothing outside the log references was never propagated.

    D16 sat unreferenced for two review rounds while the web client happily
    offered remote answering for the card types D16 says are unsafe.
    """
    log = DOCS / "architecture" / "decisions.md"
    if not log.exists():
        return []
    declared = re.findall(r"^##\s+(D\d+)\b", log.read_text(encoding="utf-8"), re.M)
    referenced: set[str] = set()
    for f in files:
        if f == log:
            continue
        for d in re.findall(r"\bD(\d+)\b", f.read_text(encoding="utf-8")):
            referenced.add(f"D{d}")
    return [
        f"{d} is declared in decisions.md and referenced by no other document — "
        f"a decision nobody cites has probably not been propagated."
        for d in declared
        if d not in referenced
    ]


def main() -> int:
    verbose = "--verbose" in sys.argv
    files = markdown_files()
    if not files:
        print("no markdown found under docs/", file=sys.stderr)
        return 1

    checks: list[tuple[str, list[str]]] = []
    fences = check_fences(files)
    bad_files, bad_anchors = check_links(files)
    checks.append(("unclosed code fences", fences))
    checks.append(("broken file links", bad_files))
    checks.append(("dangling section anchors", bad_anchors))
    checks.append(("unpropagated decisions", check_decisions_referenced(files)))

    failed = 0
    for name, problems in checks:
        if problems:
            failed += len(problems)
            print(f"\n✗ {name} ({len(problems)}):")
            for p in problems:
                print(f"    {p}")
        elif verbose:
            print(f"✓ {name}")

    print(f"\nchecked {len(files)} documents", end="")
    print(f" — {failed} problem(s)" if failed else " — clean")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
