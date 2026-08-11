"""Export locked Gradle release runtime dependencies as a CycloneDX SBOM.

Gradle's lockfile records build, lint, test, and runtime configurations in one
file. Scanning that file directly reports vulnerabilities in build-only tools
as though they were shipped in the APK. This exporter deliberately selects
only coordinates assigned to ``releaseRuntimeClasspath``.
"""

from __future__ import annotations

import argparse
import json
from dataclasses import dataclass
from pathlib import Path
from urllib.parse import quote


@dataclass(frozen=True, order=True)
class MavenCoordinate:
    group: str
    artifact: str
    version: str

    @property
    def purl(self) -> str:
        namespace = quote(self.group, safe=".-_~")
        name = quote(self.artifact, safe=".-_~")
        version = quote(self.version, safe=".-_~")
        return f"pkg:maven/{namespace}/{name}@{version}"


def release_runtime_coordinates(lockfile_text: str) -> list[MavenCoordinate]:
    """Return unique dependencies in Gradle's release runtime configuration."""

    coordinates: set[MavenCoordinate] = set()
    for line_number, raw_line in enumerate(lockfile_text.splitlines(), start=1):
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        try:
            raw_coordinate, raw_configurations = line.split("=", maxsplit=1)
        except ValueError as error:
            raise ValueError(f"line {line_number}: expected coordinate=configurations") from error

        configurations = {item.strip() for item in raw_configurations.split(",")}
        if "releaseRuntimeClasspath" not in configurations:
            continue

        parts = raw_coordinate.split(":")
        if len(parts) != 3 or not all(parts):
            raise ValueError(
                f"line {line_number}: expected Maven group:artifact:version coordinate"
            )
        coordinates.add(MavenCoordinate(*parts))

    if not coordinates:
        raise ValueError("lockfile contains no releaseRuntimeClasspath dependencies")
    return sorted(coordinates)


def cyclone_dx_document(coordinates: list[MavenCoordinate], source: Path) -> dict[str, object]:
    """Build a deterministic CycloneDX 1.5 document for OSV-Scanner."""

    components = []
    for coordinate in coordinates:
        components.append(
            {
                "type": "library",
                "bom-ref": coordinate.purl,
                "group": coordinate.group,
                "name": coordinate.artifact,
                "version": coordinate.version,
                "purl": coordinate.purl,
                "properties": [
                    {
                        "name": "usque:gradle-configuration",
                        "value": "releaseRuntimeClasspath",
                    }
                ],
            }
        )
    return {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "serialNumber": "urn:uuid:00000000-0000-4000-8000-000000000000",
        "version": 1,
        "metadata": {
            "component": {
                "type": "application",
                "name": "Usque Android release runtime",
                "version": "locked",
            },
            "properties": [{"name": "usque:source-lockfile", "value": source.as_posix()}],
        },
        "components": components,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("lockfile", type=Path)
    parser.add_argument("output", type=Path)
    arguments = parser.parse_args()

    coordinates = release_runtime_coordinates(arguments.lockfile.read_text(encoding="utf-8"))
    document = cyclone_dx_document(coordinates, arguments.lockfile)
    arguments.output.write_text(
        json.dumps(document, indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
        newline="\n",
    )
    print(f"Exported {len(coordinates)} release runtime dependencies to {arguments.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
