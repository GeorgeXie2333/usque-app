"""Synthetic APK and ELF packaging regressions; no installed Android runtime."""

from __future__ import annotations

import struct
import subprocess
import tempfile
import unittest
import warnings
import zipfile
from pathlib import Path
from unittest.mock import patch

from verify_android_apk import (
    ABI_MACHINES,
    REQUIRED_LIBRARIES,
    verify,
    verify_elf,
    verify_manifest,
    verify_zip,
)

MANIFEST = """E: manifest (line=1)
  E: application (line=2)
    A: android:extractNativeLibs(0x010104ea)=(type 0x12)0xffffffff
"""


def elf(abi: str, *, alignment: int = 16384, debug: bool = False) -> bytes:
    elf_class, machine = ABI_MACHINES[abi]
    is32 = elf_class == 1
    hf = "<HHIIIIIHHHHHH" if is32 else "<HHIQQQIHHHHHH"
    pf = "<IIIIIIII" if is32 else "<IIQQQQQQ"
    sf = "<IIIIIIIIII" if is32 else "<IIQQQQIIQQ"
    header_size = 16 + struct.calcsize(hf)
    program_size, section_size = struct.calcsize(pf), struct.calcsize(sf)
    section_offset = header_size + program_size
    body_offset = section_offset + 3 * section_size
    names = b"\0.shstrtab\0.debug_info\0"
    body = b"dwarf" if debug else b""
    size = body_offset + len(names) + len(body)
    ident = b"\x7fELF" + bytes([elf_class, 1, 1]) + bytes(9)
    header = struct.pack(
        hf,
        3,
        machine,
        1,
        0,
        header_size,
        section_offset,
        0,
        header_size,
        program_size,
        1,
        section_size,
        3,
        1,
    )
    values = (
        (1, 0, 0, 0, size, size, 5, alignment) if is32 else (1, 5, 0, 0, 0, size, size, alignment)
    )
    program = struct.pack(pf, *values)
    sections = bytes(section_size)
    sections += struct.pack(sf, 1, 3, 0, 0, body_offset, len(names), 0, 0, 1, 0)
    sections += struct.pack(
        sf, names.index(b".debug"), 1, 0, 0, body_offset + len(names), len(body), 0, 0, 1, 0
    )
    return ident + header + program + sections + names + body


class ApkPackagingTests(unittest.TestCase):
    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.apk = self.root / "test.apk"

    def package(
        self,
        abis: set[str],
        *,
        compression: int = zipfile.ZIP_DEFLATED,
        debug: bool = False,
        missing: bool = False,
        extra: str | None = None,
    ) -> None:
        with zipfile.ZipFile(self.apk, "w", compression=compression) as archive:
            for abi in abis:
                for library in REQUIRED_LIBRARIES - ({"libapp.so"} if missing else set()):
                    archive.writestr(f"lib/{abi}/{library}", elf(abi, debug=debug))
            if extra:
                archive.writestr(extra, b"forbidden")

    def test_split_and_universal_native_payloads(self) -> None:
        for abis in [set(ABI_MACHINES), *({abi} for abi in ABI_MACHINES)]:
            with self.subTest(abis=abis):
                self.package(abis)
                verify_zip(self.apk, abis)

    def test_rejects_uncompressed_missing_wrong_abi_or_debug(self) -> None:
        for options in ({"compression": zipfile.ZIP_STORED}, {"missing": True}, {"debug": True}):
            with self.subTest(options=options):
                self.package({"arm64-v8a"}, **options)
                with self.assertRaises(ValueError):
                    verify_zip(self.apk, {"arm64-v8a"})
        self.package({"arm64-v8a"})
        for abis in ({"x86_64"}, set(), {"unknown"}):
            with self.assertRaises(ValueError):
                verify_zip(self.apk, abis)

    def test_rejects_forbidden_payloads_and_duplicates(self) -> None:
        for extra in (
            "assets/app.android-arm64.symbols",
            "assets/kernel_blob.bin",
            "lib/arm64-v8a/libVkLayer_khronos_validation.so",
            "lib/arm64-v8a/readme.txt",
        ):
            self.package({"arm64-v8a"}, extra=extra)
            with self.assertRaises(ValueError):
                verify_zip(self.apk, {"arm64-v8a"})
        self.package({"arm64-v8a"})
        with zipfile.ZipFile(self.apk, "a") as archive, warnings.catch_warnings():
            warnings.simplefilter("ignore", UserWarning)
            archive.writestr("lib/arm64-v8a/libapp.so", elf("arm64-v8a"))
        with self.assertRaisesRegex(ValueError, "Duplicate"):
            verify_zip(self.apk, {"arm64-v8a"})

    def test_elf_rejects_bad_headers_debug_and_64bit_alignment(self) -> None:
        for abi in ABI_MACHINES:
            verify_elf(elf(abi), abi)
            for invalid in (b"", elf(abi)[:64], elf(abi, debug=True), elf(abi)[:-4]):
                with self.assertRaises((ValueError, struct.error)):
                    verify_elf(invalid, abi)
        verify_elf(elf("armeabi-v7a", alignment=4096), "armeabi-v7a")
        for abi in ("arm64-v8a", "x86_64"):
            with self.assertRaises(ValueError):
                verify_elf(elf(abi, alignment=4096), abi)
        with self.assertRaises(ValueError):
            verify_elf(elf("x86_64"), "arm64-v8a")

    def test_extract_native_libs_is_required_on_application(self) -> None:
        verify_manifest(MANIFEST)
        actual_aapt2 = MANIFEST.replace(
            "android:extract", "http://schemas.android.com/apk/res/android:extract"
        ).replace("(type 0x12)0xffffffff", "true")
        verify_manifest(actual_aapt2)
        with self.assertRaises(ValueError):
            verify_manifest(actual_aapt2.replace("=true", "=false"))
        for spoof in ("schemasXandroid.com", "schemas.androidXcom", "schemasXandroidYcom"):
            with self.assertRaises(ValueError):
                verify_manifest(actual_aapt2.replace("schemas.android.com", spoof))
        for invalid in (
            "",
            MANIFEST.replace("0xffffffff", "0x0"),
            MANIFEST + MANIFEST,
            MANIFEST.replace("application", "activity"),
            MANIFEST.replace("    A:", "    E: activity\n      A:"),
        ):
            with self.assertRaises(ValueError):
                verify_manifest(invalid)

    def test_sdk_commands_are_read_only_and_fail_closed(self) -> None:
        self.package({"arm64-v8a"})
        aapt2, zipalign = self.root / "aapt2", self.root / "zipalign"
        aapt2.touch()
        zipalign.touch()
        output = subprocess.CompletedProcess([], 0, stdout=MANIFEST)
        with patch("verify_android_apk.subprocess.run", return_value=output) as run:
            verify(self.apk, {"arm64-v8a"}, aapt2, zipalign)
            self.assertEqual(run.call_args_list[0].args[0][1:3], ["dump", "xmltree"])
            self.assertEqual(run.call_args_list[1].args[0][1:5], ["-c", "-P", "16", "4"])
        with patch(
            "verify_android_apk.subprocess.run",
            side_effect=[output, subprocess.CalledProcessError(1, "zipalign")],
        ):
            with self.assertRaises(subprocess.CalledProcessError):
                verify(self.apk, {"arm64-v8a"}, aapt2, zipalign)

    def test_both_workflows_require_the_shared_validator(self) -> None:
        workflows = Path(__file__).resolve().parent.parent / ".github/workflows"
        for workflow in ("build.yml", "release.yml"):
            source = (workflows / workflow).read_text(encoding="utf-8")
            self.assertIn("python tool/verify_android_apk.py", source)
            self.assertIn('read -r -a package_abis <<< "${expected_abis[$name]}"', source)
            self.assertIn('--abis "${package_abis[@]}"', source)


if __name__ == "__main__":
    unittest.main()
