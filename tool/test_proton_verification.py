"""Offline tests for official-host manual human-verification handoff encoding."""

from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import unittest
from unittest import mock
from urllib.parse import parse_qs, urlsplit

_HAVE_HTTPX = importlib.util.find_spec("httpx") is not None
if _HAVE_HTTPX:
    from proton_poc import verification
    from proton_poc.transport import PocError


@unittest.skipUnless(_HAVE_HTTPX, "Proton PoC optional HTTP dependency is not installed")
class ProtonVerificationTests(unittest.TestCase):
    def details(self, **changes):
        return {
            "HumanVerificationToken": "synthetic-private-challenge",
            "HumanVerificationMethods": ["captcha", "email"],
            **changes,
        }

    def success(self, *, value="synthetic-private-success", method="captcha", **changes):
        return json.dumps(
            {
                "type": "HUMAN_VERIFICATION_SUCCESS",
                "payload": {"token": value, "type": method},
                **changes,
            }
        )

    def challenge(self):
        return verification.VerificationChallenge.from_details(self.details())

    def test_details_accept_only_bounded_token_and_supported_method_schema(self):
        challenge = self.challenge()
        self.assertEqual(challenge.token, "synthetic-private-challenge")
        self.assertEqual(challenge.methods, ("captcha", "email"))
        for methods in (["captcha"], ["email"], ["sms"], ["captcha", "unrecognized"]):
            with self.subTest(methods=methods):
                parsed = verification.VerificationChallenge.from_details(
                    self.details(HumanVerificationMethods=methods)
                )
                self.assertEqual(parsed.methods, tuple(methods))

    def test_invalid_details_fail_closed_without_echoing_token(self):
        inputs = [
            None,
            [],
            {},
            self.details(HumanVerificationToken=None),
            self.details(HumanVerificationToken=""),
            self.details(HumanVerificationToken="synthetic-private-challenge" * 400),
            self.details(HumanVerificationMethods="captcha"),
            self.details(HumanVerificationMethods=[]),
            self.details(HumanVerificationMethods=["captcha"] * 17),
            self.details(HumanVerificationMethods=["captcha", ""]),
            self.details(HumanVerificationMethods=["captcha", "line\nbreak"]),
            self.details(HumanVerificationMethods=["captcha", "x" * 33]),
            self.details(HumanVerificationMethods=["captcha", 1]),
            self.details(HumanVerificationMethods=["captcha", "验证"]),
            self.details(HumanVerificationMethods=["unrecognized"]),
        ]
        for details in inputs:
            with (
                self.subTest(input_type=type(details).__name__),
                self.assertRaises(PocError) as raised,
            ):
                verification.VerificationChallenge.from_details(details)
            self.assertNotIn("synthetic-private-challenge", str(raised.exception))

    def test_url_uses_fixed_official_host_and_encodes_reserved_characters(self):
        challenge_value = "synthetic-private-challenge+&token=replace?#/ü\n"
        challenge = verification.VerificationChallenge.from_details(
            self.details(
                HumanVerificationToken=challenge_value, URL="https://attacker.invalid/redirect"
            )
        )
        url = challenge.url()
        parsed = urlsplit(url)
        self.assertEqual(parsed.scheme, "https")
        self.assertEqual(parsed.netloc, "verify.proton.me")
        self.assertEqual(parsed.fragment, "")
        self.assertEqual(
            parse_qs(parsed.query),
            {
                "embed": ["true"],
                "token": [challenge_value],
                "methods": ["captcha,email"],
                "theme": ["2"],
                "vpn": ["true"],
            },
        )
        self.assertNotIn("attacker.invalid", url)
        self.assertNotIn("\n", url)

    def test_success_json_produces_only_request_local_verification_headers(self):
        challenge = self.challenge()
        self.assertEqual(
            challenge.success_headers(self.success()),
            {
                "x-pm-human-verification-token": "synthetic-private-success",
                "x-pm-human-verification-token-type": "captcha",
            },
        )
        self.assertEqual(challenge.token, "synthetic-private-challenge")
        self.assertEqual(challenge.methods, ("captcha", "email"))

    def test_success_method_must_belong_to_this_challenge_and_be_supported(self):
        challenge = self.challenge()
        for method in ("sms", "unrecognized", "Captcha", None, [], 1):
            with self.subTest(method_type=type(method).__name__), self.assertRaises(PocError):
                challenge.success_headers(self.success(method=method))
        # Merely being listed by the server does not enable an unsupported mode.
        unknown = verification.VerificationChallenge.from_details(
            self.details(HumanVerificationMethods=["captcha", "unrecognized"])
        )
        with self.assertRaises(PocError):
            unknown.success_headers(self.success(method="unrecognized"))

    def test_expired_challenge_cannot_produce_headers(self):
        challenge = self.challenge()
        with mock.patch.object(
            verification.time, "monotonic", return_value=challenge.created_at + 301
        ):
            with self.assertRaises(PocError) as raised:
                challenge.success_headers(self.success())
        self.assertEqual(str(raised.exception), "human_verification_expired")
        with mock.patch.object(
            verification.time, "monotonic", return_value=challenge.created_at + 299
        ):
            self.assertEqual(
                challenge.success_headers(self.success())["x-pm-human-verification-token-type"],
                "captcha",
            )

    def test_invalid_success_json_and_header_injection_are_rejected(self):
        challenge = self.challenge()
        inputs = [
            None,
            "",
            "not-json-synthetic-private-success",
            "{}",
            "[]",
            "null",
            '["synthetic-private-success"]',
            '{"type":"HUMAN_VERIFICATION_SUCCESS","payload":null}',
            self.success(type="HUMAN_VERIFICATION_CANCEL"),
            self.success(value=""),
            self.success(value=None),
            self.success(value="synthetic-private-success\r\nx-injected: true"),
            self.success(value="synthetic-private-success with space"),
            self.success(value="synthetic-private-success\x7f"),
            self.success(value="synthetic-private-success验证"),
            self.success(value="x" * 8193),
            "x" * 32769,
            "[" * 1200 + "]" * 1200,
        ]
        for message in inputs:
            with (
                self.subTest(message_type=type(message).__name__),
                self.assertRaises(PocError) as raised,
            ):
                challenge.success_headers(message)
            self.assertNotIn("synthetic-private-success", str(raised.exception))

    def test_bounded_deterministic_malformed_json_corpus(self):
        challenge = self.challenge()
        valid = self.success()
        corpus = {valid[:index] for index in range(len(valid))}
        corpus.update(chr(control) + valid for control in range(32) if control not in (9, 10, 13))
        corpus.update(("[" * depth + "]" * depth) for depth in (1, 2, 10, 100, 1200))
        corpus.update(("\0", "{", "}", '"unterminated', "true", "false", "0", "NaN"))
        for malformed in sorted(corpus):
            with self.subTest(length=len(malformed)), self.assertRaises(PocError):
                challenge.success_headers(malformed)

    def test_repr_and_logs_do_not_contain_challenge_or_success_tokens(self):
        output = io.StringIO()
        with contextlib.redirect_stdout(output), contextlib.redirect_stderr(output):
            challenge = self.challenge()
            challenge.url()
            challenge.success_headers(self.success())
            with self.assertRaises(PocError):
                challenge.success_headers(self.success(value="synthetic-private-success\n"))
        self.assertEqual(output.getvalue(), "")
        self.assertNotIn("synthetic-private-challenge", repr(challenge))
        self.assertNotIn("synthetic-private-success", repr(challenge))


if __name__ == "__main__":
    unittest.main()
