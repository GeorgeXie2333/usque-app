from __future__ import annotations

import json
import plistlib
import struct
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import macos_package


def macho(architecture="arm64", *, minimum=0x000D0000, subtype=None, target_platform=1):
    cpu, default_subtype = macos_package.ARCHITECTURES[architecture]
    command = struct.pack("<6I", 0x32, 24, target_platform, minimum, 0x001B0000, 0)
    return (
        struct.pack(
            "<8I",
            0xFEEDFACF,
            cpu,
            default_subtype if subtype is None else subtype,
            2,
            1,
            len(command),
            0,
            0,
        )
        + command
    )


def write(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)


def flutter_fixture(app: Path, architecture="arm64") -> None:
    for relative in (
        "MacOS/Usque",
        "Frameworks/App.framework/Versions/A/App",
        "Frameworks/FlutterMacOS.framework/Versions/A/FlutterMacOS",
    ):
        write(app / "Contents" / relative, macho(architecture))
    for name in ("App", "FlutterMacOS"):
        framework = app / f"Contents/Frameworks/{name}.framework"
        (framework / name).symlink_to(f"Versions/A/{name}")
        (framework / "Versions/Current").symlink_to("A")
    write(
        app / "Contents/Info.plist",
        plistlib.dumps({"CFBundleIdentifier": macos_package.IDENTIFIER}),
    )


def prototype_fixture(app: Path, architecture="arm64") -> None:
    write(app / "Contents/MacOS/usque-macos-control", macho(architecture))
    for suffix, name in macos_package.PROVIDERS.items():
        path = app / (
            f"Contents/Library/SystemExtensions/{macos_package.IDENTIFIER}."
            f"{suffix}.systemextension/Contents/MacOS/{name}"
        )
        write(path, macho(architecture))


def verification_fixture(app: Path, architecture="arm64") -> None:
    write(app / "Contents/MacOS/UsqueProtonVerification", macho(architecture))
    write(
        app / "Contents/Info.plist",
        plistlib.dumps({"CFBundleIdentifier": macos_package.IDENTIFIER + ".proton-verification"}),
    )
    write(
        app
        / "Contents/Frameworks/Chromium Embedded Framework.framework/Chromium Embedded Framework",
        macho(architecture),
    )
    write(
        app / "Contents/Frameworks/UsqueProtonVerification Helper.app/Contents/MacOS/Helper",
        macho(architecture),
    )


class MacOSPackageTests(unittest.TestCase):
    def test_names_are_architecture_specific_and_reject_path_injection(self) -> None:
        self.assertEqual(
            macos_package.artifact_name("0.3.3", "arm64"), "usque-v0.3.3-macos-arm64.dmg"
        )
        self.assertEqual(
            macos_package.artifact_name("0.3.4-beta.1", "x86_64"),
            "usque-v0.3.4-beta.1-macos-x86_64.dmg",
        )
        for version, architecture in (
            ("../0.3.3", "arm64"),
            ("0.3.3\n", "arm64"),
            ("01.3.3", "arm64"),
            ("0.3.3", "x64"),
        ):
            with self.assertRaises(ValueError):
                macos_package.artifact_name(version, architecture)

    def test_macho_checks_minimum_os_architecture_cpu_and_platform_without_execution(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "inert"
            for arch in macos_package.ARCHITECTURES:
                path.write_bytes(macho(arch))
                self.assertEqual(macos_package.inspect_macho(path, arch)["minimum_macos"], "13.0.0")
                other = "arm64" if arch == "x86_64" else "x86_64"
                with self.assertRaises(ValueError):
                    macos_package.inspect_macho(path, other)
            for invalid in (
                macho(minimum=0x000D0001),
                macho(minimum=0),
                macho(subtype=2),
                macho("x86_64", subtype=8),
                macho(target_platform=2),
                macho()[:-1],
                b"\xcf\xfa\xed\xfe",
            ):
                path.write_bytes(invalid)
                with self.assertRaises(ValueError):
                    macos_package.inspect_macho(path, "arm64")

    def test_universal_framework_must_be_thinned_and_overlapping_slices_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "inert"
            arm, intel = macho(), macho("x86_64")
            header = struct.pack(">II", 0xCAFEBABE, 2)
            entries = struct.pack(">5I", 0x0100000C, 0, 48, len(arm), 0)
            entries += struct.pack(">5I", 0x01000007, 3, 48 + len(arm), len(intel), 0)
            path.write_bytes(header + entries + arm + intel)
            self.assertEqual(
                macos_package.inspect_macho(path, "arm64", universal=True)["architectures"],
                ["arm64", "x86_64"],
            )
            with self.assertRaises(ValueError):
                macos_package.inspect_macho(path, "arm64")
            overlapping = bytearray(path.read_bytes())
            struct.pack_into(">I", overlapping, 36, 48)
            path.write_bytes(overlapping)
            with self.assertRaises(ValueError):
                macos_package.inspect_macho(path, "arm64", universal=True)

    def test_escape_and_dangling_symlinks_cannot_enter_a_package(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            app = root / "Usque.app"
            app.mkdir()
            outside = root / "private-file"
            outside.write_text("fixture")
            link = app / "link"
            for target in (outside, Path("../private-file"), Path("missing")):
                link.symlink_to(target)
                with self.assertRaises((ValueError, FileNotFoundError)):
                    macos_package.bundle_files(app)
                link.unlink()

    def test_assembly_connects_engine_and_helper_layout_and_fails_mixed_architecture(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source, prototype, engine = root / "GUI.app", root / "Prototype.app", root / "engine"
            flutter_fixture(source)
            prototype_fixture(prototype)
            engine.write_bytes(macho())
            # Existing GUI builds sometimes carry an old Engine in Resources.
            write(source / "Contents/Resources/usque-engine", b"old engine")
            destination = root / "Usque.app"
            with patch.object(macos_package, "run", return_value=""):
                reports = macos_package.assemble_app(
                    source, engine, prototype, destination, "arm64", "0.3.3", "29"
                )
            self.assertEqual(len(reports), 9)
            compatibility = destination / "Contents/Resources/usque-engine"
            self.assertTrue(compatibility.is_symlink())
            self.assertEqual(compatibility.resolve().parent.name, "MacOS")
            self.assertEqual(compatibility.read_bytes(), engine.read_bytes())
            self.assertTrue((destination / "Contents/Helpers/usque-macos-control").is_file())
            write(destination / "Contents/Helpers/usque-macos-control", macho("x86_64"))
            with self.assertRaises(ValueError):
                macos_package.inspect_bundle(destination, "arm64")

    def test_developer_manifest_hashes_files_and_cannot_advertise_production(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source, prototype, engine = root / "GUI.app", root / "Prototype.app", root / "engine"
            flutter_fixture(source, "x86_64")
            prototype_fixture(prototype, "x86_64")
            engine.write_bytes(macho("x86_64"))
            output = root / "result"
            with patch.object(macos_package, "run", return_value=""):
                manifest = macos_package.package(
                    source, engine, prototype, output, "x86_64", "0.3.3", "29", create_dmg=False
                )
            self.assertFalse(manifest["production_capable"])
            self.assertFalse(manifest["automatic_installation_enabled"])
            self.assertFalse(manifest["notarized"])
            self.assertEqual(manifest["components"]["proton_cef_verification"], "not_included")
            self.assertEqual(manifest["validation"]["intel_native_execution"], "not_run")
            self.assertEqual(
                manifest["files"][0]["sha256"],
                macos_package.digest(output / "Usque.app" / manifest["files"][0]["path"]),
            )
            self.assertFalse(list(output.glob("*.dmg")))
            document = json.loads(next(output.glob("*.spdx.json")).read_text())
            self.assertEqual(document["spdxVersion"], "SPDX-2.3")
            self.assertEqual(len(document["files"]), len(manifest["files"]))
            info_path = output / "Usque.app/Contents/Info.plist"
            info = plistlib.loads(info_path.read_bytes())
            info["UsqueProductionCapable"] = True
            info_path.write_bytes(plistlib.dumps(info))
            with self.assertRaises(ValueError):
                macos_package.inspect_bundle(output / "Usque.app", "x86_64")

    def test_missing_or_non_executable_components_fail_verification(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            app = Path(directory) / "Usque.app"
            flutter_fixture(app)
            with self.assertRaises(ValueError):
                macos_package.inspect_bundle(app, "arm64")

    def test_cef_and_maintenance_are_included_and_every_nested_binary_is_checked(self) -> None:
        for architecture in macos_package.ARCHITECTURES:
            with (
                self.subTest(architecture=architecture),
                tempfile.TemporaryDirectory() as directory,
            ):
                root = Path(directory)
                source, prototype, engine = (
                    root / "GUI.app",
                    root / "Prototype.app",
                    root / "engine",
                )
                verification, maintenance = root / "CEF.app", root / "maintenance"
                flutter_fixture(source, architecture)
                prototype_fixture(prototype, architecture)
                verification_fixture(verification, architecture)
                engine.write_bytes(macho(architecture))
                maintenance.write_bytes(macho(architecture))
                with patch.object(macos_package, "run", return_value=""):
                    manifest = macos_package.package(
                        source,
                        engine,
                        prototype,
                        root / "output",
                        architecture,
                        "0.3.3",
                        "29",
                        create_dmg=False,
                        verification=verification,
                        maintenance=maintenance,
                    )
                self.assertEqual(len(manifest["mach_o"]), 13)
                self.assertEqual(
                    manifest["components"]["proton_cef_verification"],
                    "included_signed_launch_acceptance_pending",
                )
                self.assertEqual(
                    manifest["components"]["atomic_update"], "helper_included_installation_disabled"
                )
                self.assertFalse(manifest["automatic_installation_enabled"])
                other = "arm64" if architecture == "x86_64" else "x86_64"
                nested = (
                    verification
                    / "Contents/Frameworks/UsqueProtonVerification Helper.app/Contents/MacOS/Helper"
                )
                nested.write_bytes(macho(other))
                with (
                    patch.object(macos_package, "run", return_value=""),
                    self.assertRaises(ValueError),
                ):
                    macos_package.assemble_app(
                        source,
                        engine,
                        prototype,
                        root / "mixed.app",
                        architecture,
                        "0.3.3",
                        "29",
                        verification=verification,
                        maintenance=maintenance,
                    )

    def test_cef_identity_and_maintenance_architecture_cannot_be_substituted(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source, prototype, engine = root / "GUI.app", root / "Prototype.app", root / "engine"
            verification, maintenance = root / "CEF.app", root / "maintenance"
            flutter_fixture(source)
            prototype_fixture(prototype)
            verification_fixture(verification)
            engine.write_bytes(macho())
            maintenance.write_bytes(macho("x86_64"))
            with self.assertRaises(ValueError):
                macos_package.assemble_app(
                    source,
                    engine,
                    prototype,
                    root / "mixed.app",
                    "arm64",
                    "0.3.3",
                    "29",
                    maintenance=maintenance,
                )
            write(
                verification / "Contents/Info.plist",
                plistlib.dumps({"CFBundleIdentifier": "untrusted.component"}),
            )
            with self.assertRaises(ValueError):
                macos_package.assemble_app(
                    source,
                    engine,
                    prototype,
                    root / "substituted.app",
                    "arm64",
                    "0.3.3",
                    "29",
                    verification=verification,
                )

    def test_signs_unsigned_engine_before_sealing_outer_app(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            app = Path(directory) / "Usque.app"
            flutter_fixture(app, "x86_64")
            write(app / "Contents/MacOS/usque-engine", macho("x86_64"))
            nested = app / "Contents/Helpers/Verification.app"
            write(
                nested / "Contents/Info.plist",
                plistlib.dumps(
                    {
                        "CFBundleExecutable": "Verify",
                    }
                ),
            )
            principal = nested / "Contents/MacOS/Verify"
            write(principal, macho("x86_64"))
            write(nested / "Contents/MacOS/sidecar", macho("x86_64"))
            operations = []
            with patch.object(
                macos_package, "run", side_effect=lambda args: operations.append(args)
            ):
                macos_package._thin_and_sign(app, "x86_64")
            signed = [args[-1] for args in operations if "--sign" in args]
            self.assertNotIn(app / "Contents/MacOS/Usque", signed)
            self.assertNotIn(principal, signed)
            self.assertLess(signed.index(app / "Contents/MacOS/usque-engine"), signed.index(app))
            self.assertLess(signed.index(nested / "Contents/MacOS/sidecar"), signed.index(nested))
            self.assertLess(signed.index(nested), signed.index(app))
            self.assertEqual(operations[-1], ["codesign", "--verify", "--strict", "--deep", app])

    def test_image_is_verified_before_replacing_an_existing_artifact(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source, prototype, engine = root / "GUI.app", root / "Prototype.app", root / "engine"
            flutter_fixture(source)
            prototype_fixture(prototype)
            engine.write_bytes(macho())
            output = root / "result"
            output.mkdir()
            name = macos_package.artifact_name("0.3.3", "arm64")
            previous = output / name
            previous.write_bytes(b"previous image")
            operations = []

            def tool(arguments, **_):
                if arguments[0] == "hdiutil":
                    operations.append(arguments[1])
                    if arguments[1] == "create":
                        volume = Path(arguments[arguments.index("-srcfolder") + 1])
                        self.assertEqual(
                            (volume / "Applications").readlink(), Path("/Applications")
                        )
                        self.assertTrue((volume / "Usque.app").is_dir())
                        Path(arguments[-1]).write_bytes(b"new image")
                    else:
                        raise subprocess.CalledProcessError(1, arguments)
                return ""

            with patch.object(macos_package, "run", side_effect=tool):
                with self.assertRaises(subprocess.CalledProcessError):
                    macos_package.package(source, engine, prototype, output, "arm64", "0.3.3", "29")
            self.assertEqual(operations, ["create", "verify"])
            self.assertEqual(previous.read_bytes(), b"previous image")
            self.assertFalse((output / "Usque.app").exists())


if __name__ == "__main__":
    unittest.main()
