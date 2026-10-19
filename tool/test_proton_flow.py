"""Offline security state transitions; all credentials and tokens are synthetic."""

from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import unittest
import warnings
from dataclasses import asdict
from datetime import datetime, timezone
from types import SimpleNamespace
from unittest import mock

_HAVE_HTTPX = importlib.util.find_spec("httpx") is not None
if _HAVE_HTTPX:
    from proton_poc import flow
    from proton_poc.srp import SrpError
    from proton_poc.transport import ApiError, PocError, Result, Session


@unittest.skipUnless(_HAVE_HTTPX, "Proton PoC optional HTTP dependency is not installed")
class ProtonFlowTests(unittest.TestCase):
    def setUp(self):
        self.secret_values = [
            "synthetic-account@example.invalid",
            "synthetic-password-private",
            "synthetic-client-ephemeral",
            "synthetic-client-proof",
            "synthetic-server-proof",
            "synthetic-srp-session",
            "123456",
        ]
        self.anonymous = self.session_response("anonymous")
        self.authenticated = self.session_response("authenticated")
        self.rotated = self.session_response("rotated")
        # Normal token refresh keeps the same session UID throughout.
        self.rotated["UID"] = self.authenticated["UID"]
        self.info = {"SRPSession": "synthetic-srp-session"}
        self.authenticated["ServerProof"] = "synthetic-server-proof"
        self.authenticated["UserID"] = "synthetic-protected-user-id"
        self.secret_values.append(self.authenticated["UserID"])
        self.interaction = mock.Mock()
        self.interaction.credentials.return_value = tuple(self.secret_values[:2])
        self.interaction.totp.return_value = "123456"
        self.srp_client = mock.Mock()
        self.srp_client.proofs.return_value = SimpleNamespace(
            client_ephemeral="synthetic-client-ephemeral", client_proof="synthetic-client-proof"
        )
        self.authorizations = []
        self.human_details = {
            "HumanVerificationToken": "synthetic-challenge-private",
            "HumanVerificationMethods": ["captcha"],
        }
        self.human_headers = {
            "x-pm-human-verification-token": "synthetic-verification-private",
            "x-pm-human-verification-token-type": "captcha",
        }
        self.secret_values += ["synthetic-challenge-private", "synthetic-verification-private"]
        self.interaction.verify.return_value = self.human_headers

    def vpn_response(self):
        return {
            "Code": 1000,
            "VPN": {"Status": 1, "MaxTier": 0, "MaxConnect": 1, "Delinquent": 0},
            "Delinquent": 0,
        }

    def session_response(self, prefix):
        result = {
            "UID": f"synthetic-{prefix}-uid",
            "AccessToken": f"synthetic-{prefix}-access",
            "RefreshToken": f"synthetic-{prefix}-refresh",
        }
        self.secret_values.extend(result.values())
        return result

    def make_transport(self, script, *, close_error=None):
        class ScriptTransport:
            def __init__(self):
                self.session = Session()
                self.headers = {"x-pm-appversion": "external-vpn@0.1.0"}
                self.direct = False
                self.remaining = list(script)
                self.calls = []
                self.close_count = 0

            def request(self, method, path, payload=None, **kwargs):
                self.calls.append(
                    {
                        "method": method,
                        "path": path,
                        "payload": payload,
                        "uid": self.session.uid,
                        "access": self.session.access_token,
                        "refresh": self.session.refresh_token,
                        "kwargs": kwargs,
                    }
                )
                expected_method, expected_path, result = self.remaining.pop(0)
                if (method, path) != (expected_method, expected_path):
                    raise AssertionError("Unexpected request order")
                if isinstance(result, BaseException):
                    raise result
                return result

            def close(self):
                self.close_count += 1
                if close_error is not None:
                    raise close_error

        return ScriptTransport()

    def login_script(
        self, *, second_factor=None, refresh_response=None, revoke_error=None, retired_count=0
    ):
        auth_response = dict(self.authenticated)
        if second_factor is not None:
            auth_response["2FA"] = second_factor
        script = [
            ("POST", "/auth/v4/info", self.info),
            ("POST", "/auth/v4", auth_response),
        ]
        if second_factor == {"Enabled": 1}:
            script.append(("POST", "/auth/v4/2fa", {"Code": 1000}))
        script += [
            ("GET", "/core/v4/users", {"User": {"ID": self.authenticated["UserID"]}}),
            ("GET", "/vpn/v2", self.vpn_response()),
            (
                "POST",
                "/auth/v4/refresh",
                self.rotated if refresh_response is None else refresh_response,
            ),
            ("GET", "/vpn/v2", self.vpn_response()),
            ("DELETE", "/auth/v4", revoke_error or {"Code": 1000}),
        ]
        script.extend(("DELETE", "/auth/v4", {"Code": 1000}) for _ in range(retired_count))
        return script

    def guest_script(self, *, retired_count=1):
        return [
            ("POST", "/auth/v4/sessions", self.anonymous),
            ("POST", "/auth/v4/credentialless", self.authenticated),
            ("GET", "/core/v4/users", {"User": {"ID": self.authenticated["UserID"]}}),
            ("GET", "/vpn/v2", self.vpn_response()),
            ("POST", "/auth/v4/refresh", self.rotated),
            ("GET", "/vpn/v2", self.vpn_response()),
            ("DELETE", "/auth/v4", {"Code": 1000}),
            *[("DELETE", "/auth/v4", {"Code": 1000}) for _ in range(retired_count)],
        ]

    def execute(self, transport, *, identity="login", authorize_error=None, export_path=None):
        def authorize(instance, path):
            self.authorizations.append((instance.transport.session.access_token, path))
            if authorize_error is not None:
                raise authorize_error
            instance.result.wg_authorized = "passed"
            instance.result.config_ready = "passed"

        output = io.StringIO()
        with (
            mock.patch.object(flow.SrpClient, "from_info", return_value=self.srp_client),
            mock.patch.object(flow, "build_payload", return_value={"Payload": {"synthetic": True}}),
            mock.patch.object(flow, "datetime", wraps=datetime) as clock,
            mock.patch.object(flow.Flow, "authorize", new=authorize, create=True),
            contextlib.redirect_stdout(output),
            contextlib.redirect_stderr(output),
        ):
            clock.now.return_value = datetime(2026, 1, 2, 3, 4, 5, tzinfo=timezone.utc)
            result = flow.run(
                transport,
                identity=identity,
                interaction=self.interaction,
                export_path=export_path,
            )
        self.assertEqual(transport.remaining, [])
        self.assertEqual(transport.close_count, 1)
        self.assertEqual(transport.session.uid, "")
        self.assertEqual(transport.session.access_token, "")
        self.assertEqual(transport.session.refresh_token, "")
        self.assertEqual(output.getvalue(), "")
        rendered = json.dumps(asdict(result))
        for secret in self.secret_values:
            self.assertNotIn(secret, rendered)
        return result

    def test_login_success_verifies_server_then_refreshes_authorizes_and_revokes(self):
        transport = self.make_transport(self.login_script())
        result = self.execute(transport, export_path="private-config-path")
        self.assertEqual(result.phase, "complete")
        self.assertEqual(result.auth_ok, "passed")
        self.assertEqual(result.refresh, "passed")
        self.assertEqual(result.wg_authorized, "passed")
        self.assertEqual(result.config_ready, "passed")
        self.assertEqual(result.tunnel_connected, "not_run")
        self.assertEqual(result.cleanup, "revoked")
        self.assertEqual(result.error, "none")
        self.assertEqual(result.network, "explicit_proxy")
        info_call = next(call for call in transport.calls if call["path"] == "/auth/v4/info")
        self.assertEqual(info_call["payload"]["Intent"], "Proton")
        self.assertEqual(transport.calls[0]["path"], "/auth/v4/info")
        self.assertFalse(any(call["path"] == "/auth/v4/sessions" for call in transport.calls))
        self.srp_client.verify_server_proof.assert_called_once_with("synthetic-server-proof")
        self.assertEqual(
            self.authorizations, [(self.rotated["AccessToken"], "private-config-path")]
        )

    def test_bad_server_proof_revokes_returned_session_without_protected_requests(self):
        self.srp_client.verify_server_proof.side_effect = SrpError("synthetic-server-proof")
        transport = self.make_transport(
            self.login_script()[:2] + [("DELETE", "/auth/v4", {"Code": 1000})]
        )
        result = self.execute(transport)
        self.assertEqual(result.error, "srp_validation_failed_or_unsupported")
        self.assertEqual(result.auth_ok, "failed")
        self.assertEqual(result.cleanup, "revoked")
        self.assertEqual(self.authorizations, [])
        self.assertFalse(
            any(call["path"].startswith(("/vpn/", "/core/")) for call in transport.calls)
        )
        self.assertEqual(transport.calls[-1]["uid"], self.authenticated["UID"])
        self.assertEqual(transport.calls[-1]["access"], self.authenticated["AccessToken"])

    def test_totp_is_local_and_precedes_protected_requests(self):
        transport = self.make_transport(self.login_script(second_factor={"Enabled": 1}))
        result = self.execute(transport)
        self.assertEqual(result.error, "none")
        self.interaction.totp.assert_called_once_with()
        request = next(call for call in transport.calls if call["path"] == "/auth/v4/2fa")
        self.assertEqual(request["payload"], {"TwoFactorCode": "123456"})
        self.assertEqual(request["access"], self.authenticated["AccessToken"])

    def test_unsupported_second_factor_modes_never_skip_to_vpn(self):
        for second_factor in ({"Enabled": 2}, {"Enabled": True}, {}, "unrecognized"):
            with self.subTest(mode_type=type(second_factor).__name__):
                response = dict(self.authenticated, **{"2FA": second_factor})
                script = self.login_script()[:1] + [
                    ("POST", "/auth/v4", response),
                    ("DELETE", "/auth/v4", {"Code": 1000}),
                ]
                transport = self.make_transport(script)
                result = self.execute(transport)
                self.assertIn(
                    result.error,
                    ("unsupported_second_factor_mode", "unsupported_second_factor_response"),
                )
                self.assertEqual(result.auth_ok, "failed")
                self.assertEqual(result.cleanup, "revoked")
                self.assertFalse(any(call["path"].startswith("/vpn/") for call in transport.calls))
        self.interaction.totp.assert_not_called()

    def test_refresh_rotates_tokens_used_by_next_request_and_final_revocation(self):
        transport = self.make_transport(self.login_script())
        self.execute(transport)
        refresh = next(call for call in transport.calls if call["path"] == "/auth/v4/refresh")
        self.assertEqual(refresh["payload"]["RefreshToken"], self.authenticated["RefreshToken"])
        self.assertEqual(refresh["payload"]["UID"], self.authenticated["UID"])
        self.assertEqual(sum(call["method"] == "DELETE" for call in transport.calls), 1)
        for call in transport.calls[-2:]:
            self.assertEqual(call["uid"], self.rotated["UID"])
            self.assertEqual(call["access"], self.rotated["AccessToken"])
            self.assertEqual(call["refresh"], self.rotated["RefreshToken"])

    def test_each_distinct_owned_session_uid_is_revoked(self):
        self.authenticated["UID"] = "synthetic-distinct-authenticated-uid"
        self.rotated["UID"] = "synthetic-distinct-rotated-uid"
        self.secret_values += [self.authenticated["UID"], self.rotated["UID"]]
        transport = self.make_transport(self.guest_script(retired_count=2))
        result = self.execute(transport, identity="guest")
        revoked = [call for call in transport.calls if call["method"] == "DELETE"]
        self.assertEqual(result.cleanup, "revoked")
        self.assertEqual(
            [call["uid"] for call in revoked],
            [self.rotated["UID"], self.authenticated["UID"], self.anonymous["UID"]],
        )
        self.assertEqual(
            [call["access"] for call in revoked],
            [
                self.rotated["AccessToken"],
                self.authenticated["AccessToken"],
                self.anonymous["AccessToken"],
            ],
        )

    def test_failed_revoke_does_not_skip_other_owned_sessions(self):
        self.authenticated["UID"] = "synthetic-distinct-authenticated-uid"
        self.rotated["UID"] = "synthetic-distinct-rotated-uid"
        self.secret_values += [self.authenticated["UID"], self.rotated["UID"]]
        for failed_position in range(3):
            with self.subTest(failed_position=failed_position):
                script = self.guest_script(retired_count=2)
                script[-3 + failed_position] = ("DELETE", "/auth/v4", PocError("transport_failed"))
                transport = self.make_transport(script)
                result = self.execute(transport, identity="guest")
                revoked = [call["uid"] for call in transport.calls if call["method"] == "DELETE"]
                self.assertEqual(result.error, "none")
                self.assertEqual(result.cleanup, "revocation_failed")
                self.assertEqual(
                    revoked,
                    [self.rotated["UID"], self.authenticated["UID"], self.anonymous["UID"]],
                )

    def test_login_distinct_auth_and_refresh_sessions_revoke_only_those_two(self):
        self.rotated["UID"] = "synthetic-distinct-rotated-uid"
        self.secret_values.append(self.rotated["UID"])
        transport = self.make_transport(self.login_script(retired_count=1))
        result = self.execute(transport)
        self.assertEqual(result.cleanup, "revoked")
        self.assertEqual(
            [call["uid"] for call in transport.calls if call["method"] == "DELETE"],
            [self.rotated["UID"], self.authenticated["UID"]],
        )
        self.assertFalse(any(call["path"] == "/auth/v4/sessions" for call in transport.calls))

    def test_malformed_refresh_retains_old_session_for_cleanup(self):
        script = self.login_script(refresh_response={"UID": "bad-new-uid"})[:-2]
        script.append(("DELETE", "/auth/v4", {"Code": 1000}))
        transport = self.make_transport(script)
        result = self.execute(transport)
        self.assertEqual(result.error, "invalid_session_response")
        self.assertEqual(result.refresh, "not_run")
        self.assertEqual(result.cleanup, "remote_session_state_unknown")
        self.assertIn("known_session_handles_revoked", result.checks)
        self.assertEqual(transport.calls[-1]["uid"], self.authenticated["UID"])
        self.assertEqual(transport.calls[-1]["refresh"], self.authenticated["RefreshToken"])
        self.assertEqual(self.authorizations, [])

    def test_ctrl_c_during_local_credentials_creates_no_remote_session(self):
        self.interaction.credentials.side_effect = KeyboardInterrupt()
        transport = self.make_transport([])
        result = self.execute(transport)
        self.assertEqual(result.error, "cancelled")
        self.assertEqual(result.cleanup, "not_needed")
        self.assertEqual(transport.calls, [])

    def test_ctrl_c_before_guest_session_handle_reports_unknown_cleanup(self):
        transport = self.make_transport([("POST", "/auth/v4/sessions", KeyboardInterrupt())])
        result = self.execute(transport, identity="guest")
        self.assertEqual(result.error, "cancelled")
        self.assertEqual(result.cleanup, "unknown_without_session_handle")
        self.assertEqual(len(transport.calls), 1)

    def test_login_info_rejection_has_no_session_to_revoke_and_preserves_stage(self):
        transport = self.make_transport(
            [
                (
                    "POST",
                    "/auth/v4/info",
                    ApiError(429, 2028, {"Token": "synthetic-password-private"}),
                ),
            ]
        )
        result = self.execute(transport)
        self.assertEqual(result.phase, "srp_info")
        self.assertEqual(result.error, "rate_limited")
        self.assertEqual(result.http_status, 429)
        self.assertEqual(result.api_code, 2028)
        self.assertEqual(result.cleanup, "not_needed")
        self.assertEqual([call["path"] for call in transport.calls], ["/auth/v4/info"])

    def test_authorization_failure_retains_authenticated_result_and_cleans_up(self):
        transport = self.make_transport(self.login_script())
        result = self.execute(transport, authorize_error=PocError("authorization_rejected"))
        self.assertEqual(result.error, "authorization_rejected")
        self.assertEqual(result.auth_ok, "passed")
        self.assertEqual(result.wg_authorized, "not_run")
        self.assertEqual(result.config_ready, "not_run")
        self.assertEqual(result.cleanup, "revoked")

    def test_ctrl_c_after_refresh_revokes_rotated_session(self):
        transport = self.make_transport(self.login_script())
        result = self.execute(transport, authorize_error=KeyboardInterrupt())
        self.assertEqual(result.error, "cancelled")
        self.assertEqual(result.auth_ok, "passed")
        self.assertEqual(result.cleanup, "revoked")
        self.assertEqual(transport.calls[-1]["uid"], self.rotated["UID"])

    def test_revoke_failure_is_a_separate_result_and_still_clears_local_state(self):
        for error in (PocError("transport_failed"), KeyboardInterrupt()):
            with self.subTest(error_type=type(error).__name__):
                transport = self.make_transport(self.login_script(revoke_error=error))
                result = self.execute(transport)
                self.assertEqual(result.error, "none")
                self.assertEqual(result.auth_ok, "passed")
                self.assertEqual(result.config_ready, "passed")
                self.assertEqual(result.cleanup, "revocation_failed")

    def test_unexpected_exception_and_close_failure_never_leak_messages(self):
        message = " ".join(self.secret_values)
        transport = self.make_transport(self.login_script(), close_error=RuntimeError(message))
        result = self.execute(transport, authorize_error=RuntimeError(message))
        self.assertEqual(result.error, "unexpected_local_failure")
        self.assertEqual(result.cleanup, "revoked")
        self.assertEqual(result.checks, ["transport_close_failed"])

    def test_human_verification_retries_only_original_request_once_with_same_session(self):
        script = self.login_script()
        script.insert(0, ("POST", "/auth/v4/info", ApiError(422, 9001, self.human_details)))
        transport = self.make_transport(script)
        result = self.execute(transport)
        self.assertEqual(result.error, "none")
        self.assertEqual(result.human_verification, "passed")
        self.assertEqual(result.cleanup, "revoked")
        self.interaction.verify.assert_called_once()
        initial, retried = transport.calls[:2]
        for field in ("method", "path", "payload", "uid", "access", "refresh"):
            self.assertEqual(initial[field], retried[field])
        self.assertEqual(initial["uid"], "")
        self.assertIs(initial["payload"], retried["payload"])
        self.assertIsNone(initial["kwargs"].get("extra_headers"))
        self.assertEqual(retried["kwargs"], {"params": None, "extra_headers": self.human_headers})
        for index, call in enumerate(transport.calls):
            if index != 1:
                self.assertIsNone(call["kwargs"].get("extra_headers"))
        self.assertFalse(any(key.startswith("x-pm-human") for key in transport.headers))
        self.assertIn("browser_network_user_managed", result.checks)

    def test_human_verification_preserves_query_and_payload_binding(self):
        transport = mock.Mock()
        transport.headers = {"x-pm-appversion": "external-vpn@0.1.0"}
        transport.session = Session(
            **dict(
                zip(("uid", "access_token", "refresh_token"), self.anonymous.values(), strict=True)
            )
        )
        transport.request.side_effect = [
            ApiError(422, 9001, self.human_details),
            {"Code": 1000},
            {"Code": 1000},
        ]
        result = Result("guest", "explicit_proxy", "external-vpn@0.1.0")
        controller = flow.Flow(transport, result, self.interaction)
        payload, params = {"synthetic": "sensitive"}, {"WithState": "true"}
        controller.request("GET", "/vpn/v2/logicals", payload, params=params)
        controller.request("GET", "/vpn/v2/clientconfig")
        original, retried, next_request = transport.request.call_args_list
        self.assertEqual(original.args, retried.args)
        self.assertIs(retried.args[2], payload)
        self.assertIs(retried.kwargs["params"], params)
        self.assertEqual(retried.kwargs["extra_headers"], self.human_headers)
        self.assertIsNone(next_request.kwargs.get("extra_headers"))
        self.interaction.verify.assert_called_once()

    def test_second_human_verification_challenge_never_loops(self):
        transport = self.make_transport(
            [
                ("POST", "/auth/v4/info", ApiError(422, 9001, self.human_details)),
                ("POST", "/auth/v4/info", ApiError(422, 9001, self.human_details)),
            ]
        )
        result = self.execute(transport)
        self.assertEqual(result.error, "human_verification_required")
        self.assertEqual(result.human_verification, "required_not_completed")
        self.assertEqual(result.cleanup, "not_needed")
        self.interaction.verify.assert_called_once()
        self.assertEqual(sum(call["path"] == "/auth/v4/info" for call in transport.calls), 2)

    def test_later_new_human_challenge_also_cannot_start_second_verification(self):
        script = self.login_script()
        script.insert(0, ("POST", "/auth/v4/info", ApiError(422, 9001, self.human_details)))
        script = script[:3] + [
            ("GET", "/core/v4/users", ApiError(422, 9001, self.human_details)),
            ("DELETE", "/auth/v4", {"Code": 1000}),
        ]
        transport = self.make_transport(script)
        result = self.execute(transport)
        self.assertEqual(result.error, "human_verification_required")
        self.assertEqual(result.human_verification, "required_not_completed")
        self.assertEqual(result.cleanup, "revoked")
        self.interaction.verify.assert_called_once()
        self.assertEqual(sum(call["path"] == "/core/v4/users" for call in transport.calls), 1)

    def test_challenge_bound_session_cannot_change_before_retry(self):
        transport = mock.Mock()
        transport.session = Session(
            "synthetic-original-uid", "synthetic-access", "synthetic-refresh"
        )
        transport.request.side_effect = ApiError(422, 9001, self.human_details)
        result = Result("guest", "explicit_proxy", "external-vpn@0.1.0")
        controller = flow.Flow(transport, result, self.interaction)

        def switch_session(challenge):
            transport.session.uid = "synthetic-different-uid"
            return self.human_headers

        self.interaction.verify.side_effect = switch_session
        with self.assertRaises(PocError) as raised:
            controller.request("GET", "/core/v4/users")
        self.assertEqual(str(raised.exception), "human_verification_session_changed")
        self.assertEqual(transport.request.call_count, 1)

    def test_human_verification_cancel_or_expiry_cleans_up_without_retry(self):
        for error, category in (
            (KeyboardInterrupt(), "cancelled"),
            (PocError("human_verification_expired"), "human_verification_expired"),
        ):
            with self.subTest(error_type=type(error).__name__):
                self.interaction.verify.reset_mock()
                self.interaction.verify.side_effect = error
                script = self.login_script()[:2] + [
                    ("GET", "/core/v4/users", ApiError(422, 9001, self.human_details)),
                    ("DELETE", "/auth/v4", {"Code": 1000}),
                ]
                transport = self.make_transport(script)
                result = self.execute(transport)
                self.assertEqual(result.error, category)
                self.assertEqual(result.cleanup, "revoked")
                self.assertEqual(result.human_verification, "required_not_completed")
                self.interaction.verify.assert_called_once()
                self.assertEqual(
                    sum(call["path"] == "/core/v4/users" for call in transport.calls), 1
                )

    def test_non_human_or_device_challenge_is_not_reinterpreted_as_human_verification(self):
        for code in (9002, 2028, 8002):
            with self.subTest(code=code):
                transport = self.make_transport(
                    [
                        ("POST", "/auth/v4/info", ApiError(422, code, self.human_details)),
                    ]
                )
                result = self.execute(transport)
                self.assertEqual(result.api_code, code)
                self.assertEqual(result.human_verification, "not_triggered")
                self.assertEqual(result.cleanup, "not_needed")
        self.interaction.verify.assert_not_called()

    def test_unknown_or_denied_vpn_permissions_cannot_authorize(self):
        denied = [
            {"Code": 1000},
            {"Code": 1000, "VPN": []},
        ]
        for field, value in (
            ("Status", 0),
            ("Status", True),
            ("MaxTier", -1),
            ("MaxTier", "0"),
            ("MaxConnect", 0),
            ("MaxConnect", True),
        ):
            response = self.vpn_response()
            response["VPN"][field] = value
            denied.append(response)
        for value in (1, None, False):
            response = self.vpn_response()
            response["Delinquent"] = value
            denied.append(response)
        for index, response in enumerate(denied):
            with self.subTest(case=index):
                script = self.login_script()[:3] + [
                    ("GET", "/vpn/v2", response),
                    ("DELETE", "/auth/v4", {"Code": 1000}),
                ]
                transport = self.make_transport(script)
                result = self.execute(transport)
                self.assertEqual(result.auth_ok, "passed")
                self.assertIn(
                    result.error, ("invalid_vpn_permissions", "vpn_permission_denied_or_unknown")
                )
                self.assertEqual(result.wg_authorized, "not_run")
                self.assertEqual(result.cleanup, "revoked")
        self.assertEqual(self.authorizations, [])

    def test_wrong_protected_user_identity_blocks_permissions_and_authorization(self):
        script = self.login_script()[:2] + [
            ("GET", "/core/v4/users", {"User": {"ID": "synthetic-other-user"}}),
            ("DELETE", "/auth/v4", {"Code": 1000}),
        ]
        transport = self.make_transport(script)
        result = self.execute(transport)
        self.assertEqual(result.auth_ok, "failed")
        self.assertEqual(result.error, "protected_user_identity_mismatch")
        self.assertEqual(result.cleanup, "revoked")
        self.assertFalse(any(call["path"].startswith("/vpn/") for call in transport.calls))

    def test_session_bootstrap_parse_failures_never_report_cleanup_not_needed(self):
        for category in (
            "invalid_json_response",
            "response_too_large",
            "invalid_api_code",
            "unsupported_content_encoding",
        ):
            with self.subTest(category=category):
                transport = self.make_transport(
                    [("POST", "/auth/v4/sessions", PocError(category, status=200))]
                )
                result = self.execute(transport, identity="guest")
                self.assertEqual(result.auth_ok, "failed")
                self.assertEqual(result.error, category)
                self.assertNotIn(result.cleanup, ("not_needed", "revoked"))

    def test_login_parse_failure_reports_unknown_without_anonymous_dependency(self):
        transport = self.make_transport(
            [
                ("POST", "/auth/v4/info", self.info),
                ("POST", "/auth/v4", PocError("invalid_json_response", status=200)),
            ]
        )
        result = self.execute(transport)
        self.assertEqual(result.error, "invalid_json_response")
        self.assertEqual(result.auth_ok, "failed")
        self.assertEqual(result.cleanup, "unknown_without_session_handle")
        self.assertFalse(any(call["method"] == "DELETE" for call in transport.calls))

    def test_explicit_bootstrap_rejection_has_no_unknown_created_session(self):
        transport = self.make_transport([("POST", "/auth/v4/sessions", ApiError(422, 9002, {}))])
        result = self.execute(transport, identity="guest")
        self.assertEqual(result.error, "device_verification_required")
        self.assertEqual(result.cleanup, "not_needed")
        self.assertEqual(len(transport.calls), 1)

    def test_partial_bootstrap_handle_is_retained_for_revoke_only(self):
        partial = {"UID": "synthetic-partial-uid", "AccessToken": "synthetic-partial-access"}
        self.secret_values += list(partial.values())
        transport = self.make_transport(
            [
                ("POST", "/auth/v4/sessions", partial),
                ("DELETE", "/auth/v4", {"Code": 1000}),
            ]
        )
        result = self.execute(transport, identity="guest")
        self.assertEqual(result.auth_ok, "failed")
        self.assertEqual(result.error, "invalid_session_response")
        self.assertEqual(result.cleanup, "revoked")
        self.assertEqual(transport.calls[-1]["uid"], partial["UID"])
        self.assertEqual(transport.calls[-1]["access"], partial["AccessToken"])
        self.srp_client.proofs.assert_not_called()

    def test_partial_login_handle_is_revoked_without_authorizing(self):
        partial = {"UID": "synthetic-partial-uid", "AccessToken": "synthetic-partial-access"}
        self.secret_values += list(partial.values())
        transport = self.make_transport(
            [
                ("POST", "/auth/v4/info", self.info),
                ("POST", "/auth/v4", partial),
                ("DELETE", "/auth/v4", {"Code": 1000}),
            ]
        )
        result = self.execute(transport)
        self.assertEqual(result.auth_ok, "failed")
        self.assertEqual(result.error, "invalid_session_response")
        self.assertEqual(result.cleanup, "revoked")
        self.assertEqual(transport.calls[-1]["uid"], partial["UID"])
        self.assertEqual(transport.calls[-1]["access"], partial["AccessToken"])
        self.assertFalse(
            any(call["path"].startswith(("/core/", "/vpn/")) for call in transport.calls)
        )

    def test_partial_refresh_uses_rotated_access_token_for_cleanup_without_authorizing(self):
        partial = {"UID": self.authenticated["UID"], "AccessToken": self.rotated["AccessToken"]}
        script = self.login_script(refresh_response=partial)[:-2]
        script.append(("DELETE", "/auth/v4", {"Code": 1000}))
        transport = self.make_transport(script)
        result = self.execute(transport)
        self.assertEqual(result.auth_ok, "passed")
        self.assertEqual(result.error, "invalid_session_response")
        self.assertEqual(result.refresh, "not_run")
        self.assertEqual(result.cleanup, "revoked")
        self.assertEqual(transport.calls[-1]["uid"], partial["UID"])
        self.assertEqual(transport.calls[-1]["access"], partial["AccessToken"])
        self.assertEqual(transport.calls[-1]["refresh"], "")
        self.assertEqual(self.authorizations, [])

    def test_credentialless_parse_failure_cannot_hide_unknown_session_after_old_revoke(self):
        transport = self.make_transport(
            [
                ("POST", "/auth/v4/sessions", self.anonymous),
                ("POST", "/auth/v4/credentialless", PocError("invalid_json_response", status=200)),
                ("DELETE", "/auth/v4", {"Code": 1000}),
            ]
        )
        result = self.execute(transport, identity="guest")
        self.assertEqual(result.error, "invalid_json_response")
        self.assertEqual(result.auth_ok, "failed")
        self.assertEqual(result.cleanup, "remote_session_state_unknown")
        self.assertIn("known_session_handles_revoked", result.checks)
        self.assertEqual(transport.calls[-1]["uid"], self.anonymous["UID"])

    def test_partial_credentialless_handle_and_anonymous_session_are_both_revoked(self):
        partial = {"UID": "synthetic-partial-uid", "AccessToken": "synthetic-partial-access"}
        self.secret_values += list(partial.values())
        transport = self.make_transport(
            [
                ("POST", "/auth/v4/sessions", self.anonymous),
                ("POST", "/auth/v4/credentialless", partial),
                ("DELETE", "/auth/v4", {"Code": 1000}),
                ("DELETE", "/auth/v4", {"Code": 1000}),
            ]
        )
        result = self.execute(transport, identity="guest")
        self.assertEqual(result.error, "invalid_session_response")
        self.assertEqual(result.auth_ok, "failed")
        self.assertEqual(result.cleanup, "revoked")
        revoked = {
            call["uid"]: call["access"] for call in transport.calls if call["method"] == "DELETE"
        }
        self.assertEqual(
            revoked,
            {
                self.anonymous["UID"]: self.anonymous["AccessToken"],
                partial["UID"]: partial["AccessToken"],
            },
        )

    def test_getpass_warning_fails_closed_before_any_echo_fallback(self):
        for operation in ("hidden", "verify"):
            with self.subTest(operation=operation):
                echoed = []

                def insecure_getpass(*args, echoed=echoed, **kwargs):
                    warnings.warn(
                        "synthetic-password-private", flow.getpass.GetPassWarning, stacklevel=1
                    )
                    echoed.append("synthetic-password-private")
                    return "synthetic-password-private"

                fake_tty = mock.Mock()
                fake_tty.isatty.return_value = True
                challenge = mock.Mock()
                challenge.url.return_value = "https://verify.proton.me?synthetic-challenge-private"
                interaction = flow.LocalInteraction()
                with (
                    mock.patch.object(
                        flow, "sys", SimpleNamespace(stdin=fake_tty, stderr=fake_tty)
                    ),
                    mock.patch.object(flow.getpass, "getpass", side_effect=insecure_getpass),
                    mock.patch("builtins.open", mock.mock_open()) as opened,
                    self.assertRaises(PocError) as raised,
                ):
                    opened.return_value.__enter__.return_value = fake_tty
                    if operation == "hidden":
                        interaction.hidden("Private prompt: ")
                    else:
                        interaction.verify(challenge)
                self.assertEqual(echoed, [])
                self.assertNotIn("synthetic-password-private", str(raised.exception))

    def test_guest_uses_independent_identity_with_same_refresh_cleanup_contract(self):
        transport = self.make_transport(self.guest_script())
        result = self.execute(transport, identity="guest")
        self.assertEqual(result.identity, "guest")
        self.assertEqual(result.auth_ok, "passed")
        self.assertEqual(result.cleanup, "revoked")
        self.assertEqual(transport.calls[0]["path"], "/auth/v4/sessions")
        self.assertEqual(transport.calls[1]["path"], "/auth/v4/credentialless")
        self.assertEqual(transport.calls[1]["uid"], self.anonymous["UID"])
        self.interaction.credentials.assert_not_called()
        self.srp_client.proofs.assert_not_called()


if __name__ == "__main__":
    unittest.main()
