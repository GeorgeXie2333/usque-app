"""Verify offline native source locks, shipped notices, and native SPDX entries."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parents[1]
METADATA = {"SOURCE.md", "SOURCE-FILES.sha256", "USQUE-PATCH.md"}
NOTICE = Path("apps/usque_gui/assets/licenses/vpngate.txt")
LICENSE_FILES = {
    "openvpn3": ["LICENSE.md", "LICENSES/MPL-2.0.txt", "openvpn/http/LICENSE_1_0.txt"],
    "mbedtls": ["LICENSE"],
    "asio": ["asio/LICENSE_1_0.txt", "asio/COPYING"],
    "lz4": ["lib/LICENSE"],
    "xxhash": ["LICENSE"],
}
HEADER_NOTICES = {
    "mbedtls": [
        "3rdparty/everest/include/everest/everest.h",
        "3rdparty/everest/library/Hacl_Curve25519.c",
        "3rdparty/p256-m/p256-m/p256-m.h",
    ],
}


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def relative_path(value: str) -> bool:
    path = PurePosixPath(value)
    return (
        bool(value)
        and not path.is_absolute()
        and ".." not in path.parts
        and "\\" not in value
        and ":" not in value
    )


def check_package(root: Path, package: dict) -> list[str]:
    """Reject unknown files and edits outside the explicitly reviewed patches."""
    directory_name = package["directory"]
    if not relative_path(directory_name) or not directory_name.startswith("third_party/"):
        return ["unsafe native source directory"]
    directory = root / directory_name
    if directory.is_symlink() or not directory.resolve().is_relative_to(root.resolve()):
        return [f"{directory_name}: symlink"]
    errors = []
    records = {}
    inventory = directory / "SOURCE-FILES.sha256"
    if not inventory.is_file() or inventory.is_symlink():
        return [f"{directory_name}: missing or linked source inventory"]
    for line in inventory.read_text(encoding="utf-8").splitlines():
        parts = line.split("  ", 1)
        if len(parts) != 2:
            return [f"{directory_name}: malformed source record"]
        sha, path = parts
        if (
            not relative_path(path)
            or len(sha) != 64
            or any(c not in "0123456789abcdef" for c in sha)
            or path in records
        ):
            return [f"{directory_name}: invalid or duplicate source record"]
        records[path] = sha
    if len(records) != package["files"]:
        errors.append(f"{directory_name}: source count differs from lock")
    patches = {patch["path"]: patch for patch in package.get("patches", [])}
    if len(patches) != len(package.get("patches", [])) or not set(patches) <= records.keys():
        errors.append(f"{directory_name}: invalid patch inventory")
    actual = set()
    for path in directory.rglob("*"):
        if path.is_symlink():
            errors.append(f"{path}: symlink")
        elif path.is_file():
            actual.add(path.relative_to(directory).as_posix())
    if actual - METADATA != records.keys():
        errors.append(f"{directory_name}: missing or extra source files")
    for path, upstream_sha in records.items():
        patched = patches.get(path)
        if patched and patched["upstream_sha256"] != upstream_sha:
            errors.append(f"{directory_name}/{path}: upstream patch hash differs")
        expected = patched["sha256"] if patched else upstream_sha
        source = directory / path
        if (
            not source.is_file()
            or source.is_symlink()
            or not source.resolve().is_relative_to(directory.resolve())
            or digest(source) != expected
        ):
            errors.append(f"{directory_name}/{path}: source hash differs")
    return errors


def notice_text(root: Path, packages: list[dict]) -> str:
    result = [
        "Usque embedded OpenVPN source and license notices\n",
        "Corresponding source, local patches and reproducible build instructions: "
        "https://github.com/GeorgeXie2333/usque-app\n",
        "OpenVPN 3 Core is used under MPL-2.0; Mbed TLS under Apache-2.0. "
        "Alternate licenses in upstream texts are reproduced as notices, not selected.\n",
    ]
    for package in packages:
        result.extend(
            [
                f"\n{package['name']} {package['version']} ({package['license']})\n",
                f"Source: {package['url']}\nArchive SHA-256: {package['sha256']}\n",
            ]
        )
        for name in LICENSE_FILES[package["name"]]:
            result.append(f"\n--- {name} ---\n")
            result.append(
                (root / package["directory"] / name).read_text(encoding="utf-8").rstrip() + "\n"
            )
        for name in HEADER_NOTICES.get(package["name"], []):
            header = (root / package["directory"] / name).read_text(encoding="utf-8")
            if not header.startswith("/*") or "*/" not in header:
                raise ValueError(f"Missing upstream notice in {name}")
            result.append(f"\n--- {name} ---\n{header.split('*/', 1)[0]}*/\n")
    return "".join(result)


def append_spdx(document: dict, packages: list[dict]) -> dict:
    """Record native libraries not discoverable from Cargo/Gradle lockfiles."""
    if document.get("spdxVersion") != "SPDX-2.3" or document.get("SPDXID") != "SPDXRef-DOCUMENT":
        raise ValueError("expected a Syft SPDX 2.3 document")
    existing = {entry["SPDXID"] for entry in document.get("packages", [])}
    for package in packages:
        reference = f"SPDXRef-usque-native-{package['name']}"
        if reference in existing:
            raise ValueError("native SPDX entry already exists")
        document.setdefault("packages", []).append(
            {
                "SPDXID": reference,
                "name": package["name"],
                "versionInfo": package["version"],
                "downloadLocation": package["url"],
                "filesAnalyzed": False,
                "licenseConcluded": package["license"],
                "licenseDeclared": package["license"],
                "copyrightText": "NOASSERTION",
                "checksums": [{"algorithm": "SHA256", "checksumValue": package["sha256"]}],
                "sourceInfo": f"Pinned source subset: {package['directory']}/SOURCE-FILES.sha256; local patches: tool/openvpn_sources.json in the matching Usque source revision.",
            }
        )
        document.setdefault("relationships", []).append(
            {
                "spdxElementId": "SPDXRef-DOCUMENT",
                "relationshipType": "DESCRIBES",
                "relatedSpdxElement": reference,
            }
        )
    return document


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write-notice", action="store_true")
    parser.add_argument("--append-sbom", type=Path)
    args = parser.parse_args()
    packages = json.loads((ROOT / "tool/openvpn_sources.json").read_text(encoding="utf-8"))
    errors = [error for package in packages for error in check_package(ROOT, package)]
    if errors:
        raise SystemExit("\n".join(errors))
    notice = notice_text(ROOT, packages)
    target = ROOT / NOTICE
    if args.write_notice:
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(notice, encoding="utf-8", newline="\n")
    elif not target.is_file() or target.read_text(encoding="utf-8") != notice:
        raise SystemExit("Native license asset differs; run --write-notice after review")
    if args.append_sbom:
        document = json.loads(args.append_sbom.read_text(encoding="utf-8"))
        args.append_sbom.write_text(
            json.dumps(append_spdx(document, packages), indent=2) + "\n",
            encoding="utf-8",
            newline="\n",
        )
    print(f"Verified {len(packages)} native source locks and license notices")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
