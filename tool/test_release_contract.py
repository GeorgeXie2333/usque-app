from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

import release_contract


class ReleaseContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.tag = "v0.2.1"
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
        self.assertFalse((self.root / "SHA256SUMS").exists())
        for name in index:
            self.assertFalse((self.root / f"{name}.sha256").exists())

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


class ReleaseNotesContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.template = (
            Path(__file__).resolve().parent.parent / ".github" / "RELEASE_NOTES_TEMPLATE.md"
        )

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def render(self, template: Path | None = None) -> str:
        return release_contract.render_release_notes(
            template or self.template,
            "v9.8.7-beta.3",
            "Example/Usque",
            "b" * 64,
            "c" * 64,
        )

    def test_checked_in_template_renders_bilingual_official_links(self) -> None:
        rendered = self.render()

        self.assertNotIn("{{", rendered)
        self.assertIn("Usque v9.8.7-beta.3 official release", rendered)
        self.assertIn("usque-v9.8.7-beta.3-windows-x64-v2.msi", rendered)
        self.assertIn("usque-v9.8.7-beta.3-android-universal.apk", rendered)
        self.assertIn("`" + "b" * 64 + "`", rendered)
        self.assertIn("`" + "c" * 64 + "`", rendered)
        self.assertLess(rendered.index("Download packages only"), rendered.index("请仅从此"))

    def test_rejects_missing_or_unknown_template_tokens(self) -> None:
        invalid = Path(self.temporary.name) / "invalid.md"
        invalid.write_text(
            self.template.read_text(encoding="utf-8").replace(
                "{{android_signer_sha256}}", "{{sponsor_url}}"
            ),
            encoding="utf-8",
        )

        with self.assertRaises(release_contract.ContractError):
            self.render(invalid)

    def test_rejects_links_outside_the_official_repository(self) -> None:
        invalid = Path(self.temporary.name) / "external.md"
        invalid.write_text(
            self.template.read_text(encoding="utf-8")
            + "\n[External promotion](https://example.com/promo)\n",
            encoding="utf-8",
        )

        with self.assertRaises(release_contract.ContractError):
            self.render(invalid)


class ReleaseVersionContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        (self.root / ".github" / "workflows").mkdir(parents=True)
        (self.root / "apps" / "usque_gui" / "lib" / "core" / "l10n").mkdir(parents=True)
        (self.root / "Cargo.toml").write_text(
            '[workspace]\n[workspace.package]\nversion = "0.2.1"\n',
            encoding="utf-8",
        )
        (self.root / "apps" / "usque_gui" / "pubspec.yaml").write_text(
            "name: usque\nversion: 0.2.1+15\n", encoding="utf-8"
        )
        for name in ("en.dart", "zh_cn.dart"):
            (self.root / "apps" / "usque_gui" / "lib" / "core" / "l10n" / name).write_text(
                "const catalog = <String, String>{\n  'app_version': 'Usque 0.2.1',\n};\n",
                encoding="utf-8",
            )
        self.workflow_path = self.root / ".github" / "workflows" / "release.yml"
        self.workflow_path.write_text(
            "on:\n"
            "  push:\n"
            "    tags:\n"
            '      - "v0.2.1"\n'
            "env:\n"
            "  RELEASE_TAG: v0.2.1\n"
            '  ANDROID_VERSION_CODE: "15"\n',
            encoding="utf-8",
        )

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def test_accepts_consistent_release_version_surfaces(self) -> None:
        release_contract.verify_release_version(self.root, "v0.2.1", 15)

    def test_rejects_cargo_or_flutter_version_drift(self) -> None:
        (self.root / "Cargo.toml").write_text(
            '[workspace]\n[workspace.package]\nversion = "0.2.2"\n',
            encoding="utf-8",
        )
        with self.assertRaises(release_contract.ContractError):
            release_contract.verify_release_version(self.root, "v0.2.1", 15)

        (self.root / "Cargo.toml").write_text(
            '[workspace]\n[workspace.package]\nversion = "0.2.1"\n',
            encoding="utf-8",
        )
        (self.root / "apps" / "usque_gui" / "pubspec.yaml").write_text(
            "name: usque\nversion: 0.2.2+15\n", encoding="utf-8"
        )
        with self.assertRaises(release_contract.ContractError):
            release_contract.verify_release_version(self.root, "v0.2.1", 15)

    def test_rejects_locale_or_workflow_version_drift(self) -> None:
        locale = self.root / "apps" / "usque_gui" / "lib" / "core" / "l10n" / "en.dart"
        locale.write_text(
            "const catalog = <String, String>{\n  'app_version': 'Usque 0.2.0',\n};\n",
            encoding="utf-8",
        )
        with self.assertRaises(release_contract.ContractError):
            release_contract.verify_release_version(self.root, "v0.2.1", 15)

        locale.write_text(
            "const catalog = <String, String>{\n  'app_version': 'Usque 0.2.1',\n};\n",
            encoding="utf-8",
        )
        self.workflow_path.write_text(
            self.workflow_path.read_text(encoding="utf-8").replace(
                "RELEASE_TAG: v0.2.1", "RELEASE_TAG: v0.2.2"
            ),
            encoding="utf-8",
        )
        with self.assertRaises(release_contract.ContractError):
            release_contract.verify_release_version(self.root, "v0.2.1", 15)


if __name__ == "__main__":
    unittest.main()
