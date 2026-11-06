"""Identity checks for the source-built Windows Flutter engine.

This module never modifies an installed Flutter SDK. Build recipes describe
inputs; a successful source build and application validation are separate facts.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import struct
import subprocess
import tempfile
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parent.parent
RECIPE = ROOT / "tool" / "flutter_windows_engine_manifest.json"
MACHINES = {"x64": 0x8664, "arm64": 0xAA64}
ENGINE_REPOSITORY = "https://github.com/flutter/flutter.git"
DEPOT_REPOSITORY = "https://chromium.googlesource.com/chromium/tools/depot_tools.git"
MINIMUM_BUILD_FREE_BYTES = 40 * 1024**3


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def read_json(path: Path) -> dict:
    if path.stat().st_size > 1_000_000:
        raise ValueError(f"Engine metadata exceeds its size limit: {path}")
    data = json.loads(path.read_text(encoding="utf-8-sig"))
    if not isinstance(data, dict):
        raise ValueError(f"Expected a JSON object: {path}")
    return data


def relative_file(root: Path, name: str) -> Path:
    """Accept only plain, repository-relative manifest file paths."""
    relative = PurePosixPath(name)
    if (
        not name
        or "\\" in name
        or ":" in name
        or relative.is_absolute()
        or ".." in relative.parts
        or relative.as_posix() != name
    ):
        raise ValueError(f"Invalid relative file path: {name}")
    path = root.joinpath(*relative.parts)
    if not path.resolve().is_relative_to(root.resolve()):
        raise ValueError(f"File escapes its declared root: {name}")
    return path


def load_recipe(path: Path = RECIPE, repository: Path = ROOT) -> tuple[dict, str]:
    recipe = read_json(path)
    if type(recipe.get("schema")) is not int or recipe["schema"] != 1:
        raise ValueError("Unsupported Windows engine recipe schema")
    for key in ("framework_commit", "engine_commit", "depot_tools_commit"):
        if not re.fullmatch(r"[0-9a-f]{40}", recipe.get(key, "")):
            raise ValueError(f"Invalid revision pin: {key}")
    if (
        recipe.get("engine_repository") != ENGINE_REPOSITORY
        or recipe.get("depot_tools_repository") != DEPOT_REPOSITORY
    ):
        raise ValueError("Unexpected engine or depot_tools source repository")
    if recipe.get("architectures") != {"x64": "host_release", "arm64": "host_release_arm64"}:
        raise ValueError("Unexpected Windows engine architecture mapping")
    if not re.fullmatch(r"10\.0\.\d+\.0", recipe.get("windows_sdk_version", "")):
        raise ValueError("Invalid Windows SDK version")
    custom = recipe.get("gclient_custom_vars", {})
    if not custom or not all(
        isinstance(key, str) and re.fullmatch(r"[a-z_]+", key) and type(value) is bool
        for key, value in custom.items()
    ):
        raise ValueError("gclient custom variables must be named boolean switches")
    patches = recipe.get("patches", [])
    if [item.get("kind") for item in patches] != ["timer", "tests", "build_tools"]:
        raise ValueError("Expected timer, test and build-tool patches in that order")
    for item in patches:
        patch = relative_file(repository, item["path"])
        if digest(patch.read_bytes()) != item.get("sha256"):
            raise ValueError(f"Engine patch hash mismatch: {item['path']}")
    sources = recipe.get("patched_source_lf_sha256", {})
    if not sources:
        raise ValueError("The patched source identity map must not be empty")
    for name, value in sources.items():
        relative_file(repository, name)
        if not name.startswith("engine/src/") or not re.fullmatch(r"[0-9a-f]{64}", value):
            raise ValueError(f"Invalid patched source identity: {name}")
    required = recipe.get("required_release_files", [])
    if not required or len(set(required)) != len(required):
        raise ValueError("Required engine output paths must be nonempty and unique")
    for name in required:
        relative_file(repository, name)
    canonical = json.dumps(recipe, sort_keys=True, separators=(",", ":")).encode()
    return recipe, digest(canonical)


def create_build_directory(
    parents: list[Path], recipe_path: Path = RECIPE, repository: Path = ROOT
) -> dict:
    """Create a fresh owned root on a candidate volume with enough free space."""
    _, recipe_hash = load_recipe(recipe_path, repository)
    eligible = []
    observations = []
    for candidate in parents:
        try:
            parent = candidate.resolve(strict=True)
            if not parent.is_dir():
                raise ValueError("candidate is not a directory")
            free = shutil.disk_usage(parent).free
        except (OSError, ValueError) as error:
            observations.append(f"{candidate}: {error}")
            continue
        observations.append(f"{parent}: {free / 1024**3:.2f} GiB free")
        if free >= MINIMUM_BUILD_FREE_BYTES:
            eligible.append((free, parent))
    if not eligible:
        details = "; ".join(observations) or "no candidate directories supplied"
        raise ValueError(f"Engine source build needs at least 40 GiB free: {details}")
    # max preserves candidate order on a tie, preferring the normal runner temp.
    free, parent = max(eligible, key=lambda entry: entry[0])
    directory = Path(tempfile.mkdtemp(prefix="usque-flutter-engine-", dir=parent)).resolve()
    marker = {
        "Kind": "usque-flutter-engine-v1",
        "RecipeSha256": recipe_hash,
        "MinimumFreeBytes": MINIMUM_BUILD_FREE_BYTES,
        "FreeBytesBeforeCreation": free,
    }
    (directory / "usque-engine-build-root.json").write_text(
        json.dumps(marker, indent=2) + "\n", encoding="utf-8"
    )
    return {
        "path": str(directory),
        "selected_parent": str(parent),
        "minimum_free_bytes": MINIMUM_BUILD_FREE_BYTES,
        "available_free_bytes": free,
        "recipe_sha256": recipe_hash,
    }


def git_output(checkout: Path, *arguments: str) -> str:
    git = shutil.which("git.exe" if os.name == "nt" else "git")
    if git is None:
        raise ValueError("Git is required to verify the engine source checkout")
    # Only fixed read-only Git queries, resolved executable and argv list.
    environment = {
        key: value for key, value in os.environ.items() if not key.upper().startswith("GIT_")
    }
    environment.update(
        GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull, GIT_OPTIONAL_LOCKS="0"
    )
    result = subprocess.run(  # noqa: S603 -- no shell; caller supplies a local checkout
        [str(Path(git).resolve()), "-C", str(checkout), *arguments],
        check=True,
        capture_output=True,
        text=True,
        timeout=30,
        env=environment,
    )
    return result.stdout.strip()


def verify_source(checkout: Path, recipe: dict) -> dict:
    revision = git_output(checkout, "rev-parse", "HEAD")
    if revision != recipe["engine_commit"]:
        raise ValueError("Engine checkout revision differs from the recipe")
    expected = recipe["patched_source_lf_sha256"]
    changes = set(git_output(checkout, "diff", "HEAD", "--name-only").splitlines())
    if changes != set(expected):
        raise ValueError("Engine checkout has missing or unexpected tracked changes")
    raw_hashes = {}
    for name, expected_hash in expected.items():
        data = relative_file(checkout, name).read_bytes()
        if digest(data.replace(b"\r\n", b"\n")) != expected_hash:
            raise ValueError(f"Patched engine source differs: {name}")
        raw_hashes[name] = digest(data)
    return {"engine_commit": revision, "patched_source_raw_sha256": raw_hashes}


def verify_framework(checkout: Path, recipe: dict) -> dict:
    revision = git_output(checkout, "rev-parse", "HEAD")
    if revision != recipe["framework_commit"]:
        raise ValueError("Flutter framework checkout differs from the engine recipe")
    release_pin = checkout / "bin" / "internal" / "engine.version"
    if release_pin.read_text(encoding="utf-8").strip() != recipe["engine_commit"]:
        raise ValueError("Flutter framework expects a different engine revision")
    return {"framework_commit": revision, "required_engine_commit": recipe["engine_commit"]}


def pe_machine(path: Path) -> int:
    with path.open("rb") as source:
        header = source.read(64)
        if len(header) != 64 or header[:2] != b"MZ":
            raise ValueError(f"Not a PE file: {path}")
        offset = struct.unpack_from("<I", header, 0x3C)[0]
        if offset < 64 or offset > path.stat().st_size - 6:
            raise ValueError(f"Invalid PE header offset: {path}")
        source.seek(offset)
        signature = source.read(6)
        if signature[:4] != b"PE\0\0":
            raise ValueError(f"Invalid PE signature: {path}")
        return struct.unpack_from("<H", signature, 4)[0]


def verify_release_files(output: Path, recipe: dict) -> dict[str, str]:
    hashes = {}
    for name in recipe["required_release_files"]:
        path = relative_file(output, name)
        if not path.is_file() or path.stat().st_size == 0:
            raise ValueError(f"Required local-engine output is missing or empty: {name}")
        hashes[name] = digest(path.read_bytes())
    return hashes


def record_build(
    checkout: Path,
    architecture: str,
    destination: Path,
    tests_passed: bool,
    expected_recipe_hash: str,
) -> dict:
    """Record the output immediately after the build helper observes native exit 0."""
    recipe, recipe_hash = load_recipe()
    if recipe_hash != expected_recipe_hash:
        raise ValueError("Engine recipe changed while the build was running")
    source = verify_source(checkout, recipe)
    output = checkout / "engine" / "src" / "out" / recipe["architectures"][architecture]
    release_hashes = verify_release_files(output, recipe)
    dll = output / "flutter_windows.dll"
    if pe_machine(dll) != MACHINES[architecture]:
        raise ValueError("Built Flutter engine architecture differs from the requested target")
    tool_files = {
        "clang_cl": "engine/src/flutter/buildtools/windows-x64/clang/bin/clang-cl.exe",
        "lld_link": "engine/src/flutter/buildtools/windows-x64/clang/bin/lld-link.exe",
        "ninja": "third_party/ninja/ninja.exe",
        "deps": "DEPS",
    }
    record = {
        "schema": 1,
        "recipe_sha256": recipe_hash,
        "engine_commit": source["engine_commit"],
        "required_framework_commit": recipe["framework_commit"],
        "architecture": architecture,
        "source_build_exit_code": 0,
        "engine_tests": "passed" if tests_passed else "not_run",
        "windows_sdk_version": recipe["windows_sdk_version"],
        "flutter_windows_dll_sha256": digest(dll.read_bytes()),
        "release_file_sha256": release_hashes,
        "patched_source_raw_sha256": source["patched_source_raw_sha256"],
        "gn_args_sha256": digest((output / "args.gn").read_bytes()),
        "toolchain_environment_sha256": {
            name: digest((output / name).read_bytes())
            for name in ("environment.x86", "environment.x64", "environment.arm64")
            if (output / name).is_file()
        },
        "tool_sha256": {
            name: digest(relative_file(checkout, path).read_bytes())
            for name, path in tool_files.items()
        },
    }
    pdb = output / "flutter_windows.dll.pdb"
    if pdb.is_file():
        record["flutter_windows_pdb_sha256"] = digest(pdb.read_bytes())
    destination.write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
    return record


def verify_staged(
    dll: Path,
    build_manifest: Path,
    architecture: str,
    recipe_path: Path = RECIPE,
    repository: Path = ROOT,
) -> dict:
    recipe, recipe_hash = load_recipe(recipe_path, repository)
    record = read_json(build_manifest)
    if (
        type(record.get("schema")) is not int
        or record["schema"] != 1
        or record.get("recipe_sha256") != recipe_hash
    ):
        raise ValueError("Engine build manifest does not match the current recipe")
    if record.get("engine_commit") != recipe["engine_commit"]:
        raise ValueError("Engine build manifest has a different source revision")
    if record.get("required_framework_commit") != recipe["framework_commit"]:
        raise ValueError("Engine build manifest requires a different Flutter framework")
    if (
        architecture not in MACHINES
        or record.get("architecture") != architecture
        or pe_machine(dll) != MACHINES[architecture]
    ):
        raise ValueError("Staged Flutter engine has the wrong PE architecture")
    expected = record.get("flutter_windows_dll_sha256", "")
    if not re.fullmatch(r"[0-9a-f]{64}", expected) or digest(dll.read_bytes()) != expected:
        raise ValueError("Staged Flutter engine does not match the verified build output")
    if (
        type(record.get("source_build_exit_code")) is not int
        or record["source_build_exit_code"] != 0
    ):
        raise ValueError("Engine source build did not succeed")
    return {"recipe_sha256": recipe_hash, "architecture": architecture, "dll_sha256": expected}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("recipe")
    directory = commands.add_parser("create-build-directory")
    directory.add_argument("--parent", action="append", type=Path, required=True)
    source = commands.add_parser("verify-source")
    source.add_argument("checkout", type=Path)
    framework = commands.add_parser("verify-framework")
    framework.add_argument("checkout", type=Path)
    record = commands.add_parser("record-build")
    record.add_argument("checkout", type=Path)
    record.add_argument("--architecture", choices=sorted(MACHINES), required=True)
    record.add_argument("--output", type=Path, required=True)
    record.add_argument("--tests-passed", action="store_true")
    record.add_argument("--expected-recipe-sha256", required=True)
    staged = commands.add_parser("verify-staged")
    staged.add_argument("--dll", type=Path, required=True)
    staged.add_argument("--build-manifest", type=Path, required=True)
    staged.add_argument("--architecture", choices=sorted(MACHINES), required=True)
    args = parser.parse_args()
    if args.command == "record-build":
        result = record_build(
            args.checkout,
            args.architecture,
            args.output,
            args.tests_passed,
            args.expected_recipe_sha256,
        )
    elif args.command == "verify-staged":
        result = verify_staged(args.dll, args.build_manifest, args.architecture)
    elif args.command == "create-build-directory":
        result = create_build_directory(args.parent)
    else:
        recipe, recipe_hash = load_recipe()
        result = {"recipe": recipe, "recipe_sha256": recipe_hash}
        if args.command == "verify-source":
            result = verify_source(args.checkout, recipe)
        elif args.command == "verify-framework":
            result = verify_framework(args.checkout, recipe)
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
