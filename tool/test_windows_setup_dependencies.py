"""Native setup dependencies must remain visible in the EXE inventory."""

import unittest

from windows_setup_dependencies import append_spdx, load_dependencies


class SetupDependencyTests(unittest.TestCase):
    def test_locks_and_original_license_match(self) -> None:
        packages = load_dependencies()
        self.assertEqual(len(packages), 2)
        for package in packages:
            self.assertRegex(package["sha256"], r"^[0-9a-f]{64}$")
            self.assertIn("/5.0.2/", package["url"])
            self.assertEqual(package["license"], "MS-RL")

    def test_inventory_contains_locked_checksums_licenses_and_source(self) -> None:
        packages = load_dependencies()
        document = append_spdx({"spdxVersion": "SPDX-2.3", "SPDXID": "SPDXRef-DOCUMENT"}, packages)
        self.assertEqual(len(document["relationships"]), 2)
        for source, entry in zip(packages, document["packages"], strict=True):
            self.assertEqual(entry["checksums"][0]["checksumValue"], source["sha256"])
            self.assertIn(source["source"], entry["sourceInfo"])
            self.assertEqual(entry["licenseDeclared"], "MS-RL")
            self.assertTrue(entry["externalRefs"][0]["referenceLocator"].startswith("pkg:nuget/"))
        with self.assertRaisesRegex(ValueError, "Duplicate"):
            append_spdx(document, packages)

    def test_wrong_inventory_format_is_rejected(self) -> None:
        with self.assertRaisesRegex(ValueError, "SPDX"):
            append_spdx({}, load_dependencies())


if __name__ == "__main__":
    unittest.main()
