"""Verify Linux Ed25519 release metadata without executing package contents."""

from __future__ import annotations

import json
import re
import shutil
import subprocess
import tempfile
from pathlib import Path

from linux_package import FORMATS, ROOT, SERVICE_FILES, artifact_name, digest


def unique_fields(pairs: list[tuple[str, object]]) -> dict:
    value = {}
    for key, item in pairs:
        if key in value:
            raise ValueError("Duplicate Linux manifest field")
        value[key] = item
    return value


def verify_signed_manifest(
    manifest_path: Path,
    *,
    architecture: str,
    version: str,
    kind: str = "packages",
    public_key: str | None = None,
) -> dict:
    if public_key is None:
        public_key = (ROOT / "packaging/linux/release-public-key.hex").read_text().strip()
    if re.fullmatch(r"[0-9a-f]{64}", public_key) is None:
        raise ValueError("Linux release public key is not configured")
    openssl = shutil.which("openssl")
    if openssl is None:
        raise ValueError("OpenSSL is required to verify Linux release signatures")
    signature = Path(str(manifest_path) + ".sig")
    if not 0 < manifest_path.stat().st_size <= 64 * 1024 or signature.stat().st_size != 64:
        raise ValueError("Linux signed manifest has an invalid length")
    with tempfile.TemporaryDirectory(prefix="usque-verify-linux-") as directory:
        public = Path(directory) / "public.der"
        public.write_bytes(bytes.fromhex("302a300506032b6570032100" + public_key))
        # Fixed tool and argument vector; only the checked-in public key is used
        # in the production release contract. Overrides support inert fixtures.
        result = subprocess.run(  # noqa: S603
            [
                openssl,
                "pkeyutl",
                "-verify",
                "-pubin",
                "-keyform",
                "DER",
                "-inkey",
                str(public),
                "-rawin",
                "-in",
                str(manifest_path),
                "-sigfile",
                str(signature),
            ],
            capture_output=True,
            check=False,
        )
        if result.returncode:
            raise ValueError("Linux manifest signature verification failed")
    value = json.loads(manifest_path.read_bytes(), object_pairs_hook=unique_fields)
    if not isinstance(value, dict) or set(value) != {
        "schema_version",
        "kind",
        "version",
        "architecture",
        "protocol_version",
        "entries",
    }:
        raise ValueError("Linux manifest contains unknown or missing fields")
    if (
        type(value["schema_version"]) is not int
        or value["schema_version"] != 1
        or type(value["protocol_version"]) is not int
        or value["protocol_version"] != 3
        or value["architecture"] != architecture
        or value["version"] != version
        or value["kind"] != kind
    ):
        raise ValueError("Linux manifest candidate, protocol or platform mismatch")
    if not isinstance(value["entries"], list) or len(value["entries"]) > len(SERVICE_FILES):
        raise ValueError("Invalid Linux manifest entries")
    expected = (
        {artifact_name(version, architecture, extension) for extension in FORMATS}
        if kind == "packages"
        else set(SERVICE_FILES)
        if kind == "service"
        else set()
    )
    actual = set()
    for item in value["entries"]:
        if (
            not isinstance(item, dict)
            or set(item) != {"name", "size", "sha256"}
            or not isinstance(item["name"], str)
            or item["name"] in actual
            or type(item["size"]) is not int
            or not 0 < item["size"] <= 512 * 1024 * 1024
            or not isinstance(item["sha256"], str)
            or re.fullmatch(r"[0-9a-f]{64}", item["sha256"]) is None
        ):
            raise ValueError("Linux manifest entry is invalid or duplicated")
        actual.add(item["name"])
    if not expected or actual != expected:
        raise ValueError("Linux manifest does not contain the exact payload set")
    return value


def verify_linux_release(directory: Path, tag: str) -> str:
    if (directory / "TEST-IDENTITY-DO-NOT-PUBLISH").exists():
        raise ValueError("Compile-only Linux packages cannot be released")
    for architecture in ("amd64", "arm64"):
        path = directory / f"usque-{tag}-linux-{architecture}.manifest.json"
        value = verify_signed_manifest(
            path, architecture=architecture, version=tag.removeprefix("v")
        )
        for item in value["entries"]:
            payload = directory / item["name"]
            if (
                payload.is_symlink()
                or payload.stat().st_size != item["size"]
                or digest(payload) != item["sha256"]
            ):
                raise ValueError("Linux package differs from its signed metadata")
    return (ROOT / "packaging/linux/release-public-key.hex").read_text().strip()
