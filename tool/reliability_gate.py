"""Validate and aggregate isolated reliability-lab reports.

The tool never runs a platform test itself. Protected VM, device, and network
lab runners produce allowlisted JSON; this verifier ensures that missing,
not-run, failed, duplicated, or wrong-candidate results cannot become a release
pass.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from datetime import UTC, datetime
from pathlib import Path, PurePosixPath
from typing import Any

SCHEMA_VERSION = 1
HEX_40 = re.compile(r"^[0-9a-f]{40}$")
HEX_64 = re.compile(r"^[0-9a-f]{64}$")
ALLOWED_STATUS = {"passed", "failed", "not_run"}
REQUIRED_GATES = {
    "windows.clean_install",
    "windows.coverage_upgrade",
    "windows.connected_uninstall",
    "windows.engine_crash_recovery",
    "windows.agent_crash_fail_closed",
    "windows.sleep_network_change",
    "windows.route_dns_wfp_proxy_restore",
    "windows.wintun_residual",
    "android.wifi_to_cellular",
    "android.cellular_to_wifi",
    "android.airplane_mode",
    "android.doze_lock_screen",
    "android.flutter_process_reclaim",
    "android.vpn_process_reclaim",
    "android.always_on_lockdown",
    "android.reboot_upgrade_tv_background",
    "network.real_endpoint_h3_h2",
    "network.ipv4_protection",
    "network.ipv6_protection",
    "network.dns_leak",
    "network.kill_switch_leak",
    "network.route_leak",
    "network.direct_rule_scope",
    "performance.informational_baseline",
}
INDEPENDENT_GATES = {
    "network.ipv4_protection",
    "network.ipv6_protection",
    "network.dns_leak",
    "network.kill_switch_leak",
    "network.route_leak",
    "network.direct_rule_scope",
}
ENVIRONMENT_BY_GATE_PREFIX = {
    "windows.": "windows_snapshot_vm",
    "android.": "android_physical_device",
    "network.": "independent_network_observer",
    "performance.": "performance_lab",
}
REPORT_ARTIFACT_ENVIRONMENTS = {
    "usque-reliability-report-windows": "windows_snapshot_vm",
    "usque-reliability-report-android": "android_physical_device",
    "usque-reliability-report-network": "independent_network_observer",
    "usque-reliability-report-performance": "performance_lab",
}
RUNNER_CLASS_BY_ENVIRONMENT = {
    "windows_snapshot_vm": "usque-snapshot-vm",
    "android_physical_device": "usque-android-device",
    "independent_network_observer": "usque-network-observer",
    "performance_lab": "usque-performance-lab",
}
MAX_STRUCTURED_EVIDENCE_BYTES = 64 * 1024 * 1024
MAX_RESTRICTED_PCAP_BYTES = 2 * 1024 * 1024 * 1024


class GateError(ValueError):
    """The submitted reliability evidence does not satisfy the contract."""


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _load_report(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise GateError(f"{path}: invalid JSON: {error}") from error
    if not isinstance(value, dict):
        raise GateError(f"{path}: report root must be an object")
    return value


def _expected_environment(gate_id: str) -> str:
    for prefix, environment in ENVIRONMENT_BY_GATE_PREFIX.items():
        if gate_id.startswith(prefix):
            return environment
    raise GateError(f"unknown gate environment for {gate_id}")


def _report_environment(path: Path, reports_directory: Path) -> str:
    try:
        relative = path.resolve().relative_to(reports_directory.resolve())
    except ValueError as error:
        raise GateError(f"{path}: report leaves the reports directory") from error
    if len(relative.parts) != 2 or relative.name != "report.json":
        raise GateError(f"{path}: report must be <artifact-name>/report.json to bind its runner")
    artifact_name = relative.parts[0]
    try:
        return REPORT_ARTIFACT_ENVIRONMENTS[artifact_name]
    except KeyError as error:
        raise GateError(f"{path}: unsupported reliability report artifact") from error


def _evidence_reference(
    report_path: Path,
    evidence_root: Path,
    environment_kind: str,
    gate_id: str,
    field: str,
    value: Any,
    maximum_bytes: int,
) -> dict[str, Any]:
    if not isinstance(value, dict) or set(value) != {"path", "sha256"}:
        raise GateError(
            f"{report_path}: {gate_id} evidence {field} must contain only path and sha256"
        )
    raw_path = value.get("path")
    expected_digest = value.get("sha256")
    if not isinstance(raw_path, str) or not raw_path or len(raw_path) > 512 or "\\" in raw_path:
        raise GateError(f"{report_path}: {gate_id} has an invalid {field} path")
    relative = PurePosixPath(raw_path)
    if relative.is_absolute() or any(part in {"", ".", ".."} for part in relative.parts):
        raise GateError(f"{report_path}: {gate_id} has an unsafe {field} path")
    if not relative.parts or relative.parts[0] != environment_kind:
        raise GateError(f"{report_path}: {gate_id} {field} is outside its environment namespace")
    if not isinstance(expected_digest, str) or not HEX_64.fullmatch(expected_digest):
        raise GateError(f"{report_path}: {gate_id} has an invalid {field} SHA-256")

    candidate = evidence_root.joinpath(*relative.parts)
    try:
        root = evidence_root.resolve(strict=True)
        resolved = candidate.resolve(strict=True)
        resolved.relative_to(root)
    except (OSError, ValueError) as error:
        raise GateError(
            f"{report_path}: {gate_id} {field} evidence is missing or unsafe"
        ) from error
    current = candidate
    while current != evidence_root:
        if current.is_symlink():
            raise GateError(f"{report_path}: {gate_id} {field} evidence is a symlink")
        current = current.parent
    if not candidate.is_file():
        raise GateError(f"{report_path}: {gate_id} {field} evidence is not a regular file")
    size = candidate.stat().st_size
    if size <= 0 or size > maximum_bytes:
        raise GateError(f"{report_path}: {gate_id} {field} evidence has an invalid size")
    if sha256(candidate) != expected_digest:
        raise GateError(f"{report_path}: {gate_id} {field} evidence digest does not match")
    return {"path": raw_path, "sha256": expected_digest, "size": size}


def _validate_report(
    path: Path,
    report: dict[str, Any],
    commit: str,
    candidate_manifest_sha256: str,
    expected_environment: str,
    evidence_root: Path,
) -> list[dict[str, Any]]:
    if report.get("schema_version") != SCHEMA_VERSION:
        raise GateError(f"{path}: unsupported schema_version")
    if report.get("commit") != commit:
        raise GateError(f"{path}: commit does not match the release candidate")
    if report.get("candidate_manifest_sha256") != candidate_manifest_sha256:
        raise GateError(f"{path}: candidate manifest digest does not match")
    environment = report.get("environment")
    if not isinstance(environment, dict) or environment.get("kind") != expected_environment:
        raise GateError(f"{path}: environment.kind does not match the report artifact")
    if environment.get("runner_class") != RUNNER_CLASS_BY_ENVIRONMENT[expected_environment]:
        raise GateError(f"{path}: runner_class does not match the protected runner")
    results = report.get("results")
    if not isinstance(results, list) or not results:
        raise GateError(f"{path}: results must be a non-empty list")
    validated: list[dict[str, Any]] = []
    for result in results:
        if not isinstance(result, dict):
            raise GateError(f"{path}: every result must be an object")
        gate_id = result.get("id")
        status = result.get("status")
        if gate_id not in REQUIRED_GATES:
            raise GateError(f"{path}: unknown gate id {gate_id!r}")
        if _expected_environment(gate_id) != expected_environment:
            raise GateError(f"{path}: {gate_id} came from the wrong environment")
        if status not in ALLOWED_STATUS:
            raise GateError(f"{path}: invalid status for {gate_id}")
        if status != "passed":
            raise GateError(f"{path}: release gate {gate_id} is {status}")
        evidence = result.get("evidence")
        if not isinstance(evidence, dict):
            raise GateError(f"{path}: {gate_id} has no evidence object")
        normalized_evidence = {
            name: _evidence_reference(
                path,
                evidence_root,
                expected_environment,
                gate_id,
                name,
                evidence.get(name),
                MAX_STRUCTURED_EVIDENCE_BYTES,
            )
            for name in ("junit", "timeline", "platform_diff")
        }
        if gate_id in INDEPENDENT_GATES:
            if evidence.get("observer") != "external":
                raise GateError(f"{path}: {gate_id} was not observed independently")
            if evidence.get("zero_unexpected_packets") is not True:
                raise GateError(f"{path}: {gate_id} did not prove zero unexpected packets")
            normalized_evidence["observer"] = "external"
            normalized_evidence["zero_unexpected_packets"] = True
            normalized_evidence["restricted_pcap"] = _evidence_reference(
                path,
                evidence_root,
                expected_environment,
                gate_id,
                "restricted_pcap",
                evidence.get("restricted_pcap"),
                MAX_RESTRICTED_PCAP_BYTES,
            )
        validated.append({"id": gate_id, "status": status, "evidence": normalized_evidence})
    return validated


def aggregate(
    reports_directory: Path,
    evidence_directory: Path,
    candidate_manifest: Path,
    commit: str,
    output: Path,
    device_matrix: Path,
) -> None:
    if not HEX_40.fullmatch(commit):
        raise GateError("commit must be one lowercase 40-character SHA-1")
    manifest_digest = sha256(candidate_manifest)
    if not HEX_64.fullmatch(manifest_digest):
        raise GateError("candidate manifest digest is invalid")
    if not evidence_directory.is_dir() or evidence_directory.is_symlink():
        raise GateError("the reliability evidence root is missing or unsafe")
    report_paths = sorted(reports_directory.rglob("report.json"))
    if not report_paths:
        raise GateError("no reliability reports were supplied")
    by_id: dict[str, dict[str, Any]] = {}
    environments: list[dict[str, Any]] = []
    observed_environment_kinds: set[str] = set()
    for path in report_paths:
        expected_environment = _report_environment(path, reports_directory)
        if expected_environment in observed_environment_kinds:
            raise GateError(f"duplicate environment report: {expected_environment}")
        observed_environment_kinds.add(expected_environment)
        report = _load_report(path)
        results = _validate_report(
            path,
            report,
            commit,
            manifest_digest,
            expected_environment,
            evidence_directory,
        )
        environments.append(report["environment"])
        for result in results:
            gate_id = result["id"]
            if gate_id in by_id:
                raise GateError(f"duplicate gate result: {gate_id}")
            by_id[gate_id] = result
    missing = sorted(REQUIRED_GATES - by_id.keys())
    if missing:
        raise GateError(f"required gates are missing: {', '.join(missing)}")

    aggregate_report = {
        "schema_version": SCHEMA_VERSION,
        "commit": commit,
        "candidate_manifest_sha256": manifest_digest,
        "generated_at": datetime.now(UTC).isoformat(),
        "status": "passed",
        "results": [by_id[gate_id] for gate_id in sorted(by_id)],
        "environments": environments,
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(
        json.dumps(aggregate_report, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    lines = [
        "# Release device and environment matrix",
        "",
        f"Commit: `{commit}`",
        f"Candidate manifest SHA-256: `{manifest_digest}`",
        "",
        "| Environment | OS / API | Runner class |",
        "|---|---|---|",
    ]
    for environment in environments:
        lines.append(
            "| {kind} | {version} | {runner} |".format(
                kind=environment["kind"],
                version=environment.get("version", "unspecified"),
                runner=environment.get("runner_class", "protected"),
            )
        )
    lines.extend(["", f"All {len(by_id)} required gates passed.", ""])
    device_matrix.write_text("\n".join(lines), encoding="utf-8")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)
    aggregate_parser = subparsers.add_parser("aggregate")
    aggregate_parser.add_argument("--reports-directory", type=Path, required=True)
    aggregate_parser.add_argument("--evidence-directory", type=Path, required=True)
    aggregate_parser.add_argument("--candidate-manifest", type=Path, required=True)
    aggregate_parser.add_argument("--commit", required=True)
    aggregate_parser.add_argument("--output", type=Path, required=True)
    aggregate_parser.add_argument("--device-matrix", type=Path, required=True)
    return parser.parse_args()


def main() -> int:
    arguments = parse_args()
    try:
        aggregate(
            arguments.reports_directory,
            arguments.evidence_directory,
            arguments.candidate_manifest,
            arguments.commit,
            arguments.output,
            arguments.device_matrix,
        )
    except GateError as error:
        print(f"reliability gate failed: {error}")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
