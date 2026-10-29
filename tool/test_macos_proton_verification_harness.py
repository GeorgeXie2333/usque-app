"""Browser-free tests of the CEF test runner and its native launch guard.

Only controlled Python child processes and a pure C++ argument parser run.
This suite never launches CEF, Flutter, an app bundle or a permission prompt.
"""

from __future__ import annotations

import base64
import contextlib
import io
import json
import os
import shutil
import signal
import struct
import subprocess
import sys
import tempfile
import textwrap
import time
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

import test_macos_proton_verification as runner

ROOT = Path(__file__).resolve().parents[1]
CXX = shutil.which("clang++") or shutil.which("c++")
POSIX = os.name == "posix"


class HarnessTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="usque-cef-unit-")
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)

    def process(self, body):
        path = self.directory / "fake-process"
        path.write_text(f"#!{sys.executable}\n" + textwrap.dedent(body), encoding="utf-8")
        path.chmod(0o700)
        return path

    @unittest.skipUnless(POSIX, "POSIX fake-process/session fixture")
    def test_default_run_uses_native_browser_guard_and_a_private_home(self):
        fake = self.process(
            """
            import json, os, pathlib, struct, sys
            assert sys.argv[1:] == ['--stdio', '--no-interactive']
            assert os.environ['HOME'] == os.environ['CFFIXED_USER_HOME']
            assert os.environ['HOME'] != os.environ['TMPDIR']
            assert os.environ['TMPDIR'].endswith('/')
            assert pathlib.Path(os.environ['HOME']).stat().st_mode & 0o777 == 0o700
            assert sys.stdin.buffer.read() == b'bounded-input'
            data = json.dumps({'status': 'cancelled'}).encode()
            for chunk in (struct.pack('>I', len(data)), data[:3], data[3:]):
                sys.stdout.buffer.write(chunk)
                sys.stdout.buffer.flush()
            """
        )
        result = runner.execute(fake, b"bounded-input")
        self.assertEqual(result, {"exit": 0, "reply": {"status": "cancelled"}, "stderr_bytes": 0})

    def test_smoke_is_rejected_before_process_creation_without_opt_in(self):
        with patch.object(runner.subprocess, "Popen") as spawn:
            with self.assertRaisesRegex(ValueError, "explicit --allow-interactive"):
                runner.execute(Path("unused"), None, smoke=True)
            with self.assertRaisesRegex(ValueError, "explicit --allow-interactive"):
                runner.check(Path("unused"), smoke=True)
            spawn.assert_not_called()

    def test_cli_rejects_interactive_smoke_before_any_contract_check(self):
        for options in (["--smoke"], ["--smoke", "--no-interactive"]):
            with patch.object(runner, "check") as check, contextlib.redirect_stderr(io.StringIO()):
                with self.assertRaises(SystemExit) as failure:
                    runner.main(["--app", "unused.app", *options])
                self.assertEqual(failure.exception.code, 2)
                check.assert_not_called()

    def test_cli_default_explicitly_disables_interactive_tests(self):
        with patch.object(runner, "check", return_value={"passed": 6}) as check:
            with contextlib.redirect_stdout(io.StringIO()):
                runner.main(["--app", "unused.app"])
            self.assertFalse(check.call_args.kwargs["allow_interactive"])

    def test_cli_opt_in_is_forwarded_without_launching_a_browser(self):
        with patch.object(runner, "check", return_value={"passed": 0}) as check:
            with contextlib.redirect_stdout(io.StringIO()):
                runner.main(["--app", "unused.app", "--smoke", "--allow-interactive"])
            self.assertTrue(check.call_args.kwargs["allow_interactive"])

    @unittest.skipUnless(POSIX, "POSIX fake-process/session fixture")
    def test_full_default_contract_uses_only_no_interactive_children(self):
        fake = self.process(
            """
            import json, struct, sys
            assert sys.argv[1:] == ['--stdio', '--no-interactive']
            data = sys.stdin.buffer.read()
            reply = {'status': 'error', 'code': 'PROTON_VERIFICATION_INVALID'}
            code = 1
            if len(data) >= 4 and struct.unpack('>I', data[:4])[0] == len(data) - 4:
                try:
                    handoff = json.loads(data[4:])
                except ValueError:
                    handoff = {}
                if handoff.get('version') == 2:
                    if handoff.get('route_epoch'):
                        reply['code'] = 'PROTON_VERIFICATION_INTERACTIVE_DISABLED'
                    else:
                        reply = {'status': 'success', 'message': json.dumps({
                            'type': 'USQUE_VERIFICATION_NETWORK_FAILED', 'route_epoch': 0})}
                        code = 0
            output = json.dumps(reply).encode()
            sys.stdout.buffer.write(struct.pack('>I', len(output)) + output)
            sys.exit(code)
            """
        )
        result = runner.check(fake)
        self.assertEqual(result["passed"], 6)
        self.assertEqual(result["cases"][-1], "no_interactive_proxy_handoff")
        self.assertEqual(result["official_challenge"], "not_run")

    @unittest.skipUnless(POSIX, "POSIX fake-process/session fixture")
    def test_invalid_frame_and_trailing_output_fail(self):
        valid = json.dumps({"status": "cancelled"}).encode()
        for output in (b"", b"bad", struct.pack(">I", len(valid)) + valid + b"trailer"):
            with self.subTest(size=len(output)):
                fake = self.process(
                    f"import sys\nsys.stdin.buffer.read()\nsys.stdout.buffer.write({output!r})\n"
                )
                with self.assertRaisesRegex(ValueError, "one bounded frame"):
                    runner.execute(fake, b"")

    @unittest.skipUnless(POSIX, "POSIX fake-process/session fixture")
    def test_output_limit_is_enforced_without_printing_child_content(self):
        fake = self.process(
            "import sys\nsys.stdin.buffer.read()\nsys.stdout.buffer.write(b'x' * 140000)\n"
        )
        with self.assertRaisesRegex(ValueError, "exceeded its output bound"):
            runner.execute(fake, b"")

    @unittest.skipUnless(POSIX, "POSIX fake-process/session fixture")
    def test_input_backpressure_is_included_in_the_deadline(self):
        fake = self.process("import time\ntime.sleep(30)\n")
        started = time.monotonic()
        with self.assertRaisesRegex(TimeoutError, "bounded input/output deadline"):
            runner.execute(fake, b"x" * 65540, timeout_seconds=0.2)
        self.assertLess(time.monotonic() - started, 3)

    def test_input_and_timeout_bounds_are_checked_before_spawning(self):
        for payload, timeout in ((b"x" * 65541, 1), (b"", 0), (b"", 41)):
            with patch.object(runner.subprocess, "Popen") as spawn:
                with self.assertRaises(ValueError):
                    runner.execute(Path("unused"), payload, timeout_seconds=timeout)
                spawn.assert_not_called()

    @unittest.skipUnless(POSIX, "POSIX process-group signal fixture")
    def test_cleanup_kills_descendants_even_after_the_leader_has_exited(self):
        leader = Mock(pid=12345)
        leader.poll.return_value = 0
        leader.wait.return_value = 0
        signals = []

        def group_signal(pid, number):
            self.assertEqual(pid, leader.pid)
            signals.append(number)

        with patch.object(runner.os, "killpg", side_effect=group_signal):
            runner.terminate_group(leader, grace_seconds=0.1)
        self.assertEqual(signals, [signal.SIGTERM, signal.SIGKILL])
        leader.wait.assert_any_call(timeout=0.1)
        leader.wait.assert_called_with(timeout=3)

    def test_auth_failure_distinguishes_process_exit_from_protocol_reply(self):
        for exit_code, code, reason in (
            (2, "PROTON_VERIFICATION_LOAD", "child_exit"),
            (0, "PROTON_VERIFICATION_NAVIGATION", "unexpected_reply"),
        ):
            result = {
                "exit": exit_code,
                "reply": {"status": "error", "code": code},
                "stderr_bytes": 4,
            }
            with self.assertRaises(runner.ValidationFailure) as failure:
                runner.expect_error(result, "bound_proxy_authentication", proxy_authorized=True)
            self.assertEqual(failure.exception.details["reason"], reason)
            self.assertEqual(failure.exception.details["exit"], exit_code)
            self.assertEqual(failure.exception.details["code"], code)
            self.assertTrue(failure.exception.details["proxy_authorized"])

    def test_failure_evidence_never_echoes_response_messages_or_unknown_labels(self):
        redaction_marker = "must-not-appear-in-diagnostic"
        result = {
            "exit": 2,
            "reply": {
                "status": redaction_marker,
                "code": {"password": redaction_marker},
                "message": redaction_marker,
            },
            "stderr_bytes": 123,
        }
        failure = runner.ValidationFailure("fixture", "child_exit", result)
        self.assertNotIn(redaction_marker, str(failure))
        self.assertEqual(failure.details["status"], "unrecognized")
        self.assertEqual(failure.details["code"], "unrecognized")

    def test_cli_failure_report_preserves_sanitized_case_evidence(self):
        result = {
            "exit": 2,
            "reply": {"status": "error", "code": "PROTON_VERIFICATION_LOAD"},
            "stderr_bytes": 0,
        }
        failure = runner.ValidationFailure("bound_proxy_authentication", "child_exit", result)
        output = self.directory / "result.json"
        with patch.object(runner, "check", side_effect=failure):
            with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                runner.main(["--app", "unused.app", "--output", str(output)])
        report = json.loads(output.read_text())
        self.assertFalse(report["passed"])
        self.assertEqual(report["failure"], failure.details)

    def test_darwin_uses_a_short_private_temp_parent_without_touching_it(self):
        with patch.object(runner.tempfile, "TemporaryDirectory") as temporary:
            runner.isolated_directory(platform="darwin")
        temporary.assert_called_once_with(prefix="usque-cef-", dir="/private/tmp")

    def test_linux_uses_standard_temp_selection_without_reading_host_paths(self):
        with patch.object(runner.tempfile, "TemporaryDirectory") as temporary:
            runner.isolated_directory(platform="linux")
        temporary.assert_called_once_with(prefix="usque-cef-", dir=None)

    def test_windows_uses_standard_temp_selection_without_reading_host_paths(self):
        with patch.object(runner.tempfile, "TemporaryDirectory") as temporary:
            runner.isolated_directory(platform="win32")
        temporary.assert_called_once_with(prefix="usque-cef-", dir=None)

    def test_unsupported_process_platform_fails_before_any_spawn_or_temp_creation(self):
        executable = Path("unused")
        with patch.object(runner, "POSIX_PROCESS_RUNNER_SUPPORTED", False):
            with patch.object(runner.subprocess, "Popen") as spawn:
                with patch.object(runner, "isolated_directory") as temporary:
                    with self.assertRaisesRegex(ValueError, "requires POSIX"):
                        runner.execute(executable, b"")
                    with self.assertRaisesRegex(ValueError, "requires POSIX"):
                        runner.terminate_group(Mock(pid=12345))
                    spawn.assert_not_called()
                    temporary.assert_not_called()


class ProxyFixtureTests(unittest.TestCase):
    def test_auth_proxy_challenges_then_rejects_upstream_without_opening_sockets(self):
        authorization = b"Basic " + base64.b64encode(("a" * 32 + ":" + "b" * 64).encode())
        streams = []
        for header in (b"", b"Proxy-Authorization: " + authorization + b"\r\n"):
            request = bytearray(b"GET http://fixture.invalid/ HTTP/1.1\r\n" + header + b"\r\n")
            stream = Mock()
            stream.__enter__ = Mock(return_value=stream)
            stream.__exit__ = Mock(return_value=False)
            stream.recv.side_effect = lambda _, data=request: bytes([data.pop(0)]) if data else b""
            streams.append(stream)
        listener = Mock()
        listener.getsockname.return_value = ("127.0.0.1", 12345)
        listener.accept.side_effect = [(stream, None) for stream in streams] + [OSError()]
        with patch.object(runner.socket, "socket", return_value=listener) as create_socket:
            proxy = runner.AuthenticationProxy()
            proxy.serve()
            create_socket.assert_called_once_with()
        self.assertEqual(proxy.requests, 2)
        self.assertEqual(proxy.challenges, 1)
        self.assertTrue(proxy.authorized)
        self.assertTrue(proxy.received_credentials)
        self.assertIn(b"407 Proxy Authentication Required", streams[0].sendall.call_args.args[0])
        self.assertIn(b"502 Fixture Stops Here", streams[1].sendall.call_args.args[0])


@unittest.skipUnless(POSIX and CXX, "pure launch-guard test requires a POSIX C++ compiler")
class NativeLaunchGuardTests(unittest.TestCase):
    def test_native_opt_in_parser_without_loading_cocoa_or_cef(self):
        header = ROOT / "apps/usque_gui/macos/ProtonVerification/launch_options.h"
        with tempfile.TemporaryDirectory(prefix="usque-cef-guard-") as directory:
            source = Path(directory) / "guard.cc"
            executable = Path(directory) / "guard"
            source.write_text(
                f"#include {json.dumps(str(header))}\n"
                + textwrap.dedent(
                    """
                    #include <cassert>
                    using namespace usque::proton::macos;
                    int main(int argc, char** argv) {
                      assert(ParseLaunchOptions(argc, argv).mode == LaunchMode::kInvalid);
                      const char* normal[] = {"component", "--stdio"};
                      const char* safe[] = {"component", "--stdio", "--no-interactive"};
                      const char* bare[] = {"component", "--smoke-test"};
                      const char* denied[] = {"component", "--smoke-test", "--no-interactive"};
                      const char* allowed[] = {"component", "--smoke-test", "--allow-interactive"};
                      const char* invalid[] = {"component", "--stdio", "--allow-interactive"};
                      assert(ParseLaunchOptions(2, normal).mode == LaunchMode::kStdio);
                      assert(!ParseLaunchOptions(2, normal).no_interactive);
                      assert(ParseLaunchOptions(3, safe).mode == LaunchMode::kStdio);
                      assert(ParseLaunchOptions(3, safe).no_interactive);
                      assert(ParseLaunchOptions(2, bare).mode == LaunchMode::kInteractiveDisabled);
                      assert(ParseLaunchOptions(3, denied).mode == LaunchMode::kInteractiveDisabled);
                      assert(ParseLaunchOptions(3, allowed).mode == LaunchMode::kSmoke);
                      assert(ParseLaunchOptions(3, invalid).mode == LaunchMode::kInvalid);
                      assert(ParseLaunchOptions(4, allowed).mode == LaunchMode::kInvalid);
                      assert(ParseLaunchOptions(0, nullptr).mode == LaunchMode::kInvalid);
                    }
                    """
                ),
                encoding="utf-8",
            )
            subprocess.run(  # noqa: S603 - existing compiler and controlled parser fixture only.
                [
                    CXX,
                    "-std=c++17",
                    "-Wall",
                    "-Wextra",
                    "-Werror",
                    str(source),
                    "-o",
                    str(executable),
                ],
                check=True,
                capture_output=True,
                timeout=30,
            )
            subprocess.run(  # noqa: S603 - pure argument parser; no Cocoa, CEF or app launch.
                [str(executable)], check=True, capture_output=True, timeout=5
            )


if __name__ == "__main__":
    unittest.main()
