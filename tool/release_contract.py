#!/usr/bin/env python3
"""Fail-closed release manifest and laboratory evidence contract for Usque."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

SCHEMA_VERSION = 1
TAG_PATTERN = re.compile(r"^v\d+\.\d+\.\d+-beta\.\d+$")
SHA256_PATTERN = re.compile(r"^[0-9a-f]{64}$")
COMMIT_PATTERN = re.compile(r"^[0-9a-f]{40}$")

WINDOWS_VARIANTS = ("x64-v2",)
ANDROID_VARIANTS = ("arm64-v8a",)

COMMON_TESTS = frozenset(
    {
        "transport.h3",
        "transport.h2_fallback",
        "security.pin_rotation",
        "api.rate_limit",
        "network.tcp_only",
        "network.ipv4_only",
        "network.ipv6_only",
        "network.udp_loss",
        "network.dns_pollution",
        "network.sleep_resume",
        "network.switch",
        "dns.tunnel_only",
        "leak.ipv4",
        "leak.ipv6",
        "leak.dns",
        "leak.bypass_cidr",
        "proxy.socks5_tcp",
        "proxy.socks5_udp",
        "proxy.http_connect",
        "proxy.http_forward",
        "proxy.port_conflict",
        "proxy.non_loopback_warning",
        "stress.connect_disconnect_100",
        "soak.connected_24h",
        "resources.no_leak",
        "performance.oracle_thresholds",
    }
)

WINDOWS_TESTS = COMMON_TESTS | frozenset(
    {
        "install.clean",
        "install.upgrade",
        "install.rollback",
        "uninstall.restore",
        "lifecycle.ui_kill",
        "lifecycle.engine_kill",
        "lifecycle.agent_kill",
        "lifecycle.service_restart",
        "killswitch.wfp_persistent",
        "state.no_residual_adapter",
        "state.no_residual_routes",
        "state.no_residual_dns",
        "state.system_proxy_restored",
    }
)

ANDROID_TESTS = COMMON_TESTS | frozenset(
    {
        "install.clean",
        "install.upgrade",
        "uninstall.restore",
        "lifecycle.app_process_kill",
        "lifecycle.vpn_process_kill",
        "lifecycle.vpn_service_restart",
        "permission.vpn_revoked",
        "lockdown.always_on",
        "reconnect.tun_retained",
        "tv.launcher_entry",
        "tv.dpad_focus",
    }
)

WINDOWS_DEVICES = {
    "windows-10-19045-x64-v2": "x64-v2",
    "windows-11-x64-v2": "x64-v2",
}

ANDROID_DEVICES = {
    "android-8-arm64": "arm64-v8a",
    "android-api33-arm64": "arm64-v8a",
    "android-current-arm64": "arm64-v8a",
    "android-tv-arm64": "arm64-v8a",
}


class ContractError(ValueError):
    """A release input violates the public beta contract."""


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def canonical_json_bytes(value: Any) -> bytes:
    return (
        json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")) + "\n"
    ).encode("utf-8")


def write_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(canonical_json_bytes(value))


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ContractError(f"cannot read JSON {path}: {error}") from error
    if not isinstance(value, dict):
        raise ContractError(f"{path} must contain one JSON object")
    return value


def expected_artifact_names(tag: str) -> dict[str, tuple[str, str]]:
    if not TAG_PATTERN.fullmatch(tag):
        raise ContractError(f"unsupported public beta tag: {tag}")
    expected: dict[str, tuple[str, str]] = {}
    for variant in WINDOWS_VARIANTS:
        expected[f"usque-{tag}-windows-{variant}.msi"] = ("windows", variant)
    for variant in ANDROID_VARIANTS:
        expected[f"usque-{tag}-android-{variant}.apk"] = ("android", variant)
    return expected


def normalize_sha256(value: Any, label: str) -> str:
    normalized = str(value).strip().lower()
    if not SHA256_PATTERN.fullmatch(normalized):
        raise ContractError(f"{label} must be one SHA-256 digest")
    return normalized


def normalize_commit(value: Any) -> str:
    normalized = str(value).strip().lower()
    if not COMMIT_PATTERN.fullmatch(normalized):
        raise ContractError("commit must be one full 40-character Git SHA")
    return normalized


def create_manifest(
    directory: Path,
    tag: str,
    commit: str,
    windows_signer: str,
    android_signer: str,
) -> dict[str, Any]:
    directory = directory.resolve(strict=True)
    expected = expected_artifact_names(tag)
    commit = normalize_commit(commit)
    windows_signer = normalize_sha256(windows_signer, "Windows signer")
    android_signer = normalize_sha256(android_signer, "Android signer")

    actual_primary = {
        path.name
        for path in directory.iterdir()
        if path.is_file() and path.suffix.lower() in {".msi", ".apk"}
    }
    missing = sorted(set(expected) - actual_primary)
    unexpected = sorted(actual_primary - set(expected))
    if missing or unexpected:
        raise ContractError(
            f"release artifact set mismatch; missing={missing}, unexpected={unexpected}"
        )

    artifacts = []
    checksum_lines = []
    for name in sorted(expected):
        path = directory / name
        digest = sha256_file(path)
        platform, variant = expected[name]
        artifacts.append(
            {
                "name": name,
                "platform": platform,
                "variant": variant,
                "sha256": digest,
                "size": path.stat().st_size,
            }
        )
        checksum_line = f"{digest} *{name}\n"
        (directory / f"{name}.sha256").write_text(checksum_line, encoding="utf-8")
        checksum_lines.append(checksum_line)

    (directory / "SHA256SUMS").write_text("".join(checksum_lines), encoding="utf-8")
    return {
        "schema_version": SCHEMA_VERSION,
        "tag": tag,
        "commit": commit,
        "created_at": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "signers": {
            "windows_certificate_sha256": windows_signer,
            "android_certificate_sha256": android_signer,
        },
        "artifacts": artifacts,
    }


def artifact_index(manifest: dict[str, Any]) -> dict[str, dict[str, Any]]:
    if manifest.get("schema_version") != SCHEMA_VERSION:
        raise ContractError("unsupported release manifest schema_version")
    expected = expected_artifact_names(str(manifest.get("tag", "")))
    normalize_commit(manifest.get("commit"))
    signers = manifest.get("signers")
    if not isinstance(signers, dict):
        raise ContractError("manifest signers must be an object")
    normalize_sha256(signers.get("windows_certificate_sha256"), "Windows manifest signer")
    normalize_sha256(signers.get("android_certificate_sha256"), "Android manifest signer")

    artifacts = manifest.get("artifacts")
    if not isinstance(artifacts, list):
        raise ContractError("manifest artifacts must be an array")
    index: dict[str, dict[str, Any]] = {}
    for artifact in artifacts:
        if not isinstance(artifact, dict):
            raise ContractError("manifest artifact entries must be objects")
        name = str(artifact.get("name", ""))
        if name in index:
            raise ContractError(f"duplicate manifest artifact: {name}")
        if name not in expected:
            raise ContractError(f"unexpected manifest artifact: {name}")
        platform, variant = expected[name]
        if artifact.get("platform") != platform or artifact.get("variant") != variant:
            raise ContractError(f"wrong platform or variant for {name}")
        normalize_sha256(artifact.get("sha256"), f"artifact {name}")
        if not isinstance(artifact.get("size"), int) or artifact["size"] <= 0:
            raise ContractError(f"artifact {name} has an invalid size")
        index[name] = artifact
    if set(index) != set(expected):
        raise ContractError("manifest does not contain the exact artifact set")
    return index


def verify_artifacts(directory: Path, manifest: dict[str, Any]) -> None:
    directory = directory.resolve(strict=True)
    index = artifact_index(manifest)
    for name, artifact in index.items():
        path = directory / name
        if not path.is_file():
            raise ContractError(f"artifact is missing: {name}")
        if path.stat().st_size != artifact["size"]:
            raise ContractError(f"artifact size mismatch: {name}")
        if sha256_file(path) != artifact["sha256"]:
            raise ContractError(f"artifact SHA-256 mismatch: {name}")


def require_passed_test(tests: dict[str, Any], test_id: str, evidence_path: Path) -> dict[str, Any]:
    result = tests.get(test_id)
    if not isinstance(result, dict) or result.get("status") != "passed":
        raise ContractError(f"{evidence_path}: required test did not pass: {test_id}")
    result["evidence_sha256"] = normalize_sha256(
        result.get("evidence_sha256"), f"{evidence_path}:{test_id} evidence"
    )
    return result


def verify_performance(result: dict[str, Any], evidence_path: Path) -> None:
    required = (
        "oracle_throughput_mbps",
        "candidate_throughput_mbps",
        "oracle_p95_latency_ms",
        "candidate_p95_latency_ms",
        "oracle_memory_mib",
        "candidate_memory_mib",
    )
    values: dict[str, float] = {}
    for key in required:
        value = result.get(key)
        if not isinstance(value, (int, float)) or value <= 0:
            raise ContractError(f"{evidence_path}: performance field {key} is invalid")
        values[key] = float(value)
    if values["candidate_throughput_mbps"] < values["oracle_throughput_mbps"] * 0.90:
        raise ContractError(f"{evidence_path}: throughput is below 90% of the oracle")
    if values["candidate_p95_latency_ms"] > values["oracle_p95_latency_ms"] * 1.10:
        raise ContractError(f"{evidence_path}: p95 latency exceeds 110% of the oracle")
    if values["candidate_memory_mib"] > values["oracle_memory_mib"] * 1.25:
        raise ContractError(f"{evidence_path}: memory exceeds 125% of the oracle")

    proxy_matrix = result.get("proxy_throughput_mbps")
    if not isinstance(proxy_matrix, list) or len(proxy_matrix) != 3:
        raise ContractError(
            f"{evidence_path}: proxy_throughput_mbps must contain the 20/100/300 ms matrix"
        )
    matrix_by_rtt: dict[int, dict[str, Any]] = {}
    for sample in proxy_matrix:
        if not isinstance(sample, dict):
            raise ContractError(f"{evidence_path}: proxy throughput sample is invalid")
        rtt_ms = sample.get("rtt_ms")
        if not isinstance(rtt_ms, int) or rtt_ms in matrix_by_rtt:
            raise ContractError(f"{evidence_path}: proxy RTT values must be unique integers")
        matrix_by_rtt[rtt_ms] = sample

    single_modes = ("http_connect_single_mbps", "socks5_tcp_single_mbps")
    four_modes = ("http_connect_four_mbps", "socks5_tcp_four_mbps")
    for rtt_ms in (20, 100, 300):
        sample = matrix_by_rtt.get(rtt_ms)
        if sample is None:
            raise ContractError(
                f"{evidence_path}: proxy throughput matrix is missing {rtt_ms} ms RTT"
            )
        baselines = ("tun_single_mbps", "tun_four_mbps")
        for field in (*baselines, *single_modes, *four_modes):
            value = sample.get(field)
            if not isinstance(value, (int, float)) or value <= 0:
                raise ContractError(
                    f"{evidence_path}: proxy performance field {field} is invalid at {rtt_ms} ms RTT"
                )
        for field in single_modes:
            if float(sample[field]) < float(sample["tun_single_mbps"]) * 0.80:
                raise ContractError(
                    f"{evidence_path}: {field} is below 80% of TUN at {rtt_ms} ms RTT"
                )
        for field in four_modes:
            if float(sample[field]) < float(sample["tun_four_mbps"]) * 0.90:
                raise ContractError(
                    f"{evidence_path}: {field} is below 90% of TUN at {rtt_ms} ms RTT"
                )


def verify_evidence(
    evidence_path: Path,
    manifest: dict[str, Any],
    manifest_digest: str,
    platform: str,
) -> dict[str, Any]:
    evidence = load_json(evidence_path)
    if evidence.get("schema_version") != SCHEMA_VERSION:
        raise ContractError(f"{evidence_path}: unsupported schema_version")
    if evidence.get("platform") != platform:
        raise ContractError(f"{evidence_path}: platform must be {platform}")
    if evidence.get("tag") != manifest["tag"]:
        raise ContractError(f"{evidence_path}: tag does not match the manifest")
    if evidence.get("commit") != manifest["commit"]:
        raise ContractError(f"{evidence_path}: commit does not match the manifest")
    if (
        normalize_sha256(evidence.get("manifest_sha256"), f"{evidence_path}:manifest_sha256")
        != manifest_digest
    ):
        raise ContractError(f"{evidence_path}: manifest SHA-256 does not match")
    packet_capture_digest = normalize_sha256(
        evidence.get("packet_capture_sha256"),
        f"{evidence_path}:packet_capture_sha256",
    )

    attachments = evidence.get("attachments")
    if not isinstance(attachments, dict) or not attachments:
        raise ContractError(f"{evidence_path}: attachments must be a non-empty object")
    attachment_digests: set[str] = set()
    evidence_root = evidence_path.resolve(strict=True).parent
    for relative_name, expected_digest_value in attachments.items():
        relative_path = Path(str(relative_name))
        if relative_path.is_absolute() or ".." in relative_path.parts:
            raise ContractError(f"{evidence_path}: unsafe attachment path {relative_name}")
        attachment_path = (evidence_root / relative_path).resolve(strict=True)
        if evidence_root not in attachment_path.parents:
            raise ContractError(f"{evidence_path}: attachment escapes evidence directory")
        if not attachment_path.is_file():
            raise ContractError(f"{evidence_path}: attachment is not a file")
        expected_digest = normalize_sha256(
            expected_digest_value, f"{evidence_path}:{relative_name}"
        )
        if sha256_file(attachment_path) != expected_digest:
            raise ContractError(f"{evidence_path}: attachment digest mismatch: {relative_name}")
        attachment_digests.add(expected_digest)
    if packet_capture_digest not in attachment_digests:
        raise ContractError(f"{evidence_path}: packet capture attachment is missing")

    tests = evidence.get("tests")
    if not isinstance(tests, dict):
        raise ContractError(f"{evidence_path}: tests must be an object")
    required_tests = WINDOWS_TESTS if platform == "windows" else ANDROID_TESTS
    for test_id in sorted(required_tests):
        result = require_passed_test(tests, test_id, evidence_path)
        if result["evidence_sha256"] not in attachment_digests:
            raise ContractError(f"{evidence_path}: attachment for test {test_id} is missing")

    stress = tests["stress.connect_disconnect_100"]
    if not isinstance(stress.get("iterations"), int) or stress["iterations"] < 100:
        raise ContractError(f"{evidence_path}: fewer than 100 connection cycles")
    soak = tests["soak.connected_24h"]
    if (
        not isinstance(soak.get("duration_seconds"), (int, float))
        or soak["duration_seconds"] < 86_400
    ):
        raise ContractError(f"{evidence_path}: connected soak was shorter than 24 hours")
    verify_performance(tests["performance.oracle_thresholds"], evidence_path)

    manifest_artifacts = artifact_index(manifest)
    platform_artifacts = {
        name: artifact
        for name, artifact in manifest_artifacts.items()
        if artifact["platform"] == platform
    }
    reported_artifacts = evidence.get("artifacts")
    if not isinstance(reported_artifacts, dict):
        raise ContractError(f"{evidence_path}: artifacts must be an object")
    if set(reported_artifacts) != set(platform_artifacts):
        raise ContractError(f"{evidence_path}: evidence artifact set is incomplete")
    for name, digest in reported_artifacts.items():
        if (
            normalize_sha256(digest, f"{evidence_path}:{name}")
            != platform_artifacts[name]["sha256"]
        ):
            raise ContractError(f"{evidence_path}: artifact digest mismatch for {name}")

    required_devices = WINDOWS_DEVICES if platform == "windows" else ANDROID_DEVICES
    devices = evidence.get("devices")
    if not isinstance(devices, list):
        raise ContractError(f"{evidence_path}: devices must be an array")
    by_id: dict[str, dict[str, Any]] = {}
    for device in devices:
        if not isinstance(device, dict):
            raise ContractError(f"{evidence_path}: device entries must be objects")
        device_id = str(device.get("id", ""))
        if device_id in by_id:
            raise ContractError(f"{evidence_path}: duplicate device {device_id}")
        by_id[device_id] = device
    if set(by_id) != set(required_devices):
        raise ContractError(f"{evidence_path}: device matrix is incomplete")

    tag = manifest["tag"]
    for device_id, variant in required_devices.items():
        device = by_id[device_id]
        extension = "msi" if platform == "windows" else "apk"
        expected_name = f"usque-{tag}-{platform}-{variant}.{extension}"
        if device.get("artifact") != expected_name:
            raise ContractError(f"{evidence_path}: wrong artifact for {device_id}")
        if device.get("status") != "passed":
            raise ContractError(f"{evidence_path}: device did not pass: {device_id}")
        if not str(device.get("os_version", "")).strip():
            raise ContractError(f"{evidence_path}: missing OS version for {device_id}")
        if not str(device.get("architecture", "")).strip():
            raise ContractError(f"{evidence_path}: missing architecture for {device_id}")
        device["evidence_sha256"] = normalize_sha256(
            device.get("evidence_sha256"),
            f"{evidence_path}:{device_id} evidence",
        )
        if device["evidence_sha256"] not in attachment_digests:
            raise ContractError(f"{evidence_path}: attachment for device {device_id} is missing")
    return evidence


def create_ready(
    manifest_path: Path,
    windows_evidence_path: Path,
    android_evidence_path: Path,
) -> dict[str, Any]:
    manifest = load_json(manifest_path)
    artifact_index(manifest)
    manifest_digest = sha256_file(manifest_path)
    windows = verify_evidence(windows_evidence_path, manifest, manifest_digest, "windows")
    android = verify_evidence(android_evidence_path, manifest, manifest_digest, "android")
    return {
        "schema_version": SCHEMA_VERSION,
        "ready": True,
        "tag": manifest["tag"],
        "commit": manifest["commit"],
        "manifest_sha256": manifest_digest,
        "windows_evidence_sha256": sha256_file(windows_evidence_path),
        "android_evidence_sha256": sha256_file(android_evidence_path),
        "windows_packet_capture_sha256": windows["packet_capture_sha256"],
        "android_packet_capture_sha256": android["packet_capture_sha256"],
        "signers": manifest["signers"],
        "verified_at": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    }


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)

    create = subparsers.add_parser("create-manifest")
    create.add_argument("--directory", type=Path, required=True)
    create.add_argument("--tag", required=True)
    create.add_argument("--commit", required=True)
    create.add_argument("--windows-signer-sha256", required=True)
    create.add_argument("--android-signer-sha256", required=True)
    create.add_argument("--output", type=Path, required=True)

    verify = subparsers.add_parser("verify-artifacts")
    verify.add_argument("--directory", type=Path, required=True)
    verify.add_argument("--manifest", type=Path, required=True)

    ready = subparsers.add_parser("create-ready")
    ready.add_argument("--manifest", type=Path, required=True)
    ready.add_argument("--windows-evidence", type=Path, required=True)
    ready.add_argument("--android-evidence", type=Path, required=True)
    ready.add_argument("--output", type=Path, required=True)

    evidence = subparsers.add_parser("verify-evidence")
    evidence.add_argument("--manifest", type=Path, required=True)
    evidence.add_argument("--evidence", type=Path, required=True)
    evidence.add_argument("--platform", choices=("windows", "android"), required=True)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    try:
        if args.command == "create-manifest":
            manifest = create_manifest(
                args.directory,
                args.tag,
                args.commit,
                args.windows_signer_sha256,
                args.android_signer_sha256,
            )
            write_json(args.output, manifest)
        elif args.command == "verify-artifacts":
            verify_artifacts(args.directory, load_json(args.manifest))
        elif args.command == "create-ready":
            ready = create_ready(args.manifest, args.windows_evidence, args.android_evidence)
            write_json(args.output, ready)
        elif args.command == "verify-evidence":
            manifest = load_json(args.manifest)
            artifact_index(manifest)
            verify_evidence(
                args.evidence,
                manifest,
                sha256_file(args.manifest),
                args.platform,
            )
        else:  # pragma: no cover - argparse guarantees the command
            raise AssertionError(args.command)
    except ContractError as error:
        print(f"release contract rejected: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
