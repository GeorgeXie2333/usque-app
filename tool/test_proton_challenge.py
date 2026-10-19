"""Offline wire-contract checks using synthetic device facts."""

import datetime as dt
import json
import os
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

from proton_poc.challenge import FRAME_NAME, _timezone_name, build_payload, rolling_hash


class ChallengeTests(unittest.TestCase):
    def setUp(self):
        for name, value in (
            ("statvfs", SimpleNamespace(f_blocks=100_000, f_frsize=4096)),
            ("geteuid", 1000),
        ):
            fixture = patch("proton_poc.challenge.os." + name, return_value=value, create=True)
            fixture.start()
            self.addCleanup(fixture.stop)

    def test_signed_utf8_bytes_and_unreduced_sum(self):
        self.assertEqual(rolling_hash(""), 0)
        self.assertEqual(rolling_hash("abc"), 97 + 98 * 7 + 99 * 49)
        # U+00E9 has UTF-8 bytes C3 A9, signed as -61 and -87.
        self.assertEqual(rolling_hash("é"), -61 - 87 * 7)
        self.assertEqual(rolling_hash("aaaaaaaaaaa"), 12_566_780_971)

    @patch("proton_poc.challenge._timezone_name", return_value="Asia/Tokyo")
    @patch("proton_poc.challenge.locale.getlocale", return_value=("ja_JP", "UTF-8"))
    @patch("proton_poc.challenge.platform.system", return_value="Linux")
    @patch("proton_poc.challenge.platform.machine", return_value="x86_64")
    @patch("proton_poc.challenge.os.geteuid", return_value=1000)
    def test_frame_is_third_party_and_contains_no_account_or_activity(self, *_mocks):
        frame = build_payload()["Payload"][FRAME_NAME]
        self.assertEqual(frame["v"], "2.0.7")
        self.assertEqual(frame["appLang"], "ja")
        self.assertEqual(frame["regionCode"], "JP")
        self.assertEqual(frame["deviceName"], rolling_hash("Linux/usque x86_64"))
        self.assertEqual(frame["timezone"], "Asia/Tokyo")
        self.assertFalse(frame["isJailbreak"])
        self.assertEqual(frame["preferredContentSize"], "")
        self.assertEqual(frame["keyboards"], [])
        self.assertIsInstance(frame["timezoneOffset"], int)
        self.assertIsInstance(frame["storageCapacity"], float)
        self.assertEqual(
            set(frame),
            {
                "v",
                "appLang",
                "timezone",
                "deviceName",
                "regionCode",
                "timezoneOffset",
                "isJailbreak",
                "preferredContentSize",
                "storageCapacity",
                "isDarkmodeOn",
                "keyboards",
            },
        )
        encoded = json.dumps(build_payload())
        self.assertNotIn("vpn-android", encoded)
        self.assertNotIn("timeUsername", encoded)

    def test_timezone_name_has_safe_fallback(self):
        with (
            patch.dict(os.environ, {}, clear=True),
            patch("proton_poc.challenge.Path.resolve", return_value=Path("/unavailable/zone")),
        ):
            self.assertEqual(_timezone_name(dt.datetime(2026, 1, 1, tzinfo=dt.timezone.utc)), "UTC")

    def test_timezone_uses_runtime_override_and_correct_offset_sign(self):
        now = dt.datetime(2026, 1, 1, tzinfo=dt.timezone(dt.timedelta(hours=9)))
        with patch.dict(os.environ, {"TZ": "Asia/Tokyo"}):
            self.assertEqual(_timezone_name(now), "Asia/Tokyo")
        with patch("proton_poc.challenge.dt.datetime") as clock:
            clock.now.return_value.astimezone.return_value = now
            self.assertEqual(build_payload()["Payload"][FRAME_NAME]["timezoneOffset"], -540)

    def test_unknown_locale_does_not_invent_language_or_region(self):
        with patch("proton_poc.challenge.locale.getlocale", return_value=(None, None)):
            frame = build_payload()["Payload"][FRAME_NAME]
        self.assertEqual(frame["appLang"], "")
        self.assertEqual(frame["regionCode"], "")


if __name__ == "__main__":
    unittest.main()
