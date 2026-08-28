"""Cross-language contracts for the reliability and diagnostics catalogues."""

from __future__ import annotations

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def _section(text: str, start: str, end: str) -> str:
    start_index = text.index(start)
    end_index = text.index(end, start_index)
    return text[start_index:end_index]


class ReliabilityCatalogueTest(unittest.TestCase):
    def test_every_rust_failure_code_has_a_chinese_flutter_title(self) -> None:
        rust = (ROOT / "crates/usque-core/src/failure.rs").read_text(encoding="utf-8")
        dart = (ROOT / "apps/usque_gui/lib/core/diagnostics_strings.dart").read_text(
            encoding="utf-8"
        )
        android = (
            ROOT
            / "apps/usque_gui/android/app/src/main/kotlin/io/github/georgexie2333/usque/AndroidMaintenance.kt"
        ).read_text(encoding="utf-8")
        rust_codes = set(
            re.findall(
                r'=> "([A-Z][A-Z0-9_]+)"',
                _section(rust, "pub const fn as_str", "pub const fn metadata"),
            )
        )
        chinese_codes = set(
            re.findall(
                r"'([A-Z][A-Z0-9_]+)'\s*:",
                _section(dart, "const Map<String, String> _zhFailures", "\n};"),
            )
        )
        android_codes = set(
            re.findall(
                r'"([A-Z][A-Z0-9_]+)"',
                _section(android, "private val FAILURE_CODES", "private val REMEDIATION_KEYS"),
            )
        )
        self.assertEqual(47, len(rust_codes))
        self.assertSetEqual(rust_codes, chinese_codes)
        self.assertSetEqual(rust_codes, android_codes)

    def test_check_ids_match_engine_android_and_flutter(self) -> None:
        rust = (ROOT / "crates/usque-engine/src/diagnostics/catalog.rs").read_text(encoding="utf-8")
        android = (
            ROOT
            / "apps/usque_gui/android/app/src/main/kotlin/io/github/georgexie2333/usque/AndroidDiagnosticsCoordinator.kt"
        ).read_text(encoding="utf-8")
        android_maintenance = (
            ROOT
            / "apps/usque_gui/android/app/src/main/kotlin/io/github/georgexie2333/usque/AndroidMaintenance.kt"
        ).read_text(encoding="utf-8")
        dart = (ROOT / "apps/usque_gui/lib/core/diagnostics_strings.dart").read_text(
            encoding="utf-8"
        )
        pattern = r'"((?:engine|frontend|physical|transport|tunnel|protection)\.[a-z0-9_]+)"'
        rust_ids = set(re.findall(pattern, rust))
        android_ids = set(re.findall(pattern, android))
        android_export_ids = set(
            re.findall(
                pattern,
                _section(
                    android_maintenance,
                    "private val CHECK_IDS",
                    "private val FAILURE_CODES",
                ),
            )
        )
        english_ids = set(
            re.findall(
                r"'((?:engine|frontend|physical|transport|tunnel|protection)\.[a-z0-9_]+)'\s*:",
                _section(dart, "const Map<String, String> _englishChecks", "\n};"),
            )
        )
        chinese_ids = set(
            re.findall(
                r"'((?:engine|frontend|physical|transport|tunnel|protection)\.[a-z0-9_]+)'\s*:",
                _section(dart, "const Map<String, String> _zhChecks", "\n};"),
            )
        )
        self.assertEqual(30, len(rust_ids))
        self.assertSetEqual(rust_ids, android_ids)
        self.assertSetEqual(rust_ids, android_export_ids)
        self.assertSetEqual(rust_ids, english_ids)
        self.assertSetEqual(rust_ids, chinese_ids)

    def test_export_summary_allowlist_covers_every_runner_summary(self) -> None:
        checks = (ROOT / "crates/usque-engine/src/diagnostics/checks.rs").read_text(
            encoding="utf-8"
        )
        maintenance = (ROOT / "crates/usque-engine/src/maintenance.rs").read_text(encoding="utf-8")
        runner_summaries = set(re.findall(r'"(diagnostic_[a-z0-9_]+)"', checks))
        export_summaries = set(
            re.findall(
                r'"(diagnostic_[a-z0-9_]+)"',
                _section(maintenance, "fn safe_summary_key", "\nfn safe_evidence"),
            )
        )
        self.assertTrue(runner_summaries)
        self.assertSetEqual(runner_summaries, export_summaries)


if __name__ == "__main__":
    unittest.main()
