"""The native installer and uninstaller must ship the same complete copy."""

import json
import tempfile
import unittest
from pathlib import Path

from render_windows_setup_localization import ROOT, SOURCE, load_catalog, render_cpp


class SetupLocalizationTests(unittest.TestCase):
    def test_every_existing_installer_language_is_complete(self) -> None:
        catalog = load_catalog(SOURCE)
        expected = {path.stem for path in (ROOT / "packaging/windows/loc").glob("*.wxl")}
        self.assertEqual(set(catalog), expected)
        self.assertEqual(len(catalog), 21)
        self.assertGreaterEqual(len(catalog["en-US"]), 100)

    def test_ui_keys_exist_in_the_shared_catalog(self) -> None:
        import re

        catalog = load_catalog(SOURCE)
        sources = [
            ROOT / "packaging/windows/bootstrapper/main.cpp",
            ROOT / "packaging/windows/bootstrapper/state.h",
            ROOT / "crates/usque-uninstall/src/state.rs",
            ROOT / "crates/usque-uninstall/src/windows.rs",
            ROOT / "crates/usque-uninstall/src/windows/ui.rs",
        ]
        for path in sources:
            text = path.read_text(encoding="utf-8")
            keys = re.findall(r'\b(?:L|text|copy)\("([a-z][a-z0-9_]+)"\)', text)
            keys += re.findall(r'\b(?:setup_text|Text)\([^,]*,\s*"([a-z][a-z0-9_]+)"', text)
            keys += re.findall(
                r'(?:return|=>)\s*"((?:error|folder|uninstall|cancelled)_[a-z_]+)"',
                text,
            )
            for key in keys:
                self.assertIn(key, catalog["en-US"], f"{path.name}: {key}")

    def test_missing_translations_and_duplicates_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "strings.json"
            path.write_text(
                '{"en-US":{"language_name":"English","cancel":"Cancel"},'
                '"ja-JP":{"language_name":"日本語"}}',
                encoding="utf-8",
            )
            with self.assertRaisesRegex(ValueError, "Incomplete"):
                load_catalog(path)
            path.write_text(
                '{"en-US":{"language_name":"English","cancel":"Cancel","cancel":"No"}}',
                encoding="utf-8",
            )
            with self.assertRaisesRegex(ValueError, "Duplicate"):
                load_catalog(path)

    def test_placeholder_loss_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "strings.json"
            path.write_text(
                json.dumps(
                    {
                        "en-US": {"language_name": "English", "version": "Version {version}"},
                        "zh-CN": {"language_name": "简体中文", "version": "版本"},
                    },
                    ensure_ascii=False,
                ),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(ValueError, "Placeholder"):
                load_catalog(path)

    def test_header_escapes_quotes_newlines_and_keeps_unicode(self) -> None:
        value = 'Read "Usque"\n日本語\\Path'
        header = render_cpp({"en-US": {"language_name": "English", "details": value}})
        self.assertIn('L"Read \\"Usque\\"\\n日本語\\\\Path"', header)
        self.assertIn('text.culture == L"en-US"', header)


if __name__ == "__main__":
    unittest.main()
