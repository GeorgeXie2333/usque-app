from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

import release_contract


class ReleaseContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.tag = "v0.1.3"
        self.commit = "a" * 40
        for name in release_contract.expected_artifact_names(self.tag):
            (self.root / name).write_bytes(name.encode())

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def test_manifest_requires_the_exact_six_artifacts(self) -> None:
        manifest = release_contract.create_manifest(
            self.root, self.tag, self.commit, "b" * 64, "c" * 64
        )
        index = release_contract.artifact_index(manifest)
        self.assertEqual(6, len(index))
        release_contract.verify_artifacts(self.root, manifest)

    def test_manifest_rejects_an_unexpected_primary_artifact(self) -> None:
        (self.root / "unexpected.apk").write_bytes(b"no")
        with self.assertRaises(release_contract.ContractError):
            release_contract.create_manifest(self.root, self.tag, self.commit, "b" * 64, "c" * 64)

    def test_artifact_tampering_is_rejected(self) -> None:
        manifest = release_contract.create_manifest(
            self.root, self.tag, self.commit, "b" * 64, "c" * 64
        )
        first = self.root / manifest["artifacts"][0]["name"]
        first.write_bytes(b"tampered")
        with self.assertRaises(release_contract.ContractError):
            release_contract.verify_artifacts(self.root, manifest)

    def test_manifest_rejects_invalid_signer_fingerprints(self) -> None:
        with self.assertRaises(release_contract.ContractError):
            release_contract.create_manifest(
                self.root, self.tag, self.commit, "not-a-digest", "c" * 64
            )

    def test_manifest_rejects_an_incomplete_artifact_index(self) -> None:
        manifest = release_contract.create_manifest(
            self.root, self.tag, self.commit, "b" * 64, "c" * 64
        )
        manifest["artifacts"].pop()
        with self.assertRaises(release_contract.ContractError):
            release_contract.artifact_index(manifest)


if __name__ == "__main__":
    unittest.main()
