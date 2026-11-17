"""Fail closed if the narrowly excepted chrono maintenance notice changes."""

from __future__ import annotations

import hashlib
import json
import sys
from pathlib import Path
from urllib.error import URLError
from urllib.request import urlopen

import tomllib

from check_osv_results import validate_exceptions

ADVISORY_ID = "RUSTSEC-2026-0543"
ADVISORY_URL = "https://api.osv.dev/v1/vulns/RUSTSEC-2026-0543"
DETAILS_SHA256 = "e7e07401eab5b5f290db65820da8b88eb2b51dcdf477d01a458b48cbb2136f49"
MAX_BYTES = 1_000_000


def validate_advisory(advisory: object) -> None:
    """Accept only the reviewed, non-vulnerability maintenance notice."""

    if not isinstance(advisory, dict):
        raise ValueError("OSV advisory is not an object")
    details = advisory.get("details")
    if (
        advisory.get("id") != ADVISORY_ID
        or advisory.get("summary") != "chrono is unmaintained"
        or not isinstance(details, str)
        or hashlib.sha256(details.encode("utf-8")).hexdigest() != DETAILS_SHA256
        or advisory.get("aliases") not in (None, [])
        or advisory.get("severity") not in (None, [])
        or advisory.get("withdrawn") is not None
    ):
        raise ValueError("OSV advisory identity, text or severity changed")
    top = advisory.get("database_specific", {})
    if not isinstance(top, dict) or top.get("severity") or top.get("cvss"):
        raise ValueError("OSV advisory has additional security severity")
    affected = advisory.get("affected")
    if not isinstance(affected, list) or len(affected) != 1 or not isinstance(affected[0], dict):
        raise ValueError("OSV advisory affected-package scope changed")
    entry = affected[0]
    package = entry.get("package")
    database = entry.get("database_specific")
    ecosystem = entry.get("ecosystem_specific")
    affects = ecosystem.get("affects") if isinstance(ecosystem, dict) else None
    if (
        not isinstance(package, dict)
        or package.get("name") != "chrono"
        or package.get("ecosystem") != "crates.io"
        or entry.get("ranges") != [{"type": "SEMVER", "events": [{"introduced": "0.0.0-0"}]}]
        or entry.get("versions") not in (None, [])
        or entry.get("severity") not in (None, [])
        or not isinstance(database, dict)
        or database.get("informational") != "unmaintained"
        or database.get("severity")
        or database.get("cvss")
        or database.get("categories") not in (None, [])
        or not isinstance(ecosystem, dict)
        or ecosystem.get("affected_functions") not in (None, [])
        or not isinstance(affects, dict)
        or any(affects.get(key) not in (None, []) for key in ("functions", "arch", "os"))
    ):
        raise ValueError("OSV advisory is no longer exclusively unmaintained chrono")


def main() -> int:
    config = Path(__file__).resolve().parent.parent / "osv-scanner.toml"
    errors = validate_exceptions(config)
    if errors:
        print("OSV_MAINTENANCE_ERROR: " + "; ".join(errors), file=sys.stderr)
        return 1
    entries = tomllib.loads(config.read_text(encoding="utf-8")).get("IgnoredVulns", [])
    matches = [entry for entry in entries if entry.get("id") == ADVISORY_ID]
    if not matches:
        print("No chrono maintenance exception configured")
        return 0
    if len(matches) != 1:
        print("OSV_MAINTENANCE_ERROR: duplicate chrono exception", file=sys.stderr)
        return 1
    try:
        with urlopen(ADVISORY_URL, timeout=15) as response:  # noqa: S310 -- fixed HTTPS OSV endpoint
            raw = response.read(MAX_BYTES + 1)
        if len(raw) > MAX_BYTES:
            raise ValueError("OSV advisory exceeds the size limit")
        validate_advisory(json.loads(raw))
    except (OSError, URLError, ValueError) as error:
        print(f"OSV_MAINTENANCE_ERROR: {error}", file=sys.stderr)
        return 1
    print("OSV_MAINTENANCE_NOTICE_UNCHANGED")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
