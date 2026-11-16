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
    def test_fresh_connectors_can_retry_only_before_publication(self):
        host = "sample-issued-endpoint.trycloudflare.com"
        for outcomes, published in (
            ([TimeoutError("no DNS"), (host, ["203.0.113.1"])], True),
            ([ConnectionError("exited before publication")] * 3, False),
        ):
            with tempfile.TemporaryDirectory() as temp, self.subTest(published=published):
                directory = Path(temp)
                server = SimpleNamespace(server_port=8989, target="one-challenge")
                with (
                    patch.object(observer.subprocess, "Popen") as starter,
                    patch.object(observer, "connector_endpoint", side_effect=outcomes),
                    patch.object(observer.time, "sleep"),
                ):
                    proc = starter.return_value.__enter__.return_value
                    proc.poll.return_value = 2
                    with self.assertRaisesRegex(
                        RuntimeError, "after publication" if published else "3 attempts"
                    ):
                        observer.run_connectors(directory, directory / "verified-binary", server)
                    self.assertEqual(starter.call_count, len(outcomes))
                endpoint = directory / "endpoint.json"
                self.assertEqual(endpoint.exists(), published)
                if published:
                    self.assertEqual(observer.read_json(endpoint)["target"], server.target)
                    self.assertEqual(observer.read_json(endpoint)["host"], host)

    def test_startup_reports_missing_hostname_and_dns_without_disclosing_log_contents(self):
        for line, error in (
            ("failed to reach https://api.trycloudflare.com/tunnel", "hostname was not issued"),
            ("INF |  https://example-issued-host.trycloudflare.com   |", "IPv4 DNS unavailable"),
        ):
            with tempfile.TemporaryDirectory() as temp, self.subTest(error=error):
                log = Path(temp) / "connector.log"
                log.write_text(line)
                proc = MagicMock()
                proc.poll.return_value = None
                with (
                    patch.object(observer.time, "monotonic", side_effect=[0, 1, 91]),
                    patch.object(observer.time, "sleep"),
                    patch.object(observer.socket, "getaddrinfo", side_effect=socket.gaierror()),
                    self.assertRaisesRegex(TimeoutError, error) as raised,
                ):
                    observer.connector_endpoint(proc, log)
                self.assertNotIn("trycloudflare.com", str(raised.exception))

    def test_connector_diagnostic_exposes_only_recognized_categories(self):
        output = (
            "https://private.trycloudflare.com/?token=secret failed with status 429: too many requests\n"
            "Registered tunnel connection connIndex=secret-id\n"
            "dial tcp 1.2.3.4:443: network is unreachable"
        )
        self.assertEqual(
            observer.connector_diagnostic(output),
            "HTTP 429, provider rate limit, edge connection registered, TCP edge connection failure, unreachable IP route",
        )

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

    def test_positive_retry_requires_a_valid_ack_for_the_same_marker(self):
        for responses, expected, error in (
            ([TimeoutError("TLS read timed out"), self.config["session"].encode()], 2, None),
            ([TimeoutError("TLS read timed out")] * 3, 3, TimeoutError),
            ([b"wrong session"], 1, RuntimeError),
            ([ssl.SSLCertVerificationError("untrusted")], 1, ssl.SSLCertVerificationError),
        ):
            with (
                self.subTest(expected=expected, error=error),
                patch.object(socket, "socket") as sockets,
                patch.object(ssl, "create_default_context"),
                patch.object(http.client, "HTTPConnection") as connection,
            ):
                conn = connection.return_value
                res = conn.getresponse.return_value
                res.status = 200
                res.read.side_effect = responses
                if error is None:
                    observer.positive_request(self.config, "after")
                else:
                    with self.assertRaises(error):
                        observer.positive_request(self.config, "after")
                self.assertEqual(sockets.call_count, expected)
                self.assertEqual(len(conn.request.call_args_list), expected)
                for request in conn.request.call_args_list:
                    self.assertEqual(
                        request.args,
                        ("POST", f"/{self.config['target']}/arm64/{self.config['session']}/after"),
                    )

    def test_finish_uses_default_resolver_and_routing_after_successful_cleanup(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "client.json"
            self.config.update(phases=list(observer.PHASES))
            observer.write_json(path, self.config)
            with (
                patch.object(observer.os, "getuid", return_value=501),
                patch.object(observer, "bound_request") as bound,
                patch.object(http.client, "HTTPSConnection") as unbound,
            ):
                response = unbound.return_value.getresponse.return_value
                response.status = 200
                response.read.return_value = self.config["session"].encode()
                observer.probe("finish", path)
                bound.assert_not_called()
                self.assertEqual(unbound.call_args.args, (self.config["host"],))
                self.assertEqual(
                    unbound.return_value.request.call_args.args,
                    ("POST", f"/{self.config['target']}/arm64/{self.config['session']}/finish"),
                )
            self.assertEqual(observer.read_json(path)["phases"][-1], "finish")


if __name__ == "__main__":
    unittest.main()
