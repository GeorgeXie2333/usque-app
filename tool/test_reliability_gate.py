import hashlib
import json
import tempfile
import unittest
from pathlib import Path

import performance_gate
import reliability_gate


class ReliabilityGateTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.reports = self.root / "reports"
        self.reports.mkdir()
        self.evidence = self.root / "evidence"
        self.evidence.mkdir()
        self.manifest = self.root / "release-manifest.json"
        self.manifest.write_text('{"candidate":true}\n', encoding="utf-8")
        self.commit = "a" * 40
        self.manifest_digest = hashlib.sha256(self.manifest.read_bytes()).hexdigest()

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def _evidence_reference(
        self,
        environment: str,
        gate_id: str,
        field: str,
        content: bytes | None = None,
    ) -> dict[str, str]:
        suffix = "pcap" if field == "restricted_pcap" else "json"
        relative = Path(environment) / f"{gate_id.replace('.', '-')}-{field}.{suffix}"
        path = self.evidence / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content or f"{gate_id}:{field}\n".encode())
        return {
            "path": relative.as_posix(),
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        }

    def _performance_evidence(self, gate_id: str) -> dict[str, dict[str, str]]:
        tool = Path(__file__).resolve().parent
        scenarios = {
            scenario["gate_id"]: scenario
            for scenario in performance_gate._load_scenarios(tool / "performance_scenarios.json")
        }
        scenario = scenarios[gate_id]
        baseline = performance_gate.load_json(
            tool / "fixtures" / "performance" / "h3-small-dgram-baseline.json"
        )
        candidate = performance_gate.load_json(
            tool / "fixtures" / "performance" / "h3-small-dgram-candidate.json"
        )
        for role, report in (("baseline", baseline), ("candidate", candidate)):
            report["sample_role"] = role
            report["scenario_id"] = scenario["scenario_id"]
            report["candidate_commit"] = self.commit
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
                    {
                        "latency_p50_us": 500,
                        "latency_p95_us": 1000,
                        "latency_p99_us": 1500,
                    }
                )
        comparison = performance_gate.evaluate_pair(
            baseline,
            candidate,
            scenario,
            performance_gate._load_budget(tool / "performance_budget.json"),
        )
        self.assertEqual("passed", comparison["status"])
        raw = {
            "schema_version": 2,
            "gate_id": gate_id,
            "baseline_report": baseline,
            "candidate_report": candidate,
        }
        raw_bytes = performance_gate.canonical_json_bytes(raw)
        raw_reference = self._evidence_reference(
            "performance_lab", gate_id, "raw_samples", raw_bytes
        )
        comparison.update(
            {
                "schema_version": 2,
                "baseline_commit": baseline["baseline_commit"],
                "candidate_commit": candidate["candidate_commit"],
                "baseline_report_sha256": hashlib.sha256(
                    performance_gate.canonical_json_bytes(baseline)
                ).hexdigest(),
                "candidate_report_sha256": hashlib.sha256(
                    performance_gate.canonical_json_bytes(candidate)
                ).hexdigest(),
                "raw_samples_sha256": raw_reference["sha256"],
            }
        )
        comparison_reference = self._evidence_reference(
            "performance_lab",
            gate_id,
            "performance_report",
            performance_gate.canonical_json_bytes(comparison),
        )
        return {
            "performance_report": comparison_reference,
            "raw_samples": raw_reference,
        }

    def _report(self, name: str, gate_ids: set[str]) -> Path:
        results = []
        for gate_id in sorted(gate_ids):
            evidence = {
                field: self._evidence_reference(name, gate_id, field)
                for field in ("junit", "timeline", "platform_diff")
            }
            if gate_id.startswith("performance."):
                evidence.update(self._performance_evidence(gate_id))
            if gate_id in reliability_gate.INDEPENDENT_GATES:
                evidence.update(
                    {
                        "observer": "external",
                        "zero_unexpected_packets": True,
                        "restricted_pcap": self._evidence_reference(
                            name, gate_id, "restricted_pcap"
                        ),
                    }
                )
            results.append({"id": gate_id, "status": "passed", "evidence": evidence})
        artifact_name = next(
            artifact
            for artifact, environment in reliability_gate.REPORT_ARTIFACT_ENVIRONMENTS.items()
            if environment == name
        )
        report_directory = self.reports / artifact_name
        report_directory.mkdir()
        report_path = report_directory / "report.json"
        report = {
            "schema_version": 1,
            "commit": self.commit,
            "candidate_manifest_sha256": self.manifest_digest,
            "environment": {
                "kind": name,
                "version": "test",
                "runner_class": reliability_gate.RUNNER_CLASS_BY_ENVIRONMENT[name],
            },
            "results": results,
        }
        report_path.write_text(
            json.dumps(report),
            encoding="utf-8",
        )
        return report_path

    def test_all_required_results_generate_release_evidence(self) -> None:
        groups = {
            "windows_snapshot_vm": {
                gate for gate in reliability_gate.REQUIRED_GATES if gate.startswith("windows.")
            },
            "android_physical_device": {
                gate for gate in reliability_gate.REQUIRED_GATES if gate.startswith("android.")
            },
            "independent_network_observer": {
                gate for gate in reliability_gate.REQUIRED_GATES if gate.startswith("network.")
            },
            "performance_lab": {
                gate for gate in reliability_gate.REQUIRED_GATES if gate.startswith("performance.")
            },
        }
        for name, gate_ids in groups.items():
            self._report(name, gate_ids)
        output = self.root / "reliability-report.json"
        matrix = self.root / "device-matrix.md"

        reliability_gate.aggregate(
            self.reports,
            self.evidence,
            self.manifest,
            self.commit,
            output,
            matrix,
        )

        self.assertEqual(json.loads(output.read_text())["status"], "passed")
        self.assertIn("All 30 required gates passed", matrix.read_text())

    def test_not_run_is_never_reported_as_passed(self) -> None:
        path = self._report("windows_snapshot_vm", {"windows.clean_install"})
        report = json.loads(path.read_text())
        report["results"][0]["status"] = "not_run"
        path.write_text(json.dumps(report), encoding="utf-8")

        with self.assertRaisesRegex(reliability_gate.GateError, "not_run"):
            reliability_gate.aggregate(
                self.reports,
                self.evidence,
                self.manifest,
                self.commit,
                self.root / "out.json",
                self.root / "matrix.md",
            )

    def test_independent_leak_gate_requires_external_zero_packet_evidence(self) -> None:
        path = self._report("independent_network_observer", {"network.dns_leak"})
        report = json.loads(path.read_text())
        report["results"][0]["evidence"]["observer"] = "engine"
        path.write_text(json.dumps(report), encoding="utf-8")

        with self.assertRaisesRegex(reliability_gate.GateError, "independently"):
            reliability_gate.aggregate(
                self.reports,
                self.evidence,
                self.manifest,
                self.commit,
                self.root / "out.json",
                self.root / "matrix.md",
            )

    def test_missing_evidence_file_is_rejected(self) -> None:
        path = self._report("windows_snapshot_vm", {"windows.clean_install"})
        report = json.loads(path.read_text())
        reference = report["results"][0]["evidence"]["timeline"]
        (self.evidence / reference["path"]).unlink()

        with self.assertRaisesRegex(reliability_gate.GateError, "missing or unsafe"):
            reliability_gate.aggregate(
                self.reports,
                self.evidence,
                self.manifest,
                self.commit,
                self.root / "out.json",
                self.root / "matrix.md",
            )

    def test_tampered_evidence_digest_is_rejected(self) -> None:
        path = self._report("windows_snapshot_vm", {"windows.clean_install"})
        report = json.loads(path.read_text())
        report["results"][0]["evidence"]["junit"]["sha256"] = "0" * 64
        path.write_text(json.dumps(report), encoding="utf-8")

        with self.assertRaisesRegex(reliability_gate.GateError, "digest does not match"):
            reliability_gate.aggregate(
                self.reports,
                self.evidence,
                self.manifest,
                self.commit,
                self.root / "out.json",
                self.root / "matrix.md",
            )

    def test_evidence_path_traversal_is_rejected(self) -> None:
        path = self._report("windows_snapshot_vm", {"windows.clean_install"})
        report = json.loads(path.read_text())
        report["results"][0]["evidence"]["platform_diff"]["path"] = "../escape.json"
        path.write_text(json.dumps(report), encoding="utf-8")

        with self.assertRaisesRegex(reliability_gate.GateError, "unsafe"):
            reliability_gate.aggregate(
                self.reports,
                self.evidence,
                self.manifest,
                self.commit,
                self.root / "out.json",
                self.root / "matrix.md",
            )

    def test_gate_from_wrong_environment_is_rejected(self) -> None:
        self._report("windows_snapshot_vm", {"android.airplane_mode"})

        with self.assertRaisesRegex(reliability_gate.GateError, "wrong environment"):
            reliability_gate.aggregate(
                self.reports,
                self.evidence,
                self.manifest,
                self.commit,
                self.root / "out.json",
                self.root / "matrix.md",
            )

    def test_unstable_performance_result_is_never_reported_as_passed(self) -> None:
        path = self._report("performance_lab", {"performance.h3_batch_io"})
        report = json.loads(path.read_text())
        report["results"][0]["status"] = "unstable"
        report["results"][0]["reason_code"] = "candidate:latency_mad"
        path.write_text(json.dumps(report), encoding="utf-8")

        with self.assertRaisesRegex(reliability_gate.GateError, "unstable"):
            reliability_gate.aggregate(
                self.reports,
                self.evidence,
                self.manifest,
                self.commit,
                self.root / "out.json",
                self.root / "matrix.md",
            )

    def test_tampered_performance_raw_artifact_is_rejected(self) -> None:
        path = self._report("performance_lab", {"performance.h3_batch_io"})
        report = json.loads(path.read_text())
        reference = report["results"][0]["evidence"]["raw_samples"]
        (self.evidence / reference["path"]).write_bytes(b"tampered\n")

        with self.assertRaisesRegex(reliability_gate.GateError, "digest does not match"):
            reliability_gate.aggregate(
                self.reports,
                self.evidence,
                self.manifest,
                self.commit,
                self.root / "out.json",
                self.root / "matrix.md",
            )

    def test_unknown_performance_gate_is_rejected(self) -> None:
        path = self._report("performance_lab", {"performance.h3_batch_io"})
        report = json.loads(path.read_text())
        report["results"][0]["id"] = "performance.unknown"
        path.write_text(json.dumps(report), encoding="utf-8")

        with self.assertRaisesRegex(reliability_gate.GateError, "unknown gate"):
            reliability_gate.aggregate(
                self.reports,
                self.evidence,
                self.manifest,
                self.commit,
                self.root / "out.json",
                self.root / "matrix.md",
            )

    def test_legacy_performance_alias_cannot_replace_a_v2_gate(self) -> None:
        path = self._report("performance_lab", {"performance.h3_batch_io"})
        report = json.loads(path.read_text())
        report["results"][0]["id"] = "performance.informational_baseline"
        path.write_text(json.dumps(report), encoding="utf-8")
        with self.assertRaisesRegex(reliability_gate.GateError, "unknown gate"):
            reliability_gate.aggregate(
                self.reports,
                self.evidence,
                self.manifest,
                self.commit,
                self.root / "out.json",
                self.root / "matrix.md",
            )


if __name__ == "__main__":
    unittest.main()
