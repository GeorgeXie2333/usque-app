"""Offline CLI output, terminal and exit-code boundaries; no real API requests."""

from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import unittest
from types import SimpleNamespace
from unittest import mock

_HAVE_HTTPX = importlib.util.find_spec("httpx") is not None
if _HAVE_HTTPX:
    import httpx

    from proton_poc import __main__ as command
    from proton_poc import flow, transport
    from proton_poc.transport import Result


class _Output(io.StringIO):
    def __init__(self, tty=False):
        super().__init__()
        self.tty = tty

    def isatty(self):
        return self.tty


@unittest.skipUnless(_HAVE_HTTPX, "Proton PoC optional HTTP dependency is not installed")
class ProtonCliTests(unittest.TestCase):
    def invoke(self, arguments, *, stdin_tty=False, stderr_tty=False):
        output, errors = io.StringIO(), _Output(stderr_tty)
        with (
            mock.patch.object(command.sys, "argv", ["proton-poc", *arguments]),
            mock.patch.object(command.sys, "stdin", SimpleNamespace(isatty=lambda: stdin_tty)),
            contextlib.redirect_stdout(output),
            contextlib.redirect_stderr(errors),
            mock.patch.object(command, "implementation_digest", return_value="0" * 64),
        ):
            status = command.main()
        self.assertEqual(errors.getvalue(), "")
        return status, json.loads(output.getvalue())

    def test_login_without_interactive_never_constructs_http_client(self):
        with mock.patch.object(transport, "Transport") as client:
            status, report = self.invoke(["login", "--direct"])
        client.assert_not_called()
        self.assertEqual(status, 2)
        self.assertEqual(report, {"error": "login_requires_local_interactive_terminal"})

    def test_interactive_requires_both_local_terminals_before_network_work(self):
        for identity in ("guest", "login"):
            for stdin_tty, stderr_tty in ((False, False), (False, True), (True, False)):
                with (
                    self.subTest(identity=identity, stdin_tty=stdin_tty, stderr_tty=stderr_tty),
                    mock.patch.object(transport, "Transport") as client,
                ):
                    status, report = self.invoke(
                        [identity, "--direct", "--interactive"],
                        stdin_tty=stdin_tty,
                        stderr_tty=stderr_tty,
                    )
                client.assert_not_called()
                self.assertEqual(status, 2)
                self.assertEqual(report, {"error": "local_tty_required"})

    def test_credentialed_and_invalid_idna_proxy_values_never_reach_output(self):
        for proxy in (
            "http://synthetic-user:synthetic-private@127.0.0.1:1080",
            "http://synthetic-private-\ud800:1080",
        ):
            with self.subTest(proxy_scheme=proxy.split(":", 1)[0]):
                with mock.patch.object(flow, "run") as run:
                    status, report = self.invoke(["guest", "--proxy", proxy])
                run.assert_not_called()
                self.assertEqual(status, 2)
                self.assertIn(
                    report["error"],
                    (
                        "transport_setup_failed",
                        "invalid_proxy_use_http_connect_or_socks5h_without_credentials",
                    ),
                )
                self.assertNotIn("synthetic-private", json.dumps(report))

    def test_httpx_invalid_url_exception_is_not_a_cli_traceback(self):
        with mock.patch.object(
            transport.httpx, "Client", side_effect=httpx.InvalidURL("synthetic-private")
        ):
            status, report = self.invoke(["guest", "--proxy", "socks5h://127.0.0.1:1080"])
        self.assertEqual(status, 2)
        self.assertEqual(report, {"error": "transport_setup_failed"})

    def test_exit_success_requires_complete_operation_and_confirmed_cleanup(self):
        cases = (
            ("complete", "none", "revoked", 0),
            ("private_export", "private_export_failed", "revoked", 1),
            ("complete", "cancelled", "revoked", 1),
            ("complete", "none", "revocation_failed", 1),
            ("complete", "none", "remote_session_state_unknown", 1),
            ("private_export", "none", "revoked", 1),
        )
        for phase, category, cleanup, expected in cases:
            with self.subTest(phase=phase, category=category, cleanup=cleanup):
                result = Result(
                    "guest",
                    "direct_https",
                    "external-vpn@0.1.0",
                    phase=phase,
                    config_ready="passed",
                    cleanup=cleanup,
                    error=category,
                )
                with (
                    mock.patch.object(transport, "Transport"),
                    mock.patch.object(flow, "run", return_value=result),
                ):
                    status, report = self.invoke(["guest", "--direct"])
                self.assertEqual(status, expected)
                self.assertEqual(report["error"], category)
                self.assertEqual(report["implementation_sha256"], "0" * 64)


if __name__ == "__main__":
    unittest.main()
