"""Regression tests for bounded responses and non-sensitive proxy setup errors."""

from __future__ import annotations

import importlib.util
import unittest
from types import SimpleNamespace
from unittest import mock

_HAVE_HTTPX = importlib.util.find_spec("httpx") is not None
if _HAVE_HTTPX:
    import httpx

    from proton_poc import transport


@unittest.skipUnless(_HAVE_HTTPX, "Proton PoC optional HTTP dependency is not installed")
class ProtonTransportSecurityTests(unittest.TestCase):
    def adapter(self, handler):
        client = httpx.Client(transport=httpx.MockTransport(handler))
        self.addCleanup(client.close)
        return transport.Transport(
            app_version="external-vpn@0.1.0", direct=True, proxy=None, client=client
        )

    def test_compressed_response_is_rejected_before_body_read(self):
        class UnreadableBody(httpx.SyncByteStream):
            consumed = False
            closed = False

            def __iter__(self):
                self.consumed = True
                raise AssertionError("Compressed body must not be decoded")
                yield b""  # pragma: no cover

            def close(self):
                self.closed = True

        body = UnreadableBody()

        def handler(request):
            self.assertEqual(request.headers["accept-encoding"], "identity")
            return httpx.Response(200, headers={"Content-Encoding": "gzip"}, stream=body)

        adapter = self.adapter(handler)
        with self.assertRaises(transport.PocError) as raised:
            adapter.request("GET", "/vpn/v1/example")
        self.assertEqual(raised.exception.category, "unsupported_content_encoding")
        self.assertFalse(body.consumed)
        self.assertTrue(body.closed)

    def test_trickle_deadline_checks_each_received_block(self):
        clock = SimpleNamespace(now=0.0)

        class TrickleBody(httpx.SyncByteStream):
            yielded = 0
            closed = False

            def __iter__(self):
                for _ in range(8):
                    clock.now += 16
                    self.yielded += 1
                    yield b" "

            def close(self):
                self.closed = True

        body = TrickleBody()
        adapter = self.adapter(lambda request: httpx.Response(200, stream=body))
        with (
            mock.patch.object(transport, "time", SimpleNamespace(monotonic=lambda: clock.now)),
            self.assertRaises(transport.PocError) as raised,
        ):
            adapter.request("GET", "/vpn/v1/example")
        self.assertEqual(raised.exception.category, "response_deadline")
        self.assertEqual(body.yielded, 3)
        self.assertTrue(body.closed)

    def test_identity_response_size_limit_is_enforced(self):
        class LargeBody(httpx.SyncByteStream):
            yielded = 0
            closed = False

            def __iter__(self):
                for _ in range(10):
                    self.yielded += 1
                    yield b"x" * 8

            def close(self):
                self.closed = True

        body = LargeBody()
        adapter = self.adapter(lambda request: httpx.Response(200, stream=body))
        with (
            mock.patch.object(transport, "MAX_BODY", 12),
            self.assertRaises(transport.PocError) as raised,
        ):
            adapter.request("GET", "/vpn/v1/example")
        self.assertEqual(raised.exception.category, "response_too_large")
        self.assertEqual(body.yielded, 2)
        self.assertTrue(body.closed)

    def test_malformed_or_credentialed_proxy_is_sanitized_before_client_creation(self):
        for proxy in (
            "http://[invalid-ipv6:8080",
            "http://127.0.0.1:not-a-port",
            "http://127.0.0.1:65536",
            "http://127.0.0.1",
            "http://name:private-secret@127.0.0.1:8080",
            "socks5://127.0.0.1:1080",
            "http://127.0.0.1:8080/path?private-secret",
        ):
            with (
                self.subTest(proxy_scheme=proxy.split(":", 1)[0]),
                mock.patch.object(transport.httpx, "Client") as client,
                self.assertRaises(transport.PocError) as raised,
            ):
                transport.Transport(app_version="external-vpn@0.1.0", direct=False, proxy=proxy)
            client.assert_not_called()
            self.assertNotIn("private-secret", str(raised.exception))
            self.assertNotIn(proxy, str(raised.exception))

    def test_client_setup_errors_do_not_expose_proxy_values(self):
        for error in (
            ValueError("private-secret"),
            OSError("private-secret"),
            ImportError("private-secret"),
            httpx.InvalidURL("private-secret"),
        ):
            with (
                self.subTest(error_type=type(error).__name__),
                mock.patch.object(transport.httpx, "Client", side_effect=error),
                self.assertRaises(transport.PocError) as raised,
            ):
                transport.Transport(
                    app_version="external-vpn@0.1.0", direct=False, proxy="socks5h://127.0.0.1:1080"
                )
            self.assertEqual(str(raised.exception), "transport_setup_failed")
            self.assertNotIn("private-secret", str(raised.exception))

    def test_invalid_idna_proxy_has_no_raw_httpx_exception(self):
        with self.assertRaises(transport.PocError) as raised:
            transport.Transport(
                app_version="external-vpn@0.1.0",
                direct=False,
                proxy="http://synthetic-private-\ud800:1080",
            )
        self.assertEqual(str(raised.exception), "transport_setup_failed")
        self.assertNotIn("synthetic-private", str(raised.exception))

    def test_human_verification_get_is_not_automatically_retried_on_rate_limit(self):
        calls = []

        def handler(request):
            calls.append(request)
            return httpx.Response(429, json={"Code": 2028}, headers={"Retry-After": "1"})

        adapter = self.adapter(handler)
        with (
            mock.patch.object(transport.time, "sleep") as sleep,
            self.assertRaises(transport.ApiError),
        ):
            adapter.request(
                "GET",
                "/core/v4/users",
                extra_headers={
                    "x-pm-human-verification-token": "synthetic-private-verification",
                    "x-pm-human-verification-token-type": "captcha",
                },
            )
        self.assertEqual(len(calls), 1)
        sleep.assert_not_called()
        self.assertEqual(
            calls[0].headers["x-pm-human-verification-token"], "synthetic-private-verification"
        )

    def test_deep_json_is_a_fixed_protocol_failure(self):
        adapter = self.adapter(
            lambda request: httpx.Response(200, content=("[" * 1200 + "]" * 1200).encode())
        )
        with self.assertRaises(transport.PocError) as raised:
            adapter.request("GET", "/core/v4/users")
        self.assertEqual(str(raised.exception), "invalid_json_response")

    def test_tls_proxy_and_environment_policy_reaches_http_client(self):
        proxy = "socks5h://127.0.0.1:1080"
        with mock.patch.object(transport.httpx, "Client") as client:
            transport.Transport(app_version="external-vpn@0.1.0", direct=False, proxy=proxy)
        options = client.call_args.kwargs
        self.assertEqual(options["proxy"], proxy)
        self.assertTrue(options["verify"])
        self.assertFalse(options["trust_env"])
        self.assertFalse(options["follow_redirects"])

    def test_proxy_connection_failure_has_no_direct_retry(self):
        attempts = []

        def handler(request):
            attempts.append(request)
            raise httpx.ProxyError("private-secret")

        adapter = self.adapter(handler)
        with self.assertRaises(transport.PocError) as raised:
            adapter.request("POST", "/auth/v4/sessions", {"sensitive": "private-secret"})
        self.assertEqual(len(attempts), 1)
        self.assertEqual(str(raised.exception), "transport_failed")


if __name__ == "__main__":
    unittest.main()
