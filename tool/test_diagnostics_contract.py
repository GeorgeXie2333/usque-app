"""Regression checks for public diagnostic contract generation and drift."""

import copy
import json
import re
import unittest

import generate_diagnostics_contract as generator


class DiagnosticsContractTests(unittest.TestCase):
    def setUp(self):
        self.contract = json.loads(generator.CONTRACT.read_text(encoding="utf-8"))

    def test_checked_in_projections_are_current(self):
        for language, contents in generator.render(self.contract).items():
            self.assertEqual(generator.OUTPUTS[language].read_text(encoding="utf-8"), contents)

    def test_failure_catalog_and_wire_events_are_covered(self):
        failure = (generator.ROOT / "crates/usque-core/src/failure.rs").read_text(encoding="utf-8")
        codes = set(re.findall(r'Self::\w+ => "([A-Z_0-9]+)"', failure))
        self.assertEqual(set(self.contract["failure_codes"]), codes)
        proto = (generator.ROOT / "proto/usque/v1/control.proto").read_text(encoding="utf-8")
        events = {
            value.lower()
            for value in re.findall(r"CONNECTION_EVENT_TYPE_(\w+)\s*=", proto)
            if value != "UNSPECIFIED"
        }
        self.assertLessEqual(events, set(self.contract["event_types"]))
        self.assertLessEqual(
            {"direct_dns_degraded", "direct_dns_recovered"}, set(self.contract["event_types"])
        )

    def test_windows_fixed_recovery_log_codes_remain_exportable(self):
        source = (generator.ROOT / "crates/usque-engine/src/windows_agent.rs").read_text(
            encoding="utf-8"
        )
        lease = source.split("let code = match &error", 1)[1].split("// Never log raw", 1)[0]
        recovery = source.split("let historical_terminal = matches!", 1)[1].split(
            "tracing::warn!", 1
        )[0]
        codes = set(re.findall(r'"([A-Z][A-Z0-9_]+)"', lease + recovery))
        self.assertTrue(codes)
        self.assertLessEqual(codes, set(self.contract["log_event_codes"]))

    def test_unsafe_duplicate_missing_and_cyclic_contracts_are_rejected(self):
        mutations = [
            lambda value: value["evidence_tokens"].append("private@example.com"),
            lambda value: value["event_types"].append(value["event_types"][0]),
            lambda value: value["checks"][0]["dependencies"].append("missing.check"),
            lambda value: value["checks"][0]["dependencies"].append(value["checks"][0]["id"]),
        ]
        for mutate in mutations:
            with self.subTest(mutation=mutate):
                value = copy.deepcopy(self.contract)
                mutate(value)
                with self.assertRaises(ValueError):
                    generator.validate(value)


if __name__ == "__main__":
    unittest.main()
