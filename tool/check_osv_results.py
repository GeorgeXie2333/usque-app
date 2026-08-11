"""Classify OSV-Scanner JSON and enforce expiring vulnerability exceptions."""

from __future__ import annotations

import argparse
import json
import math
import os
import sys
from collections.abc import Iterable
from datetime import date
from pathlib import Path
from typing import Any

import tomllib

SEVERITY_ORDER = {"low": 1, "medium": 2, "high": 3, "critical": 4, "unknown": 5}
LABEL_ALIASES = {
    "LOW": "low",
    "MODERATE": "medium",
    "MEDIUM": "medium",
    "HIGH": "high",
    "CRITICAL": "critical",
}


def _round_up_tenth(value: float) -> float:
    return math.ceil(value * 10.0 - 1e-10) / 10.0


def _cvss_v3_score(vector: str) -> float | None:
    """Calculate a CVSS 3.x base score from an OSV severity vector."""

    if not vector.startswith(("CVSS:3.0/", "CVSS:3.1/")):
        return None
    try:
        metrics = dict(item.split(":", maxsplit=1) for item in vector.split("/")[1:])
        scope_changed = metrics["S"] == "C"
        av = {"N": 0.85, "A": 0.62, "L": 0.55, "P": 0.2}[metrics["AV"]]
        ac = {"L": 0.77, "H": 0.44}[metrics["AC"]]
        pr_values = (
            {"N": 0.85, "L": 0.68, "H": 0.5} if scope_changed else {"N": 0.85, "L": 0.62, "H": 0.27}
        )
        pr = pr_values[metrics["PR"]]
        ui = {"N": 0.85, "R": 0.62}[metrics["UI"]]
        impact_values = {"H": 0.56, "L": 0.22, "N": 0.0}
        confidentiality = impact_values[metrics["C"]]
        integrity = impact_values[metrics["I"]]
        availability = impact_values[metrics["A"]]
    except (KeyError, ValueError):
        return None

    impact_base = 1 - (1 - confidentiality) * (1 - integrity) * (1 - availability)
    if scope_changed:
        impact = 7.52 * (impact_base - 0.029) - 3.25 * (impact_base - 0.02) ** 15
    else:
        impact = 6.42 * impact_base
    if impact <= 0:
        return 0.0
    exploitability = 8.22 * av * ac * pr * ui
    base = (
        min(1.08 * (impact + exploitability), 10.0)
        if scope_changed
        else min(impact + exploitability, 10.0)
    )
    return _round_up_tenth(base)


def _severity_scores(vulnerability: dict[str, Any]) -> list[float]:
    scores: list[float] = []
    for item in vulnerability.get("severity", []):
        if not isinstance(item, dict):
            continue
        raw_score = item.get("score")
        if isinstance(raw_score, (int, float)):
            scores.append(float(raw_score))
        elif isinstance(raw_score, str):
            try:
                scores.append(float(raw_score))
            except ValueError:
                calculated = _cvss_v3_score(raw_score)
                if calculated is not None:
                    scores.append(calculated)
    return scores


def classify_vulnerability(vulnerability: dict[str, Any]) -> tuple[str, float | None]:
    """Return normalized severity and the highest numeric score, if present."""

    labels: list[str] = []
    for container_name in ("database_specific", "ecosystem_specific"):
        container = vulnerability.get(container_name)
        if not isinstance(container, dict):
            continue
        raw_label = container.get("severity")
        if isinstance(raw_label, str) and raw_label.upper() in LABEL_ALIASES:
            labels.append(LABEL_ALIASES[raw_label.upper()])

    scores = _severity_scores(vulnerability)
    highest_score = max(scores, default=None)
    if highest_score is not None:
        if highest_score >= 9.0:
            labels.append("critical")
        elif highest_score >= 7.0:
            labels.append("high")
        elif highest_score >= 4.0:
            labels.append("medium")
        elif highest_score > 0:
            labels.append("low")
    if not labels:
        return "unknown", highest_score
    return max(labels, key=SEVERITY_ORDER.__getitem__), highest_score


def validate_exceptions(path: Path, today: date | None = None) -> list[str]:
    """Require every OSV ignore entry to have an ID, reason, and future expiry."""

    errors: list[str] = []
    current_date = today or date.today()
    data = tomllib.loads(path.read_text(encoding="utf-8"))
    entries = data.get("IgnoredVulns", [])
    if not isinstance(entries, list):
        return ["IgnoredVulns must be an array of tables"]
    for index, entry in enumerate(entries, start=1):
        prefix = f"IgnoredVulns[{index}]"
        if not isinstance(entry, dict):
            errors.append(f"{prefix} must be a table")
            continue
        identifier = entry.get("id")
        reason = entry.get("reason")
        expiry = entry.get("ignoreUntil")
        if not isinstance(identifier, str) or not identifier.strip():
            errors.append(f"{prefix} requires a vulnerability id")
        if not isinstance(reason, str) or len(reason.strip()) < 20:
            errors.append(f"{prefix} requires a specific reason of at least 20 characters")
        if not isinstance(expiry, date):
            errors.append(f"{prefix} requires an ISO date in ignoreUntil")
        elif expiry < current_date:
            errors.append(f"{prefix} expired on {expiry.isoformat()}")
    return errors


def _vulnerabilities(data: dict[str, Any]) -> Iterable[tuple[str, str, dict[str, Any]]]:
    for result in data.get("results", []):
        if not isinstance(result, dict):
            continue
        source = result.get("source", {})
        source_path = source.get("path", "unknown") if isinstance(source, dict) else "unknown"
        for package_entry in result.get("packages", []):
            if not isinstance(package_entry, dict):
                continue
            package = package_entry.get("package", {})
            if isinstance(package, dict):
                name = f"{package.get('name', 'unknown')}@{package.get('version', 'unknown')}"
            else:
                name = "unknown"
            for vulnerability in package_entry.get("vulnerabilities", []):
                if isinstance(vulnerability, dict):
                    yield str(source_path), name, vulnerability


def summarize(path: Path, heading: str, blocking: bool) -> tuple[list[str], bool]:
    """Create Markdown summary lines and return whether blocking findings exist."""

    data = json.loads(path.read_text(encoding="utf-8"))
    lines = [
        f"## {heading}",
        "",
        "| Severity | Advisory | Package | Source |",
        "| --- | --- | --- | --- |",
    ]
    has_findings = False
    has_blockers = False
    seen: set[tuple[str, str, str]] = set()
    for source, package, vulnerability in _vulnerabilities(data):
        identifier = str(vulnerability.get("id", "UNKNOWN"))
        key = (identifier, package, source)
        if key in seen:
            continue
        seen.add(key)
        has_findings = True
        severity, score = classify_vulnerability(vulnerability)
        display = severity.title() + (f" ({score:.1f})" if score is not None else "")
        lines.append(f"| {display} | `{identifier}` | `{package}` | `{source}` |")
        if blocking and severity in {"high", "critical", "unknown"}:
            has_blockers = True
    if not has_findings:
        lines.append("| None | — | — | — |")
    lines.append("")
    if not blocking and has_findings:
        lines.append("The archived Go oracle is non-shipping and informational only.")
        lines.append("")
    return lines, has_blockers


def _write_summary(lines: list[str]) -> None:
    text = "\n".join(lines)
    print(text)
    summary_path = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary_path:
        with Path(summary_path).open("a", encoding="utf-8", newline="\n") as summary:
            summary.write(text + "\n")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--shipping", type=Path, required=True)
    parser.add_argument("--oracle", type=Path)
    parser.add_argument("--config", type=Path, default=Path("osv-scanner.toml"))
    arguments = parser.parse_args()

    errors = validate_exceptions(arguments.config)
    if errors:
        for error in errors:
            print(f"OSV_POLICY_ERROR: {error}", file=sys.stderr)
        return 1

    lines, has_blockers = summarize(arguments.shipping, "Shipping dependency scan", True)
    if arguments.oracle:
        oracle_lines, _ = summarize(arguments.oracle, "Archived Go oracle scan", False)
        lines.extend(oracle_lines)
    _write_summary(lines)
    if has_blockers:
        print("OSV_POLICY_ERROR: high, critical, or unclassified finding", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
