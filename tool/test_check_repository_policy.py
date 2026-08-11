from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from check_repository_policy import (
    validate_action_pins,
    validate_markdown,
    validate_pr_title,
)


class PullRequestTitleTests(unittest.TestCase):
    def test_accepts_conventional_title(self) -> None:
        self.assertEqual([], validate_pr_title("fix(android): reconnect HTTP proxy"))

    def test_rejects_unstructured_and_draft_titles(self) -> None:
        self.assertTrue(validate_pr_title("Reconnect the proxy"))
        self.assertTrue(validate_pr_title("WIP: fix(android): reconnect proxy"))


class RepositoryPolicyTests(unittest.TestCase):
    def test_detects_broken_relative_markdown_link(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "README.md").write_text("[missing](docs/missing.md)\n", encoding="utf-8")
            errors = validate_markdown(root)
            self.assertEqual(1, len(errors))
            self.assertIn("missing or case-mismatched", errors[0])

    def test_requires_full_action_sha(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            workflows = root / ".github" / "workflows"
            workflows.mkdir(parents=True)
            (workflows / "test.yml").write_text(
                "jobs:\n  test:\n    steps:\n      - uses: actions/checkout@v6\n",
                encoding="utf-8",
            )
            errors = validate_action_pins(root)
            self.assertEqual(1, len(errors))
            self.assertIn("not pinned to a full SHA", errors[0])

    def test_accepts_full_action_sha(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            workflows = root / ".github" / "workflows"
            workflows.mkdir(parents=True)
            (workflows / "test.yml").write_text(
                "jobs:\n  test:\n    steps:\n"
                "      - uses: actions/checkout@de0fac2e4500dabe0009e67214ff5f5447ce83dd\n",
                encoding="utf-8",
            )
            self.assertEqual([], validate_action_pins(root))


if __name__ == "__main__":
    unittest.main()
