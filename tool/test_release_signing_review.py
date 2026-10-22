"""Offline review batching fixtures; never approve a live deployment or read signing keys."""

from __future__ import annotations

import copy
import io
import json
import subprocess
import unittest
from contextlib import redirect_stderr, redirect_stdout
from dataclasses import replace
from pathlib import Path
from unittest.mock import patch

import release_signing_review as review
from release_contract import ContractError


class SigningReviewTests(unittest.TestCase):
    def setUp(self):
        self.candidate = review.Candidate(
            "owner/usque-app", 42, 1, "v0.3.3-beta.3", "a" * 40, "b" * 40
        )
        self.run = {
            "id": 42,
            "run_attempt": 1,
            "head_sha": "a" * 40,
            "head_branch": self.candidate.tag,
            "event": "push",
            "path": ".github/workflows/release.yml",
            "repository": {"full_name": self.candidate.repository},
            "status": "waiting",
        }
        self.tag_ref = {
            "ref": "refs/tags/" + self.candidate.tag,
            "object": {"type": "tag", "sha": "b" * 40},
        }
        self.tag = {
            "sha": "b" * 40,
            "tag": self.candidate.tag,
            "object": {"type": "commit", "sha": "a" * 40},
            "message": "Usque " + self.candidate.tag + "\n",
        }
        self.branch = {
            "ref": "refs/heads/release/" + self.candidate.tag,
            "object": {"type": "commit", "sha": "a" * 40},
        }
        self.jobs = [
            {
                "id": index + 1,
                "run_id": 42,
                "run_attempt": 1,
                "head_sha": "a" * 40,
                "head_branch": self.candidate.tag,
                "name": name,
                "status": "waiting",
                "conclusion": None,
            }
            for index, name in enumerate(review.SIGNING_JOBS)
        ] + [
            {
                "id": 6,
                "run_id": 42,
                "run_attempt": 1,
                "head_sha": "a" * 40,
                "head_branch": self.candidate.tag,
                "name": "Validate protected release inputs",
                "status": "completed",
                "conclusion": "success",
            }
        ]
        self.pending = [
            {"environment": {"name": "release-signing", "id": 19}, "current_user_can_approve": True}
        ]

    def verify(self, **changes):
        values = {"run": self.run, "tag_ref": self.tag_ref, "tag": self.tag, "branch": self.branch}
        review.verify_identity(self.candidate, **{**values, **changes})

    def ready(self):
        return review.signing_batch(self.candidate, self.jobs, self.pending)

    def test_exact_annotated_beta_and_stable_candidates(self):
        self.verify()
        candidate = replace(self.candidate, tag="v0.3.3")
        self.run["head_branch"] = candidate.tag
        self.tag_ref["ref"] = "refs/tags/" + candidate.tag
        self.tag.update(tag=candidate.tag, message="Usque " + candidate.tag)
        self.branch["ref"] = "refs/heads/main"
        review.verify_identity(candidate, self.run, self.tag_ref, self.tag, self.branch)

    def test_invalid_candidate_input(self):
        for changes in (
            {"run_id": 0},
            {"attempt": 0},
            {"run_id": True},
            {"commit": "a" * 7},
            {"tag_object": "b" * 7},
            {"repository": "../.."},
            {"tag": "v0.3.3-beta.0"},
            {"tag": "v0.3.3-beta.01"},
            {"tag": "v00.3.3"},
            {"tag": "v0.3.3;exit"},
        ):
            with self.subTest(changes=changes), self.assertRaises(ContractError):
                replace(self.candidate, **changes)

    def test_wrong_release_run_is_rejected(self):
        for key, value in {
            "id": 43,
            "run_attempt": 2,
            "head_sha": "c" * 40,
            "head_branch": "dev",
            "event": "workflow_dispatch",
            "path": ".github/workflows/build.yml",
            "status": "completed",
            "repository": {"full_name": "fork/usque-app"},
        }.items():
            with self.subTest(key=key), self.assertRaises(ContractError):
                self.verify(run={**self.run, key: value})

    def test_changed_tag_or_source_branch_is_rejected(self):
        cases = [
            {"tag_ref": {**self.tag_ref, "object": {"type": "commit", "sha": "a" * 40}}},
            {"tag_ref": {**self.tag_ref, "object": {"type": "tag", "sha": "c" * 40}}},
            {"tag": {**self.tag, "object": {"type": "commit", "sha": "c" * 40}}},
            {"tag": {**self.tag, "message": "different tag"}},
            {"branch": {**self.branch, "object": {"type": "commit", "sha": "c" * 40}}},
            {"branch": {**self.branch, "ref": "refs/heads/main"}},
        ]
        for changes in cases:
            with self.subTest(changes=changes), self.assertRaises(ContractError):
                self.verify(**changes)

    def test_requires_all_five_waiting_jobs_and_successful_gate(self):
        self.assertEqual(len(self.ready()["jobs"]), 5)
        for index in range(5):
            for state in ("queued", "pending", "requested"):
                jobs = copy.deepcopy(self.jobs)
                jobs[index]["status"] = state
                with self.subTest(index=index, state=state), self.assertRaises(review.NotReady):
                    review.signing_batch(self.candidate, jobs, self.pending)
            with self.assertRaises(review.NotReady):
                review.signing_batch(
                    self.candidate, self.jobs[:index] + self.jobs[index + 1 :], self.pending
                )
        for state in ("failure", "cancelled", "skipped"):
            jobs = copy.deepcopy(self.jobs)
            jobs[-1]["conclusion"] = state
            with self.subTest(gate=state), self.assertRaises(ContractError):
                review.signing_batch(self.candidate, jobs, self.pending)

    def test_started_finished_wrong_candidate_and_duplicate_jobs_fail_closed(self):
        for changes in (
            {"status": "in_progress"},
            {"status": "completed", "conclusion": "success"},
            {"status": "completed", "conclusion": "failure"},
            {"run_id": 99},
            {"run_attempt": 2},
            {"head_sha": "c" * 40},
            {"head_branch": "dev"},
            {"conclusion": "failure"},
        ):
            jobs = copy.deepcopy(self.jobs)
            jobs[0].update(changes)
            with self.subTest(changes=changes), self.assertRaises(ContractError):
                review.signing_batch(self.candidate, jobs, self.pending)
        for extra in (self.jobs[0], {**self.jobs[0], "id": 99}):
            with self.assertRaises(ContractError):
                review.signing_batch(self.candidate, self.jobs + [extra], self.pending)

    def test_no_signing_environment_or_reviewer_permission_means_no_approval(self):
        for pending in ([], [{"environment": {"name": "release-publish", "id": 20}}]):
            with self.assertRaises(review.NotReady):
                review.signing_batch(self.candidate, self.jobs, pending)
        for changes in (
            {"current_user_can_approve": False},
            {"environment": {"name": "release-signing", "id": 0}},
        ):
            with self.assertRaises(ContractError):
                review.signing_batch(self.candidate, self.jobs, [{**self.pending[0], **changes}])

    def test_readiness_is_read_only_and_batch_approval_is_one_post(self):
        ready = self.ready()
        with (
            patch.object(review, "snapshot", return_value=ready),
            patch.object(review, "gh_json") as api,
        ):
            self.assertEqual(review.review(self.candidate), ready)
            api.assert_not_called()
        deployments = [
            {
                "id": index + 1,
                "environment": "release-signing",
                "sha": self.candidate.commit,
                "ref": self.candidate.tag,
            }
            for index in range(5)
        ]
        with (
            patch.object(review, "snapshot", return_value=ready) as snapshot,
            patch.object(review, "gh_json", return_value=deployments) as api,
        ):
            self.assertEqual(review.review(self.candidate, approve=True)["state"], "approved")
            self.assertEqual(snapshot.call_count, 2)
            api.assert_called_once()
            self.assertEqual(api.call_args.args[1]["environment_ids"], [19])
            self.assertEqual(api.call_args.args[1]["state"], "approved")

    def test_publication_environment_is_never_included_in_batch(self):
        self.pending.append(
            {"environment": {"name": "release-publish", "id": 20}, "current_user_can_approve": True}
        )
        self.assertEqual(self.ready()["environment_id"], 19)

    def test_explicit_approval_requires_recorded_tag_object(self):
        with patch.object(review, "snapshot") as snapshot, self.assertRaises(ContractError):
            review.review(replace(self.candidate, tag_object=None), approve=True)
        snapshot.assert_not_called()

    def test_changed_batch_or_ambiguous_post_is_never_retried(self):
        ready = self.ready()
        changed = {**ready, "environment_id": 29}
        with (
            patch.object(review, "snapshot", side_effect=[ready, changed]),
            patch.object(review, "gh_json") as api,
            self.assertRaises(ContractError),
        ):
            review.review(self.candidate, approve=True)
        api.assert_not_called()
        for response in (
            [],
            [{"environment": "release-publish"}] * 5,
            [{"environment": "release-signing"}],
        ):
            with (
                patch.object(review, "snapshot", return_value=ready),
                patch.object(review, "gh_json", return_value=response) as api,
                self.assertRaises(ContractError),
            ):
                review.review(self.candidate, approve=True)
            api.assert_called_once()

    def test_bounded_wait_does_not_approve_partial_jobs(self):
        with (
            patch.object(
                review, "snapshot", side_effect=[review.NotReady("two jobs missing"), self.ready()]
            ),
            patch.object(review.time, "monotonic", side_effect=[0, 1, 2]),
            patch.object(review.time, "sleep") as sleep,
            patch.object(review, "gh_json") as api,
            redirect_stderr(io.StringIO()),
        ):
            self.assertEqual(review.review(self.candidate, wait_seconds=20)["state"], "ready")
            sleep.assert_called_once()
            api.assert_not_called()
        with (
            patch.object(review, "snapshot", side_effect=review.NotReady("jobs missing")),
            self.assertRaises(review.NotReady),
        ):
            review.review(self.candidate)

    def test_snapshot_uses_exact_attempt_and_read_only_endpoints(self):
        replies = [self.run, self.tag_ref, self.tag, self.branch, {"jobs": self.jobs}, self.pending]
        with patch.object(review, "gh_json", side_effect=replies) as api:
            self.assertEqual(review.snapshot(self.candidate)["state"], "ready")
        self.assertTrue(all(len(call.args) == 1 for call in api.call_args_list))
        self.assertIn("/attempts/1/jobs?", api.call_args_list[4].args[0])
        self.assertFalse(any("secrets" in call.args[0] for call in api.call_args_list))

    def test_cli_is_read_only_by_default_and_reports_not_ready(self):
        args = [
            "--repo",
            self.candidate.repository,
            "--run-id",
            "42",
            "--tag",
            self.candidate.tag,
            "--commit",
            self.candidate.commit,
        ]
        with (
            patch.object(review, "snapshot", return_value=self.ready()),
            redirect_stdout(io.StringIO()) as output,
        ):
            self.assertEqual(review.main(args), 0)
            self.assertEqual(json.loads(output.getvalue())["state"], "ready")
        with (
            patch.object(review, "snapshot", side_effect=review.NotReady("missing")),
            redirect_stderr(io.StringIO()),
        ):
            self.assertEqual(review.main(args), 2)

    def test_transport_has_timeout_no_shell_and_no_post_retry(self):
        with patch.object(
            review.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, "{}")
        ) as process:
            self.assertEqual(review.gh_json("repos/owner/repo/actions/runs/42"), {})
            self.assertEqual(process.call_args.kwargs["timeout"], 30)
            self.assertEqual(process.call_args.kwargs["encoding"], "utf-8")
            self.assertNotIn("shell", process.call_args.kwargs)
            self.assertIn("GET", process.call_args.args[0])
            self.assertIn("github.com", process.call_args.args[0])
            self.assertIn("Cache-Control: no-cache", process.call_args.args[0])
        with (
            patch.object(
                review.subprocess, "run", side_effect=subprocess.TimeoutExpired("gh", 30)
            ) as process,
            self.assertRaisesRegex(ContractError, "outcome unknown"),
        ):
            review.gh_json(
                "repos/owner/repo/actions/runs/42/pending_deployments", {"state": "approved"}
            )
        process.assert_called_once()

    def test_partial_batch_is_never_submitted_even_with_explicit_approval(self):
        with (
            patch.object(review, "snapshot", side_effect=review.NotReady("Linux not registered")),
            patch.object(review, "gh_json") as api,
            self.assertRaises(review.NotReady),
        ):
            review.review(self.candidate, approve=True)
        api.assert_not_called()

    def test_post_must_cover_five_distinct_exact_candidate_deployments(self):
        deployments = [
            {
                "id": index + 1,
                "environment": "release-signing",
                "sha": self.candidate.commit,
                "ref": self.candidate.tag,
            }
            for index in range(5)
        ]
        for change in (
            {"sha": "c" * 40},
            {"ref": "main"},
            {"environment": "release-publish"},
            {"id": 2},
        ):
            response = copy.deepcopy(deployments)
            response[0].update(change)
            with (
                self.subTest(change=change),
                patch.object(review, "snapshot", return_value=self.ready()),
                patch.object(review, "gh_json", return_value=response) as api,
                self.assertRaises(ContractError),
            ):
                review.review(self.candidate, approve=True)
            api.assert_called_once()

    def test_malformed_api_objects_are_rejected_without_approval(self):
        for field in ("run", "tag_ref", "tag", "branch"):
            with self.subTest(field=field), self.assertRaises(ContractError):
                self.verify(**{field: None})
        for pending in (None, [None], [{"environment": None}]):
            with self.assertRaises(ContractError):
                review.signing_batch(self.candidate, self.jobs, pending)
        replies = [self.run, self.tag_ref, self.tag, self.branch, {"jobs": None}]
        with patch.object(review, "gh_json", side_effect=replies), self.assertRaises(ContractError):
            review.snapshot(self.candidate)

    def test_paginated_jobs_are_collected_before_deciding(self):
        filler = [
            {**self.jobs[0], "id": index + 100, "name": f"Other job {index}"} for index in range(95)
        ]
        replies = [
            self.run,
            self.tag_ref,
            self.tag,
            self.branch,
            {"jobs": self.jobs + filler[:94]},
            {"jobs": filler[94:]},
            self.pending,
        ]
        with patch.object(review, "gh_json", side_effect=replies) as api:
            self.assertEqual(review.snapshot(self.candidate)["state"], "ready")
        self.assertTrue(api.call_args_list[5].args[0].endswith("page=2"))

    def test_cli_rejects_approval_without_tag_object_and_invalid_wait(self):
        args = [
            "--repo",
            self.candidate.repository,
            "--run-id",
            "42",
            "--tag",
            self.candidate.tag,
            "--commit",
            self.candidate.commit,
        ]
        for extra in (["--approve"], ["--wait-seconds", "-1"], ["--wait-seconds", "1801"]):
            with patch.object(review, "gh_json") as api, redirect_stderr(io.StringIO()):
                self.assertEqual(review.main(args + extra), 1)
            api.assert_not_called()

    def test_all_five_job_names_match_the_unchanged_workflow_matrix(self):
        root = Path(__file__).resolve().parents[1]
        workflow = (root / ".github/workflows/release.yml").read_text(encoding="utf-8")
        linux = (root / ".github/workflows/linux.yml").read_text(encoding="utf-8")
        self.assertIn("name: Windows ${{ matrix.variant }}", workflow)
        self.assertIn("name: Android and Android TV", workflow)
        self.assertIn("name: Linux signed packages", workflow)
        self.assertIn("name: Linux ${{ matrix.architecture }} AppImage deb rpm", linux)
        for name in review.SIGNING_JOBS[:2]:
            self.assertIn("variant: " + name.removeprefix("Windows "), workflow)
        for architecture in ("amd64", "arm64"):
            self.assertIn("architecture: " + architecture, linux)
            self.assertIn(
                f"Linux signed packages / Linux {architecture} AppImage deb rpm",
                review.SIGNING_JOBS,
            )


if __name__ == "__main__":
    unittest.main()
