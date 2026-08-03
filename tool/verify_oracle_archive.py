#!/usr/bin/env python3
"""Verify that the archived Go oracle still matches the fork point.

Text files are compared after CRLF normalization so checking out the repository
with a different Git line-ending policy cannot create a false failure. Binary
fixtures are compared byte-for-byte.
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ORACLE = ROOT / "oracle" / "go"
SOURCE_COMMIT = (ORACLE / "UPSTREAM_COMMIT").read_text(encoding="ascii").strip()
SOURCE_PATHS = (
    "_docs",
    "api",
    "cmd",
    "config",
    "internal",
    "models",
    "Dockerfile",
    "RESEARCH.md",
    "dns_android.go",
    "go.mod",
    "go.sum",
    "goreleaser.yml",
    "main.go",
)
BINARY_SUFFIXES = {".png", ".jpg", ".jpeg", ".gif", ".webp", ".ico"}


def git_bytes(path: str) -> bytes:
    # Trusted fixed argv: only `git show` against the pinned oracle commit.
    return subprocess.run(  # noqa: S603  # fixed git argv, no shell
        ["git", "show", f"{SOURCE_COMMIT}:{path}"],  # noqa: S607  # PATH-resolved git binary
        cwd=ROOT,
        check=True,
        stdout=subprocess.PIPE,
    ).stdout


def source_files() -> list[str]:
    # Trusted fixed argv: only `git ls-tree` against the pinned oracle commit.
    return [
        line
        for line in subprocess.run(  # noqa: S603  # fixed git argv, no shell
            [  # noqa: S607  # PATH-resolved git binary
                "git",
                "ls-tree",
                "-r",
                "--name-only",
                SOURCE_COMMIT,
                "--",
                *SOURCE_PATHS,
            ],
            cwd=ROOT,
            check=True,
            stdout=subprocess.PIPE,
            text=True,
            encoding="utf-8",
        ).stdout.splitlines()
        if line
    ]


def normalized(path: str, payload: bytes) -> bytes:
    if Path(path).suffix.lower() in BINARY_SUFFIXES:
        return payload
    return payload.replace(b"\r\n", b"\n")


def main() -> int:
    failures: list[str] = []
    expected = source_files()
    expected_set = set(expected)

    for relative in expected:
        archived = ORACLE / Path(relative)
        if not archived.is_file():
            failures.append(f"missing oracle file: {relative}")
            continue
        upstream = normalized(relative, git_bytes(relative))
        current = normalized(relative, archived.read_bytes())
        if upstream != current:
            failures.append(f"oracle drift: {relative}")

    for candidate in ORACLE.rglob("*"):
        if not candidate.is_file():
            continue
        relative = candidate.relative_to(ORACLE).as_posix()
        if relative in {"README.md", "UPSTREAM_COMMIT"}:
            continue
        if (
            relative.startswith(("_docs/", "api/", "cmd/", "config/", "internal/", "models/"))
            or relative in SOURCE_PATHS
        ) and relative not in expected_set:
            failures.append(f"unexpected oracle file: {relative}")

    if failures:
        print("\n".join(failures), file=sys.stderr)
        return 1
    print(f"Verified {len(expected)} archived files against {SOURCE_COMMIT}.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
