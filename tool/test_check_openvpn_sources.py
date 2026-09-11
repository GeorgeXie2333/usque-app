"""Native source and distribution metadata must fail closed on drift."""

import hashlib
import tempfile
import unittest
from pathlib import Path

from check_openvpn_sources import append_spdx, check_package, relative_path


class NativeSourcesTest(unittest.TestCase):
    def test_locked_subset_and_reviewed_patch(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            directory = root / "third_party/peer"
            directory.mkdir(parents=True)
            upstream = hashlib.sha256(b"original").hexdigest()
            patched = hashlib.sha256(b"reviewed").hexdigest()
            (directory / "peer.h").write_bytes(b"reviewed")
            (directory / "SOURCE-FILES.sha256").write_text(f"{upstream}  peer.h\n")
            package = {
                "directory": "third_party/peer",
                "files": 1,
                "patches": [{"path": "peer.h", "upstream_sha256": upstream, "sha256": patched}],
            }
            self.assertEqual(check_package(root, package), [])
            (directory / "peer.h").write_bytes(b"unreviewed")
            self.assertTrue(check_package(root, package))
            (directory / "peer.h").write_bytes(b"reviewed")
            (directory / "extra.c").write_bytes(b"extra")
            self.assertTrue(check_package(root, package))

    def test_paths_and_duplicate_sbom_entries_are_rejected(self):
        for value in ["../escape", "/absolute", "C:/absolute", "folder\\escape"]:
            self.assertFalse(relative_path(value))
        package = {
            "name": "peer",
            "directory": "third_party/peer",
            "version": "1.2.3",
            "license": "MPL-2.0",
            "url": "https://example.org/peer.tar",
            "sha256": "a" * 64,
        }
        document = append_spdx({"spdxVersion": "SPDX-2.3", "SPDXID": "SPDXRef-DOCUMENT"}, [package])
        self.assertEqual(document["packages"][0]["licenseConcluded"], "MPL-2.0")
        self.assertFalse(document["packages"][0]["filesAnalyzed"])
        self.assertEqual(
            document["relationships"][0]["relatedSpdxElement"], document["packages"][0]["SPDXID"]
        )
        with self.assertRaises(ValueError):
            append_spdx(document, [package])
        with self.assertRaises(ValueError):
            append_spdx({}, [package])


if __name__ == "__main__":
    unittest.main()
