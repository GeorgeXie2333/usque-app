import json
import sys
import tempfile
import unittest
from pathlib import Path

from export_gradle_runtime_sbom import (
    MavenCoordinate,
    cyclone_dx_document,
    main,
    release_runtime_coordinates,
)


class GradleRuntimeSbomTests(unittest.TestCase):
    def test_selects_only_release_runtime_and_deduplicates(self) -> None:
        lockfile = """
# generated
com.example:runtime:1.2.3=debugRuntimeClasspath,releaseRuntimeClasspath
com.example:build-only:9.0.0=classpath,releaseLintChecksClasspath
com.example:runtime:1.2.3=releaseRuntimeClasspath
empty=releaseImplementationDependenciesMetadata
"""
        self.assertEqual(
            release_runtime_coordinates(lockfile),
            [MavenCoordinate("com.example", "runtime", "1.2.3")],
        )

    def test_rejects_missing_runtime_configuration(self) -> None:
        with self.assertRaisesRegex(ValueError, "no releaseRuntimeClasspath"):
            release_runtime_coordinates("com.example:tool:1.0=classpath\n")

    def test_rejects_malformed_runtime_coordinate(self) -> None:
        with self.assertRaisesRegex(ValueError, "group:artifact:version"):
            release_runtime_coordinates("not-a-coordinate=releaseRuntimeClasspath\n")

    def test_document_contains_maven_purl_and_configuration(self) -> None:
        document = cyclone_dx_document(
            [MavenCoordinate("org.example", "library", "2.0")], Path("gradle.lockfile")
        )
        component = document["components"][0]
        self.assertEqual(component["purl"], "pkg:maven/org.example/library@2.0")
        self.assertEqual(component["properties"][0]["value"], "releaseRuntimeClasspath")

    def test_cli_writes_valid_json(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            lockfile = root / "gradle.lockfile"
            output = root / "android-release-runtime.cdx.json"
            lockfile.write_text(
                "org.example:library:2.0=releaseRuntimeClasspath\n", encoding="utf-8"
            )
            original_argv = sys.argv
            try:
                sys.argv = ["export_gradle_runtime_sbom.py", str(lockfile), str(output)]
                self.assertEqual(main(), 0)
            finally:
                sys.argv = original_argv
            self.assertEqual(
                json.loads(output.read_text(encoding="utf-8"))["bomFormat"], "CycloneDX"
            )


if __name__ == "__main__":
    unittest.main()
