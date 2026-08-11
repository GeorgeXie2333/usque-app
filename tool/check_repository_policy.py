"""Validate community files, Markdown links, workflow pins, and PR titles."""

from __future__ import annotations

import argparse
import os
import re
import sys
from pathlib import Path
from urllib.parse import unquote

REQUIRED_FILES = (
    "README.md",
    "README.zh-CN.md",
    "LICENSE.md",
    "CODE_OF_CONDUCT.md",
    "CONTRIBUTING.md",
    "SECURITY.md",
    ".github/CODEOWNERS",
    ".github/PULL_REQUEST_TEMPLATE.md",
    ".github/ISSUE_TEMPLATE/bug.yml",
    ".github/ISSUE_TEMPLATE/feature.yml",
    ".github/ISSUE_TEMPLATE/config.yml",
)

MARKDOWN_EXCLUDES = {
    ".git",
    "target",
    "dist",
    "build",
    ".dart_tool",
    ".gradle",
    ".wix",
    "oracle",
    "third_party",
}
PLACEHOLDERS = (
    "[INSERT CONTACT METHOD]",
    "TODO: add security contact",
    "TBD: security",
)
TITLE_PATTERN = re.compile(
    r"^(?:feat|fix|perf|refactor|docs|test|build|ci|chore|deps|revert)"
    r"(?:\([a-z0-9][a-z0-9._/-]*\))?!?: .+\S$"
)
MARKDOWN_LINK_PATTERN = re.compile(r"!?\[[^\]]*\]\(([^)]+)\)")
HTML_LINK_PATTERN = re.compile(r"(?:href|src)=[\"']([^\"']+)[\"']", re.IGNORECASE)
ACTION_PATTERN = re.compile(r"^\s*(?:-\s*)?uses:\s*[\"']?([^\s\"'#]+)@([^\s\"'#]+)", re.MULTILINE)
FULL_SHA_PATTERN = re.compile(r"^[0-9a-f]{40}$")


def validate_pr_title(title: str) -> list[str]:
    """Return policy violations for a Pull Request title."""

    normalized = title.strip()
    errors: list[str] = []
    if normalized.lower().startswith(("wip", "draft")):
        errors.append("ready Pull Request titles must not start with WIP or Draft")
    if not TITLE_PATTERN.fullmatch(normalized):
        errors.append(
            "PR title must use Conventional Commits, for example "
            "'fix(android): reconnect proxy after network change'"
        )
    return errors


def _markdown_files(root: Path) -> list[Path]:
    return sorted(
        path
        for path in root.rglob("*.md")
        if not any(part in MARKDOWN_EXCLUDES for part in path.relative_to(root).parts)
    )


def _repository_files(root: Path) -> set[str]:
    return {
        path.relative_to(root).as_posix()
        for path in root.rglob("*")
        if path.is_file() and ".git" not in path.relative_to(root).parts
    }


def _relative_target(source: Path, raw_target: str, root: Path) -> str | None:
    target = raw_target.strip().strip("<>")
    if not target or target.startswith(("#", "http://", "https://", "mailto:", "data:")):
        return None
    target = unquote(target.split("#", maxsplit=1)[0].split("?", maxsplit=1)[0])
    if not target:
        return None
    candidate = (source.parent / target).resolve()
    try:
        return candidate.relative_to(root.resolve()).as_posix()
    except ValueError:
        return f"OUTSIDE:{candidate}"


def validate_markdown(root: Path) -> list[str]:
    """Validate UTF-8, placeholders, and repository-local Markdown links."""

    errors: list[str] = []
    repository_files = _repository_files(root)
    for path in _markdown_files(root):
        relative = path.relative_to(root).as_posix()
        try:
            raw = path.read_bytes()
            text = raw.decode("utf-8")
        except UnicodeDecodeError as error:
            errors.append(f"{relative}: not valid UTF-8: {error}")
            continue
        if text.startswith("\ufeff"):
            errors.append(f"{relative}: UTF-8 BOM is not allowed")
        for placeholder in PLACEHOLDERS:
            if placeholder.lower() in text.lower():
                errors.append(f"{relative}: unresolved placeholder: {placeholder}")
        targets = MARKDOWN_LINK_PATTERN.findall(text) + HTML_LINK_PATTERN.findall(text)
        for target in targets:
            repository_target = _relative_target(path, target, root)
            if repository_target is None:
                continue
            if repository_target.startswith("OUTSIDE:"):
                errors.append(f"{relative}: link leaves repository: {target}")
            elif repository_target not in repository_files:
                errors.append(f"{relative}: missing or case-mismatched link target: {target}")
    return errors


def validate_action_pins(root: Path) -> list[str]:
    """Require external GitHub Actions to use full commit SHAs."""

    errors: list[str] = []
    workflows = root / ".github" / "workflows"
    if not workflows.is_dir():
        return [".github/workflows: directory is missing"]
    for path in sorted((*workflows.glob("*.yml"), *workflows.glob("*.yaml"))):
        text = path.read_text(encoding="utf-8")
        for action, reference in ACTION_PATTERN.findall(text):
            if action.startswith(("./", "docker://")):
                continue
            if not FULL_SHA_PATTERN.fullmatch(reference):
                relative = path.relative_to(root).as_posix()
                errors.append(f"{relative}: {action}@{reference} is not pinned to a full SHA")
    return errors


def validate_repository(root: Path) -> list[str]:
    """Run all repository-level policy checks."""

    errors = [
        name + ": required file is missing"
        for name in REQUIRED_FILES
        if not (root / name).is_file()
    ]
    errors.extend(validate_markdown(root))
    errors.extend(validate_action_pins(root))
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parent.parent)
    parser.add_argument("--pr-title", default=os.environ.get("PR_TITLE"))
    arguments = parser.parse_args()

    errors = validate_repository(arguments.root)
    if arguments.pr_title is not None:
        errors.extend(validate_pr_title(arguments.pr_title))
    if errors:
        for error in errors:
            print(f"POLICY_ERROR: {error}", file=sys.stderr)
        return 1
    print("REPOSITORY_POLICY_OK")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
