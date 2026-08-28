import hashlib
import json
import tempfile
import unittest
from pathlib import Path

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

    def _evidence_reference(self, environment: str, gate_id: str, field: str) -> dict[str, str]:
        suffix = "pcap" if field == "restricted_pcap" else "json"
        relative = Path(environment) / f"{gate_id.replace('.', '-')}-{field}.{suffix}"
        path = self.evidence / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(f"{gate_id}:{field}\n".encode())
        return {
            "path": relative.as_posix(),
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        }

    def _report(self, name: str, gate_ids: set[str]) -> Path:
        results = []
        for gate_id in sorted(gate_ids):
            evidence = {
                field: self._evidence_reference(name, gate_id, field)
                for field in ("junit", "timeline", "platform_diff")
            }
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
            "performance_lab": {"performance.informational_baseline"},
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
        self.assertIn("All 24 required gates passed", matrix.read_text())

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


if __name__ == "__main__":
    unittest.main()
