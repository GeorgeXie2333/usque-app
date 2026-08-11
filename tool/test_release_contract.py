from __future__ import annotations

import hashlib
import json
import tempfile
import unittest
from pathlib import Path

import release_contract


def digest(label: str) -> str:
    return hashlib.sha256(label.encode()).hexdigest()


class ReleaseContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.tag = "v0.1.1-beta.2"
        self.commit = "a" * 40
        for name in release_contract.expected_artifact_names(self.tag):
            (self.root / name).write_bytes(name.encode())

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def test_manifest_requires_the_exact_two_artifacts(self) -> None:
        manifest = release_contract.create_manifest(
            self.root, self.tag, self.commit, "b" * 64, "c" * 64
        )
        index = release_contract.artifact_index(manifest)
        self.assertEqual(2, len(index))
        release_contract.verify_artifacts(self.root, manifest)

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

    def test_ready_requires_thresholds_and_complete_device_matrix(self) -> None:
        manifest = release_contract.create_manifest(
            self.root, self.tag, self.commit, "b" * 64, "c" * 64
        )
        manifest_path = self.root / "release-manifest.json"
        release_contract.write_json(manifest_path, manifest)
        manifest_sha = release_contract.sha256_file(manifest_path)
        windows_path = self.root / "windows-evidence.json"
        android_path = self.root / "android-evidence.json"
        release_contract.write_json(
            windows_path,
            self._evidence(manifest, manifest_sha, "windows"),
        )
        release_contract.write_json(
            android_path,
            self._evidence(manifest, manifest_sha, "android"),
        )

        ready = release_contract.create_ready(manifest_path, windows_path, android_path)
        self.assertTrue(ready["ready"])
        self.assertEqual(self.tag, ready["tag"])

        evidence = json.loads(windows_path.read_text())
        evidence["tests"]["performance.oracle_thresholds"]["candidate_throughput_mbps"] = 89
        release_contract.write_json(windows_path, evidence)
        with self.assertRaises(release_contract.ContractError):
            release_contract.create_ready(manifest_path, windows_path, android_path)

    def test_ready_rejects_a_proxy_ratio_below_the_rtt_gate(self) -> None:
        manifest = release_contract.create_manifest(
            self.root, self.tag, self.commit, "b" * 64, "c" * 64
        )
        manifest_path = self.root / "release-manifest.json"
        release_contract.write_json(manifest_path, manifest)
        manifest_sha = release_contract.sha256_file(manifest_path)
        windows_path = self.root / "windows-evidence.json"
        android_path = self.root / "android-evidence.json"
        windows = self._evidence(manifest, manifest_sha, "windows")
        windows["tests"]["performance.oracle_thresholds"]["proxy_throughput_mbps"][1][
            "http_connect_four_mbps"
        ] = 89
        release_contract.write_json(windows_path, windows)
        release_contract.write_json(android_path, self._evidence(manifest, manifest_sha, "android"))
        with self.assertRaises(release_contract.ContractError):
            release_contract.create_ready(manifest_path, windows_path, android_path)

    def _evidence(
        self, manifest: dict[str, object], manifest_sha: str, platform: str
    ) -> dict[str, object]:
        attachments: dict[str, str] = {}

        def attachment(label: str) -> str:
            relative = Path(platform) / (label.replace(".", "-").replace(":", "-") + ".evidence")
            content = f"{platform}:{label}".encode()
            path = self.root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(content)
            value = hashlib.sha256(content).hexdigest()
            attachments[relative.as_posix()] = value
            return value

        required_tests = (
            release_contract.WINDOWS_TESTS
            if platform == "windows"
            else release_contract.ANDROID_TESTS
        )
        tests = {
            test_id: {
                "status": "passed",
                "evidence_sha256": attachment(f"test-{test_id}"),
            }
            for test_id in required_tests
        }
        tests["stress.connect_disconnect_100"]["iterations"] = 100
        tests["soak.connected_24h"]["duration_seconds"] = 86_400
        tests["performance.oracle_thresholds"].update(
            {
                "oracle_throughput_mbps": 100,
                "candidate_throughput_mbps": 90,
                "oracle_p95_latency_ms": 10,
                "candidate_p95_latency_ms": 11,
                "oracle_memory_mib": 100,
                "candidate_memory_mib": 125,
                "proxy_throughput_mbps": [
                    {
                        "rtt_ms": rtt_ms,
                        "tun_single_mbps": 100,
                        "http_connect_single_mbps": 80,
                        "socks5_tcp_single_mbps": 80,
                        "tun_four_mbps": 100,
                        "http_connect_four_mbps": 90,
                        "socks5_tcp_four_mbps": 90,
                    }
                    for rtt_ms in (20, 100, 300)
                ],
            }
        )

        artifacts = {
            artifact["name"]: artifact["sha256"]
            for artifact in manifest["artifacts"]  # type: ignore[index]
            if artifact["platform"] == platform
        }
        devices_contract = (
            release_contract.WINDOWS_DEVICES
            if platform == "windows"
            else release_contract.ANDROID_DEVICES
        )
        extension = "msi" if platform == "windows" else "apk"
        devices = [
            {
                "id": device_id,
                "artifact": f"usque-{self.tag}-{platform}-{variant}.{extension}",
                "status": "passed",
                "os_version": "test",
                "architecture": variant,
                "evidence_sha256": attachment(f"device-{device_id}"),
            }
            for device_id, variant in devices_contract.items()
        ]
        return {
            "schema_version": 1,
            "platform": platform,
            "tag": self.tag,
            "commit": self.commit,
            "manifest_sha256": manifest_sha,
            "packet_capture_sha256": attachment("packet-capture"),
            "attachments": attachments,
            "artifacts": artifacts,
            "devices": devices,
            "tests": tests,
        }


if __name__ == "__main__":
    unittest.main()
