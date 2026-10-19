"""Actual loopback tests for the bounded manual browser callback; no CAPTCHA access."""

from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import socket
import threading
import time
import unittest
from types import SimpleNamespace
from unittest import mock
from urllib.parse import urlencode

_HAVE_HTTPX = importlib.util.find_spec("httpx") is not None
if _HAVE_HTTPX:
    from proton_poc import browser_handoff as handoff
    from proton_poc import flow
    from proton_poc.transport import PocError
    from proton_poc.verification import VerificationChallenge


@unittest.skipUnless(_HAVE_HTTPX, "Proton PoC optional HTTP dependency is not installed")
class ProtonBrowserHandoffTests(unittest.TestCase):
    def challenge(self, **changes):
        return VerificationChallenge("synthetic-private-challenge", ("captcha",), **changes)

    def message(self):
        return json.dumps(
            {
                "type": "HUMAN_VERIFICATION_SUCCESS",
                "payload": {"token": "synthetic-private-success", "type": "captcha"},
            }
        )

    def body(self, bridge, **changes):
        return urlencode({"state": bridge._state, "message": self.message(), **changes}).encode()

    def request(self, bridge, *, body=None, headers=None, extra=(), start=None):
        body = self.body(bridge) if body is None else body
        values = {
            "Host": f"127.0.0.1:{bridge._port}",
            "Origin": "https://verify.proton.me",
            "Content-Type": "application/x-www-form-urlencoded",
            "Content-Length": str(len(body)),
        }
        if headers:
            values.update(headers)
        lines = [start or b"POST /complete HTTP/1.1"]
        lines += [f"{name}: {value}".encode("ascii") for name, value in values.items()]
        return b"\r\n".join([*lines, *extra]) + b"\r\n\r\n" + body

    def start(self, bridge):
        outcome = {}

        def wait():
            try:
                outcome["headers"] = bridge.wait()
            except BaseException as error:
                outcome["error"] = error

        worker = threading.Thread(target=wait, daemon=True)
        worker.start()
        self.addCleanup(worker.join, 2)
        return worker, outcome

    def exchange(self, bridge, request):
        response = bytearray()
        with socket.create_connection(("127.0.0.1", bridge._port), timeout=2) as connection:
            try:
                connection.sendall(request)
                connection.shutdown(socket.SHUT_WR)
                while chunk := connection.recv(4096):
                    response.extend(chunk)
            except ConnectionResetError:
                pass
        return bytes(response)

    def finish(self, worker, outcome):
        worker.join(2)
        self.assertFalse(worker.is_alive())
        self.assertNotIn("error", outcome)
        self.assertEqual(
            outcome["headers"],
            {
                "x-pm-human-verification-token": "synthetic-private-success",
                "x-pm-human-verification-token-type": "captcha",
            },
        )

    def assert_port_closed(self, port):
        with self.assertRaises(OSError):
            with socket.create_connection(("127.0.0.1", port), timeout=0.3):
                pass

    def test_real_callback_is_one_use_loopback_only_and_private(self):
        output = io.StringIO()
        with contextlib.redirect_stdout(output), contextlib.redirect_stderr(output):
            with handoff.BrowserHandoff(self.challenge()) as bridge:
                port = bridge._port
                self.assertEqual(bridge._listener.getsockname()[0], "127.0.0.1")
                self.assertEqual(bridge._listener.family, socket.AF_INET)
                self.assertFalse(bridge._listener.get_inheritable())
                snippet = bridge.snippet
                self.assertNotIn("synthetic-private-challenge", snippet)
                self.assertNotIn("synthetic-private-success", snippet)
                self.assertNotIn("?state=", snippet)
                worker, outcome = self.start(bridge)
                response = self.exchange(bridge, self.request(bridge))
                self.assertIn(b"200 OK", response)
                self.assertIn(b"Cache-Control: no-store", response)
                for secret in (self.challenge().token, "synthetic-private-success", bridge._state):
                    self.assertNotIn(secret.encode(), response)
                self.finish(worker, outcome)
                with self.assertRaises(PocError):
                    bridge.wait()
                self.assert_port_closed(port)
        self.assertEqual(output.getvalue(), "")

    def test_wrong_origin_host_or_nonce_cannot_consume_valid_callback(self):
        with handoff.BrowserHandoff(self.challenge()) as bridge:
            worker, outcome = self.start(bridge)
            bad = [
                self.request(bridge, headers={"Origin": "https://attacker.invalid"}),
                self.request(bridge, headers={"Origin": "null"}),
                self.request(bridge, headers={"Origin": "https://verify.proton.me/"}),
                self.request(bridge, headers={"Host": "localhost:" + str(bridge._port)}),
                self.request(bridge, body=self.body(bridge, state="synthetic-wrong-state")),
            ]
            for request in bad:
                response = self.exchange(bridge, request)
                self.assertNotIn(b"200 OK", response)
                self.assertNotIn(b"synthetic-private", response)
            self.assertIn(b"200 OK", self.exchange(bridge, self.request(bridge)))
            self.finish(worker, outcome)

    def test_duplicate_critical_headers_transfer_encoding_and_wrong_routes_fail_closed(self):
        mutations = [
            {"extra": (b"Host: 127.0.0.1:1",)},
            {"extra": (b"Origin: https://verify.proton.me",)},
            {"extra": (b"Content-Length: 1",)},
            {"extra": (b"Content-Type: application/x-www-form-urlencoded",)},
            {"extra": (b"Transfer-Encoding: chunked",)},
            {"extra": (b" Expect: 100-continue",)},
            {"headers": {"Content-Type": "application/json"}},
            {"start": b"GET /complete HTTP/1.1"},
            {"start": b"POST /complete?state=synthetic-private HTTP/1.1"},
            {"start": b"POST /complete HTTP/1.0"},
        ]
        for index, changes in enumerate(mutations):
            with self.subTest(case=index), handoff.BrowserHandoff(self.challenge()) as bridge:
                worker, outcome = self.start(bridge)
                self.assertNotIn(b"200 OK", self.exchange(bridge, self.request(bridge, **changes)))
                self.assertIn(b"200 OK", self.exchange(bridge, self.request(bridge)))
                self.finish(worker, outcome)

    def test_oversized_headers_or_declared_body_are_rejected_before_content_read(self):
        for kind in ("headers", "body"):
            with self.subTest(kind=kind), handoff.BrowserHandoff(self.challenge()) as bridge:
                worker, outcome = self.start(bridge)
                if kind == "headers":
                    request = self.request(bridge, extra=(b"X-Padding: " + b"x" * (16 * 1024),))
                else:
                    request = self.request(
                        bridge, body=b"", headers={"Content-Length": str(40 * 1024 + 1)}
                    )
                self.assertNotIn(b"200 OK", self.exchange(bridge, request))
                self.assertIn(b"200 OK", self.exchange(bridge, self.request(bridge)))
                self.finish(worker, outcome)

    def test_slow_headers_and_body_have_hard_deadline_despite_continued_bytes(self):
        for kind in ("headers", "body"):
            with (
                self.subTest(kind=kind),
                mock.patch.object(handoff, "_CONNECTION_SECONDS", 0.15),
                mock.patch.object(handoff, "_POLL_SECONDS", 0.02),
                handoff.BrowserHandoff(self.challenge()) as bridge,
            ):
                worker, outcome = self.start(bridge)
                with socket.create_connection(("127.0.0.1", bridge._port), timeout=2) as connection:
                    initial = (
                        b"POST /"
                        if kind == "headers"
                        else self.request(bridge, body=b"x", headers={"Content-Length": "1000"})
                    )
                    connection.sendall(initial)
                    stopped = threading.Event()

                    def trickle(connection=connection, stopped=stopped):
                        while not stopped.wait(0.015):
                            try:
                                connection.sendall(b"x")
                            except OSError:
                                return

                    sender = threading.Thread(target=trickle, daemon=True)
                    sender.start()
                    started = time.monotonic()
                    try:
                        try:
                            while connection.recv(4096):
                                pass
                        except ConnectionResetError:
                            pass
                    finally:
                        stopped.set()
                        sender.join(1)
                    self.assertLess(time.monotonic() - started, 1.0)
                    self.assertFalse(sender.is_alive())
                self.assertIn(b"200 OK", self.exchange(bridge, self.request(bridge)))
                self.finish(worker, outcome)

    def test_absolute_expiry_and_context_cancellation_close_listener(self):
        with handoff.BrowserHandoff(self.challenge(created_at=time.monotonic() - 299.8)) as bridge:
            port = bridge._port
            with self.assertRaises(PocError) as raised:
                bridge.wait()
            self.assertEqual(raised.exception.category, "human_verification_expired")
            self.assert_port_closed(port)
        with self.assertRaises(PocError):
            with handoff.BrowserHandoff(self.challenge(created_at=time.monotonic() - 301)):
                self.fail("Expired handoff must not start")

    def test_keyboard_interrupt_while_receiving_closes_listener_and_active_socket(self):
        with handoff.BrowserHandoff(self.challenge()) as bridge:
            port = bridge._port
            with socket.create_connection(("127.0.0.1", port), timeout=2) as connection:
                connection.sendall(self.request(bridge))
                with (
                    mock.patch.object(bridge, "_read_request", side_effect=KeyboardInterrupt()),
                    self.assertRaises(KeyboardInterrupt),
                ):
                    bridge.wait()
                try:
                    self.assertEqual(connection.recv(4096), b"")
                except ConnectionResetError:
                    pass
            self.assert_port_closed(port)

    def test_connection_budget_is_bounded_and_error_has_no_private_data(self):
        with (
            mock.patch.object(handoff, "_MAX_CONNECTIONS", 2),
            handoff.BrowserHandoff(self.challenge()) as bridge,
        ):
            worker, outcome = self.start(bridge)
            for _ in range(2):
                self.exchange(bridge, self.request(bridge, body=self.body(bridge, state="wrong")))
            worker.join(2)
            self.assertFalse(worker.is_alive())
            self.assertEqual(outcome["error"].category, "browser_handoff_request_limit")
            self.assertNotIn("synthetic-private", str(outcome["error"]))
            self.assert_port_closed(bridge._port)

    def test_bounded_deterministic_malformed_form_corpus(self):
        malformed_bodies = [
            b"",
            b"state=",
            b"state=%ZZ&message=x",
            b"state=%FF&message=x",
            b"state=x&state=y&message=z",
            b"message=x&extra=y",
            b"state=x;message=y",
            b"state=x&message=%00",
            b"state=x&message=[]",
            b"state=x&message={}",
        ]
        for index, body in enumerate(malformed_bodies):
            with self.subTest(case=index), handoff.BrowserHandoff(self.challenge()) as bridge:
                worker, outcome = self.start(bridge)
                self.assertNotIn(b"200 OK", self.exchange(bridge, self.request(bridge, body=body)))
                self.assertIn(b"200 OK", self.exchange(bridge, self.request(bridge)))
                self.finish(worker, outcome)

    def test_browser_ui_cancellation_writes_secrets_only_to_private_tty_and_closes(self):
        class PrivateTTY(io.StringIO):
            def isatty(self):
                return True

            def close(self):
                # Keep synthetic private output available for inspection.
                pass

        private = PrivateTTY()
        public = io.StringIO()
        created = []
        original = handoff.BrowserHandoff

        def bridge_factory(challenge):
            bridge = original(challenge)
            created.append(bridge)
            return bridge

        terminal = SimpleNamespace(isatty=lambda: True)
        with (
            contextlib.redirect_stdout(public),
            contextlib.redirect_stderr(public),
            mock.patch.object(flow, "sys", SimpleNamespace(stdin=terminal, stderr=terminal)),
            mock.patch("builtins.open", return_value=private),
            mock.patch.object(handoff.BrowserHandoff, "wait", side_effect=KeyboardInterrupt()),
            mock.patch.object(handoff, "BrowserHandoff", side_effect=bridge_factory),
            self.assertRaises(KeyboardInterrupt),
        ):
            flow.LocalInteraction(verification_mode="browser").verify(self.challenge())
        self.assertEqual(public.getvalue(), "")
        self.assertEqual(len(created), 1)
        self.assertIn(self.challenge().url(), private.getvalue())
        self.assertIn(created[0]._state, private.getvalue())
        self.assert_port_closed(created[0]._port)


if __name__ == "__main__":
    unittest.main()
