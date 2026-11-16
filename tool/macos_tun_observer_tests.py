"""Isolated assertions for the hosted Kill Switch observer; no network access is needed."""

import http.client
import json
import socket
import ssl
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import MagicMock, patch

import macos_tun_observer as observer


class ReceiverTests(unittest.TestCase):
    def test_only_bracketed_positive_controls_can_complete_the_run(self):
        with tempfile.TemporaryDirectory() as temp:
            server = SimpleNamespace(
                target="a" * 48,
                report=Path(temp) / "report.json",
                sessions={},
                observations={"arm64": {}, "x86_64": {}},
            )

            def request(phase):
                handler = observer.Receiver.__new__(observer.Receiver)
                handler.server = server
                handler.path = f"/{server.target}/arm64/{'7' * 32}/{phase}"
                handler.respond = MagicMock()
                handler.do_POST()
                return handler.respond.call_args.args[0]

            self.assertEqual(request("before"), 200)
            self.assertEqual(request("finish"), 409)
            self.assertEqual(request("after"), 200)
            self.assertEqual(request("finish"), 200)
            self.assertEqual(request("retained"), 200)
            self.assertEqual(request("finish"), 409)
            counts = json.loads(server.report.read_text())
            self.assertEqual(counts["arm64"]["retained"], 1)
            self.assertEqual(counts["x86_64"], {})


class BoundProbeTests(unittest.TestCase):
    def setUp(self):
        self.config = {
            "host": "unit-test.trycloudflare.com",
            "address": "192.0.2.1",
            "ifindex": 3,
            "arch": "arm64",
            "target": "a" * 48,
            "session": "5" * 32,
        }

    def test_blocked_connect_does_not_deliver_a_probe(self):
        with (
            patch.object(socket, "socket") as factory,
            patch.object(ssl, "create_default_context") as context,
        ):
            sock = factory.return_value.__enter__.return_value
            sock.connect.side_effect = TimeoutError()
            observer.bound_request(self.config, "active", forbid_connect=True)
            sock.setsockopt.assert_called_once_with(socket.IPPROTO_IP, 25, 3)
            sock.connect.assert_called_once_with(("192.0.2.1", 443))
            context.assert_not_called()

    def test_tls_failure_after_a_physical_connection_cannot_pass_the_negative_control(self):
        with patch.object(socket, "socket"), patch.object(ssl, "create_default_context") as context:
            context.return_value.wrap_socket.side_effect = TimeoutError()
            with self.assertRaises(TimeoutError):
                observer.bound_request(self.config, "retained", forbid_connect=True)

    def test_allowed_probe_requires_a_matching_external_receipt(self):
        with (
            patch.object(socket, "socket"),
            patch.object(ssl, "create_default_context"),
            patch.object(http.client, "HTTPConnection") as factory,
        ):
            conn = factory.return_value
            res = conn.getresponse.return_value
            res.status = 200
            res.read.side_effect = [self.config["session"].encode(), b"unrelated"]
            observer.bound_request(self.config, "before")
            self.assertEqual(conn.request.call_args.args[0], "POST")
            conn.close.assert_called_once()
            with self.assertRaisesRegex(RuntimeError, "did not acknowledge"):
                observer.bound_request(self.config, "after")


if __name__ == "__main__":
    unittest.main()
