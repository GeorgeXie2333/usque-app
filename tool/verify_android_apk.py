"""Read-only release APK packaging checks; never install or execute the package."""

from __future__ import annotations

import argparse
import re
import struct
import subprocess
import sys
import zipfile
from pathlib import Path

ABI_MACHINES = {"armeabi-v7a": (1, 40), "arm64-v8a": (2, 183), "x86_64": (2, 62)}
REQUIRED_LIBRARIES = {"libapp.so", "libflutter.so", "libusque_android.so"}


def verify_elf(data: bytes, abi: str) -> None:
    """Check machine, load alignment and absence of shipped DWARF sections."""
    if len(data) < 64 or data[:4] != b"\x7fELF" or data[5:7] != b"\x01\x01":
        raise ValueError("Invalid little-endian ELF header")
    elf_class, machine = ABI_MACHINES[abi]
    if data[4] != elf_class:
        raise ValueError(f"Wrong ELF class for {abi}")
    header_format = "<HHIIIIIHHHHHH" if elf_class == 1 else "<HHIQQQIHHHHHH"
    header = struct.unpack_from(header_format, data, 16)
    if header[1] != machine or header[0] != 3:
        raise ValueError(f"Wrong ELF machine or shared-object type for {abi}")
    program_offset, section_offset = header[4:6]
    program_stride, program_count, section_stride, section_count, names_index = header[8:13]
    program_format = "<IIIIIIII" if elf_class == 1 else "<IIQQQQQQ"
    section_format = "<IIIIIIIIII" if elf_class == 1 else "<IIQQQQIIQQ"
    for offset, stride, count, layout in (
        (program_offset, program_stride, program_count, program_format),
        (section_offset, section_stride, section_count, section_format),
    ):
        if not count or stride < struct.calcsize(layout) or offset + stride * count > len(data):
            raise ValueError("Invalid ELF table bounds")
    loads = 0
    for index in range(program_count):
        program = struct.unpack_from(program_format, data, program_offset + index * program_stride)
        if program[0] != 1:  # PT_LOAD
            continue
        loads += 1
        offset, address = program[1:3] if elf_class == 1 else program[2:4]
        file_size, memory_size = program[4:6] if elf_class == 1 else program[5:7]
        alignment = program[7]
        minimum = 4096 if elf_class == 1 else 16384
        if (
            alignment < minimum
            or alignment & (alignment - 1)
            or offset % alignment != address % alignment
            or file_size > memory_size
            or offset + file_size > len(data)
        ):
            raise ValueError(f"Invalid ELF LOAD bounds/alignment for {abi}")
    if not loads or names_index >= section_count:
        raise ValueError("Missing ELF LOAD or section-name table")
    sections = [
        struct.unpack_from(section_format, data, section_offset + index * section_stride)
        for index in range(section_count)
    ]
    strings = sections[names_index]
    if strings[1] != 3 or strings[4] + strings[5] > len(data):
        raise ValueError("Invalid ELF section-name data")
    names = data[strings[4] : strings[4] + strings[5]]
    for section in sections:
        if section[0] >= len(names):
            raise ValueError("Invalid ELF section name")
        end = names.find(b"\0", section[0])
        if end < 0:
            raise ValueError("Unterminated ELF section name")
        name = names[section[0] : end]
        if name.startswith((b".debug_", b".zdebug_")) and section[5]:
            raise ValueError("APK contains nonempty ELF debug information")


def verify_zip(apk: Path, expected_abis: set[str]) -> None:
    if not expected_abis or not expected_abis <= ABI_MACHINES.keys():
        raise ValueError("Unsupported expected ABI set")
    with zipfile.ZipFile(apk) as package:
        entries = package.infolist()
        names = [entry.filename for entry in entries]
        if len(names) != len(set(names)):
            raise ValueError("Duplicate APK entries")
        libraries: dict[str, set[str]] = {}
        for entry in entries:
            name = entry.filename
            if re.search(r"(^|/)kernel_blob\.bin$|\.symbols$|(^|/)libVkLayer[^/]*\.so$", name):
                raise ValueError(f"Forbidden APK debug payload: {name}")
            if not name.startswith("lib/") or entry.is_dir():
                continue
            match = re.fullmatch(r"lib/([^/]+)/([^/]+\.so)", name)
            if not match or match[1] not in expected_abis:
                raise ValueError(f"Unexpected native payload: {name}")
            if entry.compress_type != zipfile.ZIP_DEFLATED or entry.flag_bits & 1:
                raise ValueError(f"Native library must be deflated and unencrypted: {name}")
            verify_elf(package.read(entry), match[1])
            libraries.setdefault(match[1], set()).add(match[2])
        if libraries.keys() != expected_abis or any(
            not REQUIRED_LIBRARIES <= names for names in libraries.values()
        ):
            raise ValueError("APK ABI set or required native libraries mismatch")


def verify_manifest(text: str) -> None:
    """Require the application attribute, not a similarly named child attribute."""
    application_indent: int | None = None
    values: list[str] = []
    applications = 0
    for line in text.splitlines():
        indent = len(line) - len(line.lstrip())
        if line.lstrip().startswith("E:"):
            if application_indent is not None and indent <= application_indent:
                application_indent = None
            if re.match(r"\s*E: application(?:\s|$)", line):
                application_indent = indent
                applications += 1
        if application_indent is not None and indent == application_indent + 2:
            match = re.fullmatch(
                r"\s*A: (?:android|http://schemas\.android\.com/apk/res/android):"
                r"extractNativeLibs(?:\(0x[0-9a-fA-F]+\))?="
                r"(true|false|\(type 0x12\)0x[0-9a-fA-F]+)\s*",
                line,
            )
            if match:
                values.append(match[1].lower())
    if applications != 1 or len(values) != 1 or values[0] not in ("true", "(type 0x12)0xffffffff"):
        raise ValueError("Release application must set extractNativeLibs=true exactly once")


def verify(apk: Path, expected_abis: set[str], aapt2: Path, zipalign: Path) -> None:
    verify_zip(apk, expected_abis)
    # Explicit pinned SDK tool paths and argument arrays; no shell or package execution.
    manifest = subprocess.run(  # noqa: S603 - maintainer-supplied SDK executable, fixed arguments.
        [
            str(aapt2.resolve(strict=True)),
            "dump",
            "xmltree",
            str(apk.resolve()),
            "--file",
            "AndroidManifest.xml",
        ],
        check=True,
        capture_output=True,
        text=True,
        encoding="utf-8",
        timeout=120,
    )
    verify_manifest(manifest.stdout)
    subprocess.run(  # noqa: S603 - pinned zipalign only inspects the APK (-c).
        [str(zipalign.resolve(strict=True)), "-c", "-P", "16", "4", str(apk.resolve())],
        check=True,
        capture_output=True,
        text=True,
        timeout=120,
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apk", type=Path, required=True)
    parser.add_argument("--abis", nargs="+", choices=sorted(ABI_MACHINES), required=True)
    parser.add_argument("--aapt2", type=Path, required=True)
    parser.add_argument("--zipalign", type=Path, required=True)
    args = parser.parse_args()
    try:
        verify(args.apk, set(args.abis), args.aapt2, args.zipalign)
    except (
        OSError,
        ValueError,
        struct.error,
        zipfile.BadZipFile,
        subprocess.SubprocessError,
    ) as error:
        print(f"APK packaging validation failed: {error}", file=sys.stderr)
        return 1
    print(f"APK_PACKAGING_OK={args.apk.name}/{','.join(sorted(args.abis))}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
