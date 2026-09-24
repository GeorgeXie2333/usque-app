"""Archive matching Flutter AOT symbols separately from installable payloads."""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import re
import struct
import sys
import zipfile
from pathlib import Path

ARCHITECTURES = {
    "windows-x64": (62, "app.windows-x64.symbols"),
    "windows-arm64": (183, "app.windows-arm64.symbols"),
    "armeabi-v7a": (40, "app.android-arm.symbols"),
    "arm64-v8a": (183, "app.android-arm64.symbols"),
    "x86_64": (62, "app.android-x64.symbols"),
}


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def elf_identity(data: bytes) -> tuple[int, str, bool]:
    """Read the architecture, GNU build ID and nonempty DWARF section."""
    if len(data) < 64 or data[:4] != b"\x7fELF" or data[4] not in (1, 2):
        raise ValueError("Expected an ELF AOT binary or symbol file")
    if data[5] not in (1, 2):
        raise ValueError("Unsupported ELF byte order")
    endian = "<" if data[5] == 1 else ">"
    header_format = endian + ("HHIIIIIHHHHHH" if data[4] == 1 else "HHIQQQIHHHHHH")
    header = struct.unpack_from(header_format, data, 16)
    machine, section_offset = header[1], header[5]
    section_stride, count, strings_index = header[10:13]
    section_format = endian + ("IIIIIIIIII" if data[4] == 1 else "IIQQQQIIQQ")
    if (
        not count
        or strings_index >= count
        or section_stride < struct.calcsize(section_format)
        or section_offset + count * section_stride > len(data)
    ):
        raise ValueError("Invalid ELF section table")
    sections = [
        struct.unpack_from(section_format, data, section_offset + index * section_stride)
        for index in range(count)
    ]

    def contents(section: tuple[int, ...]) -> bytes:
        offset, size = section[4:6]
        if offset + size > len(data):
            raise ValueError("Truncated ELF section")
        return data[offset : offset + size]

    names = contents(sections[strings_index])
    ids: set[str] = set()
    has_debug = False
    for section in sections:
        name_offset = section[0]
        if name_offset >= len(names):
            raise ValueError("Invalid ELF section name")
        name = names[name_offset:].split(b"\0", 1)[0]
        if name == b".debug_info" and section[5]:
            contents(section)
            has_debug = True
        if section[1] != 7:  # SHT_NOTE
            continue
        notes = contents(section)
        offset = 0
        while offset < len(notes):
            if offset + 12 > len(notes):
                raise ValueError("Truncated ELF note header")
            name_size, value_size, kind = struct.unpack_from(endian + "III", notes, offset)
            name_start = offset + 12
            value_start = name_start + ((name_size + 3) & ~3)
            end = value_start + ((value_size + 3) & ~3)
            if end > len(notes):
                raise ValueError("Truncated ELF note")
            owner = notes[name_start : name_start + name_size].rstrip(b"\0")
            if owner == b"GNU" and kind == 3 and value_size:
                ids.add(notes[value_start : value_start + value_size].hex())
            offset = end
    if len(ids) != 1:
        raise ValueError("Expected exactly one GNU build ID")
    return machine, ids.pop(), has_debug


def matching_symbols(binary: bytes, symbols: bytes, architecture: str) -> str:
    expected_machine = ARCHITECTURES[architecture][0]
    machine, build_id, _ = elf_identity(binary)
    symbol_machine, symbol_id, has_debug = elf_identity(symbols)
    if machine != expected_machine or symbol_machine != expected_machine:
        raise ValueError(f"ELF architecture mismatch for {architecture}")
    if build_id != symbol_id:
        raise ValueError(f"AOT/symbol build ID mismatch for {architecture}")
    if not has_debug:
        raise ValueError(f"Symbol file has no DWARF information for {architecture}")
    return build_id


def archive(args: argparse.Namespace) -> Path:
    if not re.fullmatch(r"[0-9a-f]{40}", args.source_commit):
        raise ValueError("source-commit must be a complete lowercase Git SHA")
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:-beta\.[0-9]+)?(?:\+[0-9]+)?", args.version):
        raise ValueError("version must be the source release version")
    artifact = Path(args.artifact)
    artifact_bytes = artifact.read_bytes()
    artifact_hash = digest(artifact_bytes)
    binaries: dict[str, bytes] = {}
    if args.platform == "windows":
        if args.kind != "windows" or args.architecture not in ("windows-x64", "windows-arm64"):
            raise ValueError("Windows requires kind windows and a Windows architecture")
        binaries[args.architecture] = artifact_bytes
    else:
        if args.kind not in ("split", "universal"):
            raise ValueError("Android requires kind split or universal")
        expected = set(args.architecture.split(","))
        android_abis = {"armeabi-v7a", "arm64-v8a", "x86_64"}
        if not expected <= android_abis or not expected:
            raise ValueError("Unsupported Android ABI")
        if (args.kind == "split" and len(expected) != 1) or (
            args.kind == "universal" and expected != android_abis
        ):
            raise ValueError("Package kind does not match expected ABIs")
        with zipfile.ZipFile(io.BytesIO(artifact_bytes)) as apk:
            names = apk.namelist()
            if len(names) != len(set(names)):
                raise ValueError("Duplicate APK entries")
            for name in names:
                match = re.fullmatch(r"lib/([^/]+)/libapp\.so", name)
                if match:
                    binaries[match[1]] = apk.read(name)
        if set(binaries) != expected:
            raise ValueError("APK AOT architectures do not match the expected ABI set")
    # Validate the entire set before creating an archive. A mismatch must never
    # leave an apparently successful manifest containing only some architectures.
    records = []
    validated_symbols: dict[str, bytes] = {}
    for architecture, binary in sorted(binaries.items()):
        symbol_name = ARCHITECTURES[architecture][1]
        symbol_path = Path(args.symbols) / symbol_name
        symbols = symbol_path.read_bytes()
        build_id = matching_symbols(binary, symbols, architecture)
        validated_symbols[architecture] = symbols
        records.append(
            {
                "architecture": architecture,
                "build_id": build_id,
                "aot_sha256": digest(binary),
                "symbol_file": f"{architecture}/{symbol_name}",
                "symbol_sha256": digest(symbols),
            }
        )
    destination = (
        Path(args.output)
        / args.version
        / args.source_commit
        / args.platform
        / args.kind
        / artifact_hash
    )
    destination.mkdir(parents=True, exist_ok=False)
    for record in records:
        target = destination / record["symbol_file"]
        target.parent.mkdir()
        target.write_bytes(validated_symbols[record["architecture"]])
    manifest = {
        "schema_version": 1,
        "version": args.version,
        "source_commit": args.source_commit,
        "platform": args.platform,
        "kind": args.kind,
        "artifact_name": artifact.name,
        "artifact_sha256": artifact_hash,
        "symbols": records,
    }
    (destination / "manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    return destination


def verify_stack(symbols: Path, stack: Path) -> None:
    _, build_id, has_debug = elf_identity(symbols.read_bytes())
    stack_ids = set(re.findall(r"build_id:\s*'([0-9a-fA-F]+)'", stack.read_text(encoding="utf-8")))
    if not has_debug or {value.lower() for value in stack_ids} != {build_id}:
        raise ValueError("Stack/symbol build ID mismatch or missing DWARF information")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    save = commands.add_parser("archive")
    for name in ("artifact", "symbols", "output", "version", "source-commit", "architecture"):
        save.add_argument(f"--{name}", required=True)
    save.add_argument("--platform", choices=("windows", "android"), required=True)
    save.add_argument("--kind", choices=("windows", "split", "universal"), required=True)
    check = commands.add_parser("verify-stack")
    check.add_argument("--symbols", type=Path, required=True)
    check.add_argument("--stack", type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.command == "archive":
            print(archive(args))
        else:
            verify_stack(args.symbols, args.stack)
            print("Stack and symbols match")
    except (OSError, ValueError, struct.error, zipfile.BadZipFile) as error:
        print(f"Flutter symbol validation failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
