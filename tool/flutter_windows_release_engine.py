"""Restore the reviewed v0.3.4 Windows engine inputs from their trusted build.

The original producer identity includes its historical release-policy wording
and consumer tests.  This adapter accepts that one identity only after proving
the actual engine inputs and the remaining producer checks are unchanged.
"""

from __future__ import annotations

import argparse
import ast
import copy
import importlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

import flutter_windows_engine as engine
import flutter_windows_engine_package as package

PRODUCER = "e538b43c4df3b5fa3521d00ede8d204e38744b0a"
PRODUCER_RUN = 41924429831
LEGACY_RECIPE = "758d207dae9b89731d2680e84436ac32a935d9ef37500397f76acc80f5584481"
LEGACY_INPUTS = "106ed3bb696a08ee934d5dcc674c99eeb2bc88cc388efe45ff419bcc8af51853"
ARTIFACTS = {
    "x64": (13384238027, "59d7263973fd30710b3cceedc47c9a19e7c556209b58430887d7fe859c14d313"),
    "arm64": (13383667734, "ee28d31e4a0c323d8126be334bd8fb36b97e749e3e717b769831846a2332d6c1"),
}
PACKAGE_DIGESTS = {
    "x64": "a2d2f2f3f4e3a0d9f974667e8ee6d1bd074ed2ad6ad4737654ac9f79cfdc563b",
    "arm64": "11a13b000c3d711d1bd96a831d126c361b5743031456acf86dccd27a9a33acc3",
}
OLD_POLICY = "Fresh source compilation is required for release; an untrusted compiled-output cache is not release provenance."
NEW_POLICY = "A release may reuse an attested trusted source build; untrusted compiled-output caches are rejected."
RECIPE_PATH = "tool/flutter_windows_engine_manifest.json"
PACKAGE_PATH = "tool/flutter_windows_engine_package.py"
TEST_PATH = "tool/test_flutter_windows_engine_package.py"
CHANGED_TESTS = {
    "test_release_retains_fresh_source_builds_and_pr_access_stays_read_only",
    "test_release_requires_attested_engine_and_pr_access_stays_read_only",
    "test_windows_workflows_install_python_and_pass_its_executable_to_native_helpers",
    "test_windows_workflows_install_pinned_python_before_engine_steps",
}


class CompatibilityMismatch(ValueError):
    """The old build is not eligible for this candidate."""


def git_bytes(root: Path, *arguments: str, allow_failure: bool = False) -> bytes:
    executable = shutil.which("git.exe" if os.name == "nt" else "git")
    if not executable:
        raise ValueError("Git is required to verify the engine producer")
    environment = {k: v for k, v in os.environ.items() if not k.upper().startswith("GIT_")}
    environment.update(
        GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull, GIT_OPTIONAL_LOCKS="0"
    )
    result = subprocess.run(  # noqa: S603 -- resolved executable, fixed git operations and validated revisions
        [str(Path(executable).resolve()), "-C", str(root), *arguments],
        capture_output=True,
        timeout=60,
        check=False,
        env=environment,
    )
    if result.returncode and not allow_failure:
        raise ValueError("Unable to verify the pinned engine producer in Git")
    if result.returncode:
        raise CompatibilityMismatch("The engine producer is not an ancestor of this candidate")
    return result.stdout


def source_commit(root: Path, expected: str) -> None:
    if not re.fullmatch(r"[0-9a-f]{40}", expected):
        raise ValueError("The candidate must be a complete lowercase commit SHA")
    actual = git_bytes(root, "rev-parse", "HEAD").decode().strip()
    if actual != expected:
        raise ValueError("The checkout differs from the release candidate")
    git_bytes(root, "merge-base", "--is-ancestor", PRODUCER, expected, allow_failure=True)


def normalized_python(data: bytes, *, tests: bool = False) -> str:
    tree = ast.parse(data.decode("utf-8-sig"))
    if (
        tree.body
        and isinstance(tree.body[0], ast.Expr)
        and isinstance(tree.body[0].value, ast.Constant)
        and isinstance(tree.body[0].value.value, str)
    ):
        tree.body.pop(0)
    if tests:
        for item in tree.body:
            if isinstance(item, ast.ClassDef) and item.name == "EnginePackageTests":
                item.body = [
                    node
                    for node in item.body
                    if not isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))
                    or node.name not in CHANGED_TESTS
                ]
    return ast.dump(tree, include_attributes=False)


def snapshot_and_check(root: Path, snapshot: Path) -> dict:
    # Read only paths from the reviewed producer's existing identity contract.
    current_recipe, current_hash = engine.load_recipe(root / RECIPE_PATH, root)
    names = set(package.INPUT_FILES) | {RECIPE_PATH}
    names.update(item["path"] for item in current_recipe["patches"])
    for name in sorted(names):
        target = engine.relative_file(snapshot, name)
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(git_bytes(root, "show", f"{PRODUCER}:{name}"))
        current = engine.relative_file(root, name).read_bytes()
        old = target.read_bytes()
        if name == RECIPE_PATH:
            continue
        if name == PACKAGE_PATH:
            equal = normalized_python(current) == normalized_python(old)
        elif name == TEST_PATH:
            equal = normalized_python(current, tests=True) == normalized_python(old, tests=True)
        else:
            equal = current.replace(b"\r\n", b"\n") == old.replace(b"\r\n", b"\n")
        if not equal:
            raise CompatibilityMismatch(f"Engine producer input changed: {name}")
    old_recipe, old_hash = engine.load_recipe(snapshot / RECIPE_PATH, snapshot)
    expected = copy.deepcopy(old_recipe)
    contracts = expected.get("integration_contract", [])
    if contracts.count(OLD_POLICY) != 1:
        raise ValueError("Historical engine policy identity is invalid")
    contracts[contracts.index(OLD_POLICY)] = NEW_POLICY
    if current_recipe != expected or old_hash != LEGACY_RECIPE:
        raise CompatibilityMismatch("Engine recipe differs beyond the reviewed release policy")
    for architecture in ARTIFACTS:
        identity = package.identity(architecture, snapshot)
        if identity["recipe_sha256"] != LEGACY_RECIPE or identity["input_sha256"] != LEGACY_INPUTS:
            raise ValueError("Historical engine input identity differs from the pinned producer")
    return {"legacy_recipe_sha256": old_hash, "candidate_recipe_sha256": current_hash}


def run_worker(snapshot: Path, architecture: str, destination: Path) -> dict:
    result = subprocess.run(  # noqa: S603 -- this checked-in adapter and fixed arguments
        [
            sys.executable,
            str(Path(__file__).resolve()),
            "worker",
            "--snapshot",
            str(snapshot),
            "--architecture",
            architecture,
            "--destination",
            str(destination),
        ],
        capture_output=True,
        text=True,
        timeout=1200,
        check=False,
    )
    if result.returncode:
        raise ValueError(
            "Pinned engine artifact download or provenance verification failed: "
            + result.stderr.strip()[-800:]
        )
    value = json.loads(result.stdout)
    if not isinstance(value, dict):
        raise ValueError("Invalid engine restore result")
    return value


def pinned_artifacts(value: dict, architecture: str) -> dict:
    artifact_id, digest = ARTIFACTS[architecture]
    matches = [item for item in value.get("artifacts", []) if item.get("id") == artifact_id]
    if len(matches) != 1:
        raise ValueError("The approved engine artifact is missing")
    item = matches[0]
    if (
        item.get("digest") != f"sha256:{digest}"
        or item.get("expired") is not False
        or item.get("workflow_run", {}).get("id") != PRODUCER_RUN
    ):
        raise ValueError("The approved engine artifact identity changed")
    return {**value, "artifacts": matches}


def pinned_run(value: dict, trusted: bool) -> dict:
    if (
        value.get("head_sha") != PRODUCER
        or value.get("head_branch") != "dev"
        or value.get("event") != "push"
        or value.get("id") != PRODUCER_RUN
        or not trusted
    ):
        raise ValueError("The engine producer run is not the approved source build")
    return value


def worker(snapshot: Path, architecture: str, destination: Path) -> dict:
    # Use the exact historical, SHA-pinned verifier and its own original recipe.
    for name in ("flutter_windows_engine", "flutter_windows_engine_package"):
        sys.modules.pop(name, None)
    sys.path.insert(0, str(snapshot / "tool"))
    legacy = importlib.import_module("flutter_windows_engine_package")
    if Path(legacy.__file__).resolve() != (snapshot / PACKAGE_PATH).resolve():
        raise ValueError("Historical verifier was not loaded from the producer snapshot")
    original_api = legacy.api
    artifact_id, digest = ARTIFACTS[architecture]

    def pinned_api(path: str) -> dict:
        value = original_api(path)
        if path.startswith("actions/artifacts?name="):
            return pinned_artifacts(value, architecture)
        elif path == f"actions/runs/{PRODUCER_RUN}":
            return pinned_run(value, legacy.trusted_run(value))
        else:
            raise ValueError("Unexpected engine provenance lookup")

    legacy.api = pinned_api
    result = legacy.restore(architecture, destination)
    if (
        not result.get("restored")
        or result.get("artifact_id") != artifact_id
        or result.get("artifact_digest") != f"sha256:{digest}"
        or result.get("producer_commit") != PRODUCER
        or result.get("producer_run_id") != PRODUCER_RUN
        or result.get("package_sha256") != PACKAGE_DIGESTS[architecture]
    ):
        raise ValueError("Restored engine does not match the approved producer")
    return result


def restore(root: Path, architecture: str, destination: Path, commit: str) -> dict:
    source_commit(root, commit)
    with tempfile.TemporaryDirectory(prefix="usque-legacy-engine-") as temporary:
        snapshot = Path(temporary) / "producer"
        snapshot.mkdir()
        compatibility = snapshot_and_check(root, snapshot)
        result = run_worker(snapshot, architecture, destination)
    result.update(compatibility)
    result.update(
        candidate_commit=commit, architecture=architecture, compatibility="reviewed-v0.3.4"
    )
    (destination / "engine-restore.json").write_text(
        json.dumps(result, indent=2) + "\n", encoding="utf-8"
    )
    return result


def verify_receipt(root: Path, architecture: str, path: Path, commit: str) -> dict:
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise ValueError("The candidate must be a complete lowercase commit SHA")
    _, current_hash = engine.load_recipe(root / RECIPE_PATH, root)
    receipt = engine.read_json(path)
    artifact_id, digest = ARTIFACTS[architecture]
    expected = {
        "candidate_commit": commit,
        "architecture": architecture,
        "artifact_id": artifact_id,
        "artifact_digest": f"sha256:{digest}",
        "producer_commit": PRODUCER,
        "producer_run_id": PRODUCER_RUN,
        "compatibility": "reviewed-v0.3.4",
        "legacy_recipe_sha256": LEGACY_RECIPE,
        "candidate_recipe_sha256": current_hash,
        "recipe_sha256": LEGACY_RECIPE,
        "input_sha256": LEGACY_INPUTS,
        "package_sha256": PACKAGE_DIGESTS[architecture],
        "restored": True,
    }
    if any(receipt.get(key) != value for key, value in expected.items()):
        raise ValueError("Engine restore receipt differs from the approved candidate")
    return expected


def verify_staged(root: Path, architecture: str, build_root: Path, dll: Path, commit: str) -> dict:
    source_commit(root, commit)
    with tempfile.TemporaryDirectory(prefix="usque-legacy-engine-") as temporary:
        snapshot = Path(temporary) / "producer"
        snapshot.mkdir()
        compatibility = snapshot_and_check(root, snapshot)
        expected = verify_receipt(root, architecture, build_root / "engine-restore.json", commit)
        if any(expected.get(key) != value for key, value in compatibility.items()):
            raise ValueError("Engine compatibility evidence differs")
        record_path = build_root / f"engine-build-{architecture}.json"
        result = engine.verify_staged(
            dll, record_path, architecture, snapshot / RECIPE_PATH, snapshot
        )
        record = engine.read_json(record_path)
        old_recipe, _ = engine.load_recipe(snapshot / RECIPE_PATH, snapshot)
        pdb = (
            build_root
            / "flutter/engine/src/out"
            / old_recipe["architectures"][architecture]
            / "flutter_windows.dll.pdb"
        )
        expected_tests = "passed" if architecture == "x64" else "not_run"
        if (
            record.get("engine_tests") != expected_tests
            or not pdb.is_file()
            or package.file_digest(pdb) != record.get("flutter_windows_pdb_sha256")
        ):
            raise ValueError("Engine PDB or native test record differs from the source build")
        return {**result, **expected, "engine_tests": expected_tests}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    for name in ("restore", "verify-staged", "verify-receipt", "worker"):
        command = commands.add_parser(name)
        command.add_argument("--architecture", choices=sorted(ARTIFACTS), required=True)
        if name == "worker":
            command.add_argument("--snapshot", type=Path, required=True)
            command.add_argument("--destination", type=Path, required=True)
        else:
            command.add_argument("--commit", required=True)
            if name == "restore":
                command.add_argument("--destination", type=Path, required=True)
            elif name == "verify-staged":
                command.add_argument("--build-root", type=Path, required=True)
                command.add_argument("--dll", type=Path, required=True)
            else:
                command.add_argument("--receipt", type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.command == "worker":
            result = worker(args.snapshot, args.architecture, args.destination)
        elif args.command == "restore":
            result = restore(engine.ROOT, args.architecture, args.destination, args.commit)
        elif args.command == "verify-staged":
            result = verify_staged(
                engine.ROOT, args.architecture, args.build_root, args.dll, args.commit
            )
        else:
            result = verify_receipt(engine.ROOT, args.architecture, args.receipt, args.commit)
        print(json.dumps(result, indent=2))
        return 0
    except CompatibilityMismatch as error:
        print(f"Reviewed Windows engine inputs differ: {error}", file=sys.stderr)
        return 3
    except (OSError, ValueError, subprocess.TimeoutExpired) as error:
        print(f"Reviewed Windows engine reuse failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
