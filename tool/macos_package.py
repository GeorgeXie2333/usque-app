"""Build architecture-specific, ad-hoc macOS development apps and disk images.

This tool never installs an app, activates extensions, obtains signing credentials,
or offers its unsigned artifacts to the production update channel.
"""

from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import os
import platform
import plistlib
import re
import shutil
import stat
import struct
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
IDENTIFIER = "io.github.georgexie2333.usque"
ARCHITECTURES = {"arm64": (0x0100000C, 0), "x86_64": (0x01000007, 3)}
RUST_TARGETS = {"arm64": "aarch64-apple-darwin", "x86_64": "x86_64-apple-darwin"}
PROVIDERS = {
    "packet-tunnel": "UsquePacketTunnel",
    "transparent-proxy": "UsqueTransparentProxy",
    "dns-proxy": "UsqueDNSProxy",
    "content-filter": "UsqueContentFilter",
}
VERSION = re.compile(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-beta\.[1-9][0-9]*)?")
MACHO_MAGICS = {b"\xcf\xfa\xed\xfe", b"\xfe\xed\xfa\xcf"}
FAT_MAGICS = {
    b"\xca\xfe\xba\xbe": (">", False),
    b"\xbe\xba\xfe\xca": ("<", False),
    b"\xca\xfe\xba\xbf": (">", True),
    b"\xbf\xba\xfe\xca": ("<", True),
}


def run(arguments: list[str | Path], *, cwd: Path | None = None, env=None) -> str:
    return subprocess.check_output(  # noqa: S603 - fixed build-tool argv; no shell command text.
        [str(value) for value in arguments],
        cwd=cwd,
        env=env,
        text=True,
        stderr=subprocess.STDOUT,
    )


def artifact_name(version: str, architecture: str) -> str:
    if VERSION.fullmatch(version) is None or architecture not in ARCHITECTURES:
        raise ValueError("Invalid macOS package identity")
    return f"usque-v{version}-macos-{architecture}.dmg"


def digest(path: Path) -> str:
    result = hashlib.sha256()
    with path.open("rb") as handle:
        for data in iter(lambda: handle.read(1024 * 1024), b""):
            result.update(data)
    return result.hexdigest()


def _version(value: int) -> tuple[int, int, int]:
    return value >> 16, (value >> 8) & 255, value & 255


def inspect_macho(path: Path, architecture: str, *, universal: bool = False) -> dict:
    """Validate load commands, CPU baseline and macOS 13 compatibility without execution."""
    expected_cpu, expected_subtype = ARCHITECTURES[architecture]
    length = path.stat().st_size
    with path.open("rb") as handle:
        magic = handle.read(4)
        slices = [(0, length)]
        if magic in FAT_MAGICS:
            endian, fat64 = FAT_MAGICS[magic]
            count_bytes = handle.read(4)
            if len(count_bytes) != 4:
                raise ValueError("Truncated universal Mach-O")
            count = struct.unpack(endian + "I", count_bytes)[0]
            if not 1 <= count <= 16:
                raise ValueError("Invalid universal Mach-O slice count")
            entry_size = 32 if fat64 else 20
            table_size = 8 + count * entry_size
            slices = []
            cpus = set()
            for _ in range(count):
                entry = handle.read(entry_size)
                if len(entry) != entry_size:
                    raise ValueError("Truncated universal Mach-O slice table")
                values = struct.unpack(endian + ("IIQQII" if fat64 else "IIIII"), entry)
                cpu, _, offset, size = values[:4]
                if cpu in cpus or offset < table_size or size < 32 or offset + size > length:
                    raise ValueError("Invalid universal Mach-O slice range")
                cpus.add(cpu)
                if any(offset < old + size_old and old < offset + size for old, size_old in slices):
                    raise ValueError("Overlapping universal Mach-O slices")
                slices.append((offset, size))
            if not universal:
                raise ValueError(
                    f"Universal Mach-O must be thinned for {architecture}: {path.name}"
                )
        elif magic not in MACHO_MAGICS:
            raise ValueError(f"Not a 64-bit Mach-O: {path.name}")
        architectures = []
        selected = None
        for offset, size in slices:
            handle.seek(offset)
            header = handle.read(32)
            if len(header) != 32 or header[:4] not in MACHO_MAGICS:
                raise ValueError("Invalid 64-bit Mach-O slice header")
            endian = "<" if header[:4] == b"\xcf\xfa\xed\xfe" else ">"
            _, cpu, subtype, _, count, command_bytes, _, _ = struct.unpack(endian + "8I", header)
            known = next((name for name, (value, _) in ARCHITECTURES.items() if value == cpu), None)
            if known is None:
                raise ValueError("Unsupported Mach-O CPU")
            architectures.append(known)
            if count > 4096 or command_bytes > 16 * 1024 * 1024 or 32 + command_bytes > size:
                raise ValueError("Invalid Mach-O load command range")
            commands = handle.read(command_bytes)
            minimum = None
            cursor = 0
            for _ in range(count):
                if cursor + 8 > len(commands):
                    raise ValueError("Truncated Mach-O load command")
                command, command_size = struct.unpack_from(endian + "II", commands, cursor)
                if command_size < 8 or command_size % 8 or cursor + command_size > len(commands):
                    raise ValueError("Invalid Mach-O load command size")
                if command in (0x32, 0x24):
                    required_size = 24 if command == 0x32 else 16
                    if minimum is not None or command_size < required_size:
                        raise ValueError("Invalid Mach-O deployment declaration")
                    if command == 0x32:
                        target_platform, value = struct.unpack_from(
                            endian + "II", commands, cursor + 8
                        )
                        if target_platform != 1:
                            raise ValueError("Mach-O is not built for macOS")
                    else:
                        value = struct.unpack_from(endian + "I", commands, cursor + 8)[0]
                    minimum = _version(value)
                cursor += command_size
            if cursor != command_bytes:
                raise ValueError("Unexpected Mach-O load command bytes")
            if cpu == expected_cpu:
                if subtype & 0x00FFFFFF != expected_subtype:
                    raise ValueError(f"Mach-O requires a newer {architecture} CPU baseline")
                if minimum is None or not (0, 0, 0) < minimum <= (13, 0, 0):
                    raise ValueError(f"Mach-O does not support macOS 13: {path.name}")
                selected = {
                    "architecture": architecture,
                    "minimum_macos": ".".join(map(str, minimum)),
                }
        if selected is None:
            raise ValueError(f"Mach-O is missing {architecture}: {path.name}")
        return {
            **selected,
            "architectures": architectures,
            "universal_container": magic in FAT_MAGICS,
        }


def bundle_files(app: Path) -> list[Path]:
    root = app.resolve(strict=True)
    files = []
    for path in sorted(app.rglob("*")):
        if path.is_symlink():
            if Path(os.readlink(path)).is_absolute() or not path.resolve(
                strict=True
            ).is_relative_to(root):
                raise ValueError(f"Bundle contains an escaping symlink: {path.name}")
        elif path.is_file():
            files.append(path)
    return files


def macho_files(app: Path) -> list[Path]:
    result = []
    for path in bundle_files(app):
        with path.open("rb") as handle:
            magic = handle.read(4)
        if magic in MACHO_MAGICS or magic in FAT_MAGICS:
            result.append(path)
    return result


def required_files(app: Path) -> list[Path]:
    files = [
        app / "Contents" / name
        for name in (
            "MacOS/Usque",
            "MacOS/usque-engine",
            "Resources/usque-engine",
            "Helpers/usque-macos-control",
            "Frameworks/App.framework/App",
            "Frameworks/FlutterMacOS.framework/FlutterMacOS",
        )
    ]
    for suffix, executable in PROVIDERS.items():
        files.append(
            app
            / "Contents/Library/SystemExtensions"
            / f"{IDENTIFIER}.{suffix}.systemextension/Contents/MacOS/{executable}"
        )
    return files


def inspect_bundle(app: Path, architecture: str) -> list[dict]:
    if app.is_symlink() or app.suffix != ".app":
        raise ValueError("Expected an ordinary .app bundle")
    files = bundle_files(app)
    if not all(path.is_file() for path in required_files(app)):
        raise ValueError("macOS development bundle is incomplete")
    with (app / "Contents/Info.plist").open("rb") as handle:
        info = plistlib.load(handle)
    if info.get("CFBundleIdentifier") != IDENTIFIER or info.get("LSMinimumSystemVersion") != "13.0":
        raise ValueError("Unexpected application identity or minimum macOS version")
    if (
        info.get("UsqueProductionCapable") is not False
        or info.get("UsqueDevelopmentBuild") is not True
    ):
        raise ValueError("Development package must not advertise production installation")
    if any(path.name in {"kernel_blob.bin", "usque_ui_preview"} for path in files):
        raise ValueError("Debug/preview content must not enter the development release bundle")
    machos = macho_files(app)
    if not {path.resolve() for path in required_files(app)}.issubset(
        {path.resolve() for path in machos}
    ):
        raise ValueError("Required macOS executables are not all Mach-O binaries")
    return [
        {"path": str(path.relative_to(app)), **inspect_macho(path, architecture)} for path in machos
    ]


def _thin_and_sign(app: Path, architecture: str) -> None:
    machos = macho_files(app)
    for path in machos:
        report = inspect_macho(path, architecture, universal=True)
        if report["universal_container"]:
            mode = stat.S_IMODE(path.stat().st_mode)
            temporary = path.with_name(path.name + ".usque-thin")
            run(["xcrun", "lipo", path, "-thin", architecture, "-output", temporary])
            temporary.chmod(mode)
            temporary.replace(path)
    bundles = [
        path
        for path in app.rglob("*")
        if path.is_dir()
        and not path.is_symlink()
        and path.suffix in {".framework", ".systemextension", ".bundle", ".xpc", ".app"}
    ]
    # Signing an app's principal executable also seals the app bundle. Delay
    # those executables until their children have signatures, including Intel
    # Rust binaries (which are not automatically ad-hoc signed by the linker).
    principals = set()
    for bundle in [app, *bundles]:
        framework = bundle.suffix == ".framework"
        info = bundle / ("Resources/Info.plist" if framework else "Contents/Info.plist")
        identity = plistlib.loads(info.read_bytes()) if info.is_file() else {}
        executable = identity.get("CFBundleExecutable", bundle.stem)
        if not isinstance(executable, str) or Path(executable).name != executable:
            raise ValueError("Invalid bundle executable name")
        principal = bundle / executable if framework else bundle / "Contents/MacOS" / executable
        if principal.exists():
            principals.add(principal.resolve(strict=True))
    for path in machos:
        if path.resolve(strict=True) in principals:
            continue
        arguments = ["codesign", "--force", "--sign", "-", "--timestamp=none"]
        if path.name == "usque-macos-control":
            arguments += ["--identifier", IDENTIFIER + ".macos-control"]
        if path.name == "usque-maintenance":
            arguments += ["--identifier", IDENTIFIER + ".macos-maintenance"]
        run([*arguments, path])
    for bundle in sorted(bundles, key=lambda path: len(path.parts), reverse=True):
        run(["codesign", "--force", "--sign", "-", "--timestamp=none", bundle])
    run(["codesign", "--force", "--sign", "-", "--timestamp=none", app])
    run(["codesign", "--verify", "--strict", "--deep", app])


def assemble_app(
    source: Path,
    engine: Path,
    prototype: Path,
    destination: Path,
    architecture: str,
    version: str,
    build_number: str,
    verification: Path | None = None,
    maintenance: Path | None = None,
) -> list[dict]:
    artifact_name(version, architecture)
    if not re.fullmatch(r"[1-9][0-9]*", build_number):
        raise ValueError("Build number must be a positive integer")
    # Validate source links before copy; never dereference an external build-machine path.
    bundle_files(source)
    bundle_files(prototype)
    inspect_macho(engine, architecture)
    shutil.copytree(source, destination, symlinks=True)
    contents = destination / "Contents"
    engine_destination = contents / "MacOS/usque-engine"
    shutil.copy2(engine, engine_destination)
    engine_destination.chmod(0o755)
    compatibility = contents / "Resources/usque-engine"
    compatibility.parent.mkdir(exist_ok=True)
    if compatibility.exists() or compatibility.is_symlink():
        compatibility.unlink()
    compatibility.symlink_to("../MacOS/usque-engine")
    helpers = contents / "Helpers"
    helpers.mkdir(exist_ok=True)
    shutil.copy2(prototype / "Contents/MacOS/usque-macos-control", helpers / "usque-macos-control")
    if verification is not None:
        bundle_files(verification)
        with (verification / "Contents/Info.plist").open("rb") as handle:
            identity = plistlib.load(handle)
        if identity.get("CFBundleIdentifier") != IDENTIFIER + ".proton-verification":
            raise ValueError("Unexpected Proton verification component identity")
        shutil.copytree(verification, helpers / "UsqueProtonVerification.app", symlinks=True)
    if maintenance is not None:
        inspect_macho(maintenance, architecture)
        shutil.copy2(maintenance, helpers / "usque-maintenance")
    extensions = contents / "Library/SystemExtensions"
    if extensions.exists():
        shutil.rmtree(extensions)
    shutil.copytree(prototype / "Contents/Library/SystemExtensions", extensions, symlinks=True)
    info_path = contents / "Info.plist"
    with info_path.open("rb") as handle:
        info = plistlib.load(handle)
    info.update(
        CFBundleShortVersionString=version,
        CFBundleVersion=build_number,
        LSMinimumSystemVersion="13.0",
        UsqueDevelopmentBuild=True,
        UsqueProductionCapable=False,
        UsqueSignedPrototype=False,
        UsqueAutomaticInstallationEnabled=False,
    )
    with info_path.open("wb") as handle:
        plistlib.dump(info, handle)
    _thin_and_sign(destination, architecture)
    return inspect_bundle(destination, architecture)


def development_manifest(app: Path, architecture: str, version: str, machos: list[dict]) -> dict:
    return {
        "schema_version": 1,
        "version": version,
        "platform": "macos",
        "architecture": architecture,
        "minimum_macos": "13.0",
        "channel": "development",
        "signing": "ad-hoc",
        "notarized": False,
        "production_capable": False,
        "automatic_installation_enabled": False,
        "components": {
            "flutter": "included",
            "engine": "included",
            "network_extension_prototype": "included_activation_disabled",
            "proton_cef_verification": (
                "included_signed_launch_acceptance_pending"
                if (app / "Contents/Helpers/UsqueProtonVerification.app").is_dir()
                else "not_included"
            ),
            "atomic_update": (
                "helper_included_installation_disabled"
                if (app / "Contents/Helpers/usque-maintenance").is_file()
                else "not_included"
            ),
            "uninstall": "manual_app_removal_user_data_retained",
        },
        "validation": {
            "native_architecture_inspection": "passed",
            "codesign_integrity": "passed",
            "intel_native_execution": "not_run",
            "signed_system_vpn": "not_run",
            "macos_13_hardware": "not_run",
            "proton_cef_renderer_proxy_smoke": "not_run_by_packager",
        },
        "mach_o": machos,
        "files": [
            {
                "path": str(path.relative_to(app)),
                "size": path.stat().st_size,
                "sha256": digest(path),
            }
            for path in bundle_files(app)
        ],
    }


def spdx_inventory(manifest: dict) -> dict:
    """An explicit file inventory, not a claim that dependency/license review is complete."""
    seed = hashlib.sha256(json.dumps(manifest, sort_keys=True).encode()).hexdigest()
    return {
        "spdxVersion": "SPDX-2.3",
        "dataLicense": "CC0-1.0",
        "SPDXID": "SPDXRef-DOCUMENT",
        "name": f"Usque macOS {manifest['architecture']} development bundle inventory",
        "documentNamespace": f"https://spdx.org/spdxdocs/usque-development-{seed}",
        "creationInfo": {
            "creators": ["Tool: usque-macos-package"],
            "created": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        },
        "documentComment": "Development file inventory. Dependency/license review is incomplete.",
        "files": [
            {
                "SPDXID": f"SPDXRef-File-{index}",
                "fileName": "./" + value["path"],
                "checksums": [{"algorithm": "SHA256", "checksumValue": value["sha256"]}],
                "licenseConcluded": "NOASSERTION",
                "copyrightText": "NOASSERTION",
            }
            for index, value in enumerate(manifest["files"])
        ],
        "relationships": [
            {
                "spdxElementId": "SPDXRef-DOCUMENT",
                "relationshipType": "DESCRIBES",
                "relatedSpdxElement": f"SPDXRef-File-{index}",
            }
            for index in range(len(manifest["files"]))
        ],
    }


def package(
    source: Path,
    engine: Path,
    prototype: Path,
    output: Path,
    architecture: str,
    version: str,
    build_number: str,
    *,
    create_dmg: bool = True,
    verification: Path | None = None,
    maintenance: Path | None = None,
) -> dict:
    name = artifact_name(version, architecture)
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".usque-package-", dir=output) as directory:
        work = Path(directory)
        volume = work / "volume"
        volume.mkdir()
        app = volume / "Usque.app"
        machos = assemble_app(
            source,
            engine,
            prototype,
            app,
            architecture,
            version,
            build_number,
            verification=verification,
            maintenance=maintenance,
        )
        manifest = development_manifest(app, architecture, version, machos)
        (volume / "Applications").symlink_to("/Applications")
        (volume / "DEVELOPMENT - READ ME.txt").write_text(
            "Usque macOS development build\n\nDrag Usque.app to Applications to install.\n"
            "This ad-hoc build is not notarized. Production automatic installation is disabled.\n"
            "The VPN extensions cannot be activated by this build. Signed device tests remain pending.\n"
            "Signed verification-window and update installation acceptance remain pending.\n",
            encoding="utf-8",
        )
        if create_dmg:
            image = work / name
            run(
                [
                    "hdiutil",
                    "create",
                    "-srcfolder",
                    volume,
                    "-volname",
                    f"Usque Development {version} {architecture}",
                    "-format",
                    "UDZO",
                    "-fs",
                    "HFS+",
                    image,
                ]
            )
            run(["hdiutil", "verify", image])
            manifest["artifact"] = {
                "name": name,
                "size": image.stat().st_size,
                "sha256": digest(image),
            }
            image.replace(output / name)
            (output / (name + ".sha256")).write_text(
                f"{manifest['artifact']['sha256']}  {name}\n", encoding="ascii"
            )
        final_app = output / "Usque.app"
        if final_app.exists():
            shutil.rmtree(final_app)
        app.rename(final_app)
        (output / name.replace(".dmg", ".manifest.json")).write_text(
            json.dumps(manifest, indent=2) + "\n", encoding="utf-8"
        )
        (output / (name + ".spdx.json")).write_text(
            json.dumps(spdx_inventory(manifest), indent=2) + "\n", encoding="utf-8"
        )
    return manifest


def build(architecture: str, output: Path, *, create_dmg: bool = True) -> dict:
    if platform.system() != "Darwin":
        raise ValueError("macOS development builds require a macOS host")
    import fcntl

    metadata = (ROOT / "apps/usque_gui/pubspec.yaml").read_text(encoding="utf-8")
    match = re.search(r"^version: (\S+)\+(\d+)$", metadata, re.MULTILINE)
    if match is None:
        raise ValueError("Flutter version/build number is missing")
    version, build_number = match.groups()
    artifact_name(version, architecture)
    target = RUST_TARGETS[architecture]
    env = os.environ.copy()
    env.update(MACOSX_DEPLOYMENT_TARGET="13.0", CARGO_PROFILE_RELEASE_BUILD_OVERRIDE_STRIP="none")
    for name in ("CFLAGS", "CXXFLAGS"):
        env[name] = env.get(name, "") + " -mmacosx-version-min=13.0"
    env.update(
        FLUTTER_XCODE_ARCHS=architecture,
        FLUTTER_XCODE_ONLY_ACTIVE_ARCH="NO",
        FLUTTER_XCODE_EXCLUDED_ARCHS="x86_64" if architecture == "arm64" else "arm64",
        FLUTTER_XCODE_MACOSX_DEPLOYMENT_TARGET="13.0",
        FLUTTER_XCODE_CODE_SIGNING_ALLOWED="NO",
    )
    lock = ROOT / "build/.macos-development-package.lock"
    lock.parent.mkdir(exist_ok=True)
    with lock.open("a") as handle:
        # Flutter's generated project/build directories are shared by both architectures.
        fcntl.flock(handle, fcntl.LOCK_EX)
        run(
            [
                "cargo",
                "build",
                "--release",
                "--locked",
                "--offline",
                "--target",
                target,
                "-p",
                "usque-engine",
                "-p",
                "usque-macos-runtime",
            ],
            cwd=ROOT,
            env=env,
        )
        gui = ROOT / "apps/usque_gui"
        run(["bash", ROOT / "tool/build_macos_maintenance.sh", architecture], cwd=ROOT, env=env)
        run(
            ["python3", ROOT / "tool/build_macos_proton_verification.py", "--arch", architecture],
            cwd=ROOT,
            env=env,
        )
        run(["flutter", "pub", "get", "--offline", "--enforce-lockfile"], cwd=gui, env=env)
        run(
            [
                "flutter",
                "build",
                "macos",
                "--release",
                "--no-pub",
                "--build-name",
                version,
                "--build-number",
                build_number,
            ],
            cwd=gui,
            env=env,
        )
        with tempfile.TemporaryDirectory(prefix="usque-native-build-") as directory:
            run(
                [
                    "bash",
                    ROOT / "tool/build_macos_network_prototype.sh",
                    "--arch",
                    architecture,
                    "--output",
                    directory,
                    "--runtime-library",
                    ROOT / f"target/{target}/release/libusque_macos_runtime.a",
                ],
                cwd=ROOT,
                env=env,
            )
            return package(
                gui / "build/macos/Build/Products/Release/Usque.app",
                ROOT / f"target/{target}/release/usque-engine",
                Path(directory) / "UsqueNetworkPrototype.app",
                output,
                architecture,
                version,
                build_number,
                create_dmg=create_dmg,
                verification=ROOT
                / f"build/macos-proton-verification/{architecture}/UsqueProtonVerification.app",
                maintenance=ROOT / f"build/macos-maintenance/{architecture}/usque-maintenance",
            )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    for command in ("build", "package", "inspect"):
        sub = commands.add_parser(command)
        sub.add_argument("--arch", required=True, choices=ARCHITECTURES)
        if command == "inspect":
            sub.add_argument("--app", type=Path, required=True)
            continue
        sub.add_argument("--output", type=Path)
        sub.add_argument("--no-dmg", action="store_true", help="Assemble/verify app only")
        if command == "package":
            sub.add_argument("--verification", type=Path)
            sub.add_argument("--maintenance", type=Path)
            for name in ("app", "engine", "network-prototype"):
                sub.add_argument("--" + name, type=Path, required=True)
            sub.add_argument("--version", required=True)
            sub.add_argument("--build-number", required=True)
    arguments = parser.parse_args()
    try:
        if arguments.command == "inspect":
            print(json.dumps(inspect_bundle(arguments.app, arguments.arch), indent=2))
            return
        output = arguments.output or ROOT / "build/macos-development" / arguments.arch
        if arguments.command == "build":
            manifest = build(arguments.arch, output, create_dmg=not arguments.no_dmg)
        else:
            manifest = package(
                arguments.app,
                arguments.engine,
                arguments.network_prototype,
                output,
                arguments.arch,
                arguments.version,
                arguments.build_number,
                create_dmg=not arguments.no_dmg,
                verification=arguments.verification,
                maintenance=arguments.maintenance,
            )
        print(
            json.dumps(
                {
                    "output": str(output.resolve()),
                    "architecture": arguments.arch,
                    "artifact": manifest.get("artifact"),
                    "production_capable": False,
                },
                indent=2,
            )
        )
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        if isinstance(error, subprocess.CalledProcessError) and error.output:
            print(error.output)
        parser.exit(1, f"macOS development packaging failed: {error}\n")


if __name__ == "__main__":
    main()
