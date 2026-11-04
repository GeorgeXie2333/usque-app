"""Prepare an inert timer candidate fixture; never read or modify a Flutter SDK.

This helper does not integrate or validate an SDK or application. It applies the
checked-in patch to frozen sources, then extracts both algorithms mechanically.
Only the standalone C++ harness is built by test_flutter_windows_timer.ps1.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BUNDLE = Path(__file__).resolve().parent / "fixtures" / "flutter_windows_timer"
SOURCE_PREFIX = "engine/src/flutter/shell/platform/windows/"
SOURCE_FILES = ("task_runner_window.cc", "task_runner_window.h")
ASSETS = {
    *(f"upstream/{name}" for name in SOURCE_FILES),
    "LICENSE.flutter",
    "candidate.patch",
    "timer_fixture.cpp",
    "fixture_support.h",
}
ACCESSORS = """  // Test-only read access; the extracted methods below are unchanged.
  bool FixtureIsParked() {
    std::lock_guard<std::mutex> lock(mutex_);
    return next_fire_time_ == std::chrono::high_resolution_clock::time_point::max();
  }
  unsigned FixtureWaitEntries() const { return cv_.wait_entries(); }
  bool FixtureHasCallback() {
    std::lock_guard<std::mutex> lock(mutex_);
    return callback_ != nullptr;
  }

"""


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def checked_assets(bundle: Path) -> tuple[dict, dict[str, bytes]]:
    """Fail before staging if any frozen input or patch has changed."""
    manifest = json.loads((bundle / "manifest.json").read_text(encoding="utf-8"))
    if manifest.get("schema") != 1 or set(manifest.get("sha256", {})) != ASSETS:
        raise ValueError("Unexpected fixture manifest or asset paths")
    assets = {}
    for name in sorted(ASSETS):
        path = bundle / name
        if not path.resolve().is_relative_to(bundle.resolve()):
            raise ValueError(f"Asset escapes fixture directory: {name}")
        data = path.read_bytes()
        if sha256(data) != manifest["sha256"][name]:
            raise ValueError(f"Frozen asset hash mismatch: {name}")
        if b"\r" in data:
            raise ValueError(f"Frozen asset must use canonical LF bytes: {name}")
        data.decode("utf-8")
        assets[name] = data
    patch_paths = [
        line[4:]
        for line in assets["candidate.patch"].decode().splitlines()
        if line.startswith(("--- ", "+++ "))
    ]
    expected_paths = [f"{side}/{SOURCE_PREFIX}{name}" for name in SOURCE_FILES for side in "ab"]
    if patch_paths != expected_paths:
        raise ValueError("Patch must modify only the two frozen upstream files")
    return manifest, assets


def checked_output(output: Path) -> Path:
    """Only fresh children of the repository's dedicated target directory."""
    allowed = ROOT.resolve() / "target" / "flutter-windows-timer"
    resolved = output.resolve()
    if (
        ".." in output.parts
        or resolved == allowed
        or not resolved.is_relative_to(allowed)
        or output.exists()
        or output.is_symlink()
    ):
        raise ValueError("Output must be a new child of target/flutter-windows-timer")
    return resolved


def extract(text: str, start: str, end: str) -> str:
    if text.count(start) != 1 or text.count(end) != 1:
        raise ValueError("Timer extraction markers are not unique")
    begin, finish = text.index(start), text.index(end)
    if finish <= begin:
        raise ValueError("Timer extraction markers are out of order")
    return text[begin:finish].rstrip()


def timer_header(cc: str, header: str, namespace: str) -> str:
    declaration = extract(header, "class TimerThread {", "// Hidden HWND")
    methods = extract(cc, "TimerThread::TimerThread(", "// Timer used for PollOnce timeout.")
    if (
        declaration.count(" private:\n") != 1
        or declaration.count("std::condition_variable cv_;") != 1
    ):
        raise ValueError("Unexpected timer declaration layout")
    declaration = declaration.replace(" private:\n", ACCESSORS + " private:\n")
    declaration = declaration.replace(
        "std::condition_variable cv_;", "FixtureConditionVariable cv_;"
    )
    if namespace == "candidate" and "schedule_counter_" in declaration + methods:
        raise ValueError("Candidate must be extracted from the counter-removing patch")
    return (
        "// Copyright 2013 The Flutter Authors. All rights reserved.\n"
        "// BSD-style license: LICENSE.flutter. Candidate is not SDK-integrated.\n"
        '#include "fixture_support.h"\n\n'
        f"namespace {namespace} {{\n{declaration}\n\n{methods}\n}}\n"
    )


def run_git(git: str, directory: Path, *arguments: str) -> None:
    # Resolved Git executable, fixed local operations, argv list; no shell/network.
    # A caller's GIT_DIR/WORK_TREE/INDEX or config injection must never redirect
    # the isolated init/apply operation into its checkout or another repository.
    environment = {
        name: value for name, value in os.environ.items() if not name.upper().startswith("GIT_")
    }
    environment.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)
    subprocess.run(  # noqa: S603 -- arguments only address the isolated fixture copy
        [git, "-c", "core.autocrlf=false", "-c", "core.eol=lf", "-C", str(directory), *arguments],
        check=True,
        capture_output=True,
        text=True,
        timeout=30,
        env=environment,
    )


def prepare(output: Path, bundle: Path = BUNDLE) -> dict:
    manifest, assets = checked_assets(bundle)
    output = checked_output(output)
    git = shutil.which("git")
    if git is None:
        raise ValueError("Git is required to check and apply the frozen patch")
    git = str(Path(git).resolve())
    candidate = output / "candidate-source"
    source_directory = candidate / SOURCE_PREFIX
    source_directory.mkdir(parents=True)
    for name in SOURCE_FILES:
        (source_directory / name).write_bytes(assets[f"upstream/{name}"])
    (output / "candidate.patch").write_bytes(assets["candidate.patch"])
    run_git(
        git, candidate, "init", "--quiet", "--template=", "--initial-branch=codex/timer-fixture"
    )
    run_git(git, candidate, "apply", "--check", str(output / "candidate.patch"))
    run_git(git, candidate, "apply", str(output / "candidate.patch"))
    candidate_bytes = {name: (source_directory / name).read_bytes() for name in SOURCE_FILES}
    for namespace, source in (
        ("original", {name: assets[f"upstream/{name}"] for name in SOURCE_FILES}),
        ("candidate", candidate_bytes),
    ):
        generated = timer_header(
            *(source[name].decode("utf-8") for name in SOURCE_FILES), namespace
        )
        (output / f"{namespace}_timer.h").write_bytes(generated.encode("utf-8"))
    for name in ("timer_fixture.cpp", "fixture_support.h", "LICENSE.flutter"):
        (output / name).write_bytes(assets[name])
    report = {
        "candidate_sdk_integrated": False,
        "engine_gn_gtest_and_dll_integration": "not_run",
        "source_engine_commit": manifest["engine_commit"],
        "input_sha256": manifest["sha256"],
        "patched_source_sha256": {name: sha256(data) for name, data in candidate_bytes.items()},
        "generated_sha256": {
            name: sha256((output / name).read_bytes())
            for name in (
                "original_timer.h",
                "candidate_timer.h",
                "timer_fixture.cpp",
                "fixture_support.h",
            )
        },
        "method_extraction": "verbatim from frozen and git-applied sources",
        "declaration_adjustments": ["read-only accessors", "delegating wait-entry witness"],
        "git_apply_check": "passed",
        "shared_sdk_accessed": False,
    }
    (output / "preparation.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    return report


def validate_results(path: Path) -> dict:
    """Require every original reproduction and candidate case, not just a summary."""
    if path.stat().st_size > 128_000:
        raise ValueError("Fixture result exceeds its bounded output size")
    rows = [json.loads(line) for line in path.read_text(encoding="utf-8-sig").splitlines()]
    if not rows or len(rows) > 40 or not all(isinstance(row, dict) for row in rows):
        raise ValueError("Incomplete or malformed fixture result")
    expected = {}
    for implementation in ("frozen_original", "patched_candidate"):
        success = implementation == "patched_candidate"
        for round_number in range(1, 4):
            expected[("controlled_callback_rearm", implementation, round_number)] = success
        for case in (
            "earlier_future_notification_after_wait_entry",
            "future_notification_while_parked",
        ):
            expected[(case, implementation, None)] = success
    for case in (
        "candidate_earliest_wins_during_callback",
        "candidate_callback_self_rearm",
        "candidate_stop_parked",
        "candidate_stop_future_wait",
        "candidate_stop_during_callback",
    ):
        expected[(case, None, None)] = True
    seen = set()
    for row in rows[:-1]:
        if row.get("event") == "case_started":
            continue
        key = (row.get("case"), row.get("implementation"), row.get("round"))
        if key not in expected or key in seen:
            raise ValueError("Unexpected or duplicate fixture case")
        if row.get("contract_pass", row.get("pass")) is not expected[key]:
            raise ValueError(f"Fixture contract mismatch: {key}")
        if any(isinstance(value, float) and not math.isfinite(value) for value in row.values()):
            raise ValueError("Non-finite fixture timing")
        seen.add(key)
    summary = rows[-1]
    required = {
        "summary": "original_expiry_race_and_known_notification_bug_reproduced_candidate_checks_passed",
        "original_expiry_reproductions": 3,
        "original_notification_reproductions": 2,
        "candidate_cases_passed": 10,
        "candidate_sdk_integrated": False,
        "ui_network_or_service_operations": 0,
        "vm_high_cpu_attribution_proven": False,
    }
    if seen != expected.keys() or summary != required:
        raise ValueError("Fixture cases or final summary are incomplete")
    return summary


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("prepare").add_argument("--output", required=True, type=Path)
    commands.add_parser("validate-results").add_argument("path", type=Path)
    args = parser.parse_args()
    report = prepare(args.output) if args.command == "prepare" else validate_results(args.path)
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
