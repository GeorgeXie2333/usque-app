"""Wait for one complete signing-review batch; never review publication or bypass protection."""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import time
from dataclasses import dataclass
from typing import Any

from release_contract import COMMIT_PATTERN, REPOSITORY_PATTERN, ContractError, normalize_commit

SIGNING_JOBS = (
    "Windows x64-v2",
    "Windows arm64",
    "Android and Android TV",
    "Linux signed packages / Linux amd64 AppImage deb rpm",
    "Linux signed packages / Linux arm64 AppImage deb rpm",
)
TAG = re.compile(r"v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)(?:-beta\.[1-9][0-9]*)?")


class NotReady(ContractError):
    """No partial batch may be approved while jobs are still registering."""


def object_value(value: Any, label: str) -> dict:
    if not isinstance(value, dict):
        raise ContractError(f"invalid {label} response")
    return value


@dataclass(frozen=True)
class Candidate:
    repository: str
    run_id: int
    attempt: int
    tag: str
    commit: str
    tag_object: str | None = None

    def __post_init__(self) -> None:
        if not REPOSITORY_PATTERN.fullmatch(self.repository) or any(
            part in {".", ".."} for part in self.repository.split("/")
        ):
            raise ContractError("invalid repository")
        if (
            type(self.run_id) is not int
            or type(self.attempt) is not int
            or self.run_id <= 0
            or self.attempt <= 0
            or not TAG.fullmatch(self.tag)
        ):
            raise ContractError("positive run/attempt and canonical stable or beta tag required")
        if not COMMIT_PATTERN.fullmatch(self.commit) or (
            self.tag_object is not None and not COMMIT_PATTERN.fullmatch(self.tag_object)
        ):
            raise ContractError("full lowercase candidate/tag-object SHA required")


def gh_json(endpoint: str, body: dict | None = None) -> Any:
    # Fixed executable/argument vector, no shell, no signing-secret endpoints.
    command = [
        "gh",
        "api",
        "--hostname",
        "github.com",
        "--header",
        "Cache-Control: no-cache",
        "--method",
        "GET" if body is None else "POST",
        endpoint,
    ]
    if body is not None:
        command += ["--input", "-"]
    unknown = "approval outcome unknown; inspect GitHub history and do not automatically retry"
    try:
        result = subprocess.run(  # noqa: S603, S607 -- fixed gh executable; validated API paths, no shell.
            command,
            input=None if body is None else json.dumps(body),
            capture_output=True,
            text=True,
            encoding="utf-8",
            timeout=30,
            check=False,
        )
    except subprocess.TimeoutExpired as error:
        raise ContractError(unknown if body is not None else "GitHub API read timed out") from error
    except OSError as error:
        raise ContractError(
            unknown if body is not None else "cannot execute the authenticated GitHub CLI"
        ) from error
    except UnicodeError as error:
        raise ContractError(
            unknown if body is not None else "GitHub returned invalid UTF-8"
        ) from error
    if result.returncode:
        raise ContractError(unknown if body is not None else "GitHub API read failed")
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise ContractError(
            unknown if body is not None else "GitHub returned invalid JSON"
        ) from error


def verify_identity(
    candidate: Candidate, run: dict, tag_ref: dict, tag: dict, branch: dict
) -> None:
    run = object_value(run, "workflow run")
    tag_ref = object_value(tag_ref, "tag ref")
    tag = object_value(tag, "annotated tag")
    branch = object_value(branch, "source ref")
    expected = {
        "id": candidate.run_id,
        "run_attempt": candidate.attempt,
        "head_sha": candidate.commit,
        "head_branch": candidate.tag,
        "event": "push",
        "path": ".github/workflows/release.yml",
    }
    if (
        any(run.get(key) != value for key, value in expected.items())
        or object_value(run.get("repository"), "repository").get("full_name")
        != candidate.repository
    ):
        raise ContractError("wrong release repository, workflow, event, candidate, tag or attempt")
    if (
        run.get("status") not in {"queued", "waiting", "pending", "requested", "in_progress"}
        or run.get("conclusion") is not None
    ):
        raise ContractError("release run is not awaiting an initial signing batch")
    object_ref = object_value(tag_ref.get("object"), "tag ref object")
    object_sha = object_ref.get("sha", "")
    tagged_commit = object_value(tag.get("object"), "tagged commit")
    if (
        tag_ref.get("ref") != f"refs/tags/{candidate.tag}"
        or object_ref.get("type") != "tag"
        or not isinstance(object_sha, str)
        or not COMMIT_PATTERN.fullmatch(object_sha)
        or (candidate.tag_object is not None and object_sha != candidate.tag_object)
        or tag.get("sha") != object_sha
        or tag.get("tag") != candidate.tag
        or tagged_commit.get("type") != "commit"
        or tagged_commit.get("sha") != candidate.commit
        or str(tag.get("message", "")).strip() != f"Usque {candidate.tag}"
    ):
        raise ContractError(
            "annotated release tag moved or does not identify the approved candidate"
        )
    source = f"release/{candidate.tag}" if "-beta." in candidate.tag else "main"
    branch_object = object_value(branch.get("object"), "branch commit")
    if (
        branch.get("ref") != f"refs/heads/{source}"
        or branch_object.get("type") != "commit"
        or branch_object.get("sha") != candidate.commit
    ):
        raise ContractError("release source branch no longer points to the candidate")


def signing_batch(candidate: Candidate, jobs: list[dict], pending: list[dict]) -> dict:
    ids = set()
    names: dict[str, dict] = {}
    for job in jobs:
        if not isinstance(job, dict) or any(
            job.get(key) != value
            for key, value in {
                "run_id": candidate.run_id,
                "run_attempt": candidate.attempt,
                "head_sha": candidate.commit,
                "head_branch": candidate.tag,
            }.items()
        ):
            raise ContractError("job evidence has the wrong candidate, run or attempt")
        identifier = job.get("id")
        if type(identifier) is not int or identifier <= 0 or identifier in ids:
            raise ContractError("invalid or duplicate job ID")
        ids.add(identifier)
        name = job.get("name")
        if name in SIGNING_JOBS or name == "Validate protected release inputs":
            if name in names:
                raise ContractError(
                    "duplicate signing/gate job; inspect the run instead of approving"
                )
            names[name] = job
    gate = names.get("Validate protected release inputs", {})
    if gate.get("status") != "completed" or gate.get("conclusion") != "success":
        if gate.get("status") == "completed":
            raise ContractError("release-gate did not succeed")
        raise NotReady("waiting for successful release-gate")
    waiting = []
    for name in SIGNING_JOBS:
        job = names.get(name)
        if job is None or job.get("status") in {"queued", "requested", "pending"}:
            waiting.append(name)
        elif job.get("status") != "waiting" or job.get("conclusion") is not None:
            raise ContractError(
                f"{name} already started/finished or failed; do not approve a subset"
            )
    if waiting:
        raise NotReady("waiting for signing review requests: " + ", ".join(waiting))
    if not isinstance(pending, list) or any(not isinstance(item, dict) for item in pending):
        raise ContractError("invalid pending-environment response")
    environment = [
        item
        for item in pending
        if object_value(item.get("environment"), "environment").get("name") == "release-signing"
    ]
    if len(environment) > 1:
        raise ContractError("duplicate signing review environments; inspect the run")
    if not environment:
        raise NotReady("waiting for the single release-signing review environment")
    review = environment[0]
    identifier = review.get("environment", {}).get("id")
    if (
        type(identifier) is not int
        or identifier <= 0
        or review.get("current_user_can_approve") is not True
    ):
        raise ContractError("signing review is unavailable to this user; never bypass protection")
    return {
        "state": "ready",
        "candidate": candidate.commit,
        "run_id": candidate.run_id,
        "attempt": candidate.attempt,
        "environment": "release-signing",
        "environment_id": identifier,
        "jobs": [{"name": name, "id": names[name]["id"]} for name in SIGNING_JOBS],
    }


def snapshot(candidate: Candidate) -> dict:
    base = f"repos/{candidate.repository}"
    run = object_value(gh_json(f"{base}/actions/runs/{candidate.run_id}"), "workflow run")
    tag_ref = object_value(gh_json(f"{base}/git/ref/tags/{candidate.tag}"), "tag ref")
    tag_sha = object_value(tag_ref.get("object"), "tag ref object").get("sha", "")
    if not isinstance(tag_sha, str) or not COMMIT_PATTERN.fullmatch(tag_sha):
        raise ContractError("invalid tag object ID")
    tag = gh_json(f"{base}/git/tags/{tag_sha}")
    source = f"release/{candidate.tag}" if "-beta." in candidate.tag else "main"
    branch = gh_json(f"{base}/git/ref/heads/{source}")
    verify_identity(candidate, run, tag_ref, tag, branch)
    jobs = []
    for page in range(1, 11):
        response = object_value(
            gh_json(
                f"{base}/actions/runs/{candidate.run_id}/attempts/{candidate.attempt}/jobs?per_page=100&page={page}"
            ),
            "workflow jobs",
        )
        batch = response.get("jobs")
        if not isinstance(batch, list):
            raise ContractError("invalid workflow jobs response")
        jobs.extend(batch)
        if len(batch) < 100:
            break
    else:
        raise ContractError("too many jobs; refusing an incomplete review snapshot")
    pending = gh_json(f"{base}/actions/runs/{candidate.run_id}/pending_deployments")
    return {
        **signing_batch(candidate, jobs, pending),
        "tag": candidate.tag,
        "tag_object": tag_sha,
        "run_url": f"https://github.com/{candidate.repository}/actions/runs/{candidate.run_id}",
    }


def review(candidate: Candidate, *, approve: bool = False, wait_seconds: int = 0) -> dict:
    if approve and candidate.tag_object is None:
        raise ContractError("--approve requires the previously recorded --tag-object SHA")
    deadline = time.monotonic() + wait_seconds
    previous_message = None
    while True:
        try:
            ready = snapshot(candidate)
            break
        except NotReady as error:
            if time.monotonic() >= deadline:
                raise
            if str(error) != previous_message:
                print(str(error), file=sys.stderr)
                previous_message = str(error)
            time.sleep(min(15, max(0, deadline - time.monotonic())))
    if not approve:
        return ready
    # Re-read identities and all five waiting jobs immediately before one POST.
    # No automatic retry after a changed snapshot or any approval-side error.
    if snapshot(candidate) != ready:
        raise ContractError("review batch changed before approval; inspect the run")
    deployments = gh_json(
        f"repos/{candidate.repository}/actions/runs/{candidate.run_id}/pending_deployments",
        {
            "environment_ids": [ready["environment_id"]],
            "state": "approved",
            "comment": f"Maintainer-authorized {candidate.tag} signing batch: all five jobs ready for {candidate.commit}.",
        },
    )
    if (
        not isinstance(deployments, list)
        or len(deployments) != len(SIGNING_JOBS)
        or any(
            not isinstance(item, dict)
            or type(item.get("id")) is not int
            or item["id"] <= 0
            or item.get("environment") != "release-signing"
            or item.get("sha") != candidate.commit
            or item.get("ref") != candidate.tag
            for item in deployments
        )
    ):
        raise ContractError(
            "unexpected approval response; inspect history and do not repeat approval"
        )
    if len({item["id"] for item in deployments}) != len(SIGNING_JOBS):
        raise ContractError(
            "duplicate approved deployment; inspect history and do not repeat approval"
        )
    return {**ready, "state": "approved", "deployments": len(deployments)}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", required=True)
    parser.add_argument("--run-id", required=True, type=int)
    parser.add_argument("--attempt", type=int, default=1)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--tag-object")
    parser.add_argument("--wait-seconds", type=int, default=0)
    parser.add_argument(
        "--approve",
        action="store_true",
        help="explicit maintainer-authorized signing approval only",
    )
    args = parser.parse_args(argv)
    try:
        if not 0 <= args.wait_seconds <= 1800:
            raise ContractError("wait must be between 0 and 1800 seconds")
        candidate = Candidate(
            args.repo,
            args.run_id,
            args.attempt,
            args.tag,
            normalize_commit(args.commit),
            args.tag_object,
        )
        result = review(candidate, approve=args.approve, wait_seconds=args.wait_seconds)
    except ContractError as error:
        print(str(error), file=sys.stderr)
        return 2 if isinstance(error, NotReady) else 1
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
