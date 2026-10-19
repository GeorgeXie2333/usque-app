"""Proton SRP version 4, restricted to the signed 2048-bit group.

Protocol and test reference: ProtonMail/go-srp at
 d323688777afbb6265d8bc4a51419991a24630ca (srp.go and hash.go).
This is a PoC: Python integer exponentiation is not constant-time. Server proof
comparison is constant-time. Passwords and derived secrets are never logged.

Portions derived from the MIT reference:
Copyright (c) 2019 Proton Technologies AG

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
"""

from __future__ import annotations

import base64
import binascii
import hashlib
import hmac
import re
import secrets
from collections.abc import Mapping
from dataclasses import dataclass, field
from functools import lru_cache

from . import SRP_SOURCE

UPSTREAM_COMMIT = SRP_SOURCE
GROUP_BYTES = 256
# Exact raw Ed25519 primary key from go-srp's modulusPubkey. No key discovery,
# user keyring, network retrieval, alternate key or unverified modulus is used.
_MODULUS_PUBLIC_KEY = bytes.fromhex(
    "16ead65d7c234de9aa8c3ec25e35d5c8a7f4a1fee7f42b662fcbfd7a79338201"
)
_BCRYPT_ALPHABET = b"./ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789"
_STD_ALPHABET = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
_CLEARSIGN = re.compile(
    r"\A-----BEGIN PGP SIGNED MESSAGE-----\nHash: SHA256\n\n"
    r"([A-Za-z0-9+/]{342}==)\n"
    r"-----BEGIN PGP SIGNATURE-----\n"
    r"(?:(?:Version|Comment): [^\n]*\n)*\n"
    r"([A-Za-z0-9+/=\n]+)\n=([A-Za-z0-9+/]{4})\n"
    r"-----END PGP SIGNATURE-----(?:\n)?\Z"
)


class SrpError(ValueError):
    """A deliberately non-sensitive protocol/validation error."""


def _decode_base64(value: object, size: int, label: str) -> bytes:
    if not isinstance(value, str) or len(value) != 4 * ((size + 2) // 3):
        raise SrpError(f"Invalid SRP {label}")
    try:
        result = base64.b64decode(value, validate=True)
    except (ValueError, binascii.Error):
        raise SrpError(f"Invalid SRP {label}") from None
    if len(result) != size or base64.b64encode(result).decode("ascii") != value:
        raise SrpError(f"Invalid SRP {label}")
    return result


def expand_hash(data: bytes) -> bytes:
    """Proton's 2048-bit expansion (four SHA-512 digests)."""
    return b"".join(hashlib.sha512(data + bytes([index])).digest() for index in range(4))


def _crc24(data: bytes) -> bytes:
    crc = 0xB704CE
    for byte in data:
        crc ^= byte << 16
        for _ in range(8):
            crc <<= 1
            if crc & 0x1000000:
                crc ^= 0x1864CFB
    return (crc & 0xFFFFFF).to_bytes(3, "big")


def _signature_packet(data: bytes) -> bytes:
    """Read exactly one definite-length new-format OpenPGP signature packet."""
    if len(data) < 2 or data[0] != 0xC2:
        raise SrpError("Invalid SRP modulus signature")
    first = data[1]
    if first < 192:
        length, start = first, 2
    elif first < 224 and len(data) >= 3:
        length, start = ((first - 192) << 8) + data[2] + 192, 3
    elif first == 255 and len(data) >= 6:
        length, start = int.from_bytes(data[2:6], "big"), 6
    else:
        raise SrpError("Invalid SRP modulus signature")
    if length != len(data) - start:
        raise SrpError("Invalid SRP modulus signature")
    return data[start:]


def _read_mpi(data: bytes, offset: int) -> tuple[bytes, int]:
    if offset + 2 > len(data):
        raise SrpError("Invalid SRP modulus signature")
    bits = int.from_bytes(data[offset : offset + 2], "big")
    length = (bits + 7) // 8
    start, end = offset + 2, offset + 2 + length
    if not 1 <= bits <= 256 or end > len(data):
        raise SrpError("Invalid SRP modulus signature")
    value = data[start:end]
    if int.from_bytes(value, "big").bit_length() != bits:
        raise SrpError("Invalid SRP modulus signature")
    # Legacy OpenPGP EdDSA MPIs contain native Ed25519 octets, including S.
    return value.rjust(32, b"\0"), end


def _check_subpackets(data: bytes) -> None:
    offset = 0
    while offset < len(data):
        length = data[offset]
        offset += 1
        # The pinned signing format needs only short creation/issuer packets.
        if not 1 <= length < 192 or offset + length > len(data):
            raise SrpError("Invalid SRP modulus signature")
        kind = data[offset]
        if kind & 0x80 and (kind & 0x7F) not in (2, 16, 33):
            raise SrpError("Unsupported critical SRP signature field")
        offset += length


def verify_signed_modulus(signed_modulus: object) -> bytes:
    """Verify the exact Proton Ed25519/SHA256 clearsign format, then decode N.

    This intentionally is not a general OpenPGP reader. Unsupported formats,
    extra text/packets and altered or malformed signatures fail closed.
    """
    if not isinstance(signed_modulus, str) or len(signed_modulus) > 8192:
        raise SrpError("Invalid SRP signed modulus")
    normalized = signed_modulus.replace("\r\n", "\n")
    match = _CLEARSIGN.fullmatch(normalized)
    if match is None:
        raise SrpError("Invalid SRP signed modulus")
    message, armored, checksum = match.groups()
    try:
        packet = base64.b64decode(armored.replace("\n", ""), validate=True)
        crc = base64.b64decode(checksum, validate=True)
    except (ValueError, binascii.Error):
        raise SrpError("Invalid SRP modulus signature") from None
    if not hmac.compare_digest(_crc24(packet), crc):
        raise SrpError("Invalid SRP modulus signature")
    body = _signature_packet(packet)
    if len(body) < 10 or body[:4] != b"\x04\x01\x16\x08":
        raise SrpError("Unsupported SRP modulus signature")
    hashed_end = 6 + int.from_bytes(body[4:6], "big")
    if hashed_end + 2 > len(body):
        raise SrpError("Invalid SRP modulus signature")
    _check_subpackets(body[6:hashed_end])
    unhashed_end = hashed_end + 2 + int.from_bytes(body[hashed_end : hashed_end + 2], "big")
    if unhashed_end + 2 > len(body):
        raise SrpError("Invalid SRP modulus signature")
    _check_subpackets(body[hashed_end + 2 : unhashed_end])
    signed_fields = body[:hashed_end]
    digest = hashlib.sha256(
        message.encode("ascii")
        + signed_fields
        + b"\x04\xff"
        + len(signed_fields).to_bytes(4, "big")
    ).digest()
    if not hmac.compare_digest(digest[:2], body[unhashed_end : unhashed_end + 2]):
        raise SrpError("Invalid SRP modulus signature")
    r_part, offset = _read_mpi(body, unhashed_end + 2)
    s_part, offset = _read_mpi(body, offset)
    if offset != len(body):
        raise SrpError("Invalid SRP modulus signature")
    try:
        from cryptography.exceptions import InvalidSignature
        from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
    except ImportError:
        raise SrpError("SRP cryptographic dependencies are unavailable") from None
    try:
        Ed25519PublicKey.from_public_bytes(_MODULUS_PUBLIC_KEY).verify(r_part + s_part, digest)
    except (InvalidSignature, ValueError):
        raise SrpError("Invalid SRP modulus signature") from None
    return _decode_base64(message, GROUP_BYTES, "modulus")


def _probable_prime(number: int) -> bool:
    small_primes = (2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37)
    if number in small_primes:
        return True
    if number < 2 or any(number % prime == 0 for prime in small_primes):
        return False
    odd, shifts = number - 1, 0
    while odd % 2 == 0:
        odd //= 2
        shifts += 1
    # Independent random Miller-Rabin witnesses: false acceptance <= 2^-128.
    for _ in range(64):
        witness = secrets.randbelow(number - 3) + 2
        value = pow(witness, odd, number)
        if value in (1, number - 1):
            continue
        for _ in range(shifts - 1):
            value = pow(value, 2, number)
            if value == number - 1:
                break
        else:
            return False
    return True


@lru_cache(maxsize=8)
def _validated_modulus(modulus: bytes) -> int:
    number = int.from_bytes(modulus, "little")
    if len(modulus) != GROUP_BYTES or number.bit_length() != 2048 or number % 8 != 3:
        raise SrpError("Invalid SRP modulus parameters")
    half = (number - 1) // 2
    if not _probable_prime(half) or pow(2, half, number) != number - 1:
        raise SrpError("SRP modulus is not a safe prime with generator 2")
    return number


def hash_password_v4(password: str | bytes, salt: bytes, modulus: bytes) -> bytes:
    """The v3/v4 bcrypt($2y$, cost 10, salt || 'proton') derivation."""
    if isinstance(password, str):
        try:
            raw = password.encode("utf-8")
        except UnicodeError:
            raise SrpError("Unsupported SRP password encoding") from None
    elif isinstance(password, bytes):
        raw = password
    else:
        raise SrpError("Invalid SRP password")
    if len(salt) != 10 or len(modulus) != GROUP_BYTES:
        raise SrpError("Invalid SRP password parameters")
    # bcrypt 5 explicitly rejects >72 bytes. Do not silently reinterpret an
    # untested long-password scheme in this compatibility-limited PoC.
    if not raw or len(raw) > 72:
        raise SrpError("SRP PoC supports passwords of 1 to 72 UTF-8 bytes")
    encoded = base64.b64encode(salt + b"proton").rstrip(b"=")
    encoded = encoded.translate(bytes.maketrans(_STD_ALPHABET, _BCRYPT_ALPHABET))
    try:
        import bcrypt
    except ImportError:
        raise SrpError("SRP cryptographic dependencies are unavailable") from None
    try:
        crypted = bcrypt.hashpw(raw, b"$2y$10$" + encoded)
    except ValueError:
        raise SrpError("SRP password derivation failed") from None
    return expand_hash(crypted + modulus)


@dataclass(frozen=True)
class SrpProofs:
    client_ephemeral: str = field(repr=False)
    client_proof: str = field(repr=False)
    _expected_server_proof: bytes = field(repr=False)

    def verify_server_proof(self, proof: object) -> None:
        received = _decode_base64(proof, GROUP_BYTES, "server proof")
        if not hmac.compare_digest(received, self._expected_server_proof):
            raise SrpError("SRP server proof verification failed")


def _generate_proofs(modulus: bytes, server: bytes, hashed: bytes) -> SrpProofs:
    number = _validated_modulus(modulus)
    remote = int.from_bytes(server, "little")
    if len(server) != GROUP_BYTES or not 1 < remote < number - 1:
        raise SrpError("SRP server ephemeral is out of bounds")
    if len(hashed) != GROUP_BYTES:
        raise SrpError("Invalid SRP password derivation")
    generator = (2).to_bytes(GROUP_BYTES, "little")
    multiplier = int.from_bytes(expand_hash(generator + modulus), "little") % number
    if not 1 < multiplier < number - 1:
        raise SrpError("SRP multiplier is out of bounds")
    for _ in range(16):
        # Same strict lower/upper bound as the current MIT go-srp reference.
        secret = secrets.randbelow(number - 4098) + 4097
        client = pow(2, secret, number).to_bytes(GROUP_BYTES, "little")
        scramble = int.from_bytes(expand_hash(client + server), "little")
        if scramble:
            break
    else:
        raise SrpError("SRP ephemeral generation failed")
    password_number = int.from_bytes(hashed, "little")
    base = (remote - multiplier * pow(2, password_number, number)) % number
    if base in (0, 1, number - 1):
        raise SrpError("Degenerate SRP shared-secret base")
    exponent = (secret + scramble * password_number) % (number - 1)
    if exponent == 0:
        raise SrpError("Degenerate SRP shared-secret exponent")
    shared = pow(base, exponent, number).to_bytes(GROUP_BYTES, "little")
    client_proof = expand_hash(client + server + shared)
    expected = expand_hash(client + client_proof + shared)
    return SrpProofs(
        base64.b64encode(client).decode("ascii"),
        base64.b64encode(client_proof).decode("ascii"),
        expected,
    )


class SrpClient:
    """One authentication attempt; no password or raw session key is retained."""

    __slots__ = ("_proofs",)

    def __init__(self, proofs: SrpProofs) -> None:
        self._proofs = proofs

    @classmethod
    def from_info(cls, password: str | bytes, info: Mapping[str, object]) -> SrpClient:
        if not isinstance(info, Mapping) or type(info.get("Version")) is not int:
            raise SrpError("Invalid SRP authentication version")
        if info["Version"] != 4:
            raise SrpError("SRP PoC supports authentication version 4 only")
        modulus = verify_signed_modulus(info.get("Modulus"))
        _validated_modulus(modulus)
        salt = _decode_base64(info.get("Salt"), 10, "salt")
        server = _decode_base64(info.get("ServerEphemeral"), GROUP_BYTES, "server ephemeral")
        number = int.from_bytes(modulus, "little")
        if not 1 < int.from_bytes(server, "little") < number - 1:
            raise SrpError("SRP server ephemeral is out of bounds")
        return cls(_generate_proofs(modulus, server, hash_password_v4(password, salt, modulus)))

    def proofs(self) -> SrpProofs:
        return self._proofs

    def verify_server_proof(self, proof: object) -> None:
        self._proofs.verify_server_proof(proof)
