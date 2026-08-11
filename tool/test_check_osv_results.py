from __future__ import annotations

import json
import tempfile
import unittest
from datetime import date
from pathlib import Path

from check_osv_results import classify_vulnerability, summarize, validate_exceptions


class SeverityTests(unittest.TestCase):
    def test_calculates_high_cvss_vector(self) -> None:
        vulnerability = {
            "severity": [
                {
                    "type": "CVSS_V3",
                    "score": "CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:H",
                }
            ]
        }
        severity, score = classify_vulnerability(vulnerability)
        self.assertEqual("critical", severity)
        self.assertEqual(9.8, score)

    def test_uses_database_severity_label(self) -> None:
        severity, score = classify_vulnerability({"database_specific": {"severity": "HIGH"}})
        self.assertEqual("high", severity)
        self.assertIsNone(score)

    def test_unclassified_is_unknown(self) -> None:
        self.assertEqual(("unknown", None), classify_vulnerability({}))


class ExceptionTests(unittest.TestCase):
    def test_rejects_expired_exception(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            config = Path(directory) / "osv-scanner.toml"
            config.write_text(
                '[[IgnoredVulns]]\nid="GHSA-test"\n'
                'reason="Not reachable in the shipping binary."\nignoreUntil=2025-01-01\n',
                encoding="utf-8",
            )
            errors = validate_exceptions(config, today=date(2026, 1, 1))
            self.assertEqual(1, len(errors))
            self.assertIn("expired", errors[0])


class SummaryTests(unittest.TestCase):
    def test_low_is_summary_only_and_unknown_blocks(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "report.json"
            report.write_text(
                json.dumps(
                    {
                        "results": [
                            {
                                "source": {"path": "Cargo.lock"},
                                "packages": [
                                    {
                                        "package": {"name": "safe-ish", "version": "1.0"},
                                        "vulnerabilities": [
                                            {
                                                "id": "LOW-1",
                                                "database_specific": {"severity": "LOW"},
                                            },
                                            {"id": "UNKNOWN-1"},
                                        ],
                                    }
                                ],
                            }
                        ]
                    }
                ),
                encoding="utf-8",
            )
            lines, blockers = summarize(report, "Test", True)
            self.assertTrue(blockers)
            self.assertTrue(any("LOW-1" in line for line in lines))


if __name__ == "__main__":
    unittest.main()
