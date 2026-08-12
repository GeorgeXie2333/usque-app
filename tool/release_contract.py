#!/usr/bin/env python3
"""Fail-closed signed-artifact manifest contract for Usque releases."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

SCHEMA_VERSION = 1
TAG_PATTERN = re.compile(r"^v\d+\.\d+\.\d+(?:-beta\.\d+)?$")
SHA256_PATTERN = re.compile(r"^[0-9a-f]{64}$")
COMMIT_PATTERN = re.compile(r"^[0-9a-f]{40}$")

WINDOWS_VARIANTS = ("x64-v2", "arm64")
ANDROID_VARIANTS = ("arm64-v8a", "x86_64", "armeabi-v7a", "universal")


class ContractError(ValueError):
    """A release input violates the protected release contract."""


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def canonical_json_bytes(value: Any) -> bytes:
    return (
        json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")) + "\n"
    ).encode("utf-8")


def write_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(canonical_json_bytes(value))


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ContractError(f"cannot read JSON {path}: {error}") from error
    if not isinstance(value, dict):
        raise ContractError(f"{path} must contain one JSON object")
    return value


def expected_artifact_names(tag: str) -> dict[str, tuple[str, str]]:
    if not TAG_PATTERN.fullmatch(tag):
        raise ContractError(f"unsupported release tag: {tag}")
    expected: dict[str, tuple[str, str]] = {}
    for variant in WINDOWS_VARIANTS:
        expected[f"usque-{tag}-windows-{variant}.msi"] = ("windows", variant)
    for variant in ANDROID_VARIANTS:
        expected[f"usque-{tag}-android-{variant}.apk"] = ("android", variant)
    return expected


def normalize_sha256(value: Any, label: str) -> str:
    normalized = str(value).strip().lower()
    if not SHA256_PATTERN.fullmatch(normalized):
        raise ContractError(f"{label} must be one SHA-256 digest")
    return normalized


def normalize_commit(value: Any) -> str:
    normalized = str(value).strip().lower()
    if not COMMIT_PATTERN.fullmatch(normalized):
        raise ContractError("commit must be one full 40-character Git SHA")
    return normalized


def create_manifest(
    directory: Path,
    tag: str,
    commit: str,
    windows_signer: str,
    android_signer: str,
) -> dict[str, Any]:
    directory = directory.resolve(strict=True)
    expected = expected_artifact_names(tag)
    commit = normalize_commit(commit)
    windows_signer = normalize_sha256(windows_signer, "Windows signer")
    android_signer = normalize_sha256(android_signer, "Android signer")

    actual_primary = {
        path.name
        for path in directory.iterdir()
        if path.is_file() and path.suffix.lower() in {".msi", ".apk"}
    }
    missing = sorted(set(expected) - actual_primary)
    unexpected = sorted(actual_primary - set(expected))
    if missing or unexpected:
        raise ContractError(
            f"release artifact set mismatch; missing={missing}, unexpected={unexpected}"
        )

    artifacts = []
    checksum_lines = []
    for name in sorted(expected):
        path = directory / name
        digest = sha256_file(path)
        platform, variant = expected[name]
        artifacts.append(
            {
                "name": name,
                "platform": platform,
                "variant": variant,
                "sha256": digest,
                "size": path.stat().st_size,
            }
        )
        checksum_line = f"{digest} *{name}\n"
        (directory / f"{name}.sha256").write_text(checksum_line, encoding="utf-8")
        checksum_lines.append(checksum_line)

    (directory / "SHA256SUMS").write_text("".join(checksum_lines), encoding="utf-8")
    return {
        "schema_version": SCHEMA_VERSION,
        "tag": tag,
        "commit": commit,
        "created_at": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "signers": {
            "windows_certificate_sha256": windows_signer,
            "android_certificate_sha256": android_signer,
        },
        "artifacts": artifacts,
    }


def artifact_index(manifest: dict[str, Any]) -> dict[str, dict[str, Any]]:
    if manifest.get("schema_version") != SCHEMA_VERSION:
        raise ContractError("unsupported release manifest schema_version")
    expected = expected_artifact_names(str(manifest.get("tag", "")))
    normalize_commit(manifest.get("commit"))
    signers = manifest.get("signers")
    if not isinstance(signers, dict):
        raise ContractError("manifest signers must be an object")
    normalize_sha256(signers.get("windows_certificate_sha256"), "Windows manifest signer")
    normalize_sha256(signers.get("android_certificate_sha256"), "Android manifest signer")

    artifacts = manifest.get("artifacts")
    if not isinstance(artifacts, list):
        raise ContractError("manifest artifacts must be an array")
    index: dict[str, dict[str, Any]] = {}
    for artifact in artifacts:
        if not isinstance(artifact, dict):
            raise ContractError("manifest artifact entries must be objects")
        name = str(artifact.get("name", ""))
        if name in index:
            raise ContractError(f"duplicate manifest artifact: {name}")
        if name not in expected:
            raise ContractError(f"unexpected manifest artifact: {name}")
        platform, variant = expected[name]
        if artifact.get("platform") != platform or artifact.get("variant") != variant:
            raise ContractError(f"wrong platform or variant for {name}")
        normalize_sha256(artifact.get("sha256"), f"artifact {name}")
        if not isinstance(artifact.get("size"), int) or artifact["size"] <= 0:
            raise ContractError(f"artifact {name} has an invalid size")
        index[name] = artifact
    if set(index) != set(expected):
        raise ContractError("manifest does not contain the exact artifact set")
    return index


def verify_artifacts(directory: Path, manifest: dict[str, Any]) -> None:
    directory = directory.resolve(strict=True)
    index = artifact_index(manifest)
    for name, artifact in index.items():
        path = directory / name
        if not path.is_file():
            raise ContractError(f"artifact is missing: {name}")
        if path.stat().st_size != artifact["size"]:
            raise ContractError(f"artifact size mismatch: {name}")
        if sha256_file(path) != artifact["sha256"]:
            raise ContractError(f"artifact SHA-256 mismatch: {name}")


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)

    create = subparsers.add_parser("create-manifest")
    create.add_argument("--directory", type=Path, required=True)
    create.add_argument("--tag", required=True)
    create.add_argument("--commit", required=True)
    create.add_argument("--windows-signer-sha256", required=True)
    create.add_argument("--android-signer-sha256", required=True)
    create.add_argument("--output", type=Path, required=True)

    verify = subparsers.add_parser("verify-artifacts")
    verify.add_argument("--directory", type=Path, required=True)
    verify.add_argument("--manifest", type=Path, required=True)

    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    try:
        if args.command == "create-manifest":
            manifest = create_manifest(
                args.directory,
                args.tag,
                args.commit,
                args.windows_signer_sha256,
                args.android_signer_sha256,
            )
            write_json(args.output, manifest)
        elif args.command == "verify-artifacts":
            verify_artifacts(args.directory, load_json(args.manifest))
        else:  # pragma: no cover - argparse guarantees the command
            raise AssertionError(args.command)
    except ContractError as error:
        print(f"release contract rejected: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
