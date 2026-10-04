"""Record the locked native setup libraries in installer EXE inventories."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LOCK = ROOT / "packaging/windows/setup/dependencies.json"
LICENSE_SHA256 = "dfdf2048787635215a6baf3b9d461dee89a2904d246787258a44f072a98d4786"


def load_dependencies() -> list[dict]:
    packages = json.loads(LOCK.read_text(encoding="utf-8"))
    if {p["name"] for p in packages} != {
        "WixToolset.BootstrapperApplicationApi",
        "WixToolset.DUtil",
    } or len(packages) != 2:
        raise ValueError("Unexpected native setup dependency set")
    if any(p["version"] != "5.0.2" or p["license"] != "MS-RL" for p in packages):
        raise ValueError("Native setup SDK does not match the pinned WiX tool")
    license_file = ROOT / "packaging/windows/setup/WIX-LICENSE.txt"
    if hashlib.sha256(license_file.read_bytes()).hexdigest() != LICENSE_SHA256:
        raise ValueError("The WiX license differs from the pinned upstream source")
    return packages


def append_spdx(document: dict, packages: list[dict]) -> dict:
    if document.get("spdxVersion") != "SPDX-2.3" or document.get("SPDXID") != "SPDXRef-DOCUMENT":
        raise ValueError("Expected a Syft SPDX 2.3 document")
    existing = {p["SPDXID"] for p in document.get("packages", [])}
    for package in packages:
        reference = "SPDXRef-usque-setup-" + package["name"].replace(".", "-")
        if reference in existing:
            raise ValueError("Duplicate native setup dependency")
        existing.add(reference)
        document.setdefault("packages", []).append(
            {
                "SPDXID": reference,
                "name": package["name"],
                "versionInfo": package["version"],
                "downloadLocation": package["url"],
                "filesAnalyzed": False,
                "licenseConcluded": package["license"],
                "licenseDeclared": package["license"],
                "copyrightText": "Copyright (c) .NET Foundation and contributors.",
                "checksums": [{"algorithm": "SHA256", "checksumValue": package["sha256"]}],
                "sourceInfo": "Unmodified native WiX libraries: " + package["source"],
                "externalRefs": [
                    {
                        "referenceCategory": "PACKAGE-MANAGER",
                        "referenceType": "purl",
                        "referenceLocator": f"pkg:nuget/{package['name']}@{package['version']}",
                    }
                ],
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


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--append-sbom", type=Path)
    args = parser.parse_args()
    packages = load_dependencies()
    if args.append_sbom:
        if not args.append_sbom.name.endswith(".exe.spdx.json"):
            raise ValueError("Native setup libraries belong only to the installer EXE inventory")
        document = json.loads(args.append_sbom.read_text(encoding="utf-8"))
        args.append_sbom.write_text(
            json.dumps(append_spdx(document, packages), indent=2) + "\n",
            encoding="utf-8",
            newline="\n",
        )
    print("Verified two native setup dependency locks and their license")


if __name__ == "__main__":
    main()
