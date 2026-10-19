"""Independent, offline Proton certificate and standard WireGuard configuration helpers.

Protocol facts were checked against ProtonVPN/android-app commit
316a3d499e120bd2cab63c2cd8ac396e3ab3b24a. See the PoC protocol contract for
individual paths. No upstream implementation is copied. This module performs
no networking and does not implement Proton's additional in-tunnel local agent.
"""

from __future__ import annotations

import base64
import binascii
import hmac
import ipaddress
import os
import stat
import time
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path

from . import ANDROID_SOURCE

ANDROID_COMMIT = ANDROID_SOURCE
PROTOCOL = "WireGuardUDP"
# Constants.kt and ConnectionParamsWireguard.kt, at ANDROID_COMMIT.
CLIENT_ADDRESS = "10.2.0.2/32"
DNS_ADDRESS = "10.2.0.1"
ALLOWED_IPS = "0.0.0.0/0, ::/0"
KEEPALIVE = 60


class WireguardError(ValueError):
    """Only fixed categories may be reported; never include input data."""

    def __init__(self, category: str):
        super().__init__(category)
        self.category = category


def _key_bytes(value: object) -> bytes:
    if not isinstance(value, str) or len(value) != 44:
        raise WireguardError("invalid_wireguard_public_key")
    try:
        raw = base64.b64decode(value, validate=True)
    except (ValueError, binascii.Error):
        raise WireguardError("invalid_wireguard_public_key") from None
    if len(raw) != 32 or raw == bytes(32) or base64.b64encode(raw).decode("ascii") != value:
        raise WireguardError("invalid_wireguard_public_key")
    return raw


@dataclass(frozen=True, repr=False)
class KeyMaterial:
    public_key_pem: str
    ed25519_public_key: bytes
    x25519_public_key: bytes
    x25519_private_key: bytes

    @classmethod
    def generate(cls) -> KeyMaterial:
        return cls.from_seed(os.urandom(32))

    @classmethod
    def from_seed(cls, seed: bytes) -> KeyMaterial:
        if not isinstance(seed, bytes) or len(seed) != 32:
            raise WireguardError("invalid_key_seed")
        try:
            from cryptography.hazmat.primitives import serialization
            from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
            from nacl.signing import SigningKey
        except ImportError:
            raise WireguardError("wireguard_dependencies_missing") from None
        signing_key = SigningKey(seed)
        private_key = signing_key.to_curve25519_private_key()
        public_key = signing_key.verify_key.to_curve25519_public_key()
        if not hmac.compare_digest(bytes(private_key.public_key), bytes(public_key)):
            raise WireguardError("wireguard_key_conversion_failed")
        pem = (
            Ed25519PrivateKey.from_private_bytes(seed)
            .public_key()
            .public_bytes(
                serialization.Encoding.PEM, serialization.PublicFormat.SubjectPublicKeyInfo
            )
            .decode("ascii")
        )
        return cls(pem, bytes(signing_key.verify_key), bytes(public_key), bytes(private_key))


def certificate_request(keys: KeyMaterial, device_name: str = "Usque PoC") -> dict:
    if (
        not isinstance(device_name, str)
        or not 1 <= len(device_name) <= 64
        or not device_name.isascii()
        or any(ord(c) < 32 or ord(c) == 127 for c in device_name)
    ):
        raise WireguardError("invalid_device_name")
    # CertificateRequestBody.kt; public key is Ed25519 SPKI PEM, not its
    # X25519 conversion. Mode session intentionally follows the Android flow.
    return {
        "ClientPublicKey": keys.public_key_pem,
        "ClientPublicKeyMode": "EC",
        "DeviceName": device_name,
        "Mode": "session",
        "Features": [],
    }


@dataclass(frozen=True, repr=False)
class Authorization:
    certificate_pem: str
    expires_at: int
    refresh_at: int
    ed25519_public_key: bytes


def _now_seconds(now: int | float | datetime | None) -> float:
    if now is None:
        return time.time()
    if isinstance(now, datetime):
        if now.tzinfo is None:
            raise WireguardError("invalid_validation_time")
        return now.timestamp()
    if type(now) not in {int, float}:
        raise WireguardError("invalid_validation_time")
    return float(now)


def validate_certificate(
    response: dict, keys: KeyMaterial, now: int | float | datetime | None = None
) -> Authorization:
    """Bind a TLS-delivered certificate to local keys and validate its time bounds.

    This checks the returned leaf, not an independently verified issuer chain.
    Session ownership is supplied by the orchestrator making this request with
    the current session, since the documented response has no session-id field.
    """
    try:
        from cryptography import x509
        from cryptography.exceptions import UnsupportedAlgorithm
        from cryptography.hazmat.primitives import serialization
        from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
    except ImportError:
        raise WireguardError("wireguard_dependencies_missing") from None
    if not isinstance(response, dict):
        raise WireguardError("invalid_certificate_response")
    pem, expires, refresh = (
        response.get(k) for k in ("Certificate", "ExpirationTime", "RefreshTime")
    )
    if (
        not isinstance(pem, str)
        or not 1 <= len(pem) <= 65536
        or type(expires) is not int
        or type(refresh) is not int
        or not 0 < refresh < expires < 253402300800
    ):
        raise WireguardError("invalid_certificate_response")
    try:
        encoded = pem.encode("ascii")
        lines = encoded.strip().splitlines()
        # A chain or trailing object is not silently accepted as a leaf.
        if (
            encoded.count(b"-----BEGIN CERTIFICATE-----") != 1
            or lines[0] != b"-----BEGIN CERTIFICATE-----"
            or lines[-1] != b"-----END CERTIFICATE-----"
        ):
            raise ValueError
        body = b"".join(lines[1:-1])
        der = base64.b64decode(body, validate=True)
        if base64.b64encode(der) != body:
            raise ValueError
        certificate = x509.load_der_x509_certificate(der)
        if certificate.public_bytes(serialization.Encoding.DER) != der:
            raise ValueError
        public_key = certificate.public_key()
        if not isinstance(public_key, Ed25519PublicKey):
            raise ValueError
        raw = public_key.public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)
    except (ValueError, TypeError, UnicodeError, UnsupportedAlgorithm):
        raise WireguardError("invalid_certificate") from None
    if not hmac.compare_digest(raw, keys.ed25519_public_key):
        raise WireguardError("certificate_key_mismatch")
    current = _now_seconds(now)
    not_before = certificate.not_valid_before_utc.timestamp()
    not_after = certificate.not_valid_after_utc.timestamp()
    if not not_before <= current < not_after or expires <= current:
        raise WireguardError("certificate_expired_or_not_yet_valid")
    if not not_before <= refresh < expires <= not_after:
        raise WireguardError("certificate_time_mismatch")
    if refresh <= current:
        raise WireguardError("certificate_refresh_due")
    return Authorization(pem, expires, refresh, raw)


@dataclass(frozen=True, repr=False)
class Node:
    logical_id: str
    physical_id: str
    endpoint: str
    port: int
    public_key: str
    # Directory visibility and advertised status do not prove connectivity or
    # account authorization; the separate binary status feed is not consumed.
    availability_verified: bool = False


def _port(value: object) -> int:
    if type(value) is not int or not 1 <= value <= 65535:
        raise WireguardError("invalid_wireguard_port")
    return value


def select_free_server(directory: dict, client_config: dict | None = None) -> Node:
    """Select one advertised free UDP candidate without probing or connecting.

    Require protocol-specific endpoints instead of inferring support from an
    unrelated EntryIP. Prefer per-node ports; fallback ports, when supplied,
    must come from the current clientconfig response, never a guessed constant.
    """
    if not isinstance(directory, dict) or not isinstance(directory.get("LogicalServers"), list):
        raise WireguardError("invalid_server_directory")
    fallback_ports = []
    if client_config is not None:
        try:
            fallback_ports = client_config["DefaultPorts"]["WireGuard"]["UDP"]
        except (KeyError, TypeError):
            raise WireguardError("invalid_client_config_ports") from None
        if not isinstance(fallback_ports, list) or not fallback_ports:
            raise WireguardError("invalid_client_config_ports")
        for port in fallback_ports:
            _port(port)
    for logical in directory["LogicalServers"]:
        if not isinstance(logical, dict) or type(logical.get("Tier")) is not int:
            continue
        logical_status = logical.get("Status", 1)
        if logical["Tier"] != 0 or type(logical_status) is not int or logical_status != 1:
            continue
        # Secure Core/restricted gateway offerings are outside this experiment.
        features = logical.get("Features")
        if type(features) is not int or features & (1 | 32):
            continue
        logical_id = logical.get("ID")
        physicals = logical.get("Servers")
        if not isinstance(logical_id, str) or not logical_id or not isinstance(physicals, list):
            continue
        for physical in physicals:
            if (
                not isinstance(physical, dict)
                or type(physical.get("Status")) is not int
                or physical["Status"] != 1
            ):
                continue
            try:
                entry = physical["EntryPerProtocol"][PROTOCOL]
                endpoint = str(ipaddress.IPv4Address(entry["IPv4"]))
                address = ipaddress.IPv4Address(endpoint)
                if not address.is_global:
                    continue
                ports = entry.get("Ports")
                if ports is None:
                    ports = fallback_ports
                if not isinstance(ports, list) or not ports:
                    continue
                port = _port(ports[0])
                public_key = physical["X25519PublicKey"]
                _key_bytes(public_key)
                physical_id = physical["ID"]
                if not isinstance(physical_id, str) or not physical_id:
                    continue
            except (KeyError, TypeError, ValueError):
                continue
            return Node(logical_id, physical_id, endpoint, port, public_key)
    raise WireguardError("no_advertised_free_wireguard_candidate")


def build_config(
    keys: KeyMaterial,
    authorization: Authorization,
    node: Node,
    *,
    now: int | float | datetime | None = None,
) -> str:
    # Revalidate expiry before a sensitive export, even if the human interaction
    # took time after certificate issuance. No X.509 field is sent in WG packets.
    if not hmac.compare_digest(authorization.ed25519_public_key, keys.ed25519_public_key):
        raise WireguardError("certificate_key_mismatch")
    validate_certificate(
        {
            "Certificate": authorization.certificate_pem,
            "ExpirationTime": authorization.expires_at,
            "RefreshTime": authorization.refresh_at,
        },
        keys,
        now,
    )
    _key_bytes(node.public_key)
    _port(node.port)
    try:
        endpoint = str(ipaddress.IPv4Address(node.endpoint))
        if not ipaddress.IPv4Address(endpoint).is_global:
            raise ValueError
    except (TypeError, ValueError):
        raise WireguardError("invalid_wireguard_endpoint") from None
    private_key = base64.b64encode(keys.x25519_private_key).decode("ascii")
    # IPv4 addresses/DNS match the fixed Android configuration. Both default
    # AllowedIPs are included as in its full-tunnel policy. This does not configure
    # host routing or promise IPv6 service. MTU is deliberately omitted because
    # ConnectionParamsWireguard does not specify one (backend choice is external).
    return (
        "[Interface]\n"
        f"PrivateKey = {private_key}\n"
        f"Address = {CLIENT_ADDRESS}\n"
        f"DNS = {DNS_ADDRESS}\n\n"
        "[Peer]\n"
        f"PublicKey = {node.public_key}\n"
        f"AllowedIPs = {ALLOWED_IPS}\n"
        f"Endpoint = {endpoint}:{node.port}\n"
        f"PersistentKeepalive = {KEEPALIVE}\n"
    )


def export_private_config(path: str | Path, config: str) -> None:
    """Create a new 0600 file under an existing private Linux parent directory.

    Walk by directory descriptors with O_NOFOLLOW, so neither an ancestor nor
    the final path can redirect writes via symlinks. Never overwrite a file and
    never write private material into this checkout.
    """
    candidate = Path(path)
    repository = Path(__file__).absolute().parents[2]
    if (
        os.name != "posix"
        or not candidate.is_absolute()
        or ".." in candidate.parts
        or candidate.is_relative_to(repository)
        or not candidate.name
        or not isinstance(config, str)
        or len(config) > 65536
    ):
        raise WireguardError("invalid_private_export_path_or_content")
    descriptor = None
    file_descriptor = None
    created = False
    try:
        directory_flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
        descriptor = os.open("/", directory_flags)
        # A user namespace can map the host root owner to an unprivileged UID.
        # Trust the actual root directory's owner rather than guessing its UID.
        root_owner = os.fstat(descriptor).st_uid
        parents = candidate.parts[1:-1]
        for index, component in enumerate(parents):
            next_descriptor = os.open(component, directory_flags, dir_fd=descriptor)
            os.close(descriptor)
            descriptor = next_descriptor
            details = os.fstat(descriptor)
            writable = stat.S_IMODE(details.st_mode) & 0o022
            trusted_sticky = details.st_uid == root_owner and details.st_mode & stat.S_ISVTX
            if details.st_uid not in {root_owner, os.getuid()} or (writable and not trusted_sticky):
                raise WireguardError("unsafe_private_export_directory")
            if index == len(parents) - 1 and (
                details.st_uid != os.getuid() or stat.S_IMODE(details.st_mode) & 0o077
            ):
                raise WireguardError("private_export_parent_requires_0700")
        if not parents:
            raise WireguardError("private_export_parent_requires_0700")
        file_descriptor = os.open(
            candidate.name,
            os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
            0o600,
            dir_fd=descriptor,
        )
        created = True
        os.fchmod(file_descriptor, 0o600)
        details = os.fstat(file_descriptor)
        if not stat.S_ISREG(details.st_mode) or details.st_nlink != 1:
            raise WireguardError("unsafe_private_export_file")
        with os.fdopen(file_descriptor, "w", encoding="ascii", newline="\n") as output:
            file_descriptor = None
            output.write(config)
            output.flush()
            os.fsync(output.fileno())
    except BaseException as error:
        if created and descriptor is not None:
            os.unlink(candidate.name, dir_fd=descriptor)
        if isinstance(error, (KeyboardInterrupt, SystemExit, WireguardError)):
            raise
        raise WireguardError("private_export_failed") from None
    finally:
        if file_descriptor is not None:
            os.close(file_descriptor)
        if descriptor is not None:
            os.close(descriptor)
