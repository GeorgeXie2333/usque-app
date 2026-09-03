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
REPOSITORY_PATTERN = re.compile(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$")
RELEASE_TEMPLATE_TOKEN_PATTERN = re.compile(r"\{\{([^{}]+)\}\}")
HTTP_URL_PATTERN = re.compile(r"https?://[^\s<>)\]]+")
CARGO_WORKSPACE_PACKAGE_PATTERN = re.compile(
    r"^\[workspace\.package\]\s*$\n(.*?)(?=^\[|\Z)", re.MULTILINE | re.DOTALL
)
CARGO_VERSION_PATTERN = re.compile(r"^version\s*=\s*[\"']([^\"']+)[\"']\s*$", re.MULTILINE)
FLUTTER_VERSION_PATTERN = re.compile(r"^version:\s*([^\s+]+)\+([0-9]+)\s*$", re.MULTILINE)
APP_VERSION_PATTERN = re.compile(r"^\s*'app_version':\s*'Usque ([^']+)',\s*$", re.MULTILINE)
APP_VERSION_KEY_PATTERN = re.compile(r"^\s*['\"]app_version['\"]\s*:", re.MULTILINE)
LOCALE_IMPORT_LINE_PATTERN = re.compile(r"^[ \t]*import\b[^\r\n]*", re.MULTILINE)
LOCALE_IMPORT_PATTERN = re.compile(r"^import ['\"]([a-z][a-z0-9_]*\.dart)['\"];[ \t]*$")
RELEASE_TAG_ENV_PATTERN = re.compile(r"^  RELEASE_TAG:\s*[\"']?([^\"'\s]+)[\"']?\s*$", re.MULTILINE)
ANDROID_VERSION_CODE_ENV_PATTERN = re.compile(
    r"^  ANDROID_VERSION_CODE:\s*[\"']?([0-9]+)[\"']?\s*$", re.MULTILINE
)
RELEASE_TAG_TRIGGER_PATTERN = re.compile(
    r"^    tags:\s*\n      -\s*[\"']?([^\"'\s]+)[\"']?\s*$", re.MULTILINE
)

WINDOWS_VARIANTS = ("x64-v2", "arm64")
ANDROID_VARIANTS = ("arm64-v8a", "x86_64", "armeabi-v7a", "universal")
RELEASE_TEMPLATE_TOKENS = {
    "release_tag",
    "repository",
    "windows_signer_sha256",
    "android_signer_sha256",
}
RELEASE_NOTES_REQUIRED_HEADINGS = (
    "## Highlights / 更新亮点",
    "## Download / 下载",
    "## Verify before installing / 安装前验证",
    "## Feedback / 问题反馈",
)


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


def render_release_notes(
    template: Path,
    tag: str,
    repository: str,
    windows_signer: str,
    android_signer: str,
) -> str:
    """Render the curated bilingual release body without external links."""

    if not TAG_PATTERN.fullmatch(tag):
        raise ContractError(f"unsupported release tag: {tag}")
    repository = repository.strip()
    if not REPOSITORY_PATTERN.fullmatch(repository):
        raise ContractError(f"invalid GitHub repository: {repository!r}")
    windows_signer = normalize_sha256(windows_signer, "Windows signer")
    android_signer = normalize_sha256(android_signer, "Android signer")

    template_text = _read_text(template)
    tokens = set(RELEASE_TEMPLATE_TOKEN_PATTERN.findall(template_text))
    missing = sorted(RELEASE_TEMPLATE_TOKENS - tokens)
    unknown = sorted(tokens - RELEASE_TEMPLATE_TOKENS)
    if missing or unknown:
        raise ContractError(
            f"release-note template token mismatch; missing={missing}, unknown={unknown}"
        )
    malformed_delimiters = "{{" in RELEASE_TEMPLATE_TOKEN_PATTERN.sub("", template_text) or (
        "}}" in RELEASE_TEMPLATE_TOKEN_PATTERN.sub("", template_text)
    )
    if malformed_delimiters:
        raise ContractError("release-note template contains a malformed token")
    invalid_headings = [
        heading for heading in RELEASE_NOTES_REQUIRED_HEADINGS if template_text.count(heading) != 1
    ]
    if invalid_headings:
        raise ContractError(
            "release-note template must contain each required bilingual heading exactly once: "
            + ", ".join(invalid_headings)
        )

    repository_url = f"https://github.com/{repository}"
    replacements = {
        "release_tag": tag,
        "repository": repository,
        "windows_signer_sha256": windows_signer,
        "android_signer_sha256": android_signer,
    }
    rendered = RELEASE_TEMPLATE_TOKEN_PATTERN.sub(
        lambda match: replacements[match.group(1)], template_text
    )
    external_urls = sorted(
        {
            url
            for url in HTTP_URL_PATTERN.findall(rendered)
            if url != repository_url and not url.startswith(f"{repository_url}/")
        }
    )
    if external_urls:
        raise ContractError(
            "release notes may link only to the official repository: " + ", ".join(external_urls)
        )
    return rendered if rendered.endswith("\n") else rendered + "\n"


def _read_text(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except OSError as error:
        raise ContractError(f"cannot read {path}: {error}") from error


def _single_match(pattern: re.Pattern[str], text: str, label: str) -> str:
    matches = pattern.findall(text)
    if len(matches) != 1:
        raise ContractError(f"{label} must appear exactly once")
    match = matches[0]
    if isinstance(match, tuple):
        raise AssertionError(f"{label} unexpectedly has multiple capture groups")
    return match


def verify_release_version(root: Path, tag: str, android_version_code: int) -> None:
    """Require every shipping version surface to agree before a tag is pushed."""

    if not TAG_PATTERN.fullmatch(tag):
        raise ContractError(f"unsupported release tag: {tag}")
    if android_version_code < 1:
        raise ContractError("Android versionCode must be positive")
    root = root.resolve(strict=True)
    expected_version = tag.removeprefix("v")

    cargo_path = root / "Cargo.toml"
    cargo_sections = CARGO_WORKSPACE_PACKAGE_PATTERN.findall(_read_text(cargo_path))
    if len(cargo_sections) != 1:
        raise ContractError(f"{cargo_path} must contain one [workspace.package] section")
    cargo_version = _single_match(
        CARGO_VERSION_PATTERN, cargo_sections[0], "Cargo workspace package version"
    )
    if cargo_version != expected_version:
        raise ContractError(f"Cargo workspace version {cargo_version!r} does not match {tag!r}")

    pubspec_path = root / "apps" / "usque_gui" / "pubspec.yaml"
    pubspec_matches = FLUTTER_VERSION_PATTERN.findall(_read_text(pubspec_path))
    if len(pubspec_matches) != 1:
        raise ContractError(f"{pubspec_path} must contain one version name and build number")
    flutter_version, flutter_build = pubspec_matches[0]
    if flutter_version != expected_version:
        raise ContractError(f"Flutter version {flutter_version!r} does not match {tag!r}")
    if int(flutter_build) != android_version_code:
        raise ContractError(
            f"Android versionCode {flutter_build} does not match {android_version_code}"
        )

    locale_directory = root / "apps" / "usque_gui" / "lib" / "core" / "l10n"
    registry_path = locale_directory / "catalogs.dart"
    # Feature translation tables are not full catalogs. Use the app's registry,
    # while still rejecting missing files/versions and unsupported import syntax.
    locale_names = [
        _single_match(LOCALE_IMPORT_PATTERN, line, "locale catalog import")
        for line in LOCALE_IMPORT_LINE_PATTERN.findall(_read_text(registry_path))
    ]
    if not locale_names or len(set(locale_names)) != len(locale_names):
        raise ContractError(f"{registry_path} must register distinct locale catalogs")
    locale_paths = [locale_directory / name for name in sorted(locale_names)]
    invalid_locales = []
    for path in locale_paths:
        versions = APP_VERSION_PATTERN.findall(_read_text(path))
        if versions != [expected_version]:
            invalid_locales.append(path.name)
    if invalid_locales:
        raise ContractError(
            "locale app_version values do not match the release: " + ", ".join(invalid_locales)
        )

    # AppStrings consults feature tables before the catalog. They must never
    # override the version that was just verified, even with the same value.
    catalog_files = set(locale_names) | {"catalogs.dart"}
    for path in sorted(locale_directory.glob("*.dart")):
        if path.name not in catalog_files and APP_VERSION_KEY_PATTERN.search(_read_text(path)):
            raise ContractError(f"non-catalog translation table defines app_version: {path.name}")

    workflow_path = root / ".github" / "workflows" / "release.yml"
    workflow = _read_text(workflow_path)
    workflow_tag = _single_match(RELEASE_TAG_ENV_PATTERN, workflow, "release workflow RELEASE_TAG")
    trigger_tag = _single_match(
        RELEASE_TAG_TRIGGER_PATTERN, workflow, "release workflow tag trigger"
    )
    workflow_build = int(
        _single_match(
            ANDROID_VERSION_CODE_ENV_PATTERN,
            workflow,
            "release workflow ANDROID_VERSION_CODE",
        )
    )
    if workflow_tag != tag or trigger_tag != tag:
        raise ContractError(
            f"release workflow tags {workflow_tag!r}/{trigger_tag!r} do not match {tag!r}"
        )
    if workflow_build != android_version_code:
        raise ContractError(
            "release workflow ANDROID_VERSION_CODE "
            f"{workflow_build} does not match {android_version_code}"
        )


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

    version = subparsers.add_parser("verify-version")
    version.add_argument("--root", type=Path, required=True)
    version.add_argument("--tag", required=True)
    version.add_argument("--android-version-code", type=int, required=True)

    notes = subparsers.add_parser("render-release-notes")
    notes.add_argument("--template", type=Path, required=True)
    notes.add_argument("--tag", required=True)
    notes.add_argument("--repository", required=True)
    notes.add_argument("--windows-signer-sha256", required=True)
    notes.add_argument("--android-signer-sha256", required=True)
    notes.add_argument("--output", type=Path, required=True)

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
        elif args.command == "verify-version":
            verify_release_version(args.root, args.tag, args.android_version_code)
        elif args.command == "render-release-notes":
            notes = render_release_notes(
                args.template,
                args.tag,
                args.repository,
                args.windows_signer_sha256,
                args.android_signer_sha256,
            )
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_bytes(notes.encode("utf-8"))
        else:  # pragma: no cover - argparse guarantees the command
            raise AssertionError(args.command)
    except ContractError as error:
        print(f"release contract rejected: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
