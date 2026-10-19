"""Cancelable in-memory orchestration; every exit attempts current-session revocation."""

from __future__ import annotations

import getpass
import sys
import warnings
from datetime import datetime, timezone

from .challenge import build_payload
from .srp import SrpClient, SrpError
from .transport import ApiError, PocError, Result, Session
from .verification import VerificationChallenge
from .wireguard import (
    KeyMaterial,
    WireguardError,
    build_config,
    certificate_request,
    export_private_config,
    select_free_server,
    validate_certificate,
)


class LocalInteraction:
    def __init__(self, verification_mode="paste"):
        if verification_mode not in {"browser", "paste"}:
            raise PocError("invalid_verification_handoff_mode")
        self.verification_mode = verification_mode

    @staticmethod
    def read_without_echo(prompt, stream):
        # getpass otherwise falls back to echoed stdin if terminal controls fail.
        with warnings.catch_warnings():
            warnings.simplefilter("error", getpass.GetPassWarning)
            try:
                return getpass.getpass(prompt, stream=stream)
            except getpass.GetPassWarning:
                raise PocError("secure_input_unavailable") from None

    def hidden(self, prompt: str) -> str:
        if not sys.stdin.isatty() or not sys.stderr.isatty():
            raise PocError("local_tty_required")
        try:
            value = self.read_without_echo(prompt, sys.stderr)
        except EOFError:
            raise PocError("local_input_cancelled_or_invalid") from None
        if not value or len(value) > 16384:
            raise PocError("local_input_cancelled_or_invalid")
        return value

    def credentials(self) -> tuple[str, str]:
        return self.hidden("Proton username (local only): "), self.hidden("Password: ")

    def totp(self) -> str:
        value = self.hidden("TOTP: ")
        if not value.isascii() or not value.isdecimal() or len(value) != 6:
            raise PocError("invalid_totp")
        return value

    def verify(self, challenge):
        if not sys.stdin.isatty() or not sys.stderr.isatty():
            raise PocError("local_tty_required")
        if self.verification_mode == "browser":
            return self.verify_in_browser(challenge)
        # The challenge URL is a private UI handoff, never stdout/stderr/report.
        # No browser automation or public callback service is used.
        try:
            with open("/dev/tty", "w", encoding="utf-8") as tty:
                if not tty.isatty():
                    raise PocError("local_tty_required")
                tty.write(
                    "Open this official challenge in your local browser:\n"
                    + challenge.url()
                    + "\nBrowser networking is managed separately. The official page uses an\n"
                    "AndroidInterface dispatch bridge and has no general copy-token UI.\n"
                    "If you can inspect the legitimate HUMAN_VERIFICATION_SUCCESS message\n"
                    "using local browser developer tools, paste that JSON below.\n"
                    "Otherwise cancel with Ctrl-C. Never paste authentication data in chat.\n"
                )
                tty.flush()
                message = self.read_without_echo("Local success message (hidden): ", tty)
        except (OSError, EOFError):
            raise PocError("local_tty_unavailable") from None
        return challenge.success_headers(message)

    @staticmethod
    def verify_in_browser(challenge):
        from .browser_handoff import BrowserHandoff

        try:
            with (
                BrowserHandoff(challenge) as bridge,
                open("/dev/tty", "w", encoding="utf-8") as tty,
            ):
                if not tty.isatty():
                    raise PocError("local_tty_required")
                tty.write(
                    "Human verification required. This request and its session are paused.\n"
                    "1. Open the official page as a normal browser tab:\n"
                    + challenge.url()
                    + "\n2. In that tab's developer console, run this local callback listener once:\n"
                    + bridge.snippet
                    + "\n3. Complete the puzzle yourself. No puzzle actions are automated.\n"
                    "The page will submit its result to a temporary 127.0.0.1 listener.\n"
                    "Browser networking is separate from the CLI path. This browser handoff\n"
                    "has not been validated against a real challenge. Ctrl-C cancels and cleans up.\n"
                    "If you cancel, also close the verification tab to remove its listener.\n"
                    "If local navigation is blocked, --verification-handoff paste is available\n"
                    "for a later explicitly started experiment. Do not send tokens in chat.\n"
                )
                tty.flush()
                return bridge.wait()
        except OSError:
            raise PocError("local_browser_handoff_unavailable") from None


class Flow:
    def __init__(self, transport, result, interaction):
        self.transport = transport
        self.result = result
        self.interaction = interaction
        self.verification_attempted = False
        self.retired_sessions = []
        self.user_id = ""
        self.session_change_uncertain = False

    def accept_session(self, response):
        previous = self.transport.session
        replacement = Session()
        try:
            replacement.replace(response)
        except PocError:
            # A malformed success must never authorize protected operations. If
            # it nevertheless provides a usable cleanup handle, keep it solely
            # for revocation; a refresh token is not needed for that request.
            values = [response.get(k) for k in ("UID", "AccessToken")]
            if all(isinstance(x, str) and x and len(x) <= 16384 for x in values):
                cleanup_handle = Session(values[0], values[1], "")
                if previous.uid == cleanup_handle.uid:
                    previous.access_token = cleanup_handle.access_token
                    previous.refresh_token = ""
                else:
                    self.retired_sessions.append(cleanup_handle)
                self.session_change_uncertain = False
            raise
        if previous.uid and previous.uid != replacement.uid:
            self.retired_sessions.append(
                Session(previous.uid, previous.access_token, previous.refresh_token)
            )
        previous.replace(response)
        self.session_change_uncertain = False

    def send(self, method, path, payload=None, *, params=None, extra_headers=None):
        changes_session = method == "POST" and path in {
            "/auth/v4/sessions",
            "/auth/v4/credentialless",
            "/auth/v4",
            "/auth/v4/refresh",
        }
        if changes_session:
            # Until a complete response or an explicit client rejection arrives,
            # the server may have created/rotated a session we cannot revoke.
            self.session_change_uncertain = True
        kwargs = {"params": params}
        if extra_headers is not None:
            kwargs["extra_headers"] = extra_headers
        try:
            return self.transport.request(method, path, payload, **kwargs)
        except ApiError as error:
            if changes_session and 400 <= error.status < 500 and error.code not in {1000, 1001}:
                self.session_change_uncertain = False
            raise

    def request(self, method, path, payload=None, *, params=None):
        try:
            return self.send(method, path, payload, params=params)
        except ApiError as error:
            if error.code != 9001:
                raise
            self.result.human_verification = "required_not_completed"
            if self.interaction is None or self.verification_attempted:
                raise
            self.verification_attempted = True
            challenge = VerificationChallenge.from_details(error.details)
            bound_session = (self.transport.session.uid, self.transport.session.access_token)
            self.result.checks.append("browser_network_user_managed")
            if getattr(self.interaction, "verification_mode", "paste") == "browser":
                self.result.checks.append("manual_puzzle_browser_callback")
            extra_headers = self.interaction.verify(challenge)
            if (self.transport.session.uid, self.transport.session.access_token) != bound_session:
                raise PocError("human_verification_session_changed") from None
            # Exactly this challenge-bound request, at most once. Headers are
            # request-local and never retained on the shared transport.
            response = self.send(method, path, payload, params=params, extra_headers=extra_headers)
            self.result.human_verification = "passed"
            return response

    def bootstrap(self):
        self.result.phase = "anonymous_session"
        response = self.request("POST", "/auth/v4/sessions", build_payload())
        self.accept_session(response)

    def guest(self):
        self.result.phase = "credentialless"
        response = self.request("POST", "/auth/v4/credentialless", build_payload())
        self.accept_session(response)
        if not isinstance(response.get("UserID"), str) or not response["UserID"]:
            raise PocError("missing_guest_user_identity")
        self.user_id = response["UserID"]

    def login(self):
        if self.interaction is None:
            raise PocError("login_requires_local_interactive_terminal")
        username, password = self.interaction.credentials()
        self.result.phase = "srp_info"
        info = self.request("POST", "/auth/v4/info", {"Username": username, "Intent": "Proton"})
        self.result.phase = "srp_proof"
        client = SrpClient.from_info(password, info)
        password = ""
        proofs = client.proofs()
        srp_session = info.get("SRPSession")
        if not isinstance(srp_session, str) or not srp_session or len(srp_session) > 16384:
            raise PocError("invalid_srp_session")
        response = self.request(
            "POST",
            "/auth/v4",
            {
                "Username": username,
                "ClientEphemeral": proofs.client_ephemeral,
                "ClientProof": proofs.client_proof,
                "SRPSession": srp_session,
                **build_payload(),
            },
        )
        # Capture a returned handle solely for finally/revocation, even if the
        # server proof is invalid. No authenticated operation precedes verification.
        self.accept_session(response)
        client.verify_server_proof(response.get("ServerProof"))
        self.user_id = response.get("UserID", "")
        username = ""
        second_factor = response.get("2FA")
        if second_factor is not None:
            if not isinstance(second_factor, dict) or type(second_factor.get("Enabled")) is not int:
                raise PocError("unsupported_second_factor_response")
            enabled = second_factor["Enabled"]
            if enabled == 1:
                self.result.phase = "totp"
                self.request("POST", "/auth/v4/2fa", {"TwoFactorCode": self.interaction.totp()})
            elif enabled != 0:
                raise PocError("unsupported_second_factor_mode")

    def verify_user(self):
        self.result.phase = "protected_user"
        response = self.request("GET", "/core/v4/users")
        user = response.get("User")
        if not isinstance(user, dict) or not isinstance(user.get("ID"), str) or not user["ID"]:
            raise PocError("invalid_protected_user_response")
        if self.user_id and self.user_id != user["ID"]:
            raise PocError("protected_user_identity_mismatch")

    def permissions(self):
        response = self.request("GET", "/vpn/v2")
        vpn = response.get("VPN")
        if not isinstance(vpn, dict):
            raise PocError("invalid_vpn_permissions")
        if (
            type(vpn.get("Status")) is not int
            or vpn["Status"] != 1
            or type(vpn.get("MaxTier")) is not int
            or vpn["MaxTier"] < 0
            or type(vpn.get("MaxConnect")) is not int
            or vpn["MaxConnect"] < 1
            or type(response.get("Delinquent")) is not int
            or response["Delinquent"] != 0
        ):
            raise PocError("vpn_permission_denied_or_unknown")

    def refresh(self):
        self.result.phase = "refresh"
        session = self.transport.session
        response = self.request(
            "POST",
            "/auth/v4/refresh",
            {
                "UID": session.uid,
                "RefreshToken": session.refresh_token,
                "ResponseType": "token",
                "GrantType": "refresh_token",
                "RedirectURI": "http://protonmail.ch",
            },
        )
        self.accept_session(response)
        self.permissions()
        self.result.refresh = "passed"

    def authorize(self, export_path):
        self.result.phase = "certificate"
        self.result.wg_authorized = "failed"
        keys = KeyMaterial.generate()
        response = self.request("POST", "/vpn/v1/certificate", certificate_request(keys))
        authorization = validate_certificate(response, keys)
        self.result.wg_authorized = "passed"
        self.result.checks.extend(
            [
                "certificate_issuer_chain_not_verified",
                "certificate_session_binding_via_authenticated_request",
                "local_agent_not_implemented",
                "node_availability_not_verified",
            ]
        )
        self.result.phase = "directory"
        client_config = self.request("GET", "/vpn/v2/clientconfig")
        directory = self.request(
            "GET",
            "/vpn/v2/logicals",
            params={"WithEntriesForProtocols": "WireGuardUDP", "WithState": "true"},
        )
        self.result.phase = "configuration"
        self.result.config_ready = "failed"
        node = select_free_server(directory, client_config)
        config = build_config(keys, authorization, node)
        self.result.config_ready = "passed"
        if export_path is not None:
            self.result.phase = "private_export"
            export_private_config(export_path, config)
            self.result.checks.append("export_for_structure_only_session_will_be_revoked")


def run(transport, *, identity, interaction=None, export_path=None):
    result = Result(
        identity=identity,
        network="direct_https" if getattr(transport, "direct", True) else "explicit_proxy",
        app_version=transport.headers["x-pm-appversion"],
        timestamp=datetime.now(timezone.utc).isoformat(),
    )
    flow = Flow(transport, result, interaction)
    try:
        result.auth_ok = "failed"
        if identity == "login":
            flow.login()
        else:
            flow.bootstrap()
            flow.guest()
        flow.verify_user()
        result.auth_ok = "passed"
        result.phase = "vpn_permissions"
        flow.permissions()
        flow.refresh()
        flow.authorize(export_path)
        result.phase = "complete"
    except KeyboardInterrupt:
        result.error = "cancelled"
    except ApiError as error:
        result.error = error.category
        result.http_status = error.status
        result.api_code = error.code
        if error.code == 9001:
            result.human_verification = "required_not_completed"
    except PocError as error:
        result.error = error.category
        result.http_status = error.status
        result.api_code = error.code
    except SrpError:
        result.error = "srp_validation_failed_or_unsupported"
    except WireguardError as error:
        result.error = error.category
    except Exception:
        # Last-resort boundary: neither arbitrary third-party exception messages
        # nor exception tracebacks may contain authentication material in output.
        result.error = "unexpected_local_failure"
    finally:
        handles = [transport.session, *reversed(flow.retired_sessions)]
        seen = set()
        owned = []
        for handle in handles:
            if handle.uid and handle.uid not in seen:
                owned.append(Session(handle.uid, handle.access_token, handle.refresh_token))
                seen.add(handle.uid)
        if owned:
            result.cleanup = "revoked"
        for handle in owned:
            transport.session.uid = handle.uid
            transport.session.access_token = handle.access_token
            transport.session.refresh_token = handle.refresh_token
            try:
                transport.request("DELETE", "/auth/v4")
            except (Exception, KeyboardInterrupt):
                result.cleanup = "revocation_failed"
            finally:
                transport.session.clear()
                handle.clear()
        flow.retired_sessions.clear()
        if flow.session_change_uncertain:
            if not owned:
                result.cleanup = "unknown_without_session_handle"
            elif result.cleanup == "revoked":
                result.cleanup = "remote_session_state_unknown"
                result.checks.append("known_session_handles_revoked")
            else:
                result.cleanup = "revocation_failed_remote_state_unknown"
        try:
            transport.close()
        except (Exception, KeyboardInterrupt):
            result.checks.append("transport_close_failed")
    return result
