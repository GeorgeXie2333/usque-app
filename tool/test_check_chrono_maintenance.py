from __future__ import annotations

import copy
import unittest

from check_chrono_maintenance import validate_advisory


def reviewed_advisory() -> dict:
    return {
        "id": "RUSTSEC-2026-0543",
        "summary": "chrono is unmaintained",
        "details": (
            "The chrono crate is unmaintained; all versions are affected.\n\n"
            "Recommended alternatives:\n\n"
            "- [jiff](https://crates.io/crates/jiff)\n"
            "- [time](https://crates.io/crates/time)"
        ),
        "affected": [
            {
                "package": {"name": "chrono", "ecosystem": "crates.io"},
                "ranges": [{"type": "SEMVER", "events": [{"introduced": "0.0.0-0"}]}],
                "database_specific": {"informational": "unmaintained", "categories": []},
                "ecosystem_specific": {
                    "affected_functions": None,
                    "affects": {"functions": [], "arch": [], "os": []},
                },
            }
        ],
    }


class ChronoMaintenanceTests(unittest.TestCase):
    def test_accepts_only_the_reviewed_maintenance_notice(self) -> None:
        validate_advisory(reviewed_advisory())

    def test_changed_text_alias_severity_or_scope_is_rejected(self) -> None:
        for mutation in (
            {"details": "A security vulnerability"},
            {"aliases": ["CVE-2099-0001"]},
            {"severity": [{"type": "CVSS_V3", "score": "9.8"}]},
            {"affected": []},
            {"id": "RUSTSEC-other"},
            {"withdrawn": "2026-11-17T00:00:00Z"},
        ):
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                validate_advisory({**reviewed_advisory(), **mutation})

    def test_changed_affected_metadata_is_rejected(self) -> None:
        for field, value in (
            ("informational", "unsound"),
            ("severity", "HIGH"),
            ("cvss", "9.8"),
            ("categories", ["memory-corruption"]),
        ):
            advisory = copy.deepcopy(reviewed_advisory())
            advisory["affected"][0]["database_specific"][field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                validate_advisory(advisory)

    def test_changed_range_or_affected_function_is_rejected(self) -> None:
        for field in ("range", "function"):
            advisory = copy.deepcopy(reviewed_advisory())
            if field == "range":
                advisory["affected"][0]["ranges"][0]["events"].append({"fixed": "0.4.46"})
            else:
                advisory["affected"][0]["ecosystem_specific"]["affects"]["functions"] = ["parse"]
            with self.subTest(field=field), self.assertRaises(ValueError):
                validate_advisory(advisory)


if __name__ == "__main__":
    unittest.main()
