"""Device-frame wire encoding, with an explicitly third-party Linux namespace.

This is a fresh implementation of the documented wire contract. It sends no
Android model, package identifier, attestation, or invented input activity.
Neither this namespace nor these Linux facts have a verified server contract.
"""

from __future__ import annotations

import datetime as dt
import locale
import os
import platform
from pathlib import Path
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError

FRAME_NAME = "usque-linux-v4-challenge-0"
CHALLENGE_VERSION = "2.0.7"


def rolling_hash(value: str) -> int:
    """Encode a device description using signed UTF-8 bytes and base-7 weights.

    Only the weights are reduced modulo 100000007. The sum is not reduced.
    Android's signed Byte representation matters for non-ASCII descriptions.
    """
    weight = 1
    result = 0
    for byte in value.encode("utf-8"):
        result += (byte if byte < 128 else byte - 256) * weight
        weight = weight * 7 % 100_000_007
    return result


def _timezone_name(now: dt.datetime) -> str:
    # Read the zone name, never a machine name or persistent device identifier.
    configured = os.environ.get("TZ")
    if configured:
        try:
            return ZoneInfo(configured.removeprefix(":")).key
        except (ValueError, ZoneInfoNotFoundError):
            return now.tzname() or "UTC"
    target = Path("/etc/localtime").resolve()
    if "/zoneinfo/" in str(target):
        return str(target).split("/zoneinfo/", 1)[1]
    return now.tzname() or "UTC"


def build_payload() -> dict[str, dict[str, dict[str, object]]]:
    """Return Linux facts in the frame schema; acceptance remains unverified.

    Nonexistent GUI concepts use explicit CLI defaults: no keyboard packages,
    no dark theme, and an empty font scale. These are not Android measurements.
    The frame stays in memory and must not be logged or persisted in reports.
    """
    now = dt.datetime.now().astimezone()
    offset = now.utcoffset() or dt.timedelta()
    locale_name = locale.getlocale()[0] or ""
    language, _, region = locale_name.partition("_")
    filesystem = os.statvfs("/")
    capacity_gb = filesystem.f_blocks * filesystem.f_frsize // 1_000_000_000
    description = f"{platform.system()}/usque {platform.machine()}"
    return {
        "Payload": {
            FRAME_NAME: {
                "v": CHALLENGE_VERSION,
                "appLang": language,
                "timezone": _timezone_name(now),
                "deviceName": rolling_hash(description),
                "regionCode": region,
                "timezoneOffset": -int(offset.total_seconds() / 60),
                "isJailbreak": os.geteuid() == 0,
                "preferredContentSize": "",
                "storageCapacity": float(capacity_gb),
                "isDarkmodeOn": False,
                "keyboards": [],
            }
        }
    }
