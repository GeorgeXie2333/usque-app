"""Find completed dev checks for a main push. An evidence miss requires fresh checks."""

import argparse
import os
import sys
from pathlib import Path

from release_contract import ContractError, normalize_commit, verify_push_run
from release_signing_review import gh_json, object_value

REQUIRED_JOBS = {
    "ci.yml": {"CI / gate"},
    "codeql.yml": {"Analyze actions", "Analyze c-cpp", "Analyze python", "Analyze rust"},
}


def dev_run(repository: str, commit: str, workflow: str) -> str:
    commit = normalize_commit(commit)
    base = f"repos/{repository}"
    for branch in ("main", "dev"):
        ref = object_value(gh_json(f"{base}/git/ref/heads/{branch}"), "branch ref")
        target = object_value(ref.get("object"), "ref target")
        if (
            ref.get("ref") != f"refs/heads/{branch}"
            or target.get("type") != "commit"
            or target.get("sha") != commit
        ):
            raise ContractError(f"{branch} tip differs from the candidate")

    # Inspect the latest dev push for this SHA, including unfinished or failed runs.
    response = gh_json(
        f"{base}/actions/workflows/{workflow}/runs?branch=dev&event=push&head_sha={commit}&per_page=1"
    )
    run = verify_push_run(object_value(response, "workflow runs"), commit, "dev", workflow)
    for key in ("repository", "head_repository"):
        if object_value(run.get(key), key).get("full_name") != repository:
            raise ContractError("workflow run belongs to another repository")
    run_id, attempt = run.get("id"), run.get("run_attempt")
    if any(type(value) is not int or value <= 0 for value in (run_id, attempt)):
        raise ContractError("missing workflow run or attempt ID")
    run_path = f"actions/runs/{run_id}/attempts/{attempt}"
    response = object_value(gh_json(f"{base}/{run_path}/jobs?per_page=100"), "workflow jobs")
    jobs = response.get("jobs")
    if not isinstance(jobs, list) or len(jobs) != response.get("total_count"):
        raise ContractError("incomplete workflow jobs response")
    required = REQUIRED_JOBS[workflow]
    completed = set()
    for record in jobs:
        job = object_value(record, "workflow job")
        name = job.get("name")
        if not isinstance(name, str):
            raise ContractError("missing job name")
        if name in required:
            if (
                name in completed
                or job.get("run_id") != run_id
                or job.get("head_sha") != commit
                or job.get("status") != "completed"
                or job.get("conclusion") != "success"
            ):
                raise ContractError("required dev job did not succeed for the candidate")
            completed.add(name)
    if completed != required:
        raise ContractError("required dev jobs are missing")
    return run_path


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("workflow", choices=REQUIRED_JOBS)
    workflow = parser.parse_args().workflow
    try:
        source = dev_run(os.environ["GITHUB_REPOSITORY"], os.environ["GITHUB_SHA"], workflow)
    except ContractError as error:
        print(f"Running {workflow} in full: {error}", file=sys.stderr)
        return
    with Path(os.environ["GITHUB_STEP_SUMMARY"]).open("a", encoding="utf-8") as summary:
        repo_url = f"{os.environ['GITHUB_SERVER_URL']}/{os.environ['GITHUB_REPOSITORY']}"
        print(
            f"Reusing [dev {workflow}]({repo_url}/{source}) for `{os.environ['GITHUB_SHA']}`.",
            file=summary,
        )
    with Path(os.environ["GITHUB_OUTPUT"]).open("a", encoding="utf-8") as output:
        print("reused=true", file=output)


if __name__ == "__main__":
    main()
