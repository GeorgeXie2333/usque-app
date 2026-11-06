"""Package and restore attested Windows engine builds.

A cache miss is distinct from an invalid artifact: only the former may fall
back to a source build. Release reuse also requires its separate pinned review.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import time
import zipfile
from pathlib import Path

import flutter_windows_engine as engine

REPOSITORY = "GeorgeXie2333/usque-app"
WORKFLOW = ".github/workflows/flutter-engine.yml"
TRUSTED_BRANCHES = {"main", "dev"}
INPUT_FILES = (
    WORKFLOW,
    "tool/build_flutter_windows_engine.ps1",
    "tool/flutter_windows_engine.py",
    "tool/flutter_windows_engine_package.py",
    "tool/test_flutter_windows_engine.py",
    "tool/test_flutter_windows_engine_package.py",
    "tool/flutter_windows_timer.py",
    "tool/test_flutter_windows_timer.ps1",
    "tool/test_flutter_windows_timer.py",
    "tool/fixtures/flutter_windows_timer/manifest.json",
    "tool/fixtures/flutter_windows_timer/timer_fixture.cpp",
    "tool/fixtures/flutter_windows_timer/fixture_support.h",
    "tool/fixtures/flutter_windows_timer/upstream/task_runner_window.cc",
    "tool/fixtures/flutter_windows_timer/upstream/task_runner_window.h",
)
MAX_ARCHIVE_BYTES = 2 * 1024**3
MAX_EXPANDED_BYTES = 6 * 1024**3
MAX_FILES = 30_000
PACKAGE_METADATA = "engine-package.json"


def file_digest(path: Path) -> str:
    value = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            value.update(block)
    return value.hexdigest()


def identity(architecture: str, repository: Path = engine.ROOT) -> dict:
    if architecture not in engine.MACHINES:
        raise ValueError("Unsupported package architecture")
    _, recipe_hash = engine.load_recipe(
        repository / "tool/flutter_windows_engine_manifest.json", repository
    )
    inputs = {
        name: engine.digest(
            engine.relative_file(repository, name).read_bytes().replace(b"\r\n", b"\n")
        )
        for name in INPUT_FILES
    }
    inputs["recipe"] = recipe_hash
    key = engine.digest(json.dumps(inputs, sort_keys=True).encode())
    return {
        "schema": 1,
        "architecture": architecture,
        "recipe_sha256": recipe_hash,
        "input_sha256": key,
        "artifact_name": f"flutter-windows-engine-v1-{architecture}-{key}",
    }


def checked_name(name: str) -> str:
    engine.relative_file(Path.cwd(), name)
    reserved = {"con", "prn", "aux", "nul"} | {
        f"{prefix}{number}" for prefix in ("com", "lpt") for number in range(1, 10)
    }
    for part in name.split("/"):
        if (
            part.endswith((".", " "))
            or part.split(".")[0].lower() in reserved
            or any(ord(char) < 32 or char in '<>"|?*' for char in part)
        ):
            raise ValueError(f"Unsafe Windows package path: {name}")
    return name


def archive_entries(archive: zipfile.ZipFile) -> dict[str, zipfile.ZipInfo]:
    entries = {}
    names = set()
    total = 0
    for entry in archive.infolist():
        name = checked_name(entry.filename)
        if (
            entry.is_dir()
            or stat.S_IFMT(entry.external_attr >> 16) not in (0, stat.S_IFREG)
            or name.casefold() in names
            or entry.flag_bits & 1
        ):
            raise ValueError("Package contains a directory, link, duplicate or encrypted entry")
        total += entry.file_size
        if len(entries) >= MAX_FILES or total > MAX_EXPANDED_BYTES:
            raise ValueError("Engine package exceeds its expansion limit")
        entries[name] = entry
        names.add(name.casefold())
    return entries


def validate_outputs(root: Path, architecture: str) -> None:
    recipe, _ = engine.load_recipe()
    output = root / "flutter/engine/src/out" / recipe["architectures"][architecture]
    receipt = root / f"engine-build-{architecture}.json"
    engine.verify_staged(output / "flutter_windows.dll", receipt, architecture)
    record = engine.read_json(receipt)
    if engine.verify_release_files(output, recipe) != record.get("release_file_sha256"):
        raise ValueError("Engine package assets differ from the source-build receipt")
    expected_tests = "passed" if architecture == "x64" else "not_run"
    if record.get("engine_tests") != expected_tests:
        raise ValueError("Engine package has no matching native engine test result")
    if file_digest(output / "flutter_windows.dll.pdb") != record.get("flutter_windows_pdb_sha256"):
        raise ValueError("Engine package symbols differ from the source-build receipt")
    for name in (
        "LICENSE.flutter",
        "flutter_windows.dll.exp",
        "dart-sdk/bin/dart.exe",
        "dart-sdk/bin/dartaotruntime.exe",
        "dart-sdk/bin/snapshots/frontend_server_aot.dart.snapshot",
        "shader_lib/flutter/runtime_effect.glsl",
    ):
        if not (output / name).is_file() or not (output / name).stat().st_size:
            raise ValueError(f"Engine package is missing a build support file: {name}")


def pack(build_root: Path, architecture: str, destination: Path) -> dict:
    """Copy only application build inputs, not source/dependency or object trees."""
    expected = identity(architecture)
    recipe, _ = engine.load_recipe()
    configuration = recipe["architectures"][architecture]
    output = build_root / "flutter/engine/src/out" / configuration
    receipt = build_root / f"engine-build-{architecture}.json"
    engine.verify_staged(output / "flutter_windows.dll", receipt, architecture)
    record = engine.read_json(receipt)
    if engine.verify_release_files(output, recipe) != record.get("release_file_sha256"):
        raise ValueError("Engine outputs differ from the source-build receipt")
    files = {receipt.name: receipt}
    prefix = f"flutter/engine/src/out/{configuration}/"
    files[prefix + "LICENSE.flutter"] = build_root / "flutter/engine/src/flutter/LICENSE"
    # Published headers, required tools, matching symbols, and their CRT support.
    names = set(recipe["required_release_files"]) | {
        "flutter_windows.dll.pdb",
        "flutter_windows.dll.exp",
    }
    names.update(path.name for path in output.glob("*.h"))
    names.update(
        path.name
        for pattern in ("msvcp*.dll", "vcruntime*.dll", "vccorlib*.dll", "ucrtbase.dll")
        for path in output.glob(pattern)
    )
    for name in names:
        files[prefix + name] = engine.relative_file(output, name)
    for directory in (
        "cpp_client_wrapper",
        "gen/dart-pkg/sky_engine",
        "flutter_patched_sdk",
        "shader_lib",
    ):
        for path in (output / directory).rglob("*"):
            if path.is_file() and not path.name.endswith(".d"):
                files[prefix + path.relative_to(output).as_posix()] = path
    # Flutter's local-engine resolver also needs the engine-matched prebuilt Dart
    # SDK. Flatten it into out/<variant>/dart-sdk, which is its preferred layout.
    sdk = build_root / f"flutter/engine/src/flutter/prebuilts/windows-{architecture}/dart-sdk"
    for path in sdk.rglob("*"):
        if path.is_file():
            files[prefix + "dart-sdk/" + path.relative_to(sdk).as_posix()] = path
    metadata = {
        **expected,
        "files": {name: file_digest(path) for name, path in sorted(files.items())},
    }
    destination.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(
        destination, "x", compression=zipfile.ZIP_DEFLATED, compresslevel=1
    ) as archive:
        for name, path in sorted(files.items()):
            archive.write(path, checked_name(name))
        archive.writestr(PACKAGE_METADATA, json.dumps(metadata, sort_keys=True))
    # Validate our own serialized package before it can be attested or uploaded.
    with tempfile.TemporaryDirectory(dir=destination.parent) as temporary:
        unpack(destination, architecture, Path(temporary) / "verify")
    return {
        **expected,
        "package_sha256": file_digest(destination),
        "size_bytes": destination.stat().st_size,
    }


def unpack(package: Path, architecture: str, destination: Path) -> dict:
    """Validate a package into a fresh directory; provenance is checked by restore."""
    expected = identity(architecture)
    if destination.exists() or destination.is_symlink():
        raise ValueError("Engine package destination must not exist")
    if package.stat().st_size > MAX_ARCHIVE_BYTES:
        raise ValueError("Engine package exceeds its archive size limit")
    destination.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(package) as archive:
        entries = archive_entries(archive)
        if PACKAGE_METADATA not in entries or entries[PACKAGE_METADATA].file_size > 1_000_000:
            raise ValueError("Engine package metadata is missing or oversized")
        metadata = json.loads(archive.read(PACKAGE_METADATA))
        if (
            not isinstance(metadata, dict)
            or type(metadata.get("schema")) is not int
            or any(metadata.get(k) != v for k, v in expected.items())
        ):
            raise ValueError("Engine package does not match the requested build inputs")
        hashes = metadata.get("files")
        if not isinstance(hashes, dict) or set(hashes) != set(entries) - {PACKAGE_METADATA}:
            raise ValueError("Engine package file inventory differs from its metadata")
        prefix = f"flutter/engine/src/out/{engine.load_recipe()[0]['architectures'][architecture]}/"
        for name, value in hashes.items():
            if (
                not (name.startswith(prefix) or name == f"engine-build-{architecture}.json")
                or not isinstance(value, str)
                or not re.fullmatch(r"[0-9a-f]{64}", value)
            ):
                raise ValueError("Engine package has an invalid file identity")
        with tempfile.TemporaryDirectory(
            prefix="engine-unpack-", dir=destination.parent
        ) as temporary:
            staged = Path(temporary) / "root"
            staged.mkdir()
            for name in hashes:
                target = engine.relative_file(staged, name)
                target.parent.mkdir(parents=True, exist_ok=True)
                with archive.open(entries[name]) as source, target.open("xb") as output:
                    shutil.copyfileobj(source, output, 1024 * 1024)
                if file_digest(target) != hashes[name]:
                    raise ValueError(f"Engine package file hash mismatch: {name}")
            validate_outputs(staged, architecture)
            (staged / PACKAGE_METADATA).write_text(json.dumps(metadata, indent=2), encoding="utf-8")
            # Windows scanners can briefly hold freshly extracted executables.
            # Retry only the final rename; never merge with an existing target.
            for attempt in range(20):
                try:
                    staged.rename(destination)
                    break
                except PermissionError:
                    if attempt == 19 or destination.exists():
                        raise
                    time.sleep(0.05)
    return expected


def gh(*arguments: str, output=None) -> str:
    executable = shutil.which("gh.exe" if os.name == "nt" else "gh")
    if executable is None:
        raise ValueError("GitHub CLI is required to restore a verified engine package")
    # Fixed GitHub CLI subcommands, no shell or user-supplied executable. Never
    # echo authentication variables or raw HTTP failure bodies into build logs.
    result = subprocess.run(  # noqa: S603 -- resolved executable and argument list
        [str(Path(executable).resolve()), *arguments],
        stdout=output if output is not None else subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=900,
        check=False,
    )
    if result.returncode:
        raise ValueError(f"GitHub engine package operation failed (exit {result.returncode})")
    return "" if output is not None else result.stdout.decode("utf-8")


def api(path: str) -> dict:
    result = json.loads(gh("api", "--hostname", "github.com", f"repos/{REPOSITORY}/{path}"))
    if not isinstance(result, dict):
        raise ValueError("Invalid GitHub engine package metadata")
    return result


def trusted_run(run: dict) -> bool:
    return (
        type(run.get("id")) is int
        and run.get("event") in {"push", "workflow_dispatch"}
        and run.get("status") == "completed"
        and run.get("conclusion") == "success"
        and run.get("head_branch") in TRUSTED_BRANCHES
        and run.get("path") == WORKFLOW
        and run.get("repository", {}).get("full_name") == REPOSITORY
        and run.get("head_repository", {}).get("full_name") == REPOSITORY
        and isinstance(run.get("head_sha"), str)
        and re.fullmatch(r"[0-9a-f]{40}", run["head_sha"]) is not None
    )


def restore(architecture: str, destination: Path) -> dict:
    expected = identity(architecture)
    # The exact name includes recipe and build/test/producer code, not the app
    # commit. Bound discovery to the newest 100 instances of this exact input set.
    listing = api(f"actions/artifacts?name={expected['artifact_name']}&per_page=100")
    for artifact in listing.get("artifacts", []):
        if (
            artifact.get("name") != expected["artifact_name"]
            or artifact.get("expired") is not False
        ):
            continue
        run_id = artifact.get("workflow_run", {}).get("id")
        if type(run_id) is not int or run_id <= 0:
            continue
        run = api(f"actions/runs/{run_id}")
        if not trusted_run(run):
            continue
        artifact_id = artifact.get("id")
        if type(artifact_id) is not int or artifact_id <= 0:
            raise ValueError("Invalid engine artifact ID")
        archive_digest = artifact.get("digest", "")
        if not re.fullmatch(r"sha256:[0-9a-f]{64}", archive_digest):
            raise ValueError("Engine artifact has no SHA256 identity")
        if not 0 < artifact.get("size_in_bytes", 0) <= MAX_ARCHIVE_BYTES:
            raise ValueError("Engine artifact exceeds its download size limit")
        destination.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(
            prefix="engine-download-", dir=destination.parent
        ) as temporary:
            outer = Path(temporary) / "artifact.zip"
            with outer.open("xb") as output:
                gh(
                    "api",
                    "--hostname",
                    "github.com",
                    f"repos/{REPOSITORY}/actions/artifacts/{artifact_id}/zip",
                    output=output,
                )
            if outer.stat().st_size > MAX_ARCHIVE_BYTES or file_digest(outer) != archive_digest[7:]:
                raise ValueError("Downloaded engine artifact differs from its immutable digest")
            package = Path(temporary) / "engine.zip"
            with zipfile.ZipFile(outer) as archive:
                entries = archive_entries(archive)
                if set(entries) != {"engine.zip"}:
                    raise ValueError("Engine artifact must contain exactly engine.zip")
                with archive.open("engine.zip") as source, package.open("xb") as output:
                    shutil.copyfileobj(source, output, 1024 * 1024)
            gh(
                "attestation",
                "verify",
                str(package),
                "--hostname",
                "github.com",
                "--repo",
                REPOSITORY,
                "--signer-workflow",
                f"{REPOSITORY}/{WORKFLOW}",
                "--source-ref",
                f"refs/heads/{run['head_branch']}",
                "--source-digest",
                run["head_sha"],
                "--signer-digest",
                run["head_sha"],
                "--deny-self-hosted-runners",
            )
            package_hash = file_digest(package)
            unpack(package, architecture, destination)
        provenance = {
            **expected,
            "restored": True,
            "artifact_id": artifact_id,
            "artifact_digest": archive_digest,
            "producer_commit": run["head_sha"],
            "producer_run_id": run_id,
            "package_sha256": package_hash,
        }
        (destination / "engine-restore.json").write_text(
            json.dumps(provenance, indent=2) + "\n", encoding="utf-8"
        )
        return provenance
    return {
        **expected,
        "restored": False,
        "reason": "No successful trusted engine package for these inputs",
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    for name in ("identity", "pack", "restore"):
        command = commands.add_parser(name)
        command.add_argument("--architecture", choices=sorted(engine.MACHINES), required=True)
        if name == "pack":
            command.add_argument("--build-root", type=Path, required=True)
            command.add_argument("--output", type=Path, required=True)
        elif name == "restore":
            command.add_argument("--destination", type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.command == "identity":
            result = identity(args.architecture)
        elif args.command == "pack":
            result = pack(args.build_root, args.architecture, args.output)
        else:
            result = restore(args.architecture, args.destination)
        print(json.dumps(result, indent=2))
        return 0
    except (OSError, ValueError, zipfile.BadZipFile, subprocess.TimeoutExpired) as error:
        print(f"Windows engine package validation failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
