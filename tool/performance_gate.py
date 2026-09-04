#!/usr/bin/env python3
"""Validate protected performance samples and produce reliability gate evidence.

Normal CI exercises this parser and its deterministic budget arithmetic with
fixtures.  It deliberately does not run wall-clock benchmarks.  A protected
performance lab supplies one baseline and one candidate schema-v2 report for
every configured scenario.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from collections.abc import Iterable
from decimal import Decimal
from fractions import Fraction
from pathlib import Path
from typing import Any

REPORT_SCHEMA_VERSION = 2
EVIDENCE_SCHEMA_VERSION = 3
RELIABILITY_SCHEMA_VERSION = 1
HEX_40 = re.compile(r"^[0-9a-f]{40}$")
VERSION = re.compile(r"^[0-9]+(?:\.[0-9]+){1,3}$")
IDENTIFIER = re.compile(r"^[a-z][a-z0-9-]{0,63}$")
ALLOWED_PLATFORM_CLASSES = {"android-arm64", "linux-x64", "windows-x64-v2"}
ALLOWED_REASON_CODES = {
    "battery_unstable",
    "infrastructure_unavailable",
    "missing_baseline",
    "scenario_unavailable",
    "thermal_unstable",
    "unsupported_platform",
}
PERFORMANCE_GATES = {
    "performance.direct_dns",
    "performance.h2_high_bdp",
    "performance.h3_allocation_rate",
    "performance.h3_batch_io",
    "performance.pmtu_convergence",
    "performance.queue_pressure",
    "performance.quic_migration",
}
REQUIRED_H2_PROFILES = {"h2-bdp-100ms", "h2-bdp-500ms"}
REPORT_FIELDS = {
    "schema_version",
    "status",
    "sample_role",
    "scenario_id",
    "baseline_commit",
    "candidate_commit",
    "platform_class",
    "runner_class",
    "samples",
    "environment",
    "reason_code",
}
ENVIRONMENT_FIELDS = {
    "network_profile_id",
    "thermal_policy",
    "battery_policy",
    "tool_versions",
}
TOOL_VERSION_FIELDS = {"kernel", "runner", "rustc"}
COMMON_SAMPLE_FIELDS = {
    "run_index",
    "measurement_duration_ms",
    "goodput_bps",
    "latency_p50_us",
    "latency_p95_us",
    "latency_p99_us",
    "cpu_time_ms",
    "rss_peak_bytes",
    "queue_drop_packets",
    "udp_send_syscalls",
    "udp_recv_syscalls",
    "udp_datagrams_sent",
    "udp_datagrams_received",
    "controlled_allocations",
    "inner_packets",
}
SPECIAL_SAMPLE_FIELDS = {
    "dns_queries",
    "dns_successes",
    "fallback_completion_us",
    "full_reconnect_completion_us",
    "migration_interruption_p95_us",
    "physical_port_53_packets",
    "plaintext_fallback_queries",
    "pmtu_stable_ms",
    "send_error_spin_count",
    "silent_truncation_packets",
}
SAMPLE_FIELDS = COMMON_SAMPLE_FIELDS | SPECIAL_SAMPLE_FIELDS
SENSITIVE_KEYS = {
    "device_serial",
    "host_name",
    "hostname",
    "ip",
    "ip_address",
    "serial",
    "serial_number",
    "ssid",
    "user",
    "user_name",
    "username",
}
POLICIES = {
    "direct_dns",
    "h2_high_bdp",
    "h3_allocation_rate",
    "h3_batch_io",
    "pmtu_convergence",
    "queue_pressure",
    "quic_migration",
}
STABILITY_BUDGET_FIELDS = {
    "latency_mad_ratio_max",
    "throughput_mad_ratio_max",
}
STEADY_BUDGET_FIELDS = {
    "allocations_per_inner_packet_ratio_max",
    "cpu_per_bit_ratio_max",
    "dns_success_rate_min",
    "latency_p95_ratio_max",
    "migration_fallback_completion_us_max",
    "migration_interruption_p95_us_max",
    "physical_port_53_packets_max",
    "pmtu_stable_ms_max",
    "queue_drop_packets_max",
    "rss_peak_absolute_increase_bytes_max",
    "rss_peak_ratio_max",
    "send_error_spin_count_max",
    "syscalls_per_datagram_max",
    "throughput_ratio_min",
}
FEATURE_BUDGET_FIELDS = {
    "allocation_per_packet_max",
    "allocation_ratio_max",
    "batch_cpu_per_bit_ratio_max",
    "batch_syscall_ratio_max",
    "dns_latency_absolute_increase_us_max",
    "dns_latency_ratio_max",
    "h2_latency_ratio_max",
    "h2_throughput_ratio_min",
    "migration_interruption_ratio_max",
    "pmtu_goodput_ratio_min",
}


class GateError(ValueError):
    """A performance input violates the fail-closed contract."""


def _reject_json_constant(value: str) -> None:
    raise GateError(f"non-finite JSON number is forbidden: {value}")


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(
            path.read_text(encoding="utf-8"),
            parse_constant=_reject_json_constant,
        )
    except (OSError, json.JSONDecodeError, UnicodeDecodeError) as error:
        raise GateError(f"{path}: invalid JSON: {error}") from error
    if not isinstance(value, dict):
        raise GateError(f"{path}: root must be one object")
    return value


def canonical_json_bytes(value: Any) -> bytes:
    return (
        json.dumps(
            value,
            ensure_ascii=False,
            sort_keys=True,
            separators=(",", ":"),
            allow_nan=False,
        )
        + "\n"
    ).encode("utf-8")


def write_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(canonical_json_bytes(value))


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _require_exact_fields(
    value: dict[str, Any], allowed: set[str], required: set[str], label: str
) -> None:
    unknown = sorted(set(value) - allowed)
    missing = sorted(required - set(value))
    if unknown or missing:
        raise GateError(f"{label}: field mismatch; missing={missing}, unknown={unknown}")


def _require_nonnegative_integer(value: Any, label: str) -> int:
    if type(value) is not int or value < 0:  # bool is intentionally rejected
        raise GateError(f"{label} must be a non-negative integer")
    return value


def _major_version(value: str, label: str) -> int:
    if not VERSION.fullmatch(value):
        raise GateError(f"{label} must be a version without host or device data")
    return int(value.split(".", maxsplit=1)[0])


def _walk_sensitive_keys(value: Any, label: str = "report") -> None:
    if isinstance(value, dict):
        for key, child in value.items():
            normalized = re.sub(r"[^a-z0-9]+", "_", str(key).lower()).strip("_")
            if normalized in SENSITIVE_KEYS:
                raise GateError(f"{label}: sensitive field is forbidden: {key}")
            _walk_sensitive_keys(child, label)
    elif isinstance(value, list):
        for child in value:
            _walk_sensitive_keys(child, label)


def validate_schema_contract(schema: dict[str, Any]) -> None:
    """Ensure the checked-in JSON schema and the dependency-free validator agree."""

    if schema.get("$schema") != "https://json-schema.org/draft/2020-12/schema":
        raise GateError("performance schema must use JSON Schema draft 2020-12")
    if schema.get("additionalProperties") is not False:
        raise GateError("performance schema root must reject extra fields")
    properties = schema.get("properties")
    if not isinstance(properties, dict) or set(properties) != REPORT_FIELDS:
        raise GateError("performance schema report fields do not match the validator")
    if properties.get("schema_version", {}).get("const") != REPORT_SCHEMA_VERSION:
        raise GateError("performance schema version does not match the validator")
    samples = properties.get("samples", {})
    items = samples.get("items") if isinstance(samples, dict) else None
    if not isinstance(items, dict) or items.get("additionalProperties") is not False:
        raise GateError("performance schema sample objects must reject extra fields")
    sample_properties = items.get("properties")
    if not isinstance(sample_properties, dict) or set(sample_properties) != SAMPLE_FIELDS:
        raise GateError("performance schema sample fields do not match the validator")


def validate_report(
    report: dict[str, Any], scenario: dict[str, Any], expected_role: str
) -> dict[str, Any]:
    _walk_sensitive_keys(report)
    required = REPORT_FIELDS - {"reason_code"}
    if report.get("status") == "not_run":
        required = REPORT_FIELDS
    _require_exact_fields(report, REPORT_FIELDS, required, "performance report")
    if report["schema_version"] != REPORT_SCHEMA_VERSION:
        raise GateError("unsupported performance schema_version")
    if report["status"] not in {"measured", "not_run"}:
        raise GateError("performance report status must be measured or not_run")
    if report["sample_role"] != expected_role:
        raise GateError(f"performance report sample_role must be {expected_role}")
    if report["scenario_id"] != scenario["scenario_id"]:
        raise GateError("performance report scenario_id does not match its configured scenario")
    for field in ("baseline_commit", "candidate_commit"):
        if not isinstance(report[field], str) or not HEX_40.fullmatch(report[field]):
            raise GateError(f"performance report {field} must be a lowercase 40-character SHA")
    if report["platform_class"] not in scenario["platform_classes"]:
        raise GateError("performance report platform_class is not allowed for this scenario")
    if report["runner_class"] != "protected-performance-lab":
        raise GateError("performance report came from the wrong runner class")

    environment = report["environment"]
    if not isinstance(environment, dict):
        raise GateError("performance report environment must be an object")
    _require_exact_fields(
        environment,
        ENVIRONMENT_FIELDS,
        ENVIRONMENT_FIELDS,
        "performance report environment",
    )
    profile = environment["network_profile_id"]
    if not isinstance(profile, str) or not IDENTIFIER.fullmatch(profile):
        raise GateError("network_profile_id must be an allowlisted opaque identifier")
    if profile not in scenario["network_profile_ids"]:
        raise GateError("network_profile_id is not allowlisted for this scenario")
    if environment["thermal_policy"] != "stable":
        raise GateError("thermal_policy must be stable")
    if environment["battery_policy"] not in {"powered", "stable", "powered-or-stable"}:
        raise GateError("battery_policy is invalid")
    versions = environment["tool_versions"]
    if not isinstance(versions, dict):
        raise GateError("tool_versions must be an object")
    _require_exact_fields(
        versions,
        TOOL_VERSION_FIELDS,
        TOOL_VERSION_FIELDS,
        "performance report tool_versions",
    )
    for key, value in versions.items():
        if not isinstance(value, str):
            raise GateError(f"tool_versions.{key} must be a string")
        _major_version(value, f"tool_versions.{key}")

    samples = report["samples"]
    if not isinstance(samples, list):
        raise GateError("performance report samples must be an array")
    if report["status"] == "not_run":
        if samples:
            raise GateError("not_run performance reports must not contain samples")
        if report["reason_code"] not in ALLOWED_REASON_CODES:
            raise GateError("not_run performance report has an invalid reason_code")
        return report
    if "reason_code" in report:
        raise GateError("measured performance reports must not contain reason_code")
    if len(samples) != 7:
        raise GateError("measured performance reports must contain exactly 7 raw samples")

    required_sample_fields = COMMON_SAMPLE_FIELDS | set(scenario["required_sample_fields"])
    for index, sample in enumerate(samples, start=1):
        if not isinstance(sample, dict):
            raise GateError(f"sample {index} must be an object")
        _require_exact_fields(sample, SAMPLE_FIELDS, required_sample_fields, f"sample {index}")
        for field, value in sample.items():
            _require_nonnegative_integer(value, f"sample {index}.{field}")
        if sample["run_index"] != index:
            raise GateError("sample run_index values must be the ordered integers 1 through 7")
        if sample["measurement_duration_ms"] == 0:
            raise GateError("measurement_duration_ms must be positive")
        if sample["latency_p50_us"] > sample["latency_p95_us"]:
            raise GateError("latency_p50_us must not exceed latency_p95_us")
        if sample["latency_p95_us"] > sample["latency_p99_us"]:
            raise GateError("latency_p95_us must not exceed latency_p99_us")
        if sample["latency_p95_us"] == 0:
            raise GateError("latency_p95_us must be positive")
        if sample.get("dns_successes", 0) > sample.get("dns_queries", 0):
            raise GateError("dns_successes must not exceed dns_queries")
    return report


def _load_scenarios(path: Path) -> list[dict[str, Any]]:
    value = load_json(path)
    _require_exact_fields(
        value, {"schema_version", "scenarios"}, {"schema_version", "scenarios"}, path.name
    )
    if value["schema_version"] != 1 or not isinstance(value["scenarios"], list):
        raise GateError("performance scenarios must be a schema-version-1 array")
    scenarios: list[dict[str, Any]] = []
    seen_ids: set[str] = set()
    seen_gates: set[str] = set()
    fields = {
        "scenario_id",
        "gate_id",
        "policy",
        "platform_classes",
        "network_profile_ids",
        "required_sample_fields",
        "steady_metrics",
        "feature_acceptance",
    }
    for scenario in value["scenarios"]:
        if not isinstance(scenario, dict):
            raise GateError("every performance scenario must be an object")
        _require_exact_fields(scenario, fields, fields, "performance scenario")
        scenario_id = scenario["scenario_id"]
        gate_id = scenario["gate_id"]
        if not isinstance(scenario_id, str) or not IDENTIFIER.fullmatch(scenario_id):
            raise GateError("scenario_id must be an opaque lowercase identifier")
        if scenario_id in seen_ids:
            raise GateError(f"duplicate performance scenario: {scenario_id}")
        if gate_id not in PERFORMANCE_GATES:
            raise GateError(f"unknown performance gate: {gate_id}")
        if gate_id in seen_gates and gate_id != "performance.h2_high_bdp":
            raise GateError(f"duplicate performance gate: {gate_id}")
        if scenario["policy"] not in POLICIES:
            raise GateError(f"unknown performance policy: {scenario['policy']}")
        for field in ("platform_classes", "network_profile_ids", "steady_metrics"):
            if not isinstance(scenario[field], list) or not scenario[field]:
                raise GateError(f"scenario {scenario_id} {field} must be a non-empty array")
            if len(set(scenario[field])) != len(scenario[field]):
                raise GateError(f"scenario {scenario_id} {field} contains duplicates")
        if not isinstance(scenario["required_sample_fields"], list):
            raise GateError(f"scenario {scenario_id} required_sample_fields must be an array")
        if len(set(scenario["required_sample_fields"])) != len(scenario["required_sample_fields"]):
            raise GateError(f"scenario {scenario_id} required_sample_fields contains duplicates")
        if not set(scenario["platform_classes"]) <= ALLOWED_PLATFORM_CLASSES:
            raise GateError(f"scenario {scenario_id} has an unknown platform class")
        if any(
            not isinstance(item, str) or not IDENTIFIER.fullmatch(item)
            for item in scenario["network_profile_ids"]
        ):
            raise GateError(f"scenario {scenario_id} has an invalid network profile")
        if not set(scenario["required_sample_fields"]) <= SPECIAL_SAMPLE_FIELDS:
            raise GateError(f"scenario {scenario_id} requires an unknown sample field")
        if not set(scenario["steady_metrics"]) <= {
            "allocations_per_inner_packet",
            "cpu_per_bit",
            "latency_p95_us",
            "queue_drop_packets",
            "rss_peak_bytes",
            "throughput",
        }:
            raise GateError(f"scenario {scenario_id} has an unknown steady metric")
        if type(scenario["feature_acceptance"]) is not bool:
            raise GateError(f"scenario {scenario_id} feature_acceptance must be boolean")
        seen_ids.add(scenario_id)
        seen_gates.add(gate_id)
        scenarios.append(scenario)
    if seen_gates != PERFORMANCE_GATES:
        missing = sorted(PERFORMANCE_GATES - seen_gates)
        raise GateError(f"required performance gates are missing from scenarios: {missing}")
    h2_scenarios = [s for s in scenarios if s["gate_id"] == "performance.h2_high_bdp"]
    if (
        len(h2_scenarios) != 2
        or any(
            len(s["network_profile_ids"]) != 1 or s["policy"] != "h2_high_bdp" for s in h2_scenarios
        )
        or {s["network_profile_ids"][0] for s in h2_scenarios} != REQUIRED_H2_PROFILES
    ):
        raise GateError("H2 high-BDP requires separate single-flow and four-flow scenarios")
    return scenarios


def _load_budget(path: Path) -> dict[str, Any]:
    value = load_json(path)
    if value.get("schema_version") != 1:
        raise GateError("performance budget must use schema_version 1")
    required_sections = {"schema_version", "stability", "steady_state", "feature_acceptance"}
    _require_exact_fields(value, required_sections, required_sections, path.name)
    expected_fields = {
        "stability": STABILITY_BUDGET_FIELDS,
        "steady_state": STEADY_BUDGET_FIELDS,
        "feature_acceptance": FEATURE_BUDGET_FIELDS,
    }
    for section, fields in expected_fields.items():
        if not isinstance(value[section], dict):
            raise GateError(f"performance budget {section} must be an object")
        _require_exact_fields(value[section], fields, fields, f"performance budget {section}")
        for key, raw in value[section].items():
            if type(raw) not in {int, float} or raw < 0:
                raise GateError(f"performance budget {section}.{key} must be non-negative")
    return value


def _decimal(value: Any) -> Decimal:
    if isinstance(value, Fraction):
        return Decimal(value.numerator) / Decimal(value.denominator)
    return Decimal(str(value))


def _median(values: Iterable[Fraction | int]) -> Fraction:
    ordered = sorted(Fraction(value) for value in values)
    if not ordered:
        raise GateError("cannot aggregate an empty metric")
    return ordered[len(ordered) // 2]


def _summary(values: Iterable[Fraction | int]) -> dict[str, int | float]:
    ordered = sorted(Fraction(value) for value in values)
    median = _median(ordered)
    mad = _median(abs(value - median) for value in ordered)

    def number(value: Fraction) -> int | float:
        if value.denominator == 1:
            return value.numerator
        return round(value.numerator / value.denominator, 12)

    return {
        "min": number(ordered[0]),
        "median": number(median),
        "max": number(ordered[-1]),
        "mad": number(mad),
        "mad_ratio": number(mad / median) if median else 0,
    }


def _metric_values(report: dict[str, Any]) -> dict[str, list[Fraction | int]]:
    samples = report["samples"]
    values: dict[str, list[Fraction | int]] = {
        field: [sample[field] for sample in samples]
        for field in sorted(set.intersection(*(set(sample) for sample in samples)))
        if field != "run_index"
    }

    def ratios(numerator: str, denominator: str) -> list[Fraction]:
        result = []
        for sample in samples:
            if denominator not in sample or numerator not in sample or sample[denominator] == 0:
                raise GateError(
                    f"cannot calculate {numerator}/{denominator} with a zero or missing denominator"
                )
            result.append(Fraction(sample[numerator], sample[denominator]))
        return result

    if all(sample["goodput_bps"] > 0 for sample in samples):
        values["cpu_per_bit"] = [
            Fraction(
                sample["cpu_time_ms"] * 1000,
                sample["goodput_bps"] * sample["measurement_duration_ms"],
            )
            for sample in samples
        ]
    if all(
        sample["udp_datagrams_sent"] + sample["udp_datagrams_received"] > 0 for sample in samples
    ):
        values["syscalls_per_datagram"] = [
            Fraction(
                sample["udp_send_syscalls"] + sample["udp_recv_syscalls"],
                sample["udp_datagrams_sent"] + sample["udp_datagrams_received"],
            )
            for sample in samples
        ]
    if all(sample["inner_packets"] > 0 for sample in samples):
        values["allocations_per_inner_packet"] = ratios("controlled_allocations", "inner_packets")
    return values


def aggregate_metrics(
    report: dict[str, Any],
) -> tuple[dict[str, dict[str, int | float]], dict[str, Fraction]]:
    values = _metric_values(report)
    summaries = {name: _summary(metric_values) for name, metric_values in sorted(values.items())}
    medians = {name: _median(metric_values) for name, metric_values in values.items()}
    return summaries, medians


def _same_measurement_contract(baseline: dict[str, Any], candidate: dict[str, Any]) -> None:
    for field in ("scenario_id", "baseline_commit", "candidate_commit", "platform_class"):
        if baseline[field] != candidate[field]:
            raise GateError(f"baseline/candidate {field} mismatch")
    baseline_environment = baseline["environment"]
    candidate_environment = candidate["environment"]
    if baseline_environment["network_profile_id"] != candidate_environment["network_profile_id"]:
        raise GateError("baseline/candidate network_profile_id mismatch")
    for tool in ("runner", "rustc"):
        baseline_major = _major_version(baseline_environment["tool_versions"][tool], tool)
        candidate_major = _major_version(candidate_environment["tool_versions"][tool], tool)
        if baseline_major != candidate_major:
            raise GateError(f"baseline/candidate major {tool} toolchain mismatch")


def _fraction_decimal(value: Fraction) -> Decimal:
    return Decimal(value.numerator) / Decimal(value.denominator)


def _check_max_ratio(
    checks: list[dict[str, Any]],
    name: str,
    candidate: Fraction,
    baseline: Fraction,
    maximum: Any,
) -> None:
    if baseline <= 0:
        raise GateError(f"{name} baseline must be positive")
    limit = _decimal(maximum)
    actual = _fraction_decimal(candidate) / _fraction_decimal(baseline)
    checks.append(
        {"id": name, "passed": actual <= limit, "actual": float(actual), "limit": float(limit)}
    )


def _check_min_ratio(
    checks: list[dict[str, Any]],
    name: str,
    candidate: Fraction,
    baseline: Fraction,
    minimum: Any,
) -> None:
    if baseline <= 0:
        raise GateError(f"{name} baseline must be positive")
    limit = _decimal(minimum)
    actual = _fraction_decimal(candidate) / _fraction_decimal(baseline)
    checks.append(
        {"id": name, "passed": actual >= limit, "actual": float(actual), "limit": float(limit)}
    )


def _check_max(
    checks: list[dict[str, Any]], name: str, actual: Fraction | int, maximum: Any
) -> None:
    limit = _decimal(maximum)
    current = _fraction_decimal(Fraction(actual))
    checks.append(
        {"id": name, "passed": current <= limit, "actual": float(current), "limit": float(limit)}
    )


def _check_min(
    checks: list[dict[str, Any]], name: str, actual: Fraction | int, minimum: Any
) -> None:
    limit = _decimal(minimum)
    current = _fraction_decimal(Fraction(actual))
    checks.append(
        {"id": name, "passed": current >= limit, "actual": float(current), "limit": float(limit)}
    )


def _unstable(
    report_summary: dict[str, dict[str, int | float]], budget: dict[str, Any]
) -> list[str]:
    reasons = []
    throughput = report_summary.get("goodput_bps")
    if throughput and throughput["median"]:
        ratio = _decimal(throughput["mad"]) / _decimal(throughput["median"])
        if ratio > _decimal(budget["throughput_mad_ratio_max"]):
            reasons.append("throughput_mad")
    latency = report_summary.get("latency_p95_us")
    if latency:
        ratio = _decimal(latency["mad"]) / _decimal(latency["median"])
        if ratio > _decimal(budget["latency_mad_ratio_max"]):
            reasons.append("latency_mad")
    return reasons


def _aggregate_syscalls_per_datagram(report: dict[str, Any]) -> Fraction:
    total_syscalls = sum(
        sample["udp_send_syscalls"] + sample["udp_recv_syscalls"] for sample in report["samples"]
    )
    total_datagrams = sum(
        sample["udp_datagrams_sent"] + sample["udp_datagrams_received"]
        for sample in report["samples"]
    )
    if total_datagrams == 0:
        raise GateError("cannot calculate aggregate syscall/datagram ratio with zero datagrams")
    return Fraction(total_syscalls, total_datagrams)


def evaluate_pair(
    baseline: dict[str, Any],
    candidate: dict[str, Any],
    scenario: dict[str, Any],
    budget: dict[str, Any],
) -> dict[str, Any]:
    validate_report(baseline, scenario, "baseline")
    validate_report(candidate, scenario, "candidate")
    _same_measurement_contract(baseline, candidate)
    if baseline["status"] == "not_run" or candidate["status"] == "not_run":
        reasons = [
            f"{report['sample_role']}:{report['reason_code']}"
            for report in (baseline, candidate)
            if report["status"] == "not_run"
        ]
        return {"gate_id": scenario["gate_id"], "status": "not_run", "reason_codes": reasons}

    baseline_summary, baseline_medians = aggregate_metrics(baseline)
    candidate_summary, candidate_medians = aggregate_metrics(candidate)
    unstable_reasons = [
        f"baseline:{reason}" for reason in _unstable(baseline_summary, budget["stability"])
    ] + [f"candidate:{reason}" for reason in _unstable(candidate_summary, budget["stability"])]
    if unstable_reasons:
        return {
            "gate_id": scenario["gate_id"],
            "status": "unstable",
            "reason_codes": unstable_reasons,
            "summary": {"baseline": baseline_summary, "candidate": candidate_summary},
            "checks": [],
        }

    checks: list[dict[str, Any]] = []
    steady = budget["steady_state"]
    steady_metrics = set(scenario["steady_metrics"])
    if "throughput" in steady_metrics:
        _check_min_ratio(
            checks,
            "steady.throughput_ratio",
            candidate_medians["goodput_bps"],
            baseline_medians["goodput_bps"],
            steady["throughput_ratio_min"],
        )
    if "latency_p95_us" in steady_metrics:
        _check_max_ratio(
            checks,
            "steady.latency_p95_ratio",
            candidate_medians["latency_p95_us"],
            baseline_medians["latency_p95_us"],
            steady["latency_p95_ratio_max"],
        )
    if "cpu_per_bit" in steady_metrics:
        _check_max_ratio(
            checks,
            "steady.cpu_per_bit_ratio",
            candidate_medians["cpu_per_bit"],
            baseline_medians["cpu_per_bit"],
            steady["cpu_per_bit_ratio_max"],
        )
    if "rss_peak_bytes" in steady_metrics:
        _check_max_ratio(
            checks,
            "steady.rss_peak_ratio",
            candidate_medians["rss_peak_bytes"],
            baseline_medians["rss_peak_bytes"],
            steady["rss_peak_ratio_max"],
        )
        rss_increase = candidate_medians["rss_peak_bytes"] - baseline_medians["rss_peak_bytes"]
        _check_max(
            checks,
            "steady.rss_peak_absolute_increase_bytes",
            max(rss_increase, Fraction(0)),
            steady["rss_peak_absolute_increase_bytes_max"],
        )
    if "queue_drop_packets" in steady_metrics:
        _check_max(
            checks,
            "steady.queue_drop_packets",
            max(sample["queue_drop_packets"] for sample in candidate["samples"]),
            steady["queue_drop_packets_max"],
        )
    if "allocations_per_inner_packet" in steady_metrics:
        _check_max_ratio(
            checks,
            "steady.allocations_per_inner_packet_ratio",
            candidate_medians["allocations_per_inner_packet"],
            baseline_medians["allocations_per_inner_packet"],
            steady["allocations_per_inner_packet_ratio_max"],
        )

    feature = budget["feature_acceptance"]
    if scenario["feature_acceptance"]:
        policy = scenario["policy"]
        if policy == "h2_high_bdp":
            _check_min_ratio(
                checks,
                "feature.h2_throughput",
                candidate_medians["goodput_bps"],
                baseline_medians["goodput_bps"],
                feature["h2_throughput_ratio_min"],
            )
            _check_max_ratio(
                checks,
                "feature.h2_latency",
                candidate_medians["latency_p95_us"],
                baseline_medians["latency_p95_us"],
                feature["h2_latency_ratio_max"],
            )
        elif policy == "h3_batch_io":
            baseline_syscalls = _aggregate_syscalls_per_datagram(baseline)
            candidate_syscalls = _aggregate_syscalls_per_datagram(candidate)
            _check_max(
                checks,
                "feature.syscalls_per_datagram",
                candidate_syscalls,
                steady["syscalls_per_datagram_max"],
            )
            _check_max_ratio(
                checks,
                "feature.syscall_reduction",
                candidate_syscalls,
                baseline_syscalls,
                feature["batch_syscall_ratio_max"],
            )
            _check_max_ratio(
                checks,
                "feature.batch_cpu_per_bit",
                candidate_medians["cpu_per_bit"],
                baseline_medians["cpu_per_bit"],
                feature["batch_cpu_per_bit_ratio_max"],
            )
        elif policy == "h3_allocation_rate":
            current = candidate_medians["allocations_per_inner_packet"]
            baseline_value = baseline_medians["allocations_per_inner_packet"]
            ratio = _fraction_decimal(current) / _fraction_decimal(baseline_value)
            absolute = _fraction_decimal(current)
            ratio_limit = _decimal(feature["allocation_ratio_max"])
            absolute_limit = _decimal(feature["allocation_per_packet_max"])
            checks.append(
                {
                    "id": "feature.allocation_reduction_or_absolute",
                    "passed": ratio <= ratio_limit or absolute <= absolute_limit,
                    "actual_ratio": float(ratio),
                    "ratio_limit": float(ratio_limit),
                    "actual_per_packet": float(absolute),
                    "absolute_limit": float(absolute_limit),
                }
            )
        elif policy == "queue_pressure":
            _check_max(
                checks,
                "feature.queue_drop_packets",
                max(sample["queue_drop_packets"] for sample in candidate["samples"]),
                steady["queue_drop_packets_max"],
            )
        elif policy == "pmtu_convergence":
            _check_max(
                checks,
                "feature.pmtu_stable_ms",
                max(sample["pmtu_stable_ms"] for sample in candidate["samples"]),
                steady["pmtu_stable_ms_max"],
            )
            _check_max(
                checks,
                "feature.send_error_spin_count",
                max(sample["send_error_spin_count"] for sample in candidate["samples"]),
                steady["send_error_spin_count_max"],
            )
            _check_max(
                checks,
                "feature.silent_truncation_packets",
                max(sample["silent_truncation_packets"] for sample in candidate["samples"]),
                0,
            )
            _check_min_ratio(
                checks,
                "feature.pmtu_goodput_ratio",
                candidate_medians["goodput_bps"],
                baseline_medians["goodput_bps"],
                feature["pmtu_goodput_ratio_min"],
            )
        elif policy == "quic_migration":
            interruption = candidate_medians["migration_interruption_p95_us"]
            reconnect = baseline_medians["full_reconnect_completion_us"]
            _check_max(
                checks,
                "feature.migration_interruption_p95_us",
                interruption,
                steady["migration_interruption_p95_us_max"],
            )
            _check_max(
                checks,
                "feature.migration_fallback_completion_us",
                max(sample["fallback_completion_us"] for sample in candidate["samples"]),
                steady["migration_fallback_completion_us_max"],
            )
            if reconnect < 1_000_000:
                _check_max_ratio(
                    checks,
                    "feature.migration_not_worse_than_fast_reconnect",
                    interruption,
                    reconnect,
                    1,
                )
            else:
                _check_max_ratio(
                    checks,
                    "feature.migration_improvement",
                    interruption,
                    reconnect,
                    feature["migration_interruption_ratio_max"],
                )
        elif policy == "direct_dns":
            total_queries = sum(sample["dns_queries"] for sample in candidate["samples"])
            total_successes = sum(sample["dns_successes"] for sample in candidate["samples"])
            if total_queries == 0:
                raise GateError("direct DNS samples must contain queries")
            _check_min(
                checks,
                "feature.dns_success_rate",
                Fraction(total_successes, total_queries),
                steady["dns_success_rate_min"],
            )
            _check_max(
                checks,
                "feature.physical_port_53_packets",
                max(sample["physical_port_53_packets"] for sample in candidate["samples"]),
                steady["physical_port_53_packets_max"],
            )
            _check_max(
                checks,
                "feature.plaintext_fallback_queries",
                max(sample["plaintext_fallback_queries"] for sample in candidate["samples"]),
                0,
            )
            latency_limit = max(
                baseline_medians["latency_p95_us"]
                * Fraction(str(feature["dns_latency_ratio_max"])),
                baseline_medians["latency_p95_us"]
                + int(feature["dns_latency_absolute_increase_us_max"]),
            )
            _check_max(
                checks,
                "feature.dns_latency_p95_us",
                candidate_medians["latency_p95_us"],
                latency_limit,
            )

    failed = [check["id"] for check in checks if not check["passed"]]
    return {
        "gate_id": scenario["gate_id"],
        "status": "passed" if not failed else "failed",
        "reason_codes": failed,
        "summary": {"baseline": baseline_summary, "candidate": candidate_summary},
        "checks": checks,
    }


def evaluate_gate_reports(
    baselines: list[dict[str, Any]],
    candidates: list[dict[str, Any]],
    scenarios: list[dict[str, Any]],
    budget: dict[str, Any],
) -> dict[str, Any]:
    """Require every scenario for a gate, without selecting the best profile."""
    required = {scenario["scenario_id"] for scenario in scenarios}
    if not required or len({scenario["gate_id"] for scenario in scenarios}) != 1:
        raise GateError("a performance comparison needs exactly one gate")
    indexed = []
    for role, reports in (("baseline", baselines), ("candidate", candidates)):
        if not isinstance(reports, list) or any(not isinstance(report, dict) for report in reports):
            raise GateError(f"{role} reports must be an array of objects")
        ids = [report.get("scenario_id") for report in reports]
        if (
            any(not isinstance(value, str) for value in ids)
            or len(set(ids)) != len(ids)
            or set(ids) != required
        ):
            raise GateError(
                f"{role} reports do not cover every required gate scenario exactly once"
            )
        indexed.append(dict(zip(ids, reports, strict=True)))
    comparisons = [
        evaluate_pair(
            indexed[0][scenario["scenario_id"]],
            indexed[1][scenario["scenario_id"]],
            scenario,
            budget,
        )
        for scenario in scenarios
    ]
    identities = {
        (report["baseline_commit"], report["candidate_commit"]) for report in baselines + candidates
    }
    if len(identities) != 1:
        raise GateError("gate reports use different baseline/candidate identities")
    if len(comparisons) == 1:
        return comparisons[0]
    status = next(
        value
        for value in ("failed", "unstable", "not_run", "passed")
        if any(result["status"] == value for result in comparisons)
    )
    return {
        "gate_id": scenarios[0]["gate_id"],
        "status": status,
        "reason_codes": [
            f"{scenario['scenario_id']}:{reason}"
            for scenario, result in zip(scenarios, comparisons, strict=True)
            for reason in result["reason_codes"]
        ],
        "summary": {
            scenario["scenario_id"]: result.get("summary", {})
            for scenario, result in zip(scenarios, comparisons, strict=True)
        },
        "checks": [
            {**check, "id": f"{scenario['scenario_id']}:{check['id']}"}
            for scenario, result in zip(scenarios, comparisons, strict=True)
            for check in result.get("checks", [])
        ],
    }


def _evidence_reference(path: Path, output_directory: Path) -> dict[str, str]:
    relative = path.relative_to(output_directory / "evidence").as_posix()
    return {"path": relative, "sha256": sha256_file(path)}


def _write_text_evidence(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text if text.endswith("\n") else text + "\n", encoding="utf-8")


def evaluate_directory(
    measurements_directory: Path,
    scenarios_path: Path,
    budget_path: Path,
    schema_path: Path,
    candidate_manifest: Path,
    candidate_commit: str,
    baseline_commit: str,
    output_directory: Path,
) -> bool:
    if not HEX_40.fullmatch(candidate_commit):
        raise GateError("candidate commit must be a lowercase 40-character SHA")
    if not HEX_40.fullmatch(baseline_commit):
        raise GateError("accepted baseline commit must be a lowercase 40-character SHA")
    validate_schema_contract(load_json(schema_path))
    scenarios = _load_scenarios(scenarios_path)
    budget = _load_budget(budget_path)
    expected_files = {
        f"{scenario['scenario_id']}-{role}.json"
        for scenario in scenarios
        for role in ("baseline", "candidate")
    }
    actual_files = {path.name for path in measurements_directory.glob("*.json") if path.is_file()}
    if actual_files != expected_files:
        raise GateError(
            "performance measurement file set mismatch; "
            f"missing={sorted(expected_files - actual_files)}, "
            f"unexpected={sorted(actual_files - expected_files)}"
        )
    if not candidate_manifest.is_file():
        raise GateError("candidate manifest is missing")
    manifest_digest = sha256_file(candidate_manifest)

    result_entries = []
    evidence_root = output_directory / "evidence" / "performance_lab"
    for gate_id in sorted(PERFORMANCE_GATES):
        gate_scenarios = [scenario for scenario in scenarios if scenario["gate_id"] == gate_id]
        baselines = [
            load_json(measurements_directory / f"{scenario['scenario_id']}-baseline.json")
            for scenario in gate_scenarios
        ]
        candidates = [
            load_json(measurements_directory / f"{scenario['scenario_id']}-candidate.json")
            for scenario in gate_scenarios
        ]
        comparison = evaluate_gate_reports(baselines, candidates, gate_scenarios, budget)
        for report in baselines + candidates:
            if report["candidate_commit"] != candidate_commit:
                raise GateError(f"{gate_id}: candidate commit does not match the release candidate")
            if report["baseline_commit"] != baseline_commit:
                raise GateError(f"{gate_id}: baseline commit does not match the accepted baseline")

        gate_slug = gate_id.replace(".", "-")
        raw_path = evidence_root / f"{gate_slug}-raw-samples.json"
        raw_bundle = {
            "schema_version": EVIDENCE_SCHEMA_VERSION,
            "gate_id": gate_id,
            "baseline_reports": baselines,
            "candidate_reports": candidates,
        }
        write_json(raw_path, raw_bundle)
        baseline_digest = sha256_bytes(canonical_json_bytes(baselines))
        candidate_digest = sha256_bytes(canonical_json_bytes(candidates))
        comparison.update(
            {
                "schema_version": EVIDENCE_SCHEMA_VERSION,
                "baseline_commit": baseline_commit,
                "candidate_commit": candidate_commit,
                "baseline_reports_sha256": baseline_digest,
                "candidate_reports_sha256": candidate_digest,
                "raw_samples_sha256": sha256_file(raw_path),
            }
        )
        comparison_path = evidence_root / f"{gate_slug}-comparison.json"
        write_json(comparison_path, comparison)
        timeline_path = evidence_root / f"{gate_slug}-timeline.json"
        write_json(
            timeline_path,
            {
                "schema_version": REPORT_SCHEMA_VERSION,
                "gate_id": gate_id,
                "status": comparison["status"],
                "checks": comparison.get("checks", []),
            },
        )
        platform_path = evidence_root / f"{gate_slug}-platform-diff.json"
        write_json(
            platform_path,
            {
                "schema_version": REPORT_SCHEMA_VERSION,
                "gate_id": gate_id,
                "scenarios": [
                    {
                        "scenario_id": baseline["scenario_id"],
                        "platform_class": baseline["platform_class"],
                        "network_profile_id": baseline["environment"]["network_profile_id"],
                        "major_runner": _major_version(
                            baseline["environment"]["tool_versions"]["runner"], "runner"
                        ),
                        "major_rustc": _major_version(
                            baseline["environment"]["tool_versions"]["rustc"], "rustc"
                        ),
                    }
                    for baseline in baselines
                ],
            },
        )
        junit_path = evidence_root / f"{gate_slug}-junit.xml"
        failure = (
            ""
            if comparison["status"] == "passed"
            else f'<failure message="{comparison["status"]}" />'
        )
        _write_text_evidence(
            junit_path,
            f'<testsuite name="{gate_id}" tests="1" failures="{int(bool(failure))}">'
            f'<testcase name="budget">{failure}</testcase></testsuite>',
        )
        evidence = {
            "junit": _evidence_reference(junit_path, output_directory),
            "timeline": _evidence_reference(timeline_path, output_directory),
            "platform_diff": _evidence_reference(platform_path, output_directory),
            "performance_report": _evidence_reference(comparison_path, output_directory),
            "raw_samples": _evidence_reference(raw_path, output_directory),
        }
        result = {"id": gate_id, "status": comparison["status"], "evidence": evidence}
        if comparison["status"] != "passed":
            result["reason_code"] = comparison["reason_codes"][0]
        result_entries.append(result)

    report = {
        "schema_version": RELIABILITY_SCHEMA_VERSION,
        "commit": candidate_commit,
        "candidate_manifest_sha256": manifest_digest,
        "environment": {
            "kind": "performance_lab",
            "version": "performance-evidence-v3",
            "runner_class": "usque-performance-lab",
        },
        "results": result_entries,
    }
    write_json(output_directory / "report.json", report)
    return all(result["status"] == "passed" for result in result_entries)


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    evaluate = subparsers.add_parser("evaluate")
    evaluate.add_argument("--measurements-directory", type=Path, required=True)
    evaluate.add_argument("--scenarios", type=Path, required=True)
    evaluate.add_argument("--budget", type=Path, required=True)
    evaluate.add_argument("--schema", type=Path, required=True)
    evaluate.add_argument("--candidate-manifest", type=Path, required=True)
    evaluate.add_argument("--candidate-commit", required=True)
    evaluate.add_argument("--baseline-commit", required=True)
    evaluate.add_argument("--output-directory", type=Path, required=True)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    try:
        passed = evaluate_directory(
            args.measurements_directory,
            args.scenarios,
            args.budget,
            args.schema,
            args.candidate_manifest,
            args.candidate_commit,
            args.baseline_commit,
            args.output_directory,
        )
    except GateError as error:
        print(f"performance gate failed: {error}", file=sys.stderr)
        return 1
    if not passed:
        print("performance gate failed: one or more scenarios did not pass", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
