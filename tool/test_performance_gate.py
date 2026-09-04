from __future__ import annotations

import copy
import hashlib
import json
import tempfile
import unittest
from pathlib import Path

import performance_gate

ROOT = Path(__file__).resolve().parent
FIXTURES = ROOT / "fixtures" / "performance"
SCENARIOS = ROOT / "performance_scenarios.json"
BUDGET = ROOT / "performance_budget.json"
SCHEMA = ROOT / "schemas" / "performance_report.schema.json"


class PerformanceGateTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.scenarios = performance_gate._load_scenarios(SCENARIOS)
        cls.scenario_by_id = {scenario["scenario_id"]: scenario for scenario in cls.scenarios}
        cls.budget = performance_gate._load_budget(BUDGET)
        cls.baseline = performance_gate.load_json(FIXTURES / "h3-small-dgram-baseline.json")
        cls.candidate = performance_gate.load_json(FIXTURES / "h3-small-dgram-candidate.json")

    def report_pair(self) -> tuple[dict, dict, dict]:
        return (
            copy.deepcopy(self.baseline),
            copy.deepcopy(self.candidate),
            copy.deepcopy(self.scenario_by_id["h3-small-dgram"]),
        )

    def test_checked_in_schema_and_fixture_reproduce_expected_result(self) -> None:
        performance_gate.validate_schema_contract(performance_gate.load_json(SCHEMA))
        expected = performance_gate.load_json(FIXTURES / "h3-small-dgram-expected.json")

        result = performance_gate.evaluate_pair(
            self.baseline,
            self.candidate,
            self.scenario_by_id["h3-small-dgram"],
            self.budget,
        )

        self.assertEqual(expected["gate_id"], result["gate_id"])
        self.assertEqual(expected["status"], result["status"])
        self.assertEqual(
            expected["baseline_goodput_median_bps"],
            result["summary"]["baseline"]["goodput_bps"]["median"],
        )
        self.assertEqual(
            expected["candidate_latency_median_of_p95_us"],
            result["summary"]["candidate"]["latency_p95_us"]["median"],
        )
        self.assertEqual(
            expected["candidate_syscalls_per_datagram"],
            result["summary"]["candidate"]["syscalls_per_datagram"]["median"],
        )
        self.assertEqual(
            expected["candidate_rss_peak_median_bytes"],
            result["summary"]["candidate"]["rss_peak_bytes"]["median"],
        )

    def test_malformed_and_non_finite_json_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "bad.json"
            for content in ('{"broken":', '{"value":NaN}', '{"value":Infinity}'):
                with self.subTest(content=content):
                    path.write_text(content, encoding="utf-8")
                    with self.assertRaises(performance_gate.GateError):
                        performance_gate.load_json(path)

    def test_extra_fields_and_wrong_unit_names_are_rejected(self) -> None:
        for field in ("hostname", "latency_p95_ms"):
            baseline, _, scenario = self.report_pair()
            baseline["samples"][0][field] = 1
            with self.subTest(field=field), self.assertRaises(performance_gate.GateError):
                performance_gate.validate_report(baseline, scenario, "baseline")

    def test_sensitive_device_fields_are_rejected(self) -> None:
        baseline, _, scenario = self.report_pair()
        baseline["environment"]["device_serial"] = "secret"
        with self.assertRaisesRegex(performance_gate.GateError, "sensitive field"):
            performance_gate.validate_report(baseline, scenario, "baseline")

    def test_exactly_seven_samples_are_required(self) -> None:
        for count in (6, 8):
            baseline, _, scenario = self.report_pair()
            if count == 6:
                baseline["samples"].pop()
            else:
                extra = copy.deepcopy(baseline["samples"][-1])
                extra["run_index"] = 8
                baseline["samples"].append(extra)
            with (
                self.subTest(count=count),
                self.assertRaisesRegex(performance_gate.GateError, "exactly 7"),
            ):
                performance_gate.validate_report(baseline, scenario, "baseline")

    def test_negative_values_are_rejected(self) -> None:
        baseline, _, scenario = self.report_pair()
        baseline["samples"][0]["cpu_time_ms"] = -1
        with self.assertRaisesRegex(performance_gate.GateError, "non-negative"):
            performance_gate.validate_report(baseline, scenario, "baseline")

    def test_baseline_candidate_contract_mismatches_are_rejected(self) -> None:
        mutations = (
            ("scenario_id", "other-scenario"),
            ("platform_class", "android-arm64"),
        )
        for field, value in mutations:
            baseline, candidate, scenario = self.report_pair()
            candidate[field] = value
            if field == "scenario_id":
                # Keep per-report validation valid so pair matching is the failure.
                scenario["scenario_id"] = value
                baseline["scenario_id"] = value
                baseline[field] = "h3-small-dgram"
            with self.subTest(field=field), self.assertRaises(performance_gate.GateError):
                performance_gate.evaluate_pair(baseline, candidate, scenario, self.budget)

        baseline, candidate, scenario = self.report_pair()
        candidate["environment"]["network_profile_id"] = "other-profile"
        scenario["network_profile_ids"].append("other-profile")
        with self.assertRaisesRegex(performance_gate.GateError, "network_profile_id mismatch"):
            performance_gate.evaluate_pair(baseline, candidate, scenario, self.budget)

        baseline, candidate, scenario = self.report_pair()
        candidate["environment"]["tool_versions"]["rustc"] = "2.0.0"
        with self.assertRaisesRegex(performance_gate.GateError, "major rustc"):
            performance_gate.evaluate_pair(baseline, candidate, scenario, self.budget)

    def test_unstable_samples_fail_instead_of_selecting_the_best_run(self) -> None:
        baseline, candidate, scenario = self.report_pair()
        for sample, value in zip(
            candidate["samples"],
            (800000, 800000, 800000, 1000000, 1200000, 1200000, 1200000),
            strict=True,
        ):
            sample["goodput_bps"] = value

        result = performance_gate.evaluate_pair(baseline, candidate, scenario, self.budget)

        self.assertEqual("unstable", result["status"])
        self.assertIn("candidate:throughput_mad", result["reason_codes"])

    def test_stability_threshold_boundaries_are_inclusive(self) -> None:
        baseline, candidate, scenario = self.report_pair()
        throughput = (900000, 900000, 900000, 1000000, 1100000, 1100000, 1100000)
        latency = (850, 850, 850, 1000, 1150, 1150, 1150)
        for sample, goodput, latency_p95 in zip(
            candidate["samples"], throughput, latency, strict=True
        ):
            sample["goodput_bps"] = goodput
            sample["latency_p95_us"] = latency_p95

        result = performance_gate.evaluate_pair(baseline, candidate, scenario, self.budget)

        self.assertEqual("passed", result["status"])
        self.assertEqual(
            0.1,
            result["summary"]["candidate"]["goodput_bps"]["mad_ratio"],
        )
        self.assertEqual(
            0.15,
            result["summary"]["candidate"]["latency_p95_us"]["mad_ratio"],
        )

    def test_steady_state_threshold_boundaries_are_inclusive(self) -> None:
        baseline, candidate, scenario = self.report_pair()
        scenario["feature_acceptance"] = False
        scenario["steady_metrics"] = [
            "throughput",
            "latency_p95_us",
            "cpu_per_bit",
            "rss_peak_bytes",
            "allocations_per_inner_packet",
            "queue_drop_packets",
        ]
        for sample in baseline["samples"]:
            sample["cpu_time_ms"] = 400
        for sample in candidate["samples"]:
            sample.update(
                {
                    "goodput_bps": 950000,
                    "latency_p95_us": 1100,
                    "latency_p99_us": 1600,
                    "cpu_time_ms": 399,
                    "rss_peak_bytes": 110000000,
                    "controlled_allocations": 210,
                    "queue_drop_packets": 0,
                }
            )

        result = performance_gate.evaluate_pair(baseline, candidate, scenario, self.budget)

        self.assertEqual("passed", result["status"])
        self.assertTrue(all(check["passed"] for check in result["checks"]))

        candidate["samples"][3]["latency_p95_us"] = 1101
        candidate["samples"][3]["latency_p99_us"] = 1601
        # Four runs establish a median just beyond the inclusive 110% boundary.
        for index in (0, 1, 2):
            candidate["samples"][index]["latency_p95_us"] = 1101
            candidate["samples"][index]["latency_p99_us"] = 1601
        result = performance_gate.evaluate_pair(baseline, candidate, scenario, self.budget)
        self.assertEqual("failed", result["status"])
        self.assertIn("steady.latency_p95_ratio", result["reason_codes"])

    def test_not_run_has_a_reason_and_never_passes(self) -> None:
        baseline, candidate, scenario = self.report_pair()
        candidate["status"] = "not_run"
        candidate["reason_code"] = "infrastructure_unavailable"
        candidate["samples"] = []

        result = performance_gate.evaluate_pair(baseline, candidate, scenario, self.budget)

        self.assertEqual("not_run", result["status"])
        self.assertEqual(["candidate:infrastructure_unavailable"], result["reason_codes"])

    def test_unknown_gate_in_scenarios_is_rejected(self) -> None:
        scenarios = performance_gate.load_json(SCENARIOS)
        scenarios["scenarios"][0]["gate_id"] = "performance.unknown"
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "scenarios.json"
            path.write_text(json.dumps(scenarios), encoding="utf-8")
            with self.assertRaisesRegex(performance_gate.GateError, "unknown performance gate"):
                performance_gate._load_scenarios(path)

    def test_directory_evaluation_emits_all_hashed_gate_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            measurements = root / "measurements"
            measurements.mkdir()
            for scenario in self.scenarios:
                baseline, candidate = self._passing_reports_for_scenario(scenario)
                for role, report in (("baseline", baseline), ("candidate", candidate)):
                    (measurements / f"{scenario['scenario_id']}-{role}.json").write_text(
                        json.dumps(report), encoding="utf-8"
                    )
            manifest = root / "release-manifest.json"
            manifest.write_text('{"candidate":true}\n', encoding="utf-8")
            output = root / "output"

            passed = performance_gate.evaluate_directory(
                measurements,
                SCENARIOS,
                BUDGET,
                SCHEMA,
                manifest,
                "b" * 40,
                "a" * 40,
                output,
            )

            self.assertTrue(passed)
            with self.assertRaisesRegex(performance_gate.GateError, "accepted baseline"):
                performance_gate.evaluate_directory(
                    measurements,
                    SCENARIOS,
                    BUDGET,
                    SCHEMA,
                    manifest,
                    "b" * 40,
                    "c" * 40,
                    root / "wrong-baseline-output",
                )
            report = json.loads((output / "report.json").read_text(encoding="utf-8"))
            self.assertEqual(
                performance_gate.PERFORMANCE_GATES, {item["id"] for item in report["results"]}
            )
            self.assertEqual(len(report["results"]), 7)
            for result in report["results"]:
                self.assertEqual("passed", result["status"])
                for field in ("performance_report", "raw_samples"):
                    reference = result["evidence"][field]
                    path = output / "evidence" / reference["path"]
                    self.assertEqual(
                        reference["sha256"], hashlib.sha256(path.read_bytes()).hexdigest()
                    )

    def test_h2_requires_both_profiles_in_configuration_and_raw_reports(self) -> None:
        h2 = [
            scenario
            for scenario in self.scenarios
            if scenario["gate_id"] == "performance.h2_high_bdp"
        ]
        pairs = [self._passing_reports_for_scenario(scenario) for scenario in h2]
        baselines, candidates = [pair[0] for pair in pairs], [pair[1] for pair in pairs]
        with self.assertRaisesRegex(performance_gate.GateError, "every required gate scenario"):
            performance_gate.evaluate_gate_reports(baselines[:1], candidates[:1], h2, self.budget)
        for sample in candidates[1]["samples"]:
            sample["goodput_bps"] = 1
        result = performance_gate.evaluate_gate_reports(baselines, candidates, h2, self.budget)
        self.assertEqual(result["status"], "failed")
        configured = performance_gate.load_json(SCENARIOS)
        configured["scenarios"] = [
            scenario
            for scenario in configured["scenarios"]
            if scenario["scenario_id"] != h2[1]["scenario_id"]
        ]
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "scenarios.json"
            path.write_text(json.dumps(configured), encoding="utf-8")
            with self.assertRaisesRegex(
                performance_gate.GateError, "separate single-flow and four-flow"
            ):
                performance_gate._load_scenarios(path)

    def test_evaluator_requires_the_explicit_accepted_baseline_argument(self) -> None:
        with self.assertRaises(SystemExit):
            performance_gate.parse_args(
                [
                    "evaluate",
                    "--measurements-directory",
                    "m",
                    "--scenarios",
                    "s",
                    "--budget",
                    "b",
                    "--schema",
                    "s",
                    "--candidate-manifest",
                    "m",
                    "--candidate-commit",
                    "b" * 40,
                    "--output-directory",
                    "o",
                ]
            )

    def _passing_reports_for_scenario(self, scenario: dict) -> tuple[dict, dict]:
        baseline = copy.deepcopy(self.baseline)
        candidate = copy.deepcopy(self.candidate)
        for role, report in (("baseline", baseline), ("candidate", candidate)):
            report["sample_role"] = role
            report["scenario_id"] = scenario["scenario_id"]
            report["platform_class"] = scenario["platform_classes"][0]
            report["environment"]["network_profile_id"] = scenario["network_profile_ids"][0]
            for sample in report["samples"]:
                if "pmtu_stable_ms" in scenario["required_sample_fields"]:
                    sample.update(
                        {
                            "pmtu_stable_ms": 1000,
                            "send_error_spin_count": 0,
                            "silent_truncation_packets": 0,
                        }
                    )
                if "migration_interruption_p95_us" in scenario["required_sample_fields"]:
                    sample.update(
                        {
                            "migration_interruption_p95_us": (
                                2000000 if role == "baseline" else 500000
                            ),
                            "fallback_completion_us": 1000000,
                            "full_reconnect_completion_us": 2000000,
                        }
                    )
                if "dns_queries" in scenario["required_sample_fields"]:
                    sample.update(
                        {
                            "dns_queries": 100,
                            "dns_successes": 100,
                            "physical_port_53_packets": 0,
                            "plaintext_fallback_queries": 0,
                        }
                    )
        if scenario["policy"] == "h2_high_bdp":
            for sample in candidate["samples"]:
                sample.update(
                    {"latency_p50_us": 500, "latency_p95_us": 1000, "latency_p99_us": 1500}
                )
        return baseline, candidate


if __name__ == "__main__":
    unittest.main()
