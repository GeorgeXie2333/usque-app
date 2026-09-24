"""Exercise symbol identity checks with synthetic ELF files and APKs."""

from __future__ import annotations

import argparse
import json
import struct
import tempfile
import unittest
import zipfile
from pathlib import Path

from archive_flutter_symbols import archive, elf_identity, matching_symbols, verify_stack


def elf(machine: int = 62, build_id: bytes = b"a" * 16, *, debug: bool = False) -> bytes:
    is32 = machine == 40
    header_format = "<HHIIIIIHHHHHH" if is32 else "<HHIQQQIHHHHHH"
    section_format = "<IIIIIIIIII" if is32 else "<IIQQQQIIQQ"
    header_size = 16 + struct.calcsize(header_format)
    section_size = struct.calcsize(section_format)
    names = b"\0.shstrtab\0.note.gnu.build-id\0.debug_info\0"
    note = struct.pack("<III", 4, len(build_id), 3) + b"GNU\0" + build_id
    note += b"\0" * (-len(note) % 4)
    bodies = [b"", names, note, b"dwarf" if debug else b""]
    name_indexes = [0, 1, names.index(b".note"), names.index(b".debug")]
    section_kinds = [0, 3, 7, 1]
    offset = header_size + 4 * section_size
    sections = []
    for name, kind, body in zip(name_indexes, section_kinds, bodies, strict=True):
        sections.append(
            struct.pack(section_format, name, kind, 0, 0, offset, len(body), 0, 0, 4, 0)
        )
        offset += len(body)
    ident = b"\x7fELF" + bytes([1 if is32 else 2, 1, 1]) + b"\0" * 9
    header = struct.pack(
        header_format, 3, machine, 1, 0, 0, header_size, 0, header_size, 0, 0, section_size, 4, 1
    )
    return ident + header + b"".join(sections) + b"".join(bodies)


class SymbolsTests(unittest.TestCase):
    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.symbols = self.root / "symbols"
        self.symbols.mkdir()

    def args(self, **changes: str) -> argparse.Namespace:
        values = {
            "artifact": str(self.root / "app.so"),
            "symbols": str(self.symbols),
            "output": str(self.root / "archive"),
            "version": "0.2.7+21",
            "source_commit": "a" * 40,
            "platform": "windows",
            "kind": "windows",
            "architecture": "windows-x64",
        }
        values.update(changes)
        return argparse.Namespace(**values)

    def test_supported_elf_architectures(self) -> None:
        for machine in (40, 62, 183):
            self.assertEqual(elf_identity(elf(machine)), (machine, "61" * 16, False))
            self.assertEqual(elf_identity(elf(machine, debug=True)), (machine, "61" * 16, True))

    def test_bad_or_truncated_elf_is_rejected(self) -> None:
        for data in (b"", b"not ELF", elf()[:70], elf()[:-16]):
            with self.subTest(length=len(data)), self.assertRaises(ValueError):
                elf_identity(data)

    def test_mismatched_build_architecture_or_missing_dwarf_is_rejected(self) -> None:
        for symbols in (
            elf(debug=False),
            elf(build_id=b"b" * 16, debug=True),
            elf(machine=183, debug=True),
        ):
            with self.subTest(symbols=symbols), self.assertRaises(ValueError):
                matching_symbols(elf(), symbols, "windows-x64")

    def test_windows_archive_preserves_identity_and_refuses_overwrite(self) -> None:
        (self.root / "app.so").write_bytes(elf())
        original = elf(debug=True)
        (self.symbols / "app.windows-x64.symbols").write_bytes(original)
        path = archive(self.args())
        manifest = json.loads((path / "manifest.json").read_text())
        record = manifest["symbols"][0]
        self.assertEqual(record["build_id"], "61" * 16)
        self.assertEqual((path / record["symbol_file"]).read_bytes(), original)
        with self.assertRaises(FileExistsError):
            archive(self.args())

    def test_missing_or_empty_symbols_leave_no_archive(self) -> None:
        (self.root / "app.so").write_bytes(elf())
        with self.assertRaises(FileNotFoundError):
            archive(self.args())
        (self.symbols / "app.windows-x64.symbols").write_bytes(b"")
        with self.assertRaises(ValueError):
            archive(self.args())
        self.assertFalse((self.root / "archive").exists())

    def test_android_universal_requires_every_matching_symbol(self) -> None:
        apk = self.root / "app.apk"
        abis = {"armeabi-v7a": (40, "arm"), "arm64-v8a": (183, "arm64"), "x86_64": (62, "x64")}
        with zipfile.ZipFile(apk, "w") as package:
            for abi, (machine, _) in abis.items():
                package.writestr(f"lib/{abi}/libapp.so", elf(machine))
        args = self.args(
            artifact=str(apk), platform="android", kind="universal", architecture=",".join(abis)
        )
        for machine, symbol_arch in abis.values():
            (self.symbols / f"app.android-{symbol_arch}.symbols").write_bytes(
                elf(machine, debug=True)
            )
        mismatch = self.symbols / "app.android-arm64.symbols"
        mismatch.write_bytes(elf(183, build_id=b"b" * 16, debug=True))
        with self.assertRaises(ValueError):
            archive(args)
        self.assertFalse((self.root / "archive").exists())
        mismatch.write_bytes(elf(183, debug=True))
        path = archive(args)
        manifest = json.loads((path / "manifest.json").read_text())
        self.assertEqual(len(manifest["symbols"]), 3)

    def test_split_apk_rejects_unexpected_abi(self) -> None:
        apk = self.root / "app.apk"
        with zipfile.ZipFile(apk, "w") as package:
            package.writestr("lib/arm64-v8a/libapp.so", elf(183))
        with self.assertRaises(ValueError):
            archive(
                self.args(
                    artifact=str(apk), platform="android", kind="split", architecture="x86_64"
                )
            )

    def test_invalid_source_identity_and_path_traversal_are_rejected(self) -> None:
        for changes in ({"version": "../../outside"}, {"source_commit": "short"}):
            with self.subTest(changes=changes), self.assertRaises(ValueError):
                archive(self.args(**changes))

    def test_stack_requires_exact_symbol_build_id(self) -> None:
        symbols = self.symbols / "probe.symbols"
        symbols.write_bytes(elf(debug=True))
        stack = self.root / "trace.txt"
        stack.write_text(f"build_id: '{'61' * 16}'\n")
        verify_stack(symbols, stack)
        for text in ("no identity", f"build_id: '{'62' * 16}'", ""):
            stack.write_text(text)
            with self.subTest(text=text), self.assertRaises(ValueError):
                verify_stack(symbols, stack)


if __name__ == "__main__":
    unittest.main()
