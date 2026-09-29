"""Cross-language contracts for the reliability and diagnostics catalogues."""

from __future__ import annotations

import json
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def _contract() -> dict:
    return json.loads((ROOT / "proto/usque/diagnostics-contract.json").read_text(encoding="utf-8"))


def _section(text: str, start: str, end: str) -> str:
    start_index = text.index(start)
    end_index = text.index(end, start_index)
    return text[start_index:end_index]


class ReliabilityCatalogueTest(unittest.TestCase):
    def test_every_rust_failure_code_has_a_chinese_flutter_title(self) -> None:
        rust = (ROOT / "crates/usque-core/src/failure.rs").read_text(encoding="utf-8")
        chinese_catalog = (ROOT / "apps/usque_gui/lib/core/l10n/zh_cn.dart").read_text(
            encoding="utf-8"
        )
        android = (
            ROOT
            / "apps/usque_gui/android/app/src/main/kotlin/io/github/georgexie2333/usque/DiagnosticsContract.kt"
        ).read_text(encoding="utf-8")
        rust_codes = set(
            re.findall(
                r'=> "([A-Z][A-Z0-9_]+)"',
                _section(rust, "pub const fn as_str", "pub const fn metadata"),
            )
        )
        chinese_codes = set(
            re.findall(
                r"'diag_fail_([A-Z][A-Z0-9_]+)'\s*:",
                chinese_catalog,
            )
        )
        android_codes = set(
            re.findall(
                r'"([A-Z][A-Z0-9_]+)"',
                _section(android, "val failureCodes", "val summaryKeys"),
            )
        )
        self.assertEqual(54, len(rust_codes))
        self.assertSetEqual(rust_codes, chinese_codes)
        self.assertSetEqual(rust_codes, android_codes)
        self.assertSetEqual(rust_codes, set(_contract()["failure_codes"]))

    def test_check_ids_match_engine_android_and_flutter(self) -> None:
        rust = (ROOT / "crates/usque-engine/src/diagnostics/catalog.rs").read_text(encoding="utf-8")
        rust += (ROOT / "crates/usque-engine/src/diagnostics/probes.rs").read_text(encoding="utf-8")
        android = (
            ROOT
            / "apps/usque_gui/android/app/src/main/kotlin/io/github/georgexie2333/usque/AndroidDiagnosticsCoordinator.kt"
        ).read_text(encoding="utf-8")
        android += (
            ROOT
            / "apps/usque_gui/android/app/src/main/kotlin/io/github/georgexie2333/usque/NetworkDiagnosticChecks.kt"
        ).read_text(encoding="utf-8")
        android_maintenance = (
            ROOT
            / "apps/usque_gui/android/app/src/main/kotlin/io/github/georgexie2333/usque/AndroidMaintenance.kt"
        ).read_text(encoding="utf-8")
        english_catalog = (ROOT / "apps/usque_gui/lib/core/l10n/en.dart").read_text(
            encoding="utf-8"
        )
        chinese_catalog = (ROOT / "apps/usque_gui/lib/core/l10n/zh_cn.dart").read_text(
            encoding="utf-8"
        )
        feature_catalog = (ROOT / "apps/usque_gui/lib/core/l10n/network_quality.dart").read_text(
            encoding="utf-8"
        )
        english_catalog += _section(
            feature_catalog, "const kNetworkQualityEn", "const kNetworkQualityZhCn"
        )
        chinese_catalog += feature_catalog[feature_catalog.index("const kNetworkQualityZhCn") :]
        categories = "engine|frontend|physical|transport|tunnel|protection|quality|dns"
        pattern = rf'"((?:{categories})\.[a-z0-9_]+)"'
        rust_ids = {check["id"] for check in _contract()["checks"]}
        self.assertIn("CHECK_DEFINITIONS", rust)
        android_ids = set(re.findall(pattern, android))
        self.assertIn("CHECK_IDS = DiagnosticsContract.checkIds", android_maintenance)
        generated_android = (
            ROOT
            / "apps/usque_gui/android/app/src/main/kotlin/io/github/georgexie2333/usque/DiagnosticsContract.kt"
        ).read_text(encoding="utf-8")
        android_export_ids = set(
            re.findall(pattern, _section(generated_android, "val checkIds", "val eventTypes"))
        )
        expected_catalog_keys = {
            f"diag_check_{check_id.replace('.', '_')}" for check_id in rust_ids
        }
        catalog_key_pattern = rf"'(diag_check_(?:{categories})_[a-z0-9_]+)'\s*:"
        english_keys = set(
            re.findall(
                catalog_key_pattern,
                english_catalog,
            )
        )
        chinese_keys = set(
            re.findall(
                catalog_key_pattern,
                chinese_catalog,
            )
        )
        self.assertEqual(39, len(rust_ids))
        self.assertSetEqual(rust_ids, android_ids)
        self.assertSetEqual(rust_ids, android_export_ids)
        self.assertSetEqual(expected_catalog_keys, english_keys)
        self.assertSetEqual(expected_catalog_keys, chinese_keys)

    def test_export_summary_allowlist_covers_every_runner_summary(self) -> None:
        checks = (ROOT / "crates/usque-engine/src/diagnostics/checks.rs").read_text(
            encoding="utf-8"
        )
        for module in ("quality", "probes"):
            checks += (ROOT / f"crates/usque-engine/src/diagnostics/{module}.rs").read_text(
                encoding="utf-8"
            )
        maintenance = (ROOT / "crates/usque-engine/src/maintenance.rs").read_text(encoding="utf-8")
        summary_pattern = r'"((?:diagnostic_|nq_finding_)[a-z0-9_]+)"'
        runner_summaries = set(re.findall(summary_pattern, checks))
        self.assertRegex(
            _section(maintenance, "fn safe_summary_key", "\nfn safe_evidence"),
            r"diagnostics_contract_generated::SUMMARY_KEYS\s*\.contains",
        )
        export_summaries = set(_contract()["summary_keys"])
        self.assertTrue(runner_summaries)
        # Retired identifiers remain accepted for older saved diagnostics. New
        # runner results must be covered; append-only compatibility permits a
        # strict superset rather than deleting legacy entries to satisfy a test.
        self.assertLessEqual(runner_summaries, export_summaries)


if __name__ == "__main__":
    unittest.main()
