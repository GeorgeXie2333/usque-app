"""Offline GitHub Actions evidence for dev-to-main promotion."""

import os
import re
import subprocess
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path
from unittest.mock import patch

import promotion_check
from release_contract import ContractError


class PromotionTests(unittest.TestCase):
    repository = "owner/repository"
    sha = "a" * 40
    attempt = "actions/runs/42/attempts/2"

    def responses(self, workflow):
        base = f"repos/{self.repository}"
        run = {
            "id": 42,
            "run_attempt": 2,
            "repository": {"full_name": self.repository},
            "head_repository": {"full_name": self.repository},
            "head_sha": self.sha,
            "head_branch": "dev",
            "event": "push",
            "path": f".github/workflows/{workflow}",
            "status": "completed",
            "conclusion": "success",
        }
        jobs = [
            {
                "name": name,
                "head_sha": self.sha,
                "run_id": 42,
                "status": "completed",
                "conclusion": "success",
            }
            for name in sorted(promotion_check.REQUIRED_JOBS[workflow])
        ]
        data = {
            f"{base}/git/ref/heads/{branch}": {
                "ref": f"refs/heads/{branch}",
                "object": {"sha": self.sha, "type": "commit"},
            }
            for branch in ("dev", "main")
        }
        data[
            f"{base}/actions/workflows/{workflow}/runs?branch=dev&event=push&head_sha={self.sha}&per_page=1"
        ] = {"workflow_runs": [run]}
        data[f"{base}/{self.attempt}/jobs?per_page=100"] = {"total_count": len(jobs), "jobs": jobs}
        return data, run, jobs

    def lookup(self, workflow, data):
        with patch.object(promotion_check, "gh_json", side_effect=data.__getitem__):
            return promotion_check.dev_run(self.repository, self.sha, workflow)

    def test_ci_and_codeql_have_independent_source_attempts(self):
        for workflow in ("ci.yml", "codeql.yml"):
            with self.subTest(workflow=workflow):
                data, _, _ = self.responses(workflow)
                self.assertEqual(self.lookup(workflow, data), self.attempt)

    def test_both_tips_must_still_match(self):
        for branch in ("dev", "main"):
            data, _, _ = self.responses("ci.yml")
            data[f"repos/{self.repository}/git/ref/heads/{branch}"]["object"]["sha"] = "b" * 40
            with self.subTest(branch=branch), self.assertRaises(ContractError):
                self.lookup("ci.yml", data)

    def test_rejects_other_sources_and_incomplete_runs(self):
        for key, value in (
            ("repository", {"full_name": "fork/repo"}),
            ("head_repository", {"full_name": "fork/repo"}),
            ("head_branch", "main"),
            ("head_sha", "b" * 40),
            ("path", ".github/workflows/codeql.yml"),
            ("event", "pull_request"),
            ("event", "workflow_dispatch"),
            ("status", "queued"),
            ("conclusion", "failure"),
            ("conclusion", "cancelled"),
            ("id", True),
            ("run_attempt", None),
        ):
            data, run, _ = self.responses("ci.yml")
            run[key] = value
            with self.subTest(field=key, value=value), self.assertRaises(ContractError):
                self.lookup("ci.yml", data)

    def test_green_run_requires_successful_candidate_jobs(self):
        for workflow in ("ci.yml", "codeql.yml"):
            for change in (
                {"name": "unrelated job"},
                {"conclusion": "skipped"},
                {"conclusion": "failure"},
                {"status": "in_progress"},
                {"head_sha": "b" * 40},
                {"run_id": 41},
            ):
                data, _, jobs = self.responses(workflow)
                jobs[-1].update(change)
                with (
                    self.subTest(workflow=workflow, change=change),
                    self.assertRaises(ContractError),
                ):
                    self.lookup(workflow, data)

    def test_output_is_published_only_with_a_verified_attempt(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "outputs"
            summary = Path(directory) / "summary"
            env = {
                "GITHUB_REPOSITORY": self.repository,
                "GITHUB_SHA": self.sha,
                "GITHUB_SERVER_URL": "https://github.com",
                "GITHUB_OUTPUT": str(output),
                "GITHUB_STEP_SUMMARY": str(summary),
            }
            with patch.dict(os.environ, env), patch.object(sys, "argv", ["check", "ci.yml"]):
                with patch.object(promotion_check, "gh_json", side_effect=ContractError("offline")):
                    promotion_check.main()
                self.assertFalse(output.exists())
                data, _, _ = self.responses("ci.yml")
                with patch.object(promotion_check, "gh_json", side_effect=data.__getitem__):
                    promotion_check.main()
                self.assertEqual(output.read_text().splitlines(), ["reused=true"])
                self.assertIn(
                    f"https://github.com/{self.repository}/{self.attempt}", summary.read_text()
                )

    def test_only_main_push_probes_and_every_codeql_language_is_required(self):
        workflows = Path(__file__).resolve().parents[1] / ".github/workflows"
        for filename in promotion_check.REQUIRED_JOBS:
            workflow = (workflows / filename).read_text(encoding="utf-8")
            self.assertIn(
                "    if: github.event_name == 'push' && github.ref == 'refs/heads/main'", workflow
            )
            self.assertIn(f"      workflow: {filename}", workflow)
        matrix = (
            (workflows / "codeql.yml")
            .read_text()
            .split("        language:\n", 1)[1]
            .split("    steps:", 1)[0]
        )
        languages = re.findall(r"^          - ([\w-]+)$", matrix, re.MULTILINE)
        self.assertEqual(
            promotion_check.REQUIRED_JOBS["codeql.yml"], {f"Analyze {lang}" for lang in languages}
        )

    @unittest.skipUnless(sys.platform == "linux", "CI gate runs on Ubuntu")
    def test_ci_gate_requires_full_success_or_verified_reuse(self):
        ci = Path(__file__).resolve().parents[1] / ".github/workflows/ci.yml"
        gate = ci.read_text().split("\n  gate:\n", 1)[1]
        script = textwrap.dedent(gate.split("        run: |\n", 1)[1])
        jobs = re.findall(r"^          (\w+): \$\{\{ needs\.[\w-]+\.result", gate, re.MULTILINE)
        for ref, promotion, reused, result, rust, accepted in (
            ("main", "success", "true", "skipped", "skipped", True),
            ("main", "success", "", "success", "success", True),
            ("dev", "skipped", "", "success", "success", True),
            ("main", "failure", "true", "skipped", "skipped", False),
            ("main", "success", "", "success", "skipped", False),
            ("main", "success", "true", "skipped", "failure", False),
            ("dev", "skipped", "true", "skipped", "skipped", False),
        ):
            env = {
                **dict.fromkeys(jobs, result),
                "GITHUB_EVENT_NAME": "push",
                "GITHUB_REF": f"refs/heads/{ref}",
                "PROMOTION": promotion,
                "REUSED": reused,
                "RUST": rust,
            }
            with self.subTest(ref=ref, promotion=promotion, reused=reused, rust=rust):
                status = subprocess.run(  # noqa: S603 -- reviewed CI gate with inert job results.
                    ["/bin/bash", "-c", script], env=env, timeout=5, check=False
                )
                self.assertEqual(status.returncode == 0, accepted)
