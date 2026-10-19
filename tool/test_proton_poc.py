"""Offline checks for the standalone PoC; no Proton accounts or network required."""

import contextlib
import importlib.util
import io
import unittest
from unittest.mock import patch

HAS_HTTPX = importlib.util.find_spec("httpx") is not None
if HAS_HTTPX:
    import httpx

    from proton_poc.transport import ApiError, PocError, Session, Transport


@unittest.skipUnless(HAS_HTTPX, "install tool/proton_poc/requirements.lock in a dedicated venv")
class TransportTests(unittest.TestCase):
    def test_official_error_codes_do_not_confuse_password_and_client_version(self):
        for code, category in (
            (8002, "wrong_password"),
            (5003, "unsupported_client_version"),
            (5005, "unsupported_api_version"),
            (9002, "device_verification_required"),
            (8004, "api_rejected"),
        ):
            with self.subTest(code=code):
                self.assertEqual(ApiError(422, code, {}).category, category)

    def make(self, handler, **kwargs):
        client = httpx.Client(transport=httpx.MockTransport(handler))
        self.addCleanup(client.close)
        return Transport(app_version="external-vpn@0.1.0", direct=True, proxy=None, client=client)

    def test_sensitive_error_body_and_exception_are_not_logged(self):
        marker = "synthetic-secret-do-not-log"
        output = io.StringIO()
        transport = self.make(
            lambda _: httpx.Response(
                403, json={"Code": 9001, "Error": marker, "Details": {"Token": marker}}
            )
        )
        with contextlib.redirect_stdout(output), contextlib.redirect_stderr(output):
            with self.assertRaises(ApiError) as error:
                transport.request("POST", "/auth/v4")
        self.assertEqual(error.exception.category, "human_verification_required")
        self.assertNotIn(marker, str(error.exception))
        self.assertNotIn(marker, repr(error.exception))
        self.assertEqual(output.getvalue(), "")

    def test_post_is_never_retried_on_rate_limit(self):
        requests = []

        def handler(request):
            requests.append(request)
            return httpx.Response(429, json={"Code": 429}, headers={"Retry-After": "1"})

        with self.assertRaises(ApiError):
            self.make(handler).request("POST", "/auth/v4/sessions", {})
        self.assertEqual(len(requests), 1)

    def test_get_retry_is_bounded_and_respects_retry_after(self):
        requests = []

        def handler(request):
            requests.append(request)
            return httpx.Response(429, json={"Code": 429}, headers={"Retry-After": "2"})

        with patch("proton_poc.transport.time.sleep") as sleep:
            with self.assertRaises(ApiError):
                self.make(handler).request("GET", "/vpn/v2/logicals")
        self.assertEqual(len(requests), 2)
        sleep.assert_called_once_with(2)

    def test_oversized_and_non_json_and_redirect_bodies_fail_closed(self):
        with patch("proton_poc.transport.MAX_BODY", 100):
            with self.assertRaisesRegex(PocError, "response_too_large"):
                self.make(lambda _: httpx.Response(200, content=b"x" * 101)).request("GET", "/x")
        for response in (
            httpx.Response(200, content=b"<html>synthetic-secret</html>"),
            httpx.Response(
                302, headers={"Location": "https://example.invalid/secret"}, content=b""
            ),
        ):
            with self.subTest(response=response.status_code), self.assertRaises(PocError):
                self.make(lambda _, response=response: response).request("GET", "/x")

    def test_tls_errors_are_redacted_and_no_direct_fallback(self):
        calls = []

        def handler(request):
            calls.append(request)
            raise httpx.ConnectError("synthetic-private-proxy", request=request)

        with self.assertRaisesRegex(PocError, "transport_failed") as error:
            self.make(handler).request("GET", "/x")
        self.assertNotIn("synthetic-private-proxy", str(error.exception))
        self.assertEqual(len(calls), 1)

    def test_explicit_network_path_and_proxy_validation(self):
        for direct, proxy in (
            (False, None),
            (True, "http://localhost:1080"),
            (False, "socks5://localhost:1080"),
            (False, "http://user:secret@localhost:1080"),
        ):
            with self.subTest(direct=direct, proxy=proxy), self.assertRaises(PocError):
                Transport(app_version="external-vpn@0.1.0", direct=direct, proxy=proxy)

    def test_refresh_replaces_all_fields_atomically(self):
        session = Session("old-uid", "old-access", "old-refresh")
        with self.assertRaises(PocError):
            session.replace({"UID": "new-uid", "AccessToken": "new-access"})
        self.assertEqual(session.refresh_token, "old-refresh")
        session.replace(
            {"UID": "new-uid", "AccessToken": "new-access", "RefreshToken": "new-refresh"}
        )
        self.assertEqual(session.refresh_token, "new-refresh")
        self.assertNotIn("new-refresh", repr(session))
        session.clear()
        self.assertEqual(session.uid, "")


if __name__ == "__main__":
    unittest.main()
